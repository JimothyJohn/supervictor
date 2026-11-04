extern crate std;

use std::string::String;
use std::vec::Vec;

use super::record::*;
use super::*;
use proptest::prelude::*;

fn sample() -> Settings {
    Settings {
        ssid: "shop-floor".try_into().unwrap(),
        password: "correct horse battery staple".try_into().unwrap(),
        host: HostName::new("supervictor.advin.io").unwrap(),
        device_name: "poe2-wifi-01".try_into().unwrap(),
        uplink_interval_s: 0,
        threshold_ma: 0,
    }
}

fn frame_of(settings: &Settings, seq: u32) -> [u8; LEN] {
    let mut f = [0u8; LEN];
    encode(settings, seq, &mut f);
    f
}

#[test]
fn round_trips() {
    for (s, seq) in [
        (sample(), 1),
        (Settings::default(), 0),
        (Settings::default(), u32::MAX),
        (
            Settings {
                ssid: HString::try_from(&"s".repeat(SSID_MAX)[..]).unwrap(),
                password: HString::try_from(&"p".repeat(PASSWORD_MAX)[..]).unwrap(),
                host: HostName::new(&"h".repeat(HOST_MAX)).unwrap(),
                device_name: HString::try_from(&"n".repeat(NAME_MAX)[..]).unwrap(),
                uplink_interval_s: 600,
                threshold_ma: 1234,
            },
            7,
        ),
    ] {
        assert_eq!(decode(&frame_of(&s, seq)), Some((seq, s.clone())), "{s:?}");
    }
}

#[test]
fn every_single_byte_corruption_is_rejected() {
    let good = frame_of(&sample(), 3);
    for i in 0..LEN {
        for flip in [0x01u8, 0x80, 0xff] {
            let mut f = good;
            f[i] ^= flip;
            assert_eq!(decode(&f), None, "byte {i} ^ {flip:#x} went unnoticed");
        }
    }
}

#[test]
fn blank_flash_and_short_frames_are_nothing() {
    assert_eq!(decode(&[0xffu8; LEN]), None);
    assert_eq!(decode(&[0u8; LEN]), None);
    assert_eq!(decode(&frame_of(&sample(), 1)[..LEN - 1]), None);
    assert_eq!(decode(&[]), None);
}

#[test]
fn a_longer_frame_reads_its_first_record() {
    let mut long = [0xffu8; LEN + 100];
    long[..LEN].copy_from_slice(&frame_of(&sample(), 9));
    assert_eq!(decode(&long).map(|(seq, _)| seq), Some(9));
}

#[test]
fn other_versions_are_not_ours() {
    let mut f = frame_of(&sample(), 1);
    f[4] = 2;
    // Re-sign so only the version differs.
    let sum = {
        let mut h: u32 = 0x811c_9dc5;
        for &b in &f[..LEN - 4] {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    };
    f[LEN - 4..].copy_from_slice(&sum.to_le_bytes());
    assert_eq!(decode(&f), None);
}

#[test]
fn newest_slot_wins_and_a_bad_slot_is_skipped() {
    let a = frame_of(&sample(), 3);
    let mut newer = sample();
    newer.device_name = "renamed".try_into().unwrap();
    let b = frame_of(&newer, 4);
    let got = choose(&a, &b).unwrap();
    assert_eq!((got.slot, got.seq), (Slot::B, 4));
    assert_eq!(got.settings.device_name.as_str(), "renamed");

    let got = choose(&b, &a).unwrap();
    assert_eq!((got.slot, got.seq), (Slot::A, 4));

    let mut torn = b;
    torn[100] ^= 0xff;
    let got = choose(&a, &torn).unwrap();
    assert_eq!((got.slot, got.seq), (Slot::A, 3));

    assert_eq!(choose(&[0xff; LEN], &[0xff; LEN]), None);
    assert_eq!(choose(&[], &a).map(|f| f.slot), Some(Slot::B));
}

#[test]
fn equal_sequence_numbers_pick_a() {
    let a = frame_of(&sample(), 5);
    let b = frame_of(&Settings::default(), 5);
    assert_eq!(choose(&a, &b).unwrap().slot, Slot::A);
}

#[test]
fn writes_alternate_slots_and_count_up() {
    assert_eq!(next(None), (Slot::A, 1));
    let a = choose(&frame_of(&sample(), 1), &[0xff; LEN]).unwrap();
    assert_eq!(next(Some(&a)), (Slot::B, 2));
    let b = choose(&frame_of(&sample(), 1), &frame_of(&sample(), 2)).unwrap();
    assert_eq!(next(Some(&b)), (Slot::A, 3));
    let wrap = Found {
        slot: Slot::A,
        seq: u32::MAX,
        settings: sample(),
    };
    assert_eq!(next(Some(&wrap)), (Slot::B, 0));
}

#[test]
fn slots_fit_the_nvs_partition() {
    assert!(LEN as u32 <= SLOT_STRIDE);
    assert_eq!(Slot::B.offset() - Slot::A.offset(), SLOT_STRIDE);
    let nvs_size = include_str!("../../partitions.csv")
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("nvs,"))
        .and_then(|l| l.split(',').nth(4))
        .and_then(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .expect("partitions.csv has an nvs row");
    assert!(
        SPAN <= nvs_size,
        "two slots need {SPAN} B, nvs is {nvs_size} B"
    );
}

#[test]
fn field_limits_are_enforced_on_decode() {
    // A length byte past the field's capacity, with a matching checksum.
    let mut f = frame_of(&sample(), 1);
    f[12] = (SSID_MAX + 1) as u8;
    let sum = {
        let mut h: u32 = 0x811c_9dc5;
        for &b in &f[..LEN - 4] {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    };
    f[LEN - 4..].copy_from_slice(&sum.to_le_bytes());
    assert_eq!(decode(&f), None);
}

#[test]
fn hostname_is_nul_terminated_and_bounded() {
    let h = HostName::new("supervictor.advin.io").unwrap();
    assert_eq!(h.as_str(), "supervictor.advin.io");
    assert_eq!(h.as_cstr().to_bytes(), b"supervictor.advin.io");
    assert_eq!(h.as_cstr().to_bytes_with_nul().last(), Some(&0));
    assert!(!h.is_empty());
    assert!(HostName::EMPTY.is_empty());
    assert_eq!(HostName::EMPTY.as_cstr().to_bytes(), b"");
    assert!(HostName::new(&"x".repeat(HOST_MAX + 1)).is_none());
    assert!(HostName::new("a\0b").is_none());
    assert_eq!(HostName::new("").unwrap(), HostName::EMPTY);
}

#[test]
fn baked_without_env_is_the_bench_default() {
    // Host tests build without HOST/SSID/PASSWORD; DEVICE_NAME may come
    // from the caller's environment, so only its presence is checked.
    let b = Settings::baked();
    assert!(!b.device_name.is_empty());
    assert_eq!(b.uplink_interval_s, 0);
    assert_eq!(b.threshold_ma, 0);
    assert!(!b.is_blank());
    assert!(Settings::default().is_blank());
    assert!(!Settings::default().has_endpoint());
    assert!(sample().has_endpoint());
    assert!(sample().has_wifi());
}

fn arb_string(max: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(any::<char>().prop_filter("no NUL", |c| *c != '\0'), 0..=max)
        .prop_map(move |chars| {
            // Trim to the byte budget on a char boundary.
            let mut s = String::new();
            for c in chars {
                if s.len() + c.len_utf8() > max {
                    break;
                }
                s.push(c);
            }
            s
        })
}

fn arb_settings() -> impl Strategy<Value = Settings> {
    (
        arb_string(SSID_MAX),
        arb_string(PASSWORD_MAX),
        arb_string(HOST_MAX),
        arb_string(NAME_MAX),
        any::<u32>(),
        any::<u32>(),
    )
        .prop_map(
            |(ssid, password, host, name, interval, threshold)| Settings {
                ssid: HString::try_from(ssid.as_str()).unwrap(),
                password: HString::try_from(password.as_str()).unwrap(),
                host: HostName::new(&host).unwrap(),
                device_name: HString::try_from(name.as_str()).unwrap(),
                uplink_interval_s: interval,
                threshold_ma: threshold,
            },
        )
}

proptest! {
    #[test]
    fn any_settings_round_trip(s in arb_settings(), seq in any::<u32>()) {
        prop_assert_eq!(decode(&frame_of(&s, seq)), Some((seq, s.clone())));
    }

    #[test]
    fn any_bytes_decode_without_panicking(bytes in proptest::collection::vec(any::<u8>(), 0..LEN + 8)) {
        let _ = decode(&bytes);
        let _ = choose(&bytes, &bytes);
    }

    #[test]
    fn a_torn_write_never_loses_the_old_record(
        old in arb_settings(), new in arb_settings(), cut in 0..LEN
    ) {
        // Slot A holds `old`; a write of `new` into slot B stopped after
        // `cut` bytes (the rest still erased).
        let a = frame_of(&old, 1);
        let mut b = [0xffu8; LEN];
        let full = frame_of(&new, 2);
        b[..cut].copy_from_slice(&full[..cut]);
        let got = choose(&a, &b).unwrap();
        prop_assert!(got.settings == old || (cut == LEN && got.settings == new));
        let bytes: Vec<u8> = b.to_vec();
        prop_assert!(decode(&bytes).is_none() || cut == LEN);
    }
}
