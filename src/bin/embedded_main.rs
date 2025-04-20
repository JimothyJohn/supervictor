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

use embassy_net::tcp::TcpSocket;
use embassy_net::StackResources;
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::{clock::CpuClock, rng::Rng, timer::timg::TimerGroup};
use esp_mbedtls::Tls;
use esp_mbedtls::{asynch::Session, Mode, TlsVersion};
use esp_println::println;
use esp_wifi::{init, EspWifiController};

use supervictor::models::UplinkMessage;
use supervictor::network::http::post_request;
use supervictor::network::tls::load_certificates;
use supervictor::network::utils::{connection, net_task};

use core::ffi::CStr;

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
    esp_alloc::heap_allocator!(size: 144 * 1024);

    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let mut rng = Rng::new(peripherals.RNG);

    // Uses bit shifting to convert a 32-bit random to a 64-bit, pretty smart!
    let _tls_seed = (rng.random() as u64) << 32 | rng.random() as u64;
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

    // json_body is unused for now, prefixed with underscore
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

    // Now you can use the address with port 443 to create your endpoint
    let remote_endpoint = (
        match stack
            .dns_query(env!("HOST"), embassy_net::dns::DnsQueryType::A)
            .await
        {
            Ok(addresses) => {
                if let Some(first_addr) = addresses.first() {
                    *first_addr
                } else {
                    panic!("No addresses returned from DNS query");
                }
            }
            Err(e) => {
                panic!("DNS resolution failed: {:?}", e);
            }
        },
        443,
    );

    let mut rx_buffer = [0u8; 4096];
    let mut tx_buffer = [0u8; 4096];

    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

    socket.set_timeout(Some(Duration::from_secs(10)));

    println!("Connecting...");
    let r = socket.connect(remote_endpoint).await;
    if let Err(e) = r {
        println!("connect error: {:?}", e);
        #[allow(clippy::empty_loop)]
        loop {}
    }

    // https://github.com/esp-rs/esp-mbedtls/blob/main/examples/async_client.rs
    let mut tls = match Tls::new(peripherals.SHA) {
        Ok(t) => t,
        Err(e) => {
            panic!("   ❌ Failed to create TLS context: {:?}", e);
        }
    };

    // Set highest debug level
    tls.set_debug(0);

    let mut session = match Session::new(
        &mut socket,
        Mode::Client {
            servername: match CStr::from_bytes_with_nul(concat!(env!("HOST"), "\0").as_bytes()) {
                Ok(cstr) => {
                    cstr // Assign the valid &CStr
                }
                Err(e) => {
                    panic!("   ❌ FATAL: Invalid HOST environment variable ('{}'): Must not contain null bytes. Error: {:?}",
                env!("HOST"), e);
                }
            }, // AI-Generated comment: Pass the validated, safe host_cstr
        },
        TlsVersion::Tls1_3, // Using TLS 1.3 as per the code
        load_certificates(),
        tls.reference(),
    ) {
        Ok(s) => s,
        Err(e) => {
            panic!("   ❌ Failed to create TLS session: {:?}", e);
        }
    };

    // Connect with timeout handling
    match embassy_time::with_timeout(
        Duration::from_secs(15), // 15 second timeout
        session.connect(),
    )
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            panic!("   ❌ TLS connect error: {:?}", e);
        }
        Err(_) => {
            panic!("   ❌ TLS connect timed out after 15 seconds");
        }
    };

    loop {
        // Try sending a simple HTTP request to verify the connection
        let request = post_request(env!("HOST"), &json_body, None);
        match session.write(request.as_bytes()).await {
            Ok(written) => {
                if written != request.len() {
                    println!("   ⚠️ Only wrote {} of {} bytes", written, request.len());
                }
            }
            Err(e) => println!("   ❌ Failed to send request: {:?}", e),
        };

        // Try to read response
        let mut buffer = [0u8; 1024];
        match embassy_time::with_timeout(Duration::from_secs(5), session.read(&mut buffer)).await {
            Ok(Ok(n)) => {
                if n > 0 {
                    match core::str::from_utf8(&buffer[..n]) {
                        Ok(s) => println!("   Response: {}", s),
                        Err(_) => println!("   Response not UTF-8 (binary data)"),
                    }
                } else {
                    println!("   Empty response (0 bytes)");
                }
            }
            Ok(Err(e)) => println!("   ❌ Read failed: {:?}", e),
            Err(_) => println!("   ❌ Read timed out"),
        };
        Timer::after(Duration::from_millis(1_000)).await;
    }
}
