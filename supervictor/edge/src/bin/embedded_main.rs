//! Supervictor embedded firmware for ESP32-family boards (primary target:
//! the Olimex ESP32-POE-ISO; also runs on ESP32-C3 devkits).
//!
//! Uplinks telemetry over mTLS. The chip (`chip-esp32` / `chip-esp32c3`) and
//! network interface are chosen at compile time: `ethernet` (primary — the
//! POE-ISO is PoE-powered, wired is the main connection; implies chip-esp32)
//! or `wifi` (fallback for boards deployed off-cable, any chip).
//!
//! WiFi builds read SSID/PASSWORD from the environment at compile time.

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

#[cfg(not(any(feature = "ethernet", feature = "wifi")))]
compile_error!(
    "supervictor-embedded needs a network interface: build with --features ethernet (primary) or --features wifi"
);

use embassy_time::Timer;
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
#[cfg(feature = "chip-esp32c3")]
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::rng::{Rng, Trng, TrngSource};
use esp_hal::rtc_cntl::{Rtc, RwdtStage};
use esp_hal::timer::timg::TimerGroup;
use esp_println::println;
use mbedtls_rs::Tls;

use supervictor::app::tasks::app;
use supervictor::config::*;

#[cfg(feature = "ethernet")]
use esp_hal::gpio::{Level, Output, OutputConfig};
#[cfg(feature = "espnow")]
use supervictor::app::tasks::DEVICE_ID_ON_WIRE;
#[cfg(feature = "espnow")]
use supervictor::network::espnow;
#[cfg(feature = "ethernet")]
use supervictor::network::ethernet;

#[cfg(all(feature = "wifi", not(feature = "ethernet")))]
use embassy_net::StackResources;
#[cfg(all(feature = "wifi", not(feature = "ethernet")))]
use esp_radio::wifi::{self, ClientConfig, ModeConfig};
#[cfg(all(feature = "wifi", not(feature = "ethernet")))]
use supervictor::app::tasks::{connection, net_task};

esp_bootloader_esp_idf::esp_app_desc!();

/// Halt on fatal init error — loops forever so the watchdog can reset the device.
async fn halt() -> ! {
    println!("Halting. Device will restart if watchdog is enabled.");
    loop {
        Timer::after(embassy_time::Duration::from_secs(60)).await;
    }
}

// Magically convert a variable into a static variable
macro_rules! make_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write($val);
        x
    }};
}

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    esp_alloc::heap_allocator!(size: HEAP_SIZE);

    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let timg0 = TimerGroup::new(peripherals.TIMG0);

    // RTC watchdog, armed before radio/network init: it sits in the RTC
    // power domain, so it fires even when both cores wedge (the esp-radio
    // silent-hang mode on classic ESP32). Fed only on successful uplink
    // round-trips in app() — any failure that stops uplinks resets the board.
    let mut rwdt = Rtc::new(peripherals.LPWR).rwdt;
    rwdt.set_timeout(
        RwdtStage::Stage0,
        esp_hal::time::Duration::from_secs(WATCHDOG_TIMEOUT_S),
    );
    rwdt.enable();

    // Enable true RNG (needs RNG + ADC1 peripherals for entropy)
    let _trng_source = TrngSource::new(peripherals.RNG, peripherals.ADC1);
    let rng = Rng::new();
    let net_seed = (rng.random() as u64) << 32 | rng.random() as u64;
    let sim_seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // On Xtensa esp-rtos starts without a software interrupt; on riscv it
    // needs SoftwareInterrupt<0> for its scheduler.
    #[cfg(feature = "chip-esp32")]
    esp_rtos::start(timg0.timer0);
    #[cfg(feature = "chip-esp32c3")]
    esp_rtos::start(
        timg0.timer0,
        SoftwareInterruptControl::new(peripherals.SW_INTERRUPT).software_interrupt0,
    );

    // --- Ethernet (primary): EMAC + LAN8710A over RMII, PoE-powered ---
    #[cfg(feature = "ethernet")]
    let stack = {
        let phy_power = Output::new(peripherals.GPIO12, Level::Low, OutputConfig::default());
        let (stack, runner, emac_ptr) = match ethernet::init(phy_power, net_seed) {
            Ok(parts) => parts,
            Err(e) => {
                println!("FATAL: Ethernet init failed: {:?}", e);
                halt().await;
            }
        };
        spawner.spawn(ethernet::eth_net_task(runner)).ok();
        spawner.spawn(ethernet::eth_link_task(emac_ptr)).ok();
        spawner.spawn(ethernet::eth_dhcp_task(stack, emac_ptr)).ok();
        stack
    };

    // --- WiFi (fallback interface) ---
    #[cfg(all(feature = "wifi", not(feature = "ethernet")))]
    let stack = {
        // Initialize esp-radio controller (requires esp-rtos started)
        let esp_radio_ctrl = &*make_static!(
            esp_radio::Controller<'static>,
            match esp_radio::init() {
                Ok(ctrl) => ctrl,
                Err(e) => {
                    println!("FATAL: Failed to initialize esp-radio: {:?}", e);
                    halt().await;
                }
            }
        );

        // WiFi station config
        let client_config = ModeConfig::Client(
            ClientConfig::default()
                .with_ssid(env!("SSID").into())
                .with_password(env!("PASSWORD").into()),
        );
        let (mut controller, interfaces) =
            match wifi::new(esp_radio_ctrl, peripherals.WIFI, wifi::Config::default()) {
                Ok(r) => r,
                Err(e) => {
                    println!("FATAL: Failed to initialize WiFi: {:?}", e);
                    halt().await;
                }
            };

        // Apply WiFi config before spawning tasks
        if let Err(e) = controller.set_config(&client_config) {
            println!("FATAL: WiFi set_config failed: {:?}", e);
            halt().await;
        }

        // Initialize the network stack
        let (stack, runner) = embassy_net::new(
            interfaces.sta,
            embassy_net::Config::dhcpv4(Default::default()),
            make_static!(StackResources<3>, StackResources::<3>::new()),
            net_seed,
        );

        spawner.spawn(connection(controller)).ok();
        spawner.spawn(net_task(runner)).ok();
        stack
    };

    // --- ESP-NOW beside the EMAC (coexistence spike, TODO.md P2) ---
    // Radio init comes after the wired stack so a radio failure can never
    // take the uplink down with it; the uplink keeps feeding the watchdog.
    #[cfg(feature = "espnow")]
    {
        let radio = &*make_static!(
            esp_radio::Controller<'static>,
            match esp_radio::init() {
                Ok(ctrl) => ctrl,
                Err(e) => {
                    println!("FATAL: Failed to initialize esp-radio: {:?}", e);
                    halt().await;
                }
            }
        );
        match espnow::init(radio, peripherals.WIFI) {
            Ok((controller, esp_now)) => {
                // Dropping the controller deinits the radio: park it for good.
                let _ = make_static!(esp_radio::wifi::WifiController<'static>, controller);
                let (_manager, sender, receiver) = esp_now.split();
                spawner
                    .spawn(espnow::espnow_beacon_task(sender, DEVICE_ID_ON_WIRE))
                    .ok();
                spawner.spawn(espnow::espnow_listen_task(receiver)).ok();
                println!(
                    "ESP-NOW up on channel {} as {} (heap used {} free {})",
                    ESPNOW_CHANNEL,
                    DEVICE_ID_ON_WIRE,
                    esp_alloc::HEAP.used(),
                    esp_alloc::HEAP.free()
                );
            }
            Err(e) => println!("ESP-NOW init failed: {:?} — wired uplink continues", e),
        }
    }

    // Initialize TLS — mbedtls-rs needs Trng (implements CryptoRng)
    let trng = match Trng::try_new() {
        Ok(t) => t,
        Err(e) => {
            println!("FATAL: Failed to initialize TRNG: {:?}", e);
            halt().await;
        }
    };
    let trng_static = make_static!(Trng, trng);
    let mut tls = match Tls::new(trng_static) {
        Ok(t) => t,
        Err(e) => {
            println!("FATAL: Failed to create TLS context: {:?}", e);
            halt().await;
        }
    };
    tls.set_debug(TLS_DEBUG_LEVEL);

    spawner.spawn(app(stack, tls, rwdt, sim_seed)).ok();

    loop {
        Timer::after(NETWORK_STATUS_POLL_INTERVAL).await;
    }
}
