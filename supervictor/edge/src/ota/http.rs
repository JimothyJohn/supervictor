//! The two OTA requests and the response-head parser that frames their
//! bodies. Same dialect as `network::http`: HTTP/1.1, `Connection: close`,
//! read to EOF. The head parser is deliberately strict where it matters —
//! duplicate or oversized `Content-Length`, chunked encoding — because the
//! body goes straight into a flash slot.

use crate::error::HttpError;
use heapless::String as HString;
use supervictor_common::firmware;

/// Most header bytes the device will buffer before giving up.
pub const MAX_HEAD_LEN: usize = 1024;

fn push<const N: usize>(buf: &mut HString<N>, s: &str) -> Result<(), HttpError> {
    buf.push_str(s).map_err(|_| HttpError::BufferOverflow)
}

fn push_u32<const N: usize>(buf: &mut HString<N>, mut n: u32) -> Result<(), HttpError> {
    let mut digits = [0u8; 10];
    let mut i = 0;
    if n == 0 {
        return push(buf, "0");
    }
    while n > 0 {
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        buf.push(digits[i] as char)
            .map_err(|_| HttpError::BufferOverflow)?;
    }
    Ok(())
}

/// `<api_path>/devices/<id>/firmware/<version>` with `API_PATH`'s
/// trailing slash folded (`/` -> ``, `/staging/` -> `/staging`).
fn firmware_path<const N: usize>(
    buf: &mut HString<N>,
    api_path: &str,
    device_id: &str,
    version: &str,
) -> Result<(), HttpError> {
    push(buf, api_path.trim_end_matches('/'))?;
    push(buf, "/devices/")?;
    push(buf, device_id)?;
    push(buf, "/firmware/")?;
    push(buf, version)
}

/// `GET …/firmware/{version}/manifest`.
pub fn manifest_request(
    host: &str,
    api_path: &str,
    device_id: &str,
    version: &str,
) -> Result<HString<320>, HttpError> {
    let mut r = HString::new();
    push(&mut r, "GET ")?;
    firmware_path(&mut r, api_path, device_id, version)?;
    push(&mut r, "/manifest HTTP/1.1\r\nHost: ")?;
    push(&mut r, host)?;
    push(
        &mut r,
        "\r\nUser-Agent: Uplink/0.1.0 (Platform; ESP32)\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
    )?;
    Ok(r)
}

/// `GET …/firmware/{version}` from byte `start` to the end, pinned with
/// `If-Match` to the manifest's digest so a re-upload mid-download is a
/// 412 rather than a splice.
pub fn image_request(
    host: &str,
    api_path: &str,
    device_id: &str,
    version: &str,
    start: u32,
    sha256_hex: &str,
) -> Result<HString<448>, HttpError> {
    let mut r = HString::new();
    push(&mut r, "GET ")?;
    firmware_path(&mut r, api_path, device_id, version)?;
    push(&mut r, " HTTP/1.1\r\nHost: ")?;
    push(&mut r, host)?;
    push(&mut r, "\r\nUser-Agent: Uplink/0.1.0 (Platform; ESP32)\r\nAccept: application/octet-stream\r\nRange: bytes=")?;
    push_u32(&mut r, start)?;
    push(&mut r, "-\r\nIf-Match: \"")?;
    push(&mut r, sha256_hex)?;
    push(&mut r, "\"\r\nConnection: close\r\n\r\n")?;
    Ok(r)
}

/// What the device needs from a response head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponseHead {
    /// Status code.
    pub status: u16,
    /// `Content-Length`, if present.
    pub content_length: Option<u32>,
    /// `Content-Range: bytes <start>-<end>/<total>` as `(start, total)`.
    pub content_range: Option<(u32, u32)>,
}

/// Why a head could not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadError {
    /// No blank line yet: read more (unless the buffer is full).
    Incomplete,
    /// Not `HTTP/1.x <3 digits>`, a header without a colon, or a value
    /// that does not parse.
    Malformed,
    /// Two `Content-Length` headers that disagree, or `Transfer-Encoding`
    /// (the device does not de-chunk a flash image).
    Unsupported,
    /// A length larger than any slot (or than `u32`).
    TooLarge,
}

/// Parse the head at the start of `buf`. Returns the head and how many
/// bytes it occupied (the body starts there). Header names are matched
/// case-insensitively; unknown headers are skipped.
pub fn parse_head(buf: &[u8]) -> Result<(ResponseHead, usize), HeadError> {
    let end = find_blank_line(buf).ok_or(HeadError::Incomplete)?;
    let head = core::str::from_utf8(&buf[..end]).map_err(|_| HeadError::Malformed)?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or(HeadError::Malformed)?;
    let status = parse_status_line(status_line)?;

    let mut content_length: Option<u32> = None;
    let mut content_range = None;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or(HeadError::Malformed)?;
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("content-length") {
            let n = parse_u32(value).ok_or(HeadError::Malformed)?;
            if n > firmware::SLOT_SIZE {
                return Err(HeadError::TooLarge);
            }
            match content_length {
                Some(prev) if prev != n => return Err(HeadError::Unsupported),
                _ => content_length = Some(n),
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(HeadError::Unsupported);
        } else if name.eq_ignore_ascii_case("content-range") {
            content_range = Some(parse_content_range(value)?);
        }
    }
    Ok((
        ResponseHead {
            status,
            content_length,
            content_range,
        },
        end + 4,
    ))
}

/// Offset of the `\r\n\r\n` that ends the head.
fn find_blank_line(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_status_line(line: &str) -> Result<u16, HeadError> {
    let mut parts = line.splitn(3, ' ');
    let version = parts.next().ok_or(HeadError::Malformed)?;
    if !(version == "HTTP/1.1" || version == "HTTP/1.0") {
        return Err(HeadError::Malformed);
    }
    let code = parts.next().ok_or(HeadError::Malformed)?;
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Err(HeadError::Malformed);
    }
    code.parse().map_err(|_| HeadError::Malformed)
}

fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 10 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `bytes <start>-<end>/<total>` -> `(start, total)`; `bytes */<total>`
/// (a 416) -> `(total, total)` so the caller sees "nothing left".
fn parse_content_range(value: &str) -> Result<(u32, u32), HeadError> {
    let rest = value.strip_prefix("bytes ").ok_or(HeadError::Malformed)?;
    let (range, total) = rest.split_once('/').ok_or(HeadError::Malformed)?;
    let total = parse_u32(total.trim()).ok_or(HeadError::Malformed)?;
    if total > firmware::SLOT_SIZE {
        return Err(HeadError::TooLarge);
    }
    if range.trim() == "*" {
        return Ok((total, total));
    }
    let (start, end) = range.split_once('-').ok_or(HeadError::Malformed)?;
    let start = parse_u32(start.trim()).ok_or(HeadError::Malformed)?;
    let end = parse_u32(end.trim()).ok_or(HeadError::Malformed)?;
    if start > end || end >= total {
        return Err(HeadError::Malformed);
    }
    Ok((start, total))
}
