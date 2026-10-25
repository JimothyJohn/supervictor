use supervictor_endpoint::time::{
    format_rfc3339, now_rfc3339, now_unix_ms, parse_rfc3339_unix, parse_rfc3339_unix_ms,
};

/// Reference vectors generated with `chrono 0.4.45`'s
/// `DateTime::<Utc>::from_timestamp(secs, nanos).to_rfc3339()` before the
/// dep was removed. `format_rfc3339` must stay byte-identical to these.
const CHRONO_VECTORS: &[(u64, u32, &str)] = &[
    (0, 0, "1970-01-01T00:00:00+00:00"),
    (1, 1, "1970-01-01T00:00:01.000000001+00:00"),
    // leap day, year divisible by 400
    (951_782_400, 0, "2000-02-29T00:00:00+00:00"),
    // leap day with millisecond fraction
    (1_709_164_800, 500_000_000, "2024-02-29T00:00:00.500+00:00"),
    (
        1_719_791_999,
        999_999_999,
        "2024-06-30T23:59:59.999999999+00:00",
    ),
    // year-end boundary
    (1_735_689_599, 0, "2024-12-31T23:59:59+00:00"),
    (1_735_689_600, 123_000_000, "2025-01-01T00:00:00.123+00:00"),
    // microsecond vs nanosecond precision selection
    (
        1_752_192_000,
        123_456_000,
        "2025-07-11T00:00:00.123456+00:00",
    ),
    (
        1_752_192_000,
        123_456_789,
        "2025-07-11T00:00:00.123456789+00:00",
    ),
    (4_102_444_800, 0, "2100-01-01T00:00:00+00:00"),
    // 2100 is NOT a leap year (divisible by 100, not 400): +28d from Feb 1 lands on Mar 1
    (4_107_542_400, 42_000_000, "2100-03-01T00:00:00.042+00:00"),
    (
        253_402_300_799,
        999_999_999,
        "9999-12-31T23:59:59.999999999+00:00",
    ),
    (86_399, 1_000, "1970-01-01T23:59:59.000001+00:00"),
    (2_147_483_647, 0, "2038-01-19T03:14:07+00:00"),
];

#[test]
fn matches_chrono_reference_vectors() {
    for (secs, nanos, expected) in CHRONO_VECTORS {
        assert_eq!(
            format_rfc3339(*secs, *nanos),
            *expected,
            "mismatch for ({secs}, {nanos})"
        );
    }
}

/// Every day from 1970 through 2106 must produce a valid calendar date and
/// midnight must roll over exactly one day at a time (catches off-by-one and
/// leap-year drift anywhere in the range, not just at hand-picked vectors).
#[test]
fn day_rollover_is_continuous_for_137_years() {
    let mut prev_date = String::new();
    for day in 0u64..50_000 {
        let ts = format_rfc3339(day * 86_400, 0);
        let (date, rest) = ts.split_at(10);
        assert_eq!(rest, "T00:00:00+00:00", "bad time part at day {day}");
        assert!(
            date > prev_date.as_str(),
            "date did not advance at day {day}"
        );
        // month/day stay in calendar range
        let month: u32 = date[5..7].parse().unwrap();
        let dom: u32 = date[8..10].parse().unwrap();
        assert!((1..=12).contains(&month), "bad month at day {day}: {date}");
        assert!((1..=31).contains(&dom), "bad day at day {day}: {date}");
        prev_date = ts;
    }
}

/// Feb 29 appears exactly on leap years within 2000..2104.
#[test]
fn leap_days_land_on_leap_years_only() {
    let mut leap_years = Vec::new();
    for day in 10_957u64..49_000 {
        // 2000-01-01 onward
        let ts = format_rfc3339(day * 86_400, 0);
        if &ts[5..10] == "02-29" {
            leap_years.push(ts[0..4].parse::<u32>().unwrap());
        }
    }
    for y in &leap_years {
        assert!(
            y % 4 == 0 && (y % 100 != 0 || y % 400 == 0),
            "Feb 29 on non-leap year {y}"
        );
    }
    assert!(leap_years.contains(&2000), "2000 must be a leap year");
    assert!(leap_years.contains(&2024), "2024 must be a leap year");
    assert!(!leap_years.contains(&2100), "2100 must not be a leap year");
}

// ── parse_rfc3339_unix (inverse) ─────────────────────────────────────

#[test]
fn parse_inverts_format_across_170_years() {
    // Every ~7h step from 1970 through ~2140, plus odd offsets to hit
    // non-midnight times. Fraction is truncated by design.
    let mut t: u64 = 0;
    while t < 5_400_000_000 {
        let formatted = format_rfc3339(t, 123_000_000);
        let parsed = parse_rfc3339_unix(&formatted)
            .unwrap_or_else(|| panic!("failed to parse own output: {formatted}"));
        assert_eq!(parsed, t, "round-trip mismatch for {formatted}");
        t += 25_247; // ~7h, coprime-ish step to spread across days
    }
}

#[test]
fn parse_accepts_z_suffix_and_space_separator() {
    assert_eq!(
        parse_rfc3339_unix("2025-01-01T00:00:00Z"),
        Some(1_735_689_600)
    );
    assert_eq!(
        parse_rfc3339_unix("2025-01-01 00:00:00+00:00"),
        Some(1_735_689_600)
    );
    assert_eq!(parse_rfc3339_unix("1970-01-01T00:00:00Z"), Some(0));
}

#[test]
fn parse_rejects_malformed_and_non_utc() {
    for bad in [
        "",
        "not a date",
        "2025-13-01T00:00:00Z",      // month 13
        "2025-00-10T00:00:00Z",      // month 0
        "2025-01-32T00:00:00Z",      // day 32
        "2025-01-01T24:00:00Z",      // hour 24
        "2025-01-01T00:60:00Z",      // minute 60
        "2025-01-01T00:00:00+05:00", // non-UTC offset
        "2025-01-01T00:00:00.abcZ",  // junk fraction
        "2025-01-01T00:00:00junk",   // trailing garbage
        "1969-12-31T23:59:59Z",      // pre-epoch
        "2025-01-01",                // date only
    ] {
        assert_eq!(parse_rfc3339_unix(bad), None, "should reject {bad:?}");
    }
}

#[test]
fn now_has_rfc3339_shape() {
    let ts = now_rfc3339();
    assert!(ts.ends_with("+00:00"), "missing UTC offset: {ts}");
    assert_eq!(&ts[4..5], "-");
    assert_eq!(&ts[10..11], "T");
    // sanity: we're past 2026 and before year 3000
    let year: u32 = ts[0..4].parse().unwrap();
    assert!((2026..3000).contains(&year), "implausible year: {ts}");
}

/// Millisecond parse: whole seconds from the shared parser, the fraction
/// padded or truncated to three digits, and `now_rfc3339()` round-trips
/// to within a millisecond of `now_unix_ms()`.
#[test]
fn parse_unix_ms_reads_the_fraction() {
    assert_eq!(parse_rfc3339_unix_ms("1970-01-01T00:00:00+00:00"), Some(0));
    assert_eq!(parse_rfc3339_unix_ms("1970-01-01T00:00:01Z"), Some(1_000));
    assert_eq!(parse_rfc3339_unix_ms("1970-01-01T00:00:01.5Z"), Some(1_500));
    assert_eq!(
        parse_rfc3339_unix_ms("1970-01-01T00:00:01.25+00:00"),
        Some(1_250)
    );
    assert_eq!(
        parse_rfc3339_unix_ms("1970-01-01T00:00:01.123+00:00"),
        Some(1_123)
    );
    assert_eq!(
        parse_rfc3339_unix_ms("1970-01-01T00:00:01.123456+00:00"),
        Some(1_123)
    );
    assert_eq!(
        parse_rfc3339_unix_ms("1970-01-01T00:00:01.999999999Z"),
        Some(1_999)
    );
    for (secs, nanos, text) in CHRONO_VECTORS {
        assert_eq!(
            parse_rfc3339_unix_ms(text),
            Some(secs * 1_000 + u64::from(nanos / 1_000_000)),
            "{text}"
        );
    }
    for bad in [
        "",
        "garbage",
        "1970-01-01T00:00:01.Z",
        "1970-01-01T00:00:01.x5Z",
        "1970-01-01T00:00:01.5+01:00",
    ] {
        assert_eq!(parse_rfc3339_unix_ms(bad), None, "{bad:?}");
    }
    let before = now_unix_ms();
    let stamped = parse_rfc3339_unix_ms(&now_rfc3339()).unwrap();
    let after = now_unix_ms();
    assert!(
        before <= stamped && stamped <= after,
        "{before} {stamped} {after}"
    );
    assert_eq!(
        parse_rfc3339_unix_ms(&format_rfc3339(1_800_000_000, 7_000_000)),
        Some(1_800_000_000_007)
    );
}
