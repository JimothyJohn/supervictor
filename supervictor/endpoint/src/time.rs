//! Minimal UTC RFC 3339 timestamp formatting.
//!
//! Hand-rolled replacement for `chrono::Utc::now().to_rfc3339()` — the only
//! chrono surface this crate used. Output is byte-identical to chrono's
//! (`+00:00` offset, 0/3/6/9 fractional digits via trailing-zero trimming).
//! Date conversion follows Howard Hinnant's `civil_from_days` algorithm
//! (https://howardhinnant.github.io/date_algorithms.html), the same one
//! chrono uses internally. Reintroduce chrono if we ever need parsing,
//! arithmetic, or non-UTC time zones.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current UTC time as an RFC 3339 string, e.g. `2026-07-10T18:22:03.123456+00:00`.
pub fn now_rfc3339() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_rfc3339(now.as_secs(), now.subsec_nanos())
}

/// Parse an RFC 3339 UTC timestamp back to Unix seconds (inverse of
/// [`format_rfc3339`], fractional seconds truncated). Accepts `Z` or
/// `+00:00` offsets and the `T`/space separator; returns `None` for
/// malformed input, non-UTC offsets, or pre-1970 dates. Uses Hinnant's
/// `days_from_civil` (the counterpart of the formatter's algorithm).
pub fn parse_rfc3339_unix(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || (b[10] != b'T' && b[10] != b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    let hour: u64 = s.get(11..13)?.parse().ok()?;
    let min: u64 = s.get(14..16)?.parse().ok()?;
    let sec: u64 = s.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 60 {
        return None;
    }

    // Anything after the seconds must be an optional fraction then a UTC offset.
    let rest = &s[19..];
    let (frac, off) = match rest.find(['Z', '+', '-']) {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let frac_ok = frac.is_empty()
        || (frac.starts_with('.')
            && frac.len() > 1
            && frac[1..].bytes().all(|c| c.is_ascii_digit()));
    let off_ok = off.is_empty() || off == "Z" || off == "+00:00" || off == "-00:00";
    if !frac_ok || !off_ok {
        return None;
    }

    // days_from_civil (Hinnant): (y, m, d) -> days since 1970-01-01
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    if days < 0 {
        return None;
    }

    Some(days as u64 * 86_400 + hour * 3_600 + min * 60 + sec)
}

/// Parse an RFC 3339 UTC timestamp to Unix **milliseconds**: the seconds
/// from [`parse_rfc3339_unix`] plus the first three fraction digits (a
/// shorter fraction is right-padded, a longer one truncated). The live
/// views need sub-second placement; everything else keeps whole seconds.
pub fn parse_rfc3339_unix_ms(s: &str) -> Option<u64> {
    let secs = parse_rfc3339_unix(s)?;
    let rest = s.get(19..)?;
    let millis = match rest.strip_prefix('.') {
        Some(frac) => {
            let digits: Vec<u8> = frac
                .bytes()
                .take_while(u8::is_ascii_digit)
                .take(3)
                .map(|c| c - b'0')
                .collect();
            let mut ms = 0u64;
            for i in 0..3 {
                ms = ms * 10 + u64::from(digits.get(i).copied().unwrap_or(0));
            }
            ms
        }
        None => 0,
    };
    secs.checked_mul(1_000)?.checked_add(millis)
}

/// Current Unix time in milliseconds.
pub fn now_unix_ms() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    u64::try_from(now.as_millis()).unwrap_or(u64::MAX)
}

/// Format a Unix timestamp (seconds + nanoseconds since epoch) as RFC 3339 UTC.
pub fn format_rfc3339(unix_secs: u64, nanos: u32) -> String {
    let days = unix_secs / 86_400;
    let secs_of_day = unix_secs % 86_400;
    let (hour, min, sec) = (
        secs_of_day / 3_600,
        (secs_of_day % 3_600) / 60,
        secs_of_day % 60,
    );

    // civil_from_days (Hinnant): days since 1970-01-01 -> (y, m, d).
    // Shift epoch to 0000-03-01 so leap days land at the end of the cycle.
    let z = days as i64 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097; // day of 400-year era
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // year of era
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of March-based year
    let mp = (5 * doy + 2) / 153; // March-based month
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);

    // Fraction digits match chrono's SecondsFormat::AutoSi: trim trailing
    // zeros in whole groups of three.
    let mut out = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}");
    if nanos != 0 {
        if nanos.is_multiple_of(1_000_000) {
            out.push_str(&format!(".{:03}", nanos / 1_000_000));
        } else if nanos.is_multiple_of(1_000) {
            out.push_str(&format!(".{:06}", nanos / 1_000));
        } else {
            out.push_str(&format!(".{nanos:09}"));
        }
    }
    out.push_str("+00:00");
    out
}
