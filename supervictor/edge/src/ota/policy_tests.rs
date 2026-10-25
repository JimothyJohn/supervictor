extern crate std;

use super::policy::*;
use proptest::prelude::*;

fn roundtrip(state: State) -> State {
    let mut w = [0u32; WORDS];
    store(&mut w, state);
    load(&w)
}

#[test]
fn states_round_trip() {
    for s in [
        State::Idle,
        State::Pending {
            boots: 0,
            previous_slot: 0,
        },
        State::Pending {
            boots: 3,
            previous_slot: 1,
        },
        resume_state(4096 * 10, 1_085_760, &[0xab; 32]),
        resume_state(0, 24, &[0; 32]),
    ] {
        assert_eq!(roundtrip(s), s, "{s:?}");
    }
}

#[test]
fn zeroed_and_random_memory_read_idle() {
    assert_eq!(load(&[0; WORDS]), State::Idle);
    assert_eq!(load(&[0xffff_ffff; WORDS]), State::Idle);
    let mut w = [0u32; WORDS];
    store(
        &mut w,
        State::Pending {
            boots: 1,
            previous_slot: 0,
        },
    );
    // Any single-word corruption invalidates the record.
    for i in 0..WORDS {
        let mut c = w;
        c[i] ^= 0x8000_0001;
        assert_eq!(load(&c), State::Idle, "word {i}");
    }
}

#[test]
fn boot_budget_reverts_on_the_fourth_unconfirmed_boot() {
    let mut state = State::Pending {
        boots: 0,
        previous_slot: 1,
    };
    for expected in 1..=MAX_PENDING_BOOTS {
        let (decision, next) = on_boot(state);
        assert_eq!(decision, BootDecision::Confirming { boots: expected });
        state = next;
    }
    let (decision, next) = on_boot(state);
    assert_eq!(decision, BootDecision::Revert { previous_slot: 1 });
    assert_eq!(next, State::Idle);
}

#[test]
fn idle_and_resume_boot_normally_and_keep_their_record() {
    assert_eq!(on_boot(State::Idle), (BootDecision::Normal, State::Idle));
    let r = resume_state(8192, 100_000, &[7; 32]);
    assert_eq!(on_boot(r), (BootDecision::Normal, r));
}

#[test]
fn resume_offset_only_for_the_same_image() {
    let sha = [0x11u8; 32];
    let r = resume_state(4096 * 3, 50_000, &sha);
    assert_eq!(resume_offset(r, 50_000, &sha), 4096 * 3);
    assert_eq!(resume_offset(r, 50_001, &sha), 0, "size differs");
    let mut other = sha;
    other[0] = 0x12;
    assert_eq!(resume_offset(r, 50_000, &other), 0, "digest differs");
    assert_eq!(resume_offset(State::Idle, 50_000, &sha), 0);
    assert_eq!(
        resume_offset(
            State::Pending {
                boots: 1,
                previous_slot: 0
            },
            50_000,
            &sha
        ),
        0
    );
}

#[test]
fn implausible_records_with_a_valid_checksum_read_idle() {
    // Hand-built records that pass the checksum but say nonsense: a
    // resume offset past the size, an unaligned offset, a slot > 15.
    let mut w = [0u32; WORDS];
    store(&mut w, resume_state(4096, 100, &[0; 32]));
    assert_eq!(load(&w), State::Idle, "offset past size");
    let mut w = [0u32; WORDS];
    store(&mut w, resume_state(4000, 100_000, &[0; 32]));
    assert_eq!(load(&w), State::Idle, "unaligned offset");
    let mut w = [0u32; WORDS];
    store(
        &mut w,
        State::Pending {
            boots: 0,
            previous_slot: 16,
        },
    );
    assert_eq!(load(&w), State::Idle, "slot out of range");
    let mut w = [0u32; WORDS];
    store(
        &mut w,
        State::Pending {
            boots: 99,
            previous_slot: 0,
        },
    );
    assert_eq!(load(&w), State::Idle, "boots out of range");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// Random RTC contents never produce a Revert: the only way to revert
    /// is a record this firmware wrote.
    #[test]
    fn random_memory_never_reverts(words in prop::array::uniform8(any::<u32>())) {
        let (decision, _) = on_boot(load(&words));
        prop_assert_ne!(
            core::mem::discriminant(&decision),
            core::mem::discriminant(&BootDecision::Revert { previous_slot: 0 })
        );
    }

    /// Flipping any bit of a valid record never yields a *different*
    /// valid record (it reads Idle or the original).
    #[test]
    fn bit_flips_are_detected(boots in 0u32..=3, slot in 0u8..=15, word in 0usize..WORDS, bit in 0u32..32) {
        let state = State::Pending { boots, previous_slot: slot };
        let mut w = [0u32; WORDS];
        store(&mut w, state);
        w[word] ^= 1 << bit;
        let got = load(&w);
        prop_assert!(got == State::Idle || got == state, "{got:?}");
    }

    #[test]
    fn resume_records_round_trip(sectors in 0u32..=(0x1F_0000 / 4096), size in 24u32..=0x1F_0000, sha in prop::array::uniform32(any::<u8>())) {
        let offset = (sectors * 4096).min(size / 4096 * 4096);
        let s = resume_state(offset, size, &sha);
        prop_assert_eq!(roundtrip(s), s);
        prop_assert_eq!(resume_offset(s, size, &sha), offset);
    }
}
