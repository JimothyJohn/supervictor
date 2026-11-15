use super::*;
use supervictor_common::config::{version, UPLINK_INTERVAL_MAX_S};

fn fresh() -> Settings {
    Settings {
        device_name: "d".try_into().unwrap(),
        ..Default::default()
    }
}

#[test]
fn a_new_version_is_applied_and_persisted_by_the_caller() {
    let mut s = fresh();
    let v = version(10, 12_000);
    assert!(s.apply_pushed(v, 10, 12_000));
    assert_eq!(
        (s.config_version, s.uplink_interval_s, s.threshold_ma),
        (v, 10, 12_000)
    );
    assert_eq!(s.effective_interval_s(5), 10);
}

#[test]
fn version_zero_and_the_held_version_are_nothing() {
    let mut s = fresh();
    assert!(!s.apply_pushed(0, 10, 12_000), "0 is 'no block'");
    assert_eq!(s, fresh());
    let v = version(10, 12_000);
    assert!(s.apply_pushed(v, 10, 12_000));
    let after = s.clone();
    assert!(!s.apply_pushed(v, 10, 12_000), "same version again");
    assert!(
        !s.apply_pushed(v, 99, 1),
        "same version, different values: the version is the contract"
    );
    assert_eq!(s, after);
}

#[test]
fn the_interval_is_clamped_and_the_default_kept() {
    let mut s = fresh();
    assert!(s.apply_pushed(1, 601, 0));
    assert_eq!(s.uplink_interval_s, UPLINK_INTERVAL_MAX_S);
    assert!(s.apply_pushed(2, u32::MAX, 0));
    assert_eq!(s.uplink_interval_s, UPLINK_INTERVAL_MAX_S);
    assert!(s.apply_pushed(3, 0, 0));
    assert_eq!(s.uplink_interval_s, 0, "0 stays 'compiled default'");
    assert_eq!(s.effective_interval_s(5), 5);
    assert!(s.apply_pushed(4, 1, 0));
    assert_eq!(s.effective_interval_s(5), 1);
}

#[test]
fn the_threshold_is_never_negative_and_saturates() {
    let mut s = fresh();
    assert!(s.apply_pushed(1, 5, -1));
    assert_eq!(s.threshold_ma, 0);
    assert!(s.apply_pushed(2, 5, i64::MIN));
    assert_eq!(s.threshold_ma, 0);
    assert!(s.apply_pushed(3, 5, i64::MAX));
    assert_eq!(s.threshold_ma, u32::MAX);
    assert!(s.apply_pushed(4, 5, 1000));
    assert_eq!(s.threshold_ma, 1000);
}

#[test]
fn pushed_values_survive_the_record() {
    let mut s = fresh();
    s.host = HostName::new("h").unwrap();
    assert!(s.apply_pushed(version(30, 500), 30, 500));
    let mut frame = [0u8; record::LEN];
    record::encode(&s, 9, &mut frame);
    let (seq, back) = record::decode(&frame).unwrap();
    assert_eq!(seq, 9);
    assert_eq!(back, s);
    assert_eq!(back.config_version, version(30, 500));
    assert!(!back.is_blank());
}
