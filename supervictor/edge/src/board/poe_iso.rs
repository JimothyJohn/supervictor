//! Olimex ESP32-POE-ISO board support.
//!
//! Pin map verified against arduino-esp32 `variants/esp32-poe-iso/pins_arduino.h`
//! (the non-PSRAM variant — GPIO17 clock out). The board's LAN8710A PHY shares
//! the LAN8720A's register map and PHY ID family (`0x0007C0Fx`), so the
//! `ph-esp32-mac` LAN8720A driver applies for standard 10/100 bring-up.
//!
//! The ESP32 sources the PHY's 50 MHz RMII reference clock internally (the
//! board has no external oscillator): APLL → EMAC → `EMAC_CLK_OUT_180` on
//! GPIO17. `ph-esp32-mac` only flips the EMAC ext-register clock selector;
//! powering and tuning the APLL and muxing GPIO17 is done here, mirroring
//! ESP-IDF (`rtc_clk_apll_enable` / `rtc_clk_apll_coeff_set` /
//! `emac_esp_iomux_rmii_clk_ouput`).

// =============================================================================
// Pin map / PHY identity (pure constants, host-testable)
// =============================================================================

/// PHY address on the SMI bus (LAN8710A PHYAD0 strapped low).
pub const PHY_ADDR: u8 = 0;
/// SMI management clock GPIO (ESP32 default, wired as such on the board).
pub const MDC_GPIO: u8 = 23;
/// SMI management data GPIO (ESP32 default, wired as such on the board).
pub const MDIO_GPIO: u8 = 18;
/// Gates 3.3 V to the PHY — must be driven high before any MDIO access.
pub const PHY_POWER_GPIO: u8 = 12;
/// RMII reference clock output (`EMAC_CLK_OUT_180` IO_MUX function).
pub const RMII_CLK_OUT_GPIO: u8 = 17;

/// RMII reference clock frequency required by the PHY.
pub const RMII_CLK_HZ: u32 = 50_000_000;

/// ESP32 crystal frequency in MHz (all production boards use 40 MHz).
pub const XTAL_FREQ_MHZ: u32 = 40;

// =============================================================================
// APLL coefficient calculation (pure, host-testable)
// =============================================================================

/// APLL tuning coefficients for a target output frequency.
///
/// `apll_freq = xtal * (4 + sdm2 + sdm1/256 + sdm0/65536) / ((o_div + 2) * 2)`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApllCoeffs {
    /// Output divider (0–31).
    pub o_div: u8,
    /// Sigma-delta modulator fractional low byte.
    pub sdm0: u8,
    /// Sigma-delta modulator fractional high byte.
    pub sdm1: u8,
    /// Sigma-delta modulator integer part (0–63).
    pub sdm2: u8,
    /// The frequency actually produced by these coefficients.
    pub real_freq_hz: u32,
}

/// Compute APLL coefficients for `freq_hz`, replicating ESP-IDF's
/// `rtc_clk_apll_coeff_calc` (the numerator must land in 350–500 MHz).
///
/// Returns `None` when the frequency is outside the APLL's reachable range.
pub fn apll_coeffs(freq_hz: u32, xtal_freq_mhz: u32) -> Option<ApllCoeffs> {
    const MULTIPLIER_MIN_HZ: f64 = 350_000_000.0;
    const MULTIPLIER_MAX_HZ: f64 = 500_000_000.0;
    let xtal_hz = f64::from(xtal_freq_mhz) * 1_000_000.0;
    let freq = f64::from(freq_hz);

    let mut o_div = (MULTIPLIER_MIN_HZ / (freq * 2.0) + 1.0) as i32 - 2;
    if o_div > 31 {
        return None; // frequency too small
    }
    if o_div < 0 {
        o_div = (MULTIPLIER_MAX_HZ / (freq * 2.0)) as i32 - 2;
        if o_div < 0 {
            return None; // frequency too big
        }
    }

    // Integer division first, exactly like the C code.
    let denom = (o_div as u64 + 2) * 2;
    let mut sdm2 =
        ((denom * u64::from(freq_hz)) / (u64::from(xtal_freq_mhz) * 1_000_000)) as i32 - 4;
    let numerator = (denom as f64 * freq) / xtal_hz - 4.0 - f64::from(sdm2);

    let mut sdm0 = 0i32;
    let mut sdm1 = 0i32;
    if numerator > 1.0 - (1.0 / 65536.0) / 2.0 {
        sdm2 += 1;
    } else if numerator > (1.0 / 65536.0) / 2.0 {
        let scaled = (numerator * 65536.0 + 0.5) as i32;
        sdm1 = scaled / 256;
        sdm0 = scaled % 256;
    }

    let real_freq_hz = (xtal_hz
        * (4.0 + f64::from(sdm2) + f64::from(sdm1) / 256.0 + f64::from(sdm0) / 65536.0)
        / (denom as f64)) as u32;

    Some(ApllCoeffs {
        o_div: o_div as u8,
        sdm0: sdm0 as u8,
        sdm1: sdm1 as u8,
        sdm2: sdm2 as u8,
        real_freq_hz,
    })
}

/// Derive the Ethernet MAC from the efuse base MAC.
///
/// Espressif's universal-MAC scheme (`esp_read_mac` with `ESP_MAC_ETH`)
/// assigns interfaces base+0 (WiFi STA), +1 (AP), +2 (BT), +3 (Ethernet) on
/// the last octet.
pub fn ethernet_mac(base: [u8; 6]) -> [u8; 6] {
    let mut mac = base;
    mac[5] = mac[5].wrapping_add(3);
    mac
}

// =============================================================================
// Hardware bring-up (target-only)
// =============================================================================

/// APLL power-up and 50 MHz RMII clock routing to GPIO17.
#[cfg(feature = "ethernet")]
mod bringup {
    use super::{apll_coeffs, ApllCoeffs, RMII_CLK_HZ, XTAL_FREQ_MHZ};

    // ROM regi2c accessors for the APLL's internal-I2C register file. The
    // symbols are PROVIDEd by esp-rom-sys's esp32.rom.ld; signatures match
    // ESP-IDF's esp_rom_regi2c.h (esp-hal declares rom_i2c_writeReg the same
    // way for its BBPLL setup).
    extern "C" {
        fn rom_i2c_writeReg(block: u8, host_id: u8, reg_add: u8, data: u8);
        fn rom_i2c_writeReg_Mask(block: u8, host_id: u8, reg_add: u8, msb: u8, lsb: u8, data: u8);
        fn rom_i2c_readReg_Mask(block: u8, host_id: u8, reg_add: u8, msb: u8, lsb: u8) -> u8;
    }

    // Register map from ESP-IDF soc/regi2c_apll.h and hal/esp32/clk_tree_ll.h.
    const I2C_APLL: u8 = 0x6D;
    const I2C_APLL_HOSTID: u8 = 3;
    const I2C_APLL_IR_CAL_DELAY: u8 = 0;
    const I2C_APLL_OR_CAL_END: u8 = 3; // bit 7
    const I2C_APLL_OR_OUTPUT_DIV: u8 = 4; // bits 4:0
    const I2C_APLL_SDM_STOP: u8 = 5;
    const I2C_APLL_DSDM2: u8 = 7; // bits 5:0
    const I2C_APLL_DSDM1: u8 = 8;
    const I2C_APLL_DSDM0: u8 = 9;
    const APLL_SDM_STOP_VAL_1: u8 = 0x09;
    const APLL_SDM_STOP_VAL_2_REV0: u8 = 0x69;
    const APLL_SDM_STOP_VAL_2_REV1: u8 = 0x49;
    const APLL_CAL_DELAY_1: u8 = 0x0f;
    const APLL_CAL_DELAY_2: u8 = 0x3f;
    const APLL_CAL_DELAY_3: u8 = 0x1f;

    /// IO_MUX function 5 on GPIO17 = `EMAC_CLK_OUT_180` (soc/io_mux_reg.h).
    const FUNC_GPIO17_EMAC_CLK_OUT_180: u8 = 5;

    /// Power up the APLL, tune it to 50 MHz, and mux GPIO17 as the RMII
    /// reference clock output.
    ///
    /// Mirrors ESP-IDF `rtc_clk_apll_enable(true)`, `rtc_clk_apll_coeff_set`,
    /// and `emac_esp_iomux_rmii_clk_ouput(17)`. Call before EMAC init; the
    /// clock only reaches the pin once the EMAC's internal-clock selector is
    /// set (ph-esp32-mac does that during `Emac::init`).
    pub fn enable_rmii_clk_out(delay: &mut esp_hal::delay::Delay) {
        let ApllCoeffs {
            o_div,
            mut sdm0,
            mut sdm1,
            sdm2,
            real_freq_hz: _,
        } = apll_coeffs(RMII_CLK_HZ, XTAL_FREQ_MHZ)
            .expect("50 MHz is always reachable by the APLL");

        // Power up the APLL analog block.
        let rtc_cntl = unsafe { &*esp32::RTC_CNTL::ptr() };
        rtc_cntl
            .ana_conf()
            .modify(|_, w| w.plla_force_pd().clear_bit().plla_force_pu().set_bit());

        // Revision-0 silicon can't use the fractional SDM stages.
        let is_rev0 = esp_hal::efuse::Efuse::chip_revision() < 100;
        let sdm_stop_val_2 = if is_rev0 {
            sdm0 = 0;
            sdm1 = 0;
            APLL_SDM_STOP_VAL_2_REV0
        } else {
            APLL_SDM_STOP_VAL_2_REV1
        };

        unsafe {
            rom_i2c_writeReg_Mask(I2C_APLL, I2C_APLL_HOSTID, I2C_APLL_DSDM2, 5, 0, sdm2);
            rom_i2c_writeReg_Mask(I2C_APLL, I2C_APLL_HOSTID, I2C_APLL_DSDM0, 7, 0, sdm0);
            rom_i2c_writeReg_Mask(I2C_APLL, I2C_APLL_HOSTID, I2C_APLL_DSDM1, 7, 0, sdm1);
            rom_i2c_writeReg(
                I2C_APLL,
                I2C_APLL_HOSTID,
                I2C_APLL_SDM_STOP,
                APLL_SDM_STOP_VAL_1,
            );
            rom_i2c_writeReg(I2C_APLL, I2C_APLL_HOSTID, I2C_APLL_SDM_STOP, sdm_stop_val_2);
            rom_i2c_writeReg_Mask(
                I2C_APLL,
                I2C_APLL_HOSTID,
                I2C_APLL_OR_OUTPUT_DIV,
                4,
                0,
                o_div,
            );

            // Calibration pulse train, then wait for OR_CAL_END.
            rom_i2c_writeReg(
                I2C_APLL,
                I2C_APLL_HOSTID,
                I2C_APLL_IR_CAL_DELAY,
                APLL_CAL_DELAY_1,
            );
            rom_i2c_writeReg(
                I2C_APLL,
                I2C_APLL_HOSTID,
                I2C_APLL_IR_CAL_DELAY,
                APLL_CAL_DELAY_2,
            );
            rom_i2c_writeReg(
                I2C_APLL,
                I2C_APLL_HOSTID,
                I2C_APLL_IR_CAL_DELAY,
                APLL_CAL_DELAY_3,
            );
            while rom_i2c_readReg_Mask(I2C_APLL, I2C_APLL_HOSTID, I2C_APLL_OR_CAL_END, 7, 7) == 0 {
                delay.delay_micros(1);
            }
        }

        // Route the EMAC reference clock out on GPIO17.
        let io_mux = unsafe { &*esp32::IO_MUX::ptr() };
        io_mux
            .gpio17()
            .modify(|_, w| unsafe { w.mcu_sel().bits(FUNC_GPIO17_EMAC_CLK_OUT_180) });
    }
}

#[cfg(feature = "ethernet")]
pub use bringup::enable_rmii_clk_out;
