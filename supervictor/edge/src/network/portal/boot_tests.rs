use super::boot::*;

fn record(mark: Mark) -> [u32; WORDS] {
    let mut w = [0; WORDS];
    store(&mut w, mark);
    w
}

#[test]
fn marks_round_trip() {
    for m in [Mark::Booted, Mark::Joined, Mark::Portal] {
        assert_eq!(load(&record(m)), Some(m));
    }
}

#[test]
fn garbage_and_zero_read_as_nothing() {
    assert_eq!(load(&[0, 0]), None);
    assert_eq!(load(&[0xdead_beef, 1]), None);
    assert_eq!(load(&[MAGIC, 0]), None);
    assert_eq!(load(&[MAGIC, 4]), None);
}

#[test]
fn no_ssid_always_opens_the_portal() {
    for prev in [
        None,
        Some(Mark::Booted),
        Some(Mark::Joined),
        Some(Mark::Portal),
    ] {
        assert_eq!(decide(prev, false), Some(Reason::Unprovisioned), "{prev:?}");
    }
}

#[test]
fn only_a_boot_that_never_joined_opens_the_portal() {
    assert_eq!(decide(Some(Mark::Booted), true), Some(Reason::JoinFailed));
    assert_eq!(decide(Some(Mark::Joined), true), None);
    assert_eq!(
        decide(Some(Mark::Portal), true),
        None,
        "portal reset tries the station"
    );
    assert_eq!(decide(None, true), None, "power-on reset tries the station");
}
