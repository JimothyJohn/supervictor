extern crate std;

use std::string::String;

use super::page::*;
use crate::settings::{HostName, Settings, HOST_MAX, NAME_MAX, PASSWORD_MAX, SSID_MAX};
use proptest::prelude::*;

#[test]
fn routes() {
    assert_eq!(route("GET", "/"), Route::Form);
    assert_eq!(route("GET", "/index.html"), Route::Form);
    assert_eq!(route("GET", "/?x=1"), Route::Form);
    assert_eq!(route("POST", "/configure"), Route::Configure);
    assert_eq!(route("GET", "/configure"), Route::Redirect);
    assert_eq!(route("GET", "/generate_204"), Route::Redirect);
    assert_eq!(route("GET", "/hotspot-detect.html"), Route::Redirect);
    assert_eq!(route("GET", "/connecttest.txt"), Route::Redirect);
    assert_eq!(route("POST", "/"), Route::Redirect);
    assert_eq!(route("", ""), Route::Redirect);
}

#[test]
fn content_length_is_case_insensitive_and_defaults_to_zero() {
    assert_eq!(
        content_length("POST /configure HTTP/1.1\r\nHost: x\r\nContent-Length: 42\r\n"),
        42
    );
    assert_eq!(content_length("POST / HTTP/1.1\r\ncontent-length:7\r\n"), 7);
    assert_eq!(content_length("GET / HTTP/1.1\r\nHost: x\r\n"), 0);
    assert_eq!(
        content_length("POST / HTTP/1.1\r\nContent-Length: lots\r\n"),
        0
    );
    assert_eq!(
        content_length("POST / HTTP/1.1\r\nContent-Length: -1\r\n"),
        0
    );
}

#[test]
fn form_decodes_percent_and_plus() {
    let s = parse_form("ssid=shop+floor&password=p%40ss%2Bword%20x&host=supervictor.advin.io&device_name=c3-wifi-01").unwrap();
    assert_eq!(s.ssid.as_str(), "shop floor");
    assert_eq!(s.password.as_str(), "p@ss+word x");
    assert_eq!(s.host.as_str(), "supervictor.advin.io");
    assert_eq!(s.device_name.as_str(), "c3-wifi-01");
    assert_eq!(s.uplink_interval_s, 0);
    assert_eq!(s.threshold_ma, 0);
}

#[test]
fn form_field_order_and_extras_do_not_matter() {
    let s = parse_form("device_name=n&host=h&junk=1&ssid=s&password=&&").unwrap();
    assert_eq!(
        (
            s.ssid.as_str(),
            s.password.as_str(),
            s.host.as_str(),
            s.device_name.as_str()
        ),
        ("s", "", "h", "n")
    );
}

#[test]
fn form_rejects_missing_or_bad_fields() {
    assert_eq!(
        parse_form("password=x&host=h&device_name=n"),
        Err(FormError::Field("ssid"))
    );
    assert_eq!(
        parse_form("ssid=&host=h&device_name=n"),
        Err(FormError::Field("ssid"))
    );
    assert_eq!(
        parse_form("ssid=s&device_name=n"),
        Err(FormError::Field("host"))
    );
    assert_eq!(
        parse_form("ssid=s&host=&device_name=n"),
        Err(FormError::Field("host"))
    );
    assert_eq!(
        parse_form("ssid=s&host=h"),
        Err(FormError::Field("device_name"))
    );
    // Host must look like a hostname; a URL, a space or a NUL is not one.
    assert_eq!(
        parse_form("ssid=s&host=https%3A%2F%2Fx&device_name=n"),
        Err(FormError::Field("host"))
    );
    assert_eq!(
        parse_form("ssid=s&host=a+b&device_name=n"),
        Err(FormError::Field("host"))
    );
    assert_eq!(
        parse_form("ssid=s&host=a%00b&device_name=n"),
        Err(FormError::Field("host"))
    );
    // Device names are what goes in a URL path on the endpoint.
    assert_eq!(
        parse_form("ssid=s&host=h&device_name=a%2Fb"),
        Err(FormError::Field("device_name"))
    );
    assert_eq!(
        parse_form("ssid=s&host=h&device_name=a+b"),
        Err(FormError::Field("device_name"))
    );
    // Bad escapes and over-long values.
    assert_eq!(
        parse_form("ssid=%zz&host=h&device_name=n"),
        Err(FormError::Field("ssid"))
    );
    assert_eq!(
        parse_form("ssid=%4&host=h&device_name=n"),
        Err(FormError::Field("ssid"))
    );
    let long = "s".repeat(SSID_MAX + 1);
    assert_eq!(
        parse_form(&std::format!("ssid={long}&host=h&device_name=n")),
        Err(FormError::Field("ssid"))
    );
    let long = "p".repeat(PASSWORD_MAX + 1);
    assert_eq!(
        parse_form(&std::format!("ssid=s&password={long}&host=h&device_name=n")),
        Err(FormError::Field("password"))
    );
    let long = "h".repeat(HOST_MAX + 1);
    assert_eq!(
        parse_form(&std::format!("ssid=s&host={long}&device_name=n")),
        Err(FormError::Field("host"))
    );
    let long = "n".repeat(NAME_MAX + 1);
    assert_eq!(
        parse_form(&std::format!("ssid=s&host=h&device_name={long}")),
        Err(FormError::Field("device_name"))
    );
    // Invalid UTF-8 after decoding.
    assert_eq!(
        parse_form("ssid=%ff%fe&host=h&device_name=n"),
        Err(FormError::Field("ssid"))
    );
}

#[test]
fn form_accepts_maximum_lengths_and_utf8_ssid() {
    let body = std::format!(
        "ssid={}&password={}&host={}&device_name={}",
        "s".repeat(SSID_MAX),
        "p".repeat(PASSWORD_MAX),
        "h".repeat(HOST_MAX),
        "n".repeat(NAME_MAX)
    );
    assert!(parse_form(&body).is_ok());
    let s = parse_form("ssid=caf%C3%A9&host=h&device_name=n").unwrap();
    assert_eq!(s.ssid.as_str(), "café");
}

fn current() -> Settings {
    Settings {
        ssid: "old-net".try_into().unwrap(),
        password: "secret-secret".try_into().unwrap(),
        host: HostName::new("supervictor.advin.io").unwrap(),
        device_name: "c3-wifi-01".try_into().unwrap(),
        uplink_interval_s: 0,
        threshold_ma: 0,
        config_version: 0,
    }
}

#[test]
fn form_page_prefills_everything_but_the_password_and_escapes() {
    let mut cur = current();
    cur.ssid = "a\"b<c>&d".try_into().unwrap();
    let page = form_page("c3-wifi-01", "0.3.0", &cur, Outcome::Fresh);
    let page = page.as_str();
    assert!(
        page.contains("value=\"a&quot;b&lt;c&gt;&amp;d\""),
        "ssid escaped"
    );
    assert!(page.contains("value=\"supervictor.advin.io\""));
    assert!(page.contains("name=\"device_name\"") && page.contains("value=\"c3-wifi-01\""));
    assert!(!page.contains("secret-secret"), "password never echoed");
    assert!(page.contains("name=\"password\" type=\"password\""));
    assert!(page.contains("action=\"/configure\""));
    assert!(
        !page.contains("class=\"err\""),
        "no error box on a fresh form"
    );
    assert!(page.contains("firmware 0.3.0"));
    assert!(page.contains("maxlength=\"32\""));
}

#[test]
fn form_page_shows_the_outcome() {
    for o in [
        Outcome::Join,
        Outcome::Dhcp,
        Outcome::Dns,
        Outcome::Tcp,
        Outcome::Tls,
        Outcome::Flash,
        Outcome::BadForm("host"),
    ] {
        let page = form_page("d", "v", &current(), o);
        assert!(page.contains("class=\"err\""), "{o:?}");
    }
    assert!(form_page("d", "v", &current(), Outcome::Tls).contains("not saved"));
}

#[test]
fn pages_fit_their_buffers_at_maximum_field_lengths() {
    let max = Settings {
        ssid: std::string::String::from_utf8(std::vec![b'&'; SSID_MAX])
            .unwrap()
            .as_str()
            .try_into()
            .unwrap(),
        password: "p".repeat(PASSWORD_MAX).as_str().try_into().unwrap(),
        host: HostName::new(&"h".repeat(HOST_MAX)).unwrap(),
        device_name: "<".repeat(NAME_MAX).as_str().try_into().unwrap(),
        uplink_interval_s: 0,
        threshold_ma: 0,
        config_version: 0,
    };
    let name = "<".repeat(NAME_MAX);
    let page = form_page(&name, "0.10.100", &max, Outcome::Tls);
    assert!(
        page.len() < PAGE_MAX - 64,
        "form page {} of {}",
        page.len(),
        PAGE_MAX
    );
    assert!(page.ends_with("</html>"), "truncated");
    let saved = saved_page(&name, &max);
    assert!(saved.ends_with("</html>"), "truncated");
    let resp = html_response(page.as_str());
    assert!(resp.ends_with("</html>"));
    let hdr_end = resp.find("\r\n\r\n").unwrap();
    assert_eq!(&resp[hdr_end + 4..], page.as_str());
    let cl: usize = resp
        .lines()
        .find_map(|l| l.strip_prefix("Content-Length: "))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(cl, page.len());
}

#[test]
fn redirect_goes_to_the_portal() {
    let r = redirect_response();
    assert!(r.starts_with("HTTP/1.0 302 Found\r\nLocation: http://192.168.4.1/\r\n"));
    assert!(r.ends_with("\r\n\r\n"));
}

#[test]
fn ap_ssid_is_prefixed_and_bounded() {
    assert_eq!(ap_ssid("c3-wifi-01").as_str(), "supervictor-c3-wifi-01");
    let long = ap_ssid(&"x".repeat(NAME_MAX));
    assert_eq!(long.len(), SSID_MAX);
    assert!(long.starts_with("supervictor-"));
    assert_eq!(ap_ssid("").as_str(), "supervictor-");
}

proptest! {
    #[test]
    fn any_body_parses_without_panicking(body in "[ -~]{0,600}") {
        let _ = parse_form(&body);
    }

    #[test]
    fn decoded_values_round_trip_through_the_form(
        ssid in "[a-zA-Z0-9 _.!@#$%^&*()=+/?,'\"-]{1,32}",
        password in "[ -~]{0,64}",
        host in "[a-z0-9.-]{1,64}",
        name in "[a-zA-Z0-9_-]{1,64}",
    ) {
        fn enc(s: &str) -> String {
            let mut out = String::new();
            for b in s.bytes() {
                match b {
                    b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(b as char),
                    b' ' => out.push('+'),
                    _ => out.push_str(&std::format!("%{b:02X}")),
                }
            }
            out
        }
        let body = std::format!("ssid={}&password={}&host={}&device_name={}", enc(&ssid), enc(&password), enc(&host), enc(&name));
        let s = parse_form(&body).unwrap();
        prop_assert_eq!(s.ssid.as_str(), ssid.as_str());
        prop_assert_eq!(s.password.as_str(), password.as_str());
        prop_assert_eq!(s.host.as_str(), host.as_str());
        prop_assert_eq!(s.device_name.as_str(), name.as_str());
        // And the page built from it stays well-formed.
        let page = form_page(&name, "v", &s, Outcome::Join);
        prop_assert!(page.ends_with("</html>"));
    }
}

#[test]
fn checking_page_reloads_itself_and_explains_the_drop() {
    let page = checking_page("c3-wifi-01", &current());
    let page = page.as_str();
    assert!(page.contains(&std::format!(
        "http-equiv=\"refresh\" content=\"{CHECKING_REFRESH_S};url=/\""
    )));
    assert!(page.contains("joining <b>old-net</b>"), "{page}");
    assert!(page.contains("<b>supervictor.advin.io</b>"), "{page}");
    assert!(page.contains("<b>supervictor-c3-wifi-01</b>"), "{page}");
    assert!(page.contains("Nothing is saved unless both work"));
    assert!(!page.contains("secret-secret"), "password never echoed");
    assert!(page.ends_with("</html>"));
}

#[test]
fn checking_and_saved_pages_escape_and_fit_at_maximum_lengths() {
    let max = Settings {
        ssid: "<".repeat(SSID_MAX).as_str().try_into().unwrap(),
        password: "p".repeat(PASSWORD_MAX).as_str().try_into().unwrap(),
        host: HostName::new(&"h".repeat(HOST_MAX)).unwrap(),
        device_name: "&".repeat(NAME_MAX).as_str().try_into().unwrap(),
        uplink_interval_s: 0,
        threshold_ma: 0,
        config_version: 0,
    };
    let name = "\"".repeat(NAME_MAX);
    for page in [checking_page(&name, &max), saved_page(&name, &max)] {
        assert!(page.ends_with("</html>"), "truncated at {}", page.len());
        assert!(page.len() < PAGE_MAX - 64, "{} of {}", page.len(), PAGE_MAX);
        assert!(!page.contains("<<"), "ssid escaped");
    }
    assert!(saved_page("d", &current()).contains("about a minute"));
}
