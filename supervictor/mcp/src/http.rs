//! HTTP/1.1 on the wire, no sockets: build a GET, parse a response.
//!
//! One request per connection with `Connection: close`, the same dialect
//! the firmware speaks (`supervictor/edge/src/network/http.rs`). The body
//! is delimited by `Content-Length`, by chunked transfer encoding, or by
//! EOF — axum answers the admin routes with a Content-Length today, but the
//! parser does not depend on that.

/// Hard cap on a response we are willing to hold (a 7-day history for a
/// large fleet is well under this; anything bigger is not a status answer).
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// A parsed response: status, headers (names lower-cased), body bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// HTTP status code.
    pub status: u16,
    /// Headers in wire order, names lower-cased.
    pub headers: Vec<(String, String)>,
    /// Body with transfer encoding removed.
    pub body: Vec<u8>,
}

impl Response {
    /// First header value with this (case-insensitive) name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// `GET path` with the headers an admin API call needs. No
/// `Accept-Encoding`, so the compression layer leaves the body alone.
pub fn get_request(host_header: &str, path: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\nHost: {host_header}\r\nUser-Agent: supervictor-mcp/{}\r\n\
         Accept: application/json\r\nConnection: close\r\n\r\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// Parse a complete response (everything the peer sent before closing).
pub fn parse_response(raw: &[u8]) -> Result<Response, String> {
    let mut header_buf = [httparse::EMPTY_HEADER; 64];
    let mut parsed = httparse::Response::new(&mut header_buf);
    let head_len = match parsed
        .parse(raw)
        .map_err(|e| format!("malformed response head: {e}"))?
    {
        httparse::Status::Complete(n) => n,
        httparse::Status::Partial => {
            return Err(if raw.is_empty() {
                "empty response (the connection closed before any bytes arrived)".into()
            } else {
                "truncated response head".into()
            })
        }
    };
    let status = parsed
        .code
        .ok_or_else(|| "response head without a status code".to_string())?;
    let headers: Vec<(String, String)> = parsed
        .headers
        .iter()
        .map(|h| {
            (
                h.name.to_ascii_lowercase(),
                String::from_utf8_lossy(h.value).trim().to_string(),
            )
        })
        .collect();

    let rest = &raw[head_len..];
    let chunked = headers.iter().any(|(n, v)| {
        n == "transfer-encoding"
            && v.split(',')
                .any(|t| t.trim().eq_ignore_ascii_case("chunked"))
    });
    let body = if chunked {
        dechunk(rest)?
    } else if let Some(len) = headers.iter().find(|(n, _)| n == "content-length") {
        let len: usize = len
            .1
            .parse()
            .map_err(|_| format!("bad Content-Length '{}'", len.1))?;
        if rest.len() < len {
            return Err(format!(
                "truncated body: Content-Length {len}, received {}",
                rest.len()
            ));
        }
        rest[..len].to_vec()
    } else {
        rest.to_vec()
    };
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(format!(
            "response body of {} bytes exceeds the {MAX_RESPONSE_BYTES}-byte limit",
            body.len()
        ));
    }
    Ok(Response {
        status,
        headers,
        body,
    })
}

/// Decode a chunked body: `<hex-size>[;ext]\r\n<data>\r\n ... 0\r\n[trailers]\r\n`.
fn dechunk(mut rest: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = find(rest, b"\r\n").ok_or("truncated chunk size line")?;
        let size_line = std::str::from_utf8(&rest[..line_end])
            .map_err(|_| "chunk size line is not UTF-8".to_string())?;
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| format!("bad chunk size '{size_hex}'"))?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            return Ok(out); // trailers, if any, are ignored
        }
        if rest.len() < size + 2 {
            return Err(format!(
                "truncated chunk: expected {size} bytes, have {}",
                rest.len().saturating_sub(2)
            ));
        }
        out.extend_from_slice(&rest[..size]);
        if &rest[size..size + 2] != b"\r\n" {
            return Err("chunk data not followed by CRLF".into());
        }
        rest = &rest[size + 2..];
        if out.len() > MAX_RESPONSE_BYTES {
            return Err(format!(
                "chunked body exceeds the {MAX_RESPONSE_BYTES}-byte limit"
            ));
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_request_shape() {
        let req = get_request("localhost:8443", "/fleet/summary");
        assert!(req.starts_with("GET /fleet/summary HTTP/1.1\r\n"));
        assert!(req.contains("Host: localhost:8443\r\n"));
        assert!(req.contains("Connection: close\r\n"));
        assert!(!req.contains("Accept-Encoding"));
        assert!(req.ends_with("\r\n\r\n"));
    }

    #[test]
    fn content_length_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}";
        let r = parse_response(raw).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.header("content-type"), Some("application/json"));
        assert_eq!(r.header("Content-Type"), Some("application/json"));
        assert_eq!(r.body, b"{\"status\":\"ok\"}");
    }

    #[test]
    fn content_length_ignores_trailing_garbage_and_rejects_short_bodies() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}junk";
        assert_eq!(parse_response(raw).unwrap().body, b"{}");
        let short = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n{}";
        assert!(parse_response(short)
            .unwrap_err()
            .contains("truncated body"));
        let bad = b"HTTP/1.1 200 OK\r\nContent-Length: x\r\n\r\n{}";
        assert!(parse_response(bad).unwrap_err().contains("Content-Length"));
    }

    #[test]
    fn chunked_body_reassembles() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n[{\"a\r\n7;ext=1\r\n\":1}]\r\n\r\n0\r\nX-Trailer: y\r\n\r\n";
        let r = parse_response(raw).unwrap();
        assert_eq!(r.body, b"[{\"a\":1}]\r\n");
    }

    #[test]
    fn chunked_malformed() {
        for raw in [
            &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nab"[..],
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nab\r\n0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nabcd\r\n0\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2",
        ] {
            assert!(
                parse_response(raw).is_err(),
                "{:?}",
                String::from_utf8_lossy(raw)
            );
        }
    }

    #[test]
    fn read_to_eof_body_when_undelimited() {
        let raw = b"HTTP/1.1 403 Forbidden\r\n\r\nforbidden: admin certificate required";
        let r = parse_response(raw).unwrap();
        assert_eq!(r.status, 403);
        assert_eq!(r.body, b"forbidden: admin certificate required");
    }

    #[test]
    fn empty_and_truncated_heads() {
        assert!(parse_response(b"").unwrap_err().contains("empty response"));
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Len")
            .unwrap_err()
            .contains("truncated"));
        assert!(parse_response(b"not http at all\r\n\r\n").is_err());
        // A status line alone, no headers, no body.
        let r = parse_response(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap();
        assert_eq!(r.status, 204);
        assert!(r.body.is_empty());
    }

    #[test]
    fn header_values_are_trimmed_and_binary_safe() {
        let raw =
            b"HTTP/1.1 200 OK\r\nX-Thing:   padded   \r\nContent-Length: 3\r\n\r\n\xff\x00\x01";
        let r = parse_response(raw).unwrap();
        assert_eq!(r.header("x-thing"), Some("padded"));
        assert_eq!(r.body, b"\xff\x00\x01");
    }
}
