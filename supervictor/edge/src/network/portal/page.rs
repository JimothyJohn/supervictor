//! The provisioning page: one form, four fields, served from the board's
//! own access point. Pure functions — routing, form decoding, HTML
//! building — host-tested; the task that serves them is in `mod.rs`.

use heapless::String as HString;

use crate::settings::{HostName, Settings, HOST_MAX, NAME_MAX, PASSWORD_MAX, SSID_MAX};

/// Where the portal lives on its own network.
pub const PORTAL_URL: &str = "http://192.168.4.1/";

/// Longest request the portal reads: the headers a phone sends plus a
/// form of four fields at their maximum, percent-encoded.
pub const REQUEST_MAX: usize = 2048;
/// Longest response page the portal builds.
pub const PAGE_MAX: usize = 4096;

/// What a request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// `GET /`: the form.
    Form,
    /// `POST /configure`: a submission.
    Configure,
    /// Anything else — captive-portal probes above all — goes to the form.
    Redirect,
}

/// Decide from the request line.
pub fn route(method: &str, path: &str) -> Route {
    let path = path.split('?').next().unwrap_or("");
    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => Route::Form,
        ("POST", "/configure") => Route::Configure,
        _ => Route::Redirect,
    }
}

/// `Content-Length` of a request, 0 when absent or unparsable.
pub fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())
                .flatten()
        })
        .unwrap_or(0)
}

/// What the form said, as settings to try.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormError {
    /// A field is missing, too long, or not UTF-8 after decoding.
    Field(&'static str),
}

/// Decode an `application/x-www-form-urlencoded` body with `ssid`,
/// `password`, `host` and `device_name` into settings. Unknown fields are
/// ignored; `password` may be empty (open network); the rest may not.
pub fn parse_form(body: &str) -> Result<Settings, FormError> {
    let mut ssid: Option<HString<SSID_MAX>> = None;
    let mut password: Option<HString<PASSWORD_MAX>> = None;
    let mut host: Option<HString<HOST_MAX>> = None;
    let mut name: Option<HString<NAME_MAX>> = None;
    for pair in body.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let k = decode::<32>(k).map_err(|_| FormError::Field("key"))?;
        match k.as_str() {
            "ssid" => ssid = Some(decode(v).map_err(|_| FormError::Field("ssid"))?),
            "password" => password = Some(decode(v).map_err(|_| FormError::Field("password"))?),
            "host" => host = Some(decode(v).map_err(|_| FormError::Field("host"))?),
            "device_name" => name = Some(decode(v).map_err(|_| FormError::Field("device_name"))?),
            _ => {}
        }
    }
    let ssid = ssid
        .filter(|s| !s.is_empty())
        .ok_or(FormError::Field("ssid"))?;
    let host = host
        .filter(|s| !s.is_empty() && s.chars().all(is_host_char))
        .ok_or(FormError::Field("host"))?;
    let device_name = name
        .filter(|s| !s.is_empty() && s.chars().all(is_name_char))
        .ok_or(FormError::Field("device_name"))?;
    Ok(Settings {
        ssid,
        password: password.unwrap_or_default(),
        host: HostName::new(host.as_str()).ok_or(FormError::Field("host"))?,
        device_name,
        uplink_interval_s: 0,
        threshold_ma: 0,
        config_version: 0,
    })
}

fn is_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == '-'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Percent-decode one form value (`+` is a space). Errors on a bad escape,
/// a value longer than `N` bytes, or bytes that are not UTF-8.
fn decode<const N: usize>(v: &str) -> Result<HString<N>, ()> {
    let mut bytes = heapless::Vec::<u8, N>::new();
    let src = v.as_bytes();
    let mut i = 0;
    while i < src.len() {
        let b = match src[i] {
            b'+' => b' ',
            b'%' => {
                let hi = hex(*src.get(i + 1).ok_or(())?)?;
                let lo = hex(*src.get(i + 2).ok_or(())?)?;
                i += 2;
                hi << 4 | lo
            }
            b => b,
        };
        bytes.push(b).map_err(|_| ())?;
        i += 1;
    }
    let s = core::str::from_utf8(&bytes).map_err(|_| ())?;
    HString::try_from(s).map_err(|_| ())
}

fn hex(b: u8) -> Result<u8, ()> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(()),
    }
}

/// Why a submission was not saved; shown on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing tried yet: the plain form.
    Fresh,
    /// The form was incomplete.
    BadForm(&'static str),
    /// Could not associate with the SSID (wrong password, out of range).
    Join,
    /// Associated, but no DHCP lease arrived.
    Dhcp,
    /// The host did not resolve from that network.
    Dns,
    /// TCP to port 443 failed.
    Tcp,
    /// The TLS handshake failed: the host's certificate is not one the
    /// board's CA trusts, or the board's own certificate was refused.
    Tls,
    /// Everything worked but the record could not be written.
    Flash,
}

impl Outcome {
    fn message(self) -> &'static str {
        match self {
            Outcome::Fresh => "",
            Outcome::BadForm(f) => match f {
                "ssid" => "A network name is required.",
                "host" => "The host must be a hostname: letters, digits, dots and dashes.",
                "device_name" => "The device name may use letters, digits, dashes and underscores.",
                _ => "The form could not be read.",
            },
            Outcome::Join => "Could not join that network. Check the name and password, and that the board is in range.",
            Outcome::Dhcp => "Joined the network but got no IP address from it.",
            Outcome::Dns => "On the network, but that host name did not resolve.",
            Outcome::Tcp => "Resolved the host, but nothing answered on port 443.",
            Outcome::Tls => "Reached the host, but the TLS handshake failed: its certificate is not one this board trusts, or the board's certificate was refused. The settings were not saved.",
            Outcome::Flash => "Everything checked out, but the settings could not be written to flash.",
        }
    }
}

const STYLE: &str = "<style>body{margin:0;padding:24px 16px;background:#141414;color:#e8e8e8;font:16px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace}main{max-width:420px;margin:0 auto}h1{font-size:18px;letter-spacing:.08em;text-transform:uppercase;color:#ff8a1f;margin:0 0 4px}p{margin:0 0 16px;color:#aaa}label{display:block;font-size:12px;letter-spacing:.12em;text-transform:uppercase;color:#888;margin:14px 0 4px}input{width:100%;box-sizing:border-box;padding:10px;border:1px solid #333;border-radius:6px;background:#1c1c1c;color:#e8e8e8;font:inherit}button{margin-top:20px;width:100%;padding:12px;border:0;border-radius:6px;background:#ff8a1f;color:#111;font:inherit;font-weight:700;letter-spacing:.08em;text-transform:uppercase}.err{border-left:3px solid #ff4d4d;padding:8px 12px;margin:0 0 8px;color:#ffb3b3;background:#231515}.ok{border-left:3px solid #35c46a;padding:8px 12px;color:#b8f0cc;background:#132018}small{color:#666}</style>";

fn push_escaped<const N: usize>(out: &mut HString<N>, s: &str) {
    for c in s.chars() {
        let _ = match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        };
    }
}

fn field<const N: usize>(
    out: &mut HString<N>,
    label: &str,
    name: &str,
    kind: &str,
    value: &str,
    max: usize,
    extra: &str,
) {
    let _ = out.push_str("<label for=\"");
    let _ = out.push_str(name);
    let _ = out.push_str("\">");
    let _ = out.push_str(label);
    let _ = out.push_str("</label><input id=\"");
    let _ = out.push_str(name);
    let _ = out.push_str("\" name=\"");
    let _ = out.push_str(name);
    let _ = out.push_str("\" type=\"");
    let _ = out.push_str(kind);
    let _ = out.push_str("\" maxlength=\"");
    push_num(out, max);
    let _ = out.push_str("\" value=\"");
    push_escaped(out, value);
    let _ = out.push_str("\" ");
    let _ = out.push_str(extra);
    let _ = out.push_str(">");
}

fn push_num<const N: usize>(out: &mut HString<N>, n: usize) {
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    let mut n = n;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let _ = out.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("0"));
}

/// The form, prefilled with `current` (the password is never echoed),
/// with the previous attempt's outcome above it.
pub fn form_page(
    device_name: &str,
    fw: &str,
    current: &Settings,
    outcome: Outcome,
) -> HString<PAGE_MAX> {
    let mut out = HString::new();
    let _ = out.push_str("<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>supervictor ");
    push_escaped(&mut out, device_name);
    let _ = out.push_str("</title>");
    let _ = out.push_str(STYLE);
    let _ = out.push_str("</head><body><main><h1>supervictor</h1><p>");
    push_escaped(&mut out, device_name);
    let _ = out.push_str(" &middot; firmware ");
    push_escaped(&mut out, fw);
    let _ = out.push_str("</p>");
    if outcome != Outcome::Fresh {
        let _ = out.push_str("<div class=\"err\">");
        let _ = out.push_str(outcome.message());
        let _ = out.push_str("</div>");
    }
    let _ = out.push_str("<form method=\"post\" action=\"/configure\" autocomplete=\"off\">");
    field(
        &mut out,
        "Network",
        "ssid",
        "text",
        current.ssid.as_str(),
        SSID_MAX,
        "required autocapitalize=\"none\"",
    );
    field(
        &mut out,
        "Password",
        "password",
        "password",
        "",
        PASSWORD_MAX,
        "",
    );
    field(
        &mut out,
        "Endpoint host",
        "host",
        "text",
        current.host.as_str(),
        HOST_MAX,
        "required inputmode=\"url\" autocapitalize=\"none\"",
    );
    field(
        &mut out,
        "Device name",
        "device_name",
        "text",
        current.device_name.as_str(),
        NAME_MAX,
        "required autocapitalize=\"none\"",
    );
    let _ = out.push_str("<small>The device name is the identity its certificate was issued for; change it only if the certificate was.</small>");
    let _ = out.push_str("<button type=\"submit\">Join and verify</button></form><p><small>The board joins the network and completes a TLS handshake with the host before anything is saved.</small></p></main></body></html>");
    out
}

/// The page shown once everything worked and the record is written.
pub fn saved_page(device_name: &str, saved: &Settings) -> HString<PAGE_MAX> {
    let mut out = HString::new();
    let _ = out.push_str("<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>supervictor ");
    push_escaped(&mut out, device_name);
    let _ = out.push_str("</title>");
    let _ = out.push_str(STYLE);
    let _ = out.push_str("</head><body><main><h1>supervictor</h1><div class=\"ok\">Joined <b>");
    push_escaped(&mut out, saved.ssid.as_str());
    let _ = out.push_str("</b> and reached <b>");
    push_escaped(&mut out, saved.host.as_str());
    let _ = out.push_str("</b> over TLS. Settings saved. The board is rebooting as <b>");
    push_escaped(&mut out, saved.device_name.as_str());
    let _ = out.push_str("</b>; this network goes away now.</div></main></body></html>");
    out
}

/// A complete HTTP/1.0 response around `body`.
pub fn html_response(body: &str) -> HString<{ PAGE_MAX + 128 }> {
    let mut out = HString::new();
    let _ = out.push_str("HTTP/1.0 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: ");
    push_num(&mut out, body.len());
    let _ = out.push_str("\r\n\r\n");
    let _ = out.push_str(body);
    out
}

/// The redirect every other path gets; captive-portal detectors follow
/// it and show the form.
pub fn redirect_response() -> HString<256> {
    let mut out = HString::new();
    let _ = out.push_str("HTTP/1.0 302 Found\r\nLocation: ");
    let _ = out.push_str(PORTAL_URL);
    let _ = out
        .push_str("\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    out
}

/// The access point's name: `supervictor-<device name>`, cut to the
/// 802.11 limit.
pub fn ap_ssid(device_name: &str) -> HString<SSID_MAX> {
    let mut s: HString<SSID_MAX> = HString::new();
    let _ = s.push_str("supervictor-");
    for c in device_name.chars() {
        if s.push(c).is_err() {
            break;
        }
    }
    s
}
