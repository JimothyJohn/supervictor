//! The A/B partition table is frozen. Changing it means one more USB flash
//! of every board in the field — agreed 2026-10-26 as the last one, never
//! again. Edit the expectation here only together with that decision, and
//! say so in the PR.

extern crate std;

use std::string::String;
use std::vec::Vec;

const TABLE: &str = include_str!("../../partitions.csv");

fn rows() -> Vec<String> {
    TABLE
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split(',').map(str::trim).collect::<Vec<_>>().join(","))
        .collect()
}

#[test]
fn partition_table_is_the_frozen_ab_layout() {
    assert_eq!(
        rows(),
        [
            "nvs,data,nvs,0x9000,0x6000",
            "otadata,data,ota,0xf000,0x2000",
            "phy_init,data,phy,0x11000,0x1000",
            "ota_0,app,ota_0,0x20000,0x1F0000",
            "ota_1,app,ota_1,0x210000,0x1F0000",
        ]
    );
}

#[test]
fn slot_size_matches_the_table() {
    assert_eq!(supervictor_common::firmware::SLOT_SIZE, 0x1F_0000);
}
