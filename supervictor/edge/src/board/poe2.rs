//! Olimex ESP32-POE2 board support.
//!
//! Same LAN8720 PHY, SMI pins and PHY power gate as the POE-ISO — the one
//! difference is the RMII reference clock. The POE2's ESP32-WROVER-E has
//! PSRAM on GPIO16/17, so Olimex takes the 50 MHz clock out of **GPIO0**
//! instead (their `SOFTWARE/ARDUINO/LAN8720-POE2` example: `ETH_CLK_MODE
//! ETH_CLOCK_GPIO0_OUT`). Build the POE-ISO driver for this board and the PHY
//! never sees a clock: MDIO reads `0xffffffff`.
//!
//! GPIO0 has no EMAC clock-out IO_MUX function. ESP-IDF routes the raw APLL
//! through the chip's generic clock-output channel 1 instead
//! (`esp_clock_output_start(CLKOUT_SIG_APLL, 0)` in `esp_eth_mac_esp.c`;
//! `REG_SET_FIELD(PIN_CTRL, CLK_OUT1, 6)` in v4.x): IO_MUX GPIO0 function
//! `CLK_OUT1`, and `PIN_CTRL[3:0]` = `CLKOUT_SIG_APLL`.
//!
//! GPIO0 is also the boot strap. The PHY is unpowered (GPIO12 low) until
//! bring-up, so the pad floats high through its pull-up at reset and the
//! clock only starts once we are running; same arrangement Olimex ships.

pub use super::poe_iso::{
    apll_coeffs, ethernet_mac, ApllCoeffs, MDC_GPIO, MDIO_GPIO, PHY_ADDR, PHY_POWER_GPIO,
    RMII_CLK_HZ, XTAL_FREQ_MHZ,
};

/// RMII reference clock output: APLL via clock-output channel 1 on GPIO0.
pub const RMII_CLK_OUT_GPIO: u8 = 0;

/// IO_MUX function 1 on GPIO0 = `CLK_OUT1` (soc/io_mux_reg.h
/// `FUNC_GPIO0_CLK_OUT1`).
pub const FUNC_GPIO0_CLK_OUT1: u8 = 1;

/// `PIN_CTRL` clock-output selector value for the APLL (soc/clk_tree_defs.h
/// `CLKOUT_SIG_APLL`).
pub const CLKOUT_SIG_APLL: u8 = 6;

/// GPIOs the WROVER-E's PSRAM owns; the driver must never touch them here.
pub const PSRAM_GPIOS: [u8; 2] = [16, 17];

// Field widths the register writes below rely on.
const _: () = assert!(CLKOUT_SIG_APLL < 16, "PIN_CTRL.CLK_OUT1 is a 4-bit field");
const _: () = assert!(FUNC_GPIO0_CLK_OUT1 < 8, "IO_MUX MCU_SEL is a 3-bit field");

#[cfg(feature = "ethernet")]
mod bringup {
    use super::{CLKOUT_SIG_APLL, FUNC_GPIO0_CLK_OUT1, RMII_CLK_OUT_GPIO};

    /// Power up the APLL at 50 MHz and route it out of GPIO0.
    ///
    /// Call before EMAC init, exactly like the POE-ISO variant: the EMAC's
    /// internal-clock selector (set by ph-esp32-mac in `Emac::init`) is what
    /// finally lets the MAC run from the same APLL.
    pub fn enable_rmii_clk_out(delay: &mut esp_hal::delay::Delay) {
        super::super::poe_iso::bringup::enable_apll_50mhz(delay);
        route_apll_clk_out_gpio0();
    }

    /// GPIO0 as an output pad driven by clock-output channel 1, channel 1
    /// fed by the APLL. Mirrors ESP-IDF `esp_clock_output_start(CLKOUT_SIG_APLL, 0)`:
    /// `gpio_set_direction(0, OUTPUT)`, IO_MUX function `CLK_OUT1`,
    /// `PIN_CTRL.CLK_OUT1 = CLKOUT_SIG_APLL`.
    fn route_apll_clk_out_gpio0() {
        let gpio = unsafe { &*esp32::GPIO::ptr() };
        let io_mux = unsafe { &*esp32::IO_MUX::ptr() };
        // Output enable for pad 0 (GPIO_ENABLE_W1TS bit 0).
        gpio.enable_w1ts()
            .write(|w| unsafe { w.bits(1 << RMII_CLK_OUT_GPIO) });
        // Pad function: CLK_OUT1, input buffer off.
        io_mux
            .gpio0()
            .modify(|_, w| unsafe { w.mcu_sel().bits(FUNC_GPIO0_CLK_OUT1).fun_ie().clear_bit() });
        // Clock-output channel 1 source: APLL.
        io_mux
            .pin_ctrl()
            .modify(|_, w| unsafe { w.clk1().bits(CLKOUT_SIG_APLL) });
    }
}

#[cfg(feature = "ethernet")]
pub use bringup::enable_rmii_clk_out;
