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
use esp_hal::{clock::CpuClock, rng::Rng, timer::systimer::SystemTimer, timer::timg::TimerGroup};
use esp_mbedtls::Tls;
use esp_println::println;

use esp_wifi::{init, EspWifiController};

use supervictor::config::*;
use supervictor::tasks::{connection, net_task, new_main};

macro_rules! make_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

#[esp_hal_embassy::main]
async fn main(spawner: embassy_executor::Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    // TODO: Optimize this once able
    esp_alloc::heap_allocator!(size: HEAP_SIZE);

    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let mut rng = Rng::new(peripherals.RNG);

    // Uses bit shifting to convert a 32-bit random to a 64-bit, pretty smart!
    let net_seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // AI-Generated comment: Initialize WiFi controller using match for error handling.
    let wifi_init_result = init(timg0.timer0, rng, peripherals.RADIO_CLK);
    let wifi_ctrl = match wifi_init_result {
        Ok(ctrl) => ctrl, // AI-Generated comment: Successfully initialized controller.
        Err(e) => {
            // AI-Generated comment: Log the specific initialization error and panic.
            println!("   ❌ FATAL: Failed to initialize WiFi driver: {:?}", e);
            panic!("WiFi driver initialization failed");
        }
    };
    // AI-Generated comment: Place the initialized controller into static storage.
    let esp_wifi_ctrl = &*make_static!(EspWifiController<'static>, wifi_ctrl);
    println!("   ✅ WiFi Controller initialized and stored.");

    let (controller, interfaces) = match esp_wifi::wifi::new(esp_wifi_ctrl, peripherals.WIFI) {
        Ok((controller, interfaces)) => (controller, interfaces),
        Err(e) => {
            println!("   ❌ FATAL: Failed to initialize WiFi: {:?}", e);
            panic!("WiFi initialization failed");
        }
    };

    let systimer = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(systimer.alarm0);
    println!("   ℹ️ Embassy HAL initialized.");

    let (stack, runner) = embassy_net::new(
        interfaces.sta,
        embassy_net::Config::dhcpv4(Default::default()),
        make_static!(StackResources<3>, StackResources::<3>::new()),
        net_seed,
    );

    // https://github.com/esp-rs/esp-mbedtls/blob/main/examples/async_client.rs
    let mut tls = match Tls::new(peripherals.SHA) {
        Ok(t) => t,
        Err(e) => {
            println!("Failed to create TLS context: {:?}", e);
            panic!("Failed to create TLS context: {:?}", e);
        }
    };

    // TODO: Reduce this once we have a working program
    tls.set_debug(TLS_DEBUG_LEVEL);

    spawner
        .spawn(connection(controller, env!("SSID"), env!("PASSWORD")))
        .ok();
    spawner.spawn(net_task(runner)).ok();
    spawner.spawn(new_main(stack, tls)).ok();

    loop {
        Timer::after(NETWORK_STATUS_POLL_INTERVAL).await;
    }
}
