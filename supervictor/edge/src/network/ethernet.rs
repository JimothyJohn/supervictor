//! Wired Ethernet uplink for the ESP32-POE-ISO (EMAC + LAN8710A over RMII).
//!
//! Bring-up sequence (order matters — the PHY needs the 50 MHz reference
//! clock and power before it will answer on MDIO):
//!
//! 1. APLL → 50 MHz → GPIO17 mux ([`crate::board::poe_iso::enable_rmii_clk_out`])
//! 2. PHY power on GPIO12, then a stabilization delay
//! 3. EMAC init (also enables the internal-clock selector, so the reference
//!    clock actually reaches the pin here)
//! 4. PHY soft-reset + auto-negotiation over MDIO
//! 5. EMAC start + ISR bind, embassy-net stack on top
//!
//! The DHCP task mirrors the `ph-esp32-mac` embassy-net example (the crate's
//! hardware-verified recipe): DHCP starts only after link-up, with promiscuous
//! mode temporarily enabled until a lease is held.

use embassy_net::{Config, ConfigV4, DhcpConfig, Runner, Stack};
use embassy_time::Timer;
use esp_hal::delay::Delay;
use esp_hal::gpio::Output;
use esp_println::println;
use ph_esp32_mac::esp_hal::{EmacBuilder, EmacExt, EmacPhyBundle, Priority};
use ph_esp32_mac::hal::MdioController;
use ph_esp32_mac::{Emac, EmacConfig, EmbassyEmac, Lan8720a, PhyInterface, RmiiClockMode};

use crate::board::poe_iso;
use crate::config::{
    EMAC_FRAME_BUF_SIZE, EMAC_RX_FRAMES, EMAC_TX_FRAMES, ETH_LINK_POLL_INTERVAL,
    PHY_POWER_STABLE_DELAY_MS,
};

/// EMAC instance shape used throughout this module.
pub type EthEmac = Emac<EMAC_RX_FRAMES, EMAC_TX_FRAMES, EMAC_FRAME_BUF_SIZE>;
/// embassy-net driver over the EMAC.
pub type EthDriver = EmbassyEmac<'static, EMAC_RX_FRAMES, EMAC_TX_FRAMES, EMAC_FRAME_BUF_SIZE>;

// EMAC hardware instance (placed in DRAM for DMA), shared ISR/link state, and
// embassy-net stack resources (3 sockets: TCP uplink + DNS + slack).
ph_esp32_mac::embassy_net_statics!(
    EMAC,
    EMAC_STATE,
    NET_RESOURCES,
    EMAC_RX_FRAMES,
    EMAC_TX_FRAMES,
    EMAC_FRAME_BUF_SIZE,
    3
);

// EMAC interrupt: wake the embassy-net driver on RX/TX/error events.
ph_esp32_mac::emac_isr!(EMAC_IRQ, Priority::Priority1, {
    EMAC_STATE.handle_interrupt();
});

/// Output backend for the vendored C printf shim (`csrc/printf.c`) that
/// ethernet-only builds link for mbedtls's prebuilt libs (build.rs decides).
/// WiFi builds resolve printf from esp-wifi-sys's blob instead; a strong
/// definition here is harmless there.
#[no_mangle]
extern "C" fn _putchar(c: u8) {
    esp_println::print!("{}", c as char);
}

/// Bring up the EMAC + PHY and build the embassy-net stack.
///
/// `phy_power` must be the GPIO12 output (still low); `seed` feeds embassy-net's
/// TCP sequence randomness. Returns the stack, its runner (spawn
/// [`eth_net_task`] with it), and the EMAC pointer for the link/DHCP tasks.
pub fn init(
    mut phy_power: Output<'static>,
    seed: u64,
) -> Result<(Stack<'static>, Runner<'static, EthDriver>, *mut EthEmac), ph_esp32_mac::Error> {
    let mut delay = Delay::new();

    // 1. 50 MHz reference clock plumbing (APLL + GPIO17 mux).
    poe_iso::enable_rmii_clk_out(&mut delay);

    // 2. PHY power. The LAN8710A latches its configuration straps on power-on
    //    and needs a settle window before MDIO responds.
    phy_power.set_high();
    delay.delay_millis(PHY_POWER_STABLE_DELAY_MS);

    // 3. EMAC init. Locally-administered fallback never happens: the efuse
    //    base MAC is factory-programmed on every ESP32.
    let mac = poe_iso::ethernet_mac(esp_hal::efuse::Efuse::mac_address());
    let config = EmacConfig::rmii_esp32_default()
        .with_phy_interface(PhyInterface::Rmii)
        .with_rmii_clock(RmiiClockMode::InternalOutput {
            gpio: poe_iso::RMII_CLK_OUT_GPIO,
        })
        .with_mac_address(mac);

    let emac_ptr = EMAC.init(Emac::new()) as *mut EthEmac;
    // SAFETY: emac_ptr points to static storage valid for the program lifetime.
    let emac = unsafe { &mut *emac_ptr };
    EmacBuilder::new(emac)
        .with_config(config)
        .init(&mut delay)?;

    // 4. PHY init (soft reset + auto-negotiation). Log the PHY ID so a wiring
    //    or strap problem is visible on the console.
    {
        // SAFETY: same static EMAC; the builder borrow above has ended.
        let emac = unsafe { &mut *emac_ptr };
        let mut bundle = EmacPhyBundle::new(
            emac,
            Lan8720a::new(poe_iso::PHY_ADDR),
            MdioController::new(Delay::new()),
        );
        bundle.init_phy()?;
        let (_, phy, mut mdio) = bundle.into_parts();
        use ph_esp32_mac::PhyDriver;
        match phy.phy_id(&mut mdio) {
            // LAN8710A/LAN8720A family reads 0x0007C0Fx.
            Ok(id) => println!(
                "Ethernet PHY at addr {}: id {:#010x}",
                poe_iso::PHY_ADDR,
                id
            ),
            Err(e) => println!("Ethernet PHY id read failed: {:?}", e),
        }
    }

    // 5. Start the MAC and bind the interrupt.
    // SAFETY: same static EMAC; prior borrows have ended.
    let emac = unsafe { &mut *emac_ptr };
    emac.start()?;
    emac.bind_interrupt(EMAC_IRQ);

    // 6. embassy-net stack. IPv4 config is applied by eth_dhcp_task once the
    //    link is up (the crate's verified recipe), so start unconfigured.
    let driver = ph_esp32_mac::embassy_net_driver!(emac_ptr, &EMAC_STATE);
    let (stack, runner) =
        ph_esp32_mac::embassy_net_stack!(driver, NET_RESOURCES, Config::default(), seed);

    Ok((stack, runner, emac_ptr))
}

/// Drives the Embassy network stack (packet rx/tx) for the Ethernet interface.
#[embassy_executor::task]
pub async fn eth_net_task(mut runner: Runner<'static, EthDriver>) -> ! {
    runner.run().await
}

/// Polls the PHY for link changes; keeps the driver's link state and the MAC's
/// speed/duplex in sync with auto-negotiation results.
#[embassy_executor::task]
pub async fn eth_link_task(emac_ptr: *mut EthEmac) -> ! {
    let mut mdio = MdioController::new(Delay::new());
    let mut phy = Lan8720a::new(poe_iso::PHY_ADDR);
    let mut was_up = false;

    loop {
        match EMAC_STATE.update_link_from_phy(&mut phy, &mut mdio) {
            Ok(Some(status)) => {
                if !was_up {
                    println!("Ethernet link up: {:?} {:?}", status.speed, status.duplex);
                    // SAFETY: EMAC is static; this is the only task touching
                    // speed/duplex after init.
                    unsafe {
                        let emac = &mut *emac_ptr;
                        emac.set_speed(status.speed);
                        emac.set_duplex(status.duplex);
                    }
                    was_up = true;
                }
            }
            Ok(None) => {
                if was_up {
                    println!("Ethernet link down");
                    was_up = false;
                }
            }
            Err(e) => println!("Ethernet PHY poll error: {:?}", e),
        }

        Timer::after(ETH_LINK_POLL_INTERVAL).await;
    }
}

/// Starts DHCP once the link is up and re-arms it if the lease is lost.
///
/// Promiscuous mode is held only while waiting for a lease (mirrors the
/// ph-esp32-mac embassy-net example, where MAC filtering during DHCP was
/// unreliable on real hardware).
#[embassy_executor::task]
pub async fn eth_dhcp_task(stack: Stack<'static>, emac_ptr: *mut EthEmac) -> ! {
    let mut promisc = false;
    loop {
        stack.wait_link_up().await;
        println!("Ethernet link up — starting DHCP");
        Timer::after(embassy_time::Duration::from_secs(1)).await;

        if !promisc {
            // SAFETY: EMAC is static; only this task toggles promiscuous mode.
            unsafe { (*emac_ptr).set_promiscuous(true) };
            promisc = true;
        }

        stack.set_config_v4(ConfigV4::Dhcp(DhcpConfig::default()));

        loop {
            if let Some(config) = stack.config_v4() {
                println!("DHCP acquired: {}", config.address);
                if promisc {
                    // SAFETY: as above.
                    unsafe { (*emac_ptr).set_promiscuous(false) };
                    promisc = false;
                }
                break;
            }
            Timer::after(embassy_time::Duration::from_secs(1)).await;
        }

        stack.wait_config_down().await;
        println!("DHCP configuration lost — waiting for link");
    }
}
