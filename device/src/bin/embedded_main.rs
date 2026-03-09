//!
//! Set SSID and PASSWORD env variable before running this example.
//!
//! Because of the huge task-arena size configured this won't work on ESP32-S2
//!
//% CHIPS: esp32 esp32s2 esp32s3 esp32c2 esp32c3 esp32c6

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use embassy_net::StackResources;
use embassy_time::Timer;
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::rng::{Rng, Trng, TrngSource};
use esp_hal::timer::timg::TimerGroup;
use esp_println::println;
use esp_radio::wifi::{self, ClientConfig, ModeConfig};
use mbedtls_rs::Tls;

use supervictor::app::tasks::{app, connection, net_task};
use supervictor::config::*;

esp_bootloader_esp_idf::esp_app_desc!();

// Magically convert a variable into a static variable
macro_rules! make_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    esp_alloc::heap_allocator!(size: HEAP_SIZE);

    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let timg0 = TimerGroup::new(peripherals.TIMG0);

    // Enable true RNG (needs RNG + ADC1 peripherals for entropy)
    let _trng_source = TrngSource::new(peripherals.RNG, peripherals.ADC1);
    let rng = Rng::new();
    let net_seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // esp-rtos requires explicit start on RISC-V with SoftwareInterrupt<0>
    let software_interrupt = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, software_interrupt.software_interrupt0);

    // Initialize esp-radio controller (requires esp-rtos started)
    let esp_radio_ctrl = &*make_static!(
        esp_radio::Controller<'static>,
        match esp_radio::init() {
            Ok(ctrl) => ctrl,
            Err(e) => {
                println!("FATAL: Failed to initialize esp-radio: {:?}", e);
                panic!("esp-radio initialization failed");
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
        match wifi::new(&esp_radio_ctrl, peripherals.WIFI, wifi::Config::default()) {
            Ok(r) => r,
            Err(e) => {
                println!("FATAL: Failed to initialize WiFi: {:?}", e);
                panic!("WiFi initialization failed");
            }
        };

    // Apply WiFi config before spawning tasks
    if let Err(e) = controller.set_config(&client_config) {
        println!("FATAL: WiFi set_config failed: {:?}", e);
        panic!("WiFi set_config failed");
    }

    // Initialize the network stack
    let (stack, runner) = embassy_net::new(
        interfaces.sta,
        embassy_net::Config::dhcpv4(Default::default()),
        make_static!(StackResources<3>, StackResources::<3>::new()),
        net_seed,
    );

    // Initialize TLS — mbedtls-rs needs Trng (implements CryptoRng)
    let trng = Trng::try_new().expect("TrngSource must be active");
    let trng_static = make_static!(Trng, trng);
    let mut tls = match Tls::new(trng_static) {
        Ok(t) => t,
        Err(e) => {
            println!("Failed to create TLS context: {:?}", e);
            panic!("Failed to create TLS context: {:?}", e);
        }
    };
    tls.set_debug(TLS_DEBUG_LEVEL);

    spawner.spawn(connection(controller)).ok();
    spawner.spawn(net_task(runner)).ok();
    spawner.spawn(app(stack, tls)).ok();

    loop {
        Timer::after(NETWORK_STATUS_POLL_INTERVAL).await;
    }
}
