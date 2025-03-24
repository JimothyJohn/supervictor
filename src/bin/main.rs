// https://github.com/esp-rs/esp-hal/blob/main/examples/src/bin/wifi_embassy_dhcp.rs
//! Embassy DHCP Example
//!
//!
//! Set SSID and PASSWORD env variable before running this example.
//!
//! This gets an ip address via DHCP then performs an HTTP get request to an echo server
//!
//! Because of the huge task-arena size configured this won't work on ESP32-S2

//% FEATURES: embassy esp-wifi esp-wifi/wifi esp-hal/unstable
//% CHIPS: esp32 esp32s2 esp32s3 esp32c2 esp32c3 esp32c6

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use embassy_net::StackResources;
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::{clock::CpuClock, rng::Rng, timer::timg::TimerGroup};
use esp_println::println;
use esp_wifi::{init, EspWifiController};

use supervictor::models::UplinkMessage;
use supervictor::network::{connection, net_task, post_request};
use supervictor::utils::config_esp;

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
    config_esp();

    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let mut rng = Rng::new(peripherals.RNG);

    // Uses bit shifting to convert a 32-bit random to a 64-bit, pretty smart!
    let tls_seed = (rng.random() as u64) << 32 | rng.random() as u64;
    let net_seed = (rng.random() as u64) << 32 | rng.random() as u64;

    let esp_wifi_ctrl = &*make_static!(
        EspWifiController<'static>,
        init(timg0.timer0, rng, peripherals.RADIO_CLK).unwrap()
    );

    let (controller, interfaces) = esp_wifi::wifi::new(esp_wifi_ctrl, peripherals.WIFI).unwrap();

    let systimer = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(systimer.alarm0);

    // Init network stack
    let (stack, runner) = embassy_net::new(
        interfaces.sta,
        embassy_net::Config::dhcpv4(Default::default()),
        make_static!(StackResources<3>, StackResources::<3>::new()),
        net_seed,
    );

    spawner
        .spawn(connection(controller, env!("SSID"), env!("PASSWORD")))
        .ok();
    spawner.spawn(net_task(runner)).ok();

    loop {
        Timer::after(Duration::from_millis(2000)).await;
        if stack.is_link_up() {
            break;
        }
        println!("Initializing network stack...");
    }

    loop {
        Timer::after(Duration::from_millis(2000)).await;
        if let Some(_config) = stack.config_v4() {
            break;
        }
        println!("Waiting to get IP address...");
    }

    let json_body: heapless::String<128> = match serde_json_core::to_string(&UplinkMessage {
        // Don't use unwrap in production, use a fixed length string
        id: "1234567890".try_into().unwrap(),
        current: 100,
    }) {
        Ok(body) => body,
        Err(e) => {
            println!("Error serializing JSON: {:?}", e);
            let json_body: heapless::String<128> = "{}".try_into().unwrap();
            json_body
        }
    };

    loop {
        post_request(&stack, tls_seed, env!("HOST"), &json_body).await;
        Timer::after(Duration::from_millis(3_000)).await;
    }
}
