//! Supervictor embedded firmware for ESP32-family boards (primary target:
//! the Olimex ESP32-POE-ISO; also runs on ESP32-C3 devkits).
//!
//! Uplinks telemetry over mTLS. The chip (`chip-esp32` / `chip-esp32c3`) and
//! network interface are chosen at compile time: `ethernet` (primary — the
//! POE-ISO is PoE-powered, wired is the main connection; implies chip-esp32)
//! or `wifi` (fallback for boards deployed off-cable, any chip).
//!
//! SSID, password, endpoint host and device id come from the settings
//! record in the `nvs` partition; the values the image was built with
//! (SSID/PASSWORD/HOST/DEVICE_NAME) seed it on first boot.

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
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::rng::{Rng, Trng, TrngSource};
use esp_hal::rtc_cntl::{Rtc, RwdtStage};
use esp_hal::timer::timg::TimerGroup;
use esp_println::println;
use mbedtls_rs::Tls;

use supervictor::app::tasks::app;
use supervictor::config::*;
use supervictor::settings::Settings;

#[cfg(feature = "ethernet")]
use esp_hal::gpio::{Level, Output, OutputConfig};
#[cfg(feature = "espnow")]
use supervictor::network::espnow;
#[cfg(feature = "ethernet")]
use supervictor::network::ethernet;

#[cfg(all(feature = "wifi", not(feature = "ethernet")))]
use embassy_net::StackResources;
#[cfg(all(feature = "wifi", not(feature = "ethernet")))]
use esp_radio::wifi::{self, sta::StationConfig, ControllerConfig};
#[cfg(all(feature = "wifi", not(feature = "ethernet")))]
use supervictor::app::tasks::{connection, net_task};
#[cfg(all(feature = "wifi", not(feature = "ethernet")))]
use supervictor::network::portal;

// Application descriptor: the version is FIRMWARE_VERSION (FW_VERSION at
// build time, else the crate version) so `espflash board-info` and the
// OTA manifest agree on what a slot holds.
esp_bootloader_esp_idf::esp_app_desc!(
    supervictor::models::uplink::FIRMWARE_VERSION,
    env!("CARGO_PKG_NAME"),
    esp_bootloader_esp_idf::BUILD_TIME,
    esp_bootloader_esp_idf::BUILD_DATE,
    esp_bootloader_esp_idf::ESP_IDF_COMPATIBLE_VERSION,
    esp_bootloader_esp_idf::MMU_PAGE_SIZE,
    0,
    u16::MAX,
    esp_bootloader_esp_idf::SECURE_VERSION
);

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
    // Second heap region in the otherwise unused dram2 segment (config.rs).
    #[cfg(feature = "chip-esp32")]
    esp_alloc::heap_allocator!(#[link_section = ".dram2_uninit"] size: DRAM2_HEAP_SIZE);

    let mut peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
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

    // Settings (network, endpoint, device id) from the nvs partition,
    // seeded from the image on first boot (settings::device). Read on a
    // reborrow before OTA takes the flash peripheral for good; the record
    // lives in a static for the life of the board.
    let pt_buf = make_static!(
        [u8; esp_bootloader_esp_idf::partitions::PARTITION_TABLE_MAX_LEN],
        [0; esp_bootloader_esp_idf::partitions::PARTITION_TABLE_MAX_LEN]
    );
    let settings: &'static Settings = {
        let mut flash = esp_storage::FlashStorage::new(peripherals.FLASH.reborrow());
        make_static!(
            Settings,
            supervictor::settings::device::boot(&mut flash, pt_buf)
        )
    };
    if !settings.has_endpoint() {
        println!("FATAL: no endpoint host / device id on flash or in the image — unprovisioned; reflash with HOST and DEVICE_NAME set");
        halt().await;
    }

    // OTA: count this boot against an unconfirmed image (reverts + resets
    // when the budget is spent), before anything that could wedge. None on
    // the factory partition table — the board still uplinks.
    // Buffers and the state machine itself live in statics, not in main's
    // frame or the app task's future (see `ota::device::Ota`).
    let ota = supervictor::ota::device::Ota::new(
        peripherals.FLASH,
        pt_buf,
        make_static!(
            [u8; supervictor::ota::install::SECTOR],
            [0; supervictor::ota::install::SECTOR]
        ),
        settings,
    );
    let mut ota = ota.map(|o| &mut *make_static!(supervictor::ota::device::Ota, o));
    if let Some(o) = ota.as_mut() {
        o.boot_check();
    }

    // Enable true RNG (needs RNG + ADC1 peripherals for entropy)
    let _trng_source = TrngSource::new(peripherals.RNG, peripherals.ADC1);
    let rng = Rng::new();
    let net_seed = (rng.random() as u64) << 32 | rng.random() as u64;
    let sim_seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // esp-rtos 0.3 drives its scheduler from SoftwareInterrupt<0> on every
    // chip (0.2 only needed it on riscv).
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
        spawner.spawn(ethernet::eth_net_task(runner).unwrap());
        spawner.spawn(ethernet::eth_link_task(emac_ptr).unwrap());
        spawner.spawn(ethernet::eth_dhcp_task(stack, emac_ptr).unwrap());
        stack
    };

    // --- WiFi (fallback interface) ---
    #[cfg(all(feature = "wifi", not(feature = "ethernet")))]
    let stack = {
        // Provisioning portal (network/portal): no SSID anywhere, or the
        // previous boot never joined the one it has, and the board becomes
        // an access point serving the settings form instead of a station.
        // `run` reboots the board when it is done; nothing below runs.
        if let Some(reason) = portal::should_open(settings) {
            portal::run(
                spawner,
                peripherals.WIFI,
                settings,
                ota,
                rwdt,
                net_seed,
                reason,
            )
            .await
        }
        // Station boot: on the record. `connection()` upgrades it to
        // "joined" on association; a watchdog reset before that opens
        // the portal next boot.
        portal::mark_booted();

        // esp-radio 0.18: the radio comes up with its station config in
        // one step (no separate init/start); the controller is running
        // when `new` returns and `connection()` only has to associate.
        let station_config = wifi::Config::Station(
            StationConfig::default()
                .with_ssid(settings.ssid.as_str())
                .with_password(settings.password.as_str().into()),
        );
        let (controller, interfaces) = match wifi::new(
            peripherals.WIFI,
            ControllerConfig::default().with_initial_config(station_config),
        ) {
            Ok(r) => r,
            Err(e) => {
                println!("FATAL: Failed to initialize WiFi: {:?}", e);
                halt().await;
            }
        };

        // Initialize the network stack
        let (stack, runner) = embassy_net::new(
            interfaces.station,
            embassy_net::Config::dhcpv4(Default::default()),
            make_static!(StackResources<3>, StackResources::<3>::new()),
            net_seed,
        );

        spawner.spawn(connection(controller).unwrap());
        spawner.spawn(net_task(runner).unwrap());
        stack
    };

    // --- ESP-NOW beside the EMAC (the mesh's first module, docs/MESH.md) ---
    // Radio init comes after the wired stack so a radio failure can never
    // take the uplink down with it; the uplink keeps feeding the watchdog.
    #[cfg(feature = "espnow")]
    {
        match espnow::init(peripherals.WIFI) {
            Ok((controller, esp_now)) => {
                // Dropping the controller deinits the radio: park it for good.
                let _ = make_static!(esp_radio::wifi::WifiController<'static>, controller);
                let (_manager, sender, receiver) = esp_now.split();
                spawner.spawn(
                    espnow::espnow_beacon_task(sender, settings.device_name.as_str()).unwrap(),
                );
                spawner.spawn(espnow::espnow_listen_task(receiver).unwrap());
                println!(
                    "ESP-NOW up on channel {} as {} (heap used {} free {})",
                    ESPNOW_CHANNEL,
                    settings.device_name.as_str(),
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

    spawner.spawn(
        app(
            stack,
            tls,
            settings,
            rwdt,
            sim_seed,
            ota,
            make_static!([u8; TCP_RX_BUFFER_SIZE], [0; TCP_RX_BUFFER_SIZE]),
            make_static!([u8; TCP_TX_BUFFER_SIZE], [0; TCP_TX_BUFFER_SIZE]),
        )
        .unwrap(),
    );

    loop {
        Timer::after(NETWORK_STATUS_POLL_INTERVAL).await;
    }
}
