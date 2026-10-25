use super::poe_iso::*;

// =============================================================================
// Pin map — guards against drift from the values verified in the arduino-esp32
// variant file (variants/esp32-poe-iso/pins_arduino.h).
// =============================================================================

#[test]
fn pin_map_matches_olimex_poe_iso() {
    assert_eq!(PHY_ADDR, 0);
    assert_eq!(MDC_GPIO, 23);
    assert_eq!(MDIO_GPIO, 18);
    assert_eq!(PHY_POWER_GPIO, 12);
    assert_eq!(RMII_CLK_OUT_GPIO, 17);
}

// =============================================================================
// APLL coefficients — the 50 MHz solution is cross-checked against ESP-IDF
// v4.x, which hardcoded rtc_clk_apll_enable(true, sdm0=0, sdm1=0, sdm2=6,
// o_div=2) for the EMAC RMII clock.
// =============================================================================

#[test]
fn apll_50mhz_matches_esp_idf_hardcoded_coefficients() {
    let c = apll_coeffs(RMII_CLK_HZ, XTAL_FREQ_MHZ).unwrap();
    assert_eq!(c.o_div, 2);
    assert_eq!(c.sdm0, 0);
    assert_eq!(c.sdm1, 0);
    assert_eq!(c.sdm2, 6);
    assert_eq!(
        c.real_freq_hz, 50_000_000,
        "coefficients must produce exactly 50 MHz"
    );
}

#[test]
fn apll_real_freq_formula_holds() {
    // apll_freq = xtal * (4 + sdm2 + sdm1/256 + sdm0/65536) / ((o_div + 2) * 2)
    let c = apll_coeffs(RMII_CLK_HZ, XTAL_FREQ_MHZ).unwrap();
    let num = 40_000_000u64 * (4 + c.sdm2 as u64);
    let den = (c.o_div as u64 + 2) * 2;
    assert_eq!(num / den, c.real_freq_hz as u64);
}

#[test]
fn apll_numerator_stays_in_hardware_range() {
    // The APLL multiplier output must land in 350–500 MHz for any frequency
    // the calc accepts (this is the constraint the o_div search enforces).
    for freq in [
        6_000_000u32,
        12_288_000,
        44_100 * 512,
        50_000_000,
        100_000_000,
    ] {
        let c = apll_coeffs(freq, XTAL_FREQ_MHZ).unwrap();
        let numerator =
            40_000_000f64 * (4.0 + c.sdm2 as f64 + c.sdm1 as f64 / 256.0 + c.sdm0 as f64 / 65536.0);
        assert!(
            (350_000_000.0..=500_000_000.0).contains(&numerator),
            "freq {freq}: numerator {numerator} out of range"
        );
    }
}

#[test]
fn apll_rejects_out_of_range_frequencies() {
    // Below ~5.3 MHz the o_div range is exhausted; above 175 MHz both o_div
    // branches go negative. (ESP-IDF's calc — replicated bug-for-bug — accepts
    // 125–175 MHz with an out-of-spec numerator; we never request those.)
    assert!(apll_coeffs(1_000_000, XTAL_FREQ_MHZ).is_none());
    assert!(apll_coeffs(200_000_000, XTAL_FREQ_MHZ).is_none());
}

#[test]
fn apll_close_approximation_for_fractional_targets() {
    // A target that needs the fractional SDM stages: result within 1 kHz.
    let target = 44_100u32 * 256; // 11.2896 MHz audio clock
    let c = apll_coeffs(target, XTAL_FREQ_MHZ).unwrap();
    let err = (c.real_freq_hz as i64 - target as i64).abs();
    assert!(err < 1_000, "real {} vs target {target}", c.real_freq_hz);
}

// =============================================================================
// MAC derivation
// =============================================================================

#[test]
fn ethernet_mac_is_base_plus_three() {
    let base = [0xAC, 0x67, 0xB2, 0x01, 0x02, 0x03];
    assert_eq!(ethernet_mac(base), [0xAC, 0x67, 0xB2, 0x01, 0x02, 0x06]);
}

#[test]
fn ethernet_mac_wraps_last_octet() {
    let base = [0xAC, 0x67, 0xB2, 0x01, 0x02, 0xFE];
    assert_eq!(ethernet_mac(base), [0xAC, 0x67, 0xB2, 0x01, 0x02, 0x01]);
}

#[test]
fn ethernet_mac_preserves_oui() {
    let base = [0xAC, 0x67, 0xB2, 0xFF, 0xFF, 0xFF];
    let mac = ethernet_mac(base);
    assert_eq!(&mac[..5], &base[..5], "only the last octet may change");
}
