use super::poe2::*;

// =============================================================================
// Pin map — Olimex's own SOFTWARE/ARDUINO/LAN8720-POE2 example and
// SOFTWARE/ESPHOME/esp32-poe2.yaml (github.com/OLIMEX/ESP32-POE2): LAN8720,
// addr 0, MDC 23, MDIO 18, power 12, ETH_CLOCK_GPIO0_OUT.
// =============================================================================

#[test]
fn pin_map_matches_olimex_poe2() {
    assert_eq!(PHY_ADDR, 0);
    assert_eq!(MDC_GPIO, 23);
    assert_eq!(MDIO_GPIO, 18);
    assert_eq!(PHY_POWER_GPIO, 12);
    assert_eq!(RMII_CLK_OUT_GPIO, 0);
}

#[test]
fn only_the_clock_pin_differs_from_the_poe_iso() {
    use super::poe_iso;
    assert_eq!(PHY_ADDR, poe_iso::PHY_ADDR);
    assert_eq!(MDC_GPIO, poe_iso::MDC_GPIO);
    assert_eq!(MDIO_GPIO, poe_iso::MDIO_GPIO);
    assert_eq!(PHY_POWER_GPIO, poe_iso::PHY_POWER_GPIO);
    assert_ne!(RMII_CLK_OUT_GPIO, poe_iso::RMII_CLK_OUT_GPIO);
}

#[test]
fn clock_pin_stays_off_the_psram_bus() {
    // The WROVER-E's PSRAM owns 16/17; the POE-ISO's GPIO17 route would
    // clobber it. Nothing the POE2 driver drives may land there.
    for pin in [RMII_CLK_OUT_GPIO, MDC_GPIO, MDIO_GPIO, PHY_POWER_GPIO] {
        assert!(
            !PSRAM_GPIOS.contains(&pin),
            "GPIO{pin} belongs to the PSRAM"
        );
    }
}

// =============================================================================
// Register constants — ESP-IDF soc/io_mux_reg.h and soc/clk_tree_defs.h for
// the ESP32 (v5.3), cross-checked against v4.4's literal
// `REG_SET_FIELD(PIN_CTRL, CLK_OUT1, 6)` in esp_eth_mac_esp.c.
// =============================================================================

#[test]
fn clock_output_register_values_match_esp_idf() {
    assert_eq!(FUNC_GPIO0_CLK_OUT1, 1);
    assert_eq!(CLKOUT_SIG_APLL, 6);
}

#[test]
fn apll_50mhz_is_the_same_solution_as_the_poe_iso() {
    // Shared APLL math: the POE2 only changes where the clock leaves the chip.
    let c = apll_coeffs(RMII_CLK_HZ, XTAL_FREQ_MHZ).unwrap();
    assert_eq!(c.real_freq_hz, 50_000_000);
    assert_eq!(
        c,
        super::poe_iso::apll_coeffs(RMII_CLK_HZ, XTAL_FREQ_MHZ).unwrap()
    );
}

#[test]
fn ethernet_mac_is_base_plus_three() {
    assert_eq!(
        ethernet_mac([0x10, 0x97, 0xbd, 0xe5, 0x9b, 0x0c]),
        [0x10, 0x97, 0xbd, 0xe5, 0x9b, 0x0f]
    );
}
