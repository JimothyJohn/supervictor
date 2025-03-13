// https://github.com/esp-rs/esp-hal/blob/main/examples/src/bin/wifi_embassy_dhcp.rs
//! Embassy DHCP Example
//!
//!
//! Set SSID and PASSWORD env variable before running this example.
//!
//! This gets an ip address via DHCP then performs an HTTP get request to some "random" server
//!
//! Because of the huge task-arena size configured this won't work on ESP32-S2

//% FEATURES: embassy esp-wifi esp-wifi/wifi esp-hal/unstable
//% CHIPS: esp32 esp32s2 esp32s3 esp32c2 esp32c3 esp32c6

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

// use embassy_net::dns::DnsSocket;
// use embassy_net::tcp::client::{TcpClient, TcpClientState};
// use embassy_net::{tcp::TcpSocket, StackResources};
use embassy_net::StackResources;
use embassy_time::{Duration, Timer};
// use embedded_io_async::Write;
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::{clock::CpuClock, rng::Rng, timer::timg::TimerGroup};
use esp_println::println;
use esp_wifi::{init, EspWifiController};
// use heapless::String as HString;

// use supervictor::models::RequestBody;
use supervictor::network::{access_website, connection, net_task};
use supervictor::utils::config_esp;

// When you are okay with using a nightly compiler it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! make_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");
// const HOST: &str = env!("HOST");

#[esp_hal_embassy::main]
async fn main(spawner: embassy_executor::Spawner) -> ! {
    // let mut rx_buffer = [0; 4096];
    // let mut tx_buffer = [0; 4096];
    // let mut buf = [0; 1024];

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

    spawner.spawn(connection(controller, SSID, PASSWORD)).ok();
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

    loop {
        access_website(&stack, tls_seed).await;
        /*
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10)));

        let r = socket
            .connect(supervictor::constants::endpoints::LOCAL_DEV)
            .await;
        if let Err(e) = r {
            println!("Connect error: {:?}", e);
            continue;
        }

        let data: RequestBody = RequestBody {
            body: HString::<64>::try_from("message").unwrap(),
        };

        let r = socket
            .write_all(post_request(HOST, &data, Some("/post")).as_bytes())
            .await;
        if let Err(e) = r {
            println!("Write error: {:?}", e);
            continue;
        }

        // Create a buffer to collect the complete HTTP response
        let mut http_buffer = heapless::String::<512>::new();

        // Read loop to collect the complete response
        let mut response_result = None;
        loop {
            let n = match socket.read(&mut buf).await {
                Ok(0) => {
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    println!("Read error: {:?}", e);
                    break;
                }
            };

            // Convert bytes to string and append to buffer
            if let Ok(str_data) = core::str::from_utf8(&buf[..n]) {
                // Append to HTTP buffer
                if http_buffer.push_str(str_data).is_err() {
                    println!("Buffer overflow, message too large");
                    break;
                }
            } else {
                println!("Invalid UTF-8 data received");
                break;
            }

            // Try to process the HTTP response
            match process_http_response(&http_buffer) {
                ok_result @ Ok(_) => {
                    response_result = Some(ok_result);
                    break;
                }
                Err(_) => {
                    // Continue reading more data
                }
            }
        }

        // Use the stored result
        if let Some(Ok(response)) = response_result {
            println!("Received: {}", response.message);
            // Do something with the response
        } else {
            println!("Failed to parse JSON");
        }
        */

        Timer::after(Duration::from_millis(3_000)).await;
    }
}
