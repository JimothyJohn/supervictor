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

use core::ffi::CStr;

use embassy_net::{tcp::TcpSocket, StackResources};
use embassy_time::Timer;
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, rng::Rng, timer::systimer::SystemTimer, timer::timg::TimerGroup};
use esp_mbedtls::{asynch::Session, Mode, Tls, TlsVersion};
use esp_println::println;

use esp_wifi::{init, EspWifiController};

use supervictor::config::*;
use supervictor::models::UplinkMessage;
use supervictor::network::{http::post_request, tls::load_certificates};
use supervictor::utils::{connection, net_task};

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
    println!("   ℹ️ Network seed generated.");

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

    let (controller, interfaces) = esp_wifi::wifi::new(esp_wifi_ctrl, peripherals.WIFI).unwrap();

    let systimer = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(systimer.alarm0);
    println!("   ℹ️ Embassy HAL initialized.");

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
        Timer::after(NETWORK_STATUS_POLL_INTERVAL).await;
        if stack.is_link_up() {
            break;
        }
        println!("Initializing network stack...");
    }

    loop {
        Timer::after(NETWORK_STATUS_POLL_INTERVAL).await;
        if let Some(_config) = stack.config_v4() {
            break;
        }
        println!("Waiting to get IP address...");
    }

    // AI-Generated comment: Call the function to create and serialize the data map.
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
                    println!("No addresses returned from DNS query");
                    panic!("No addresses returned from DNS query");
                }
            }
            Err(e) => {
                println!("DNS resolution failed: {:?}", e);
                panic!("DNS resolution failed: {:?}", e);
            }
        },
        AWS_IOT_PORT,
    );

    let mut rx_buffer = [0u8; TCP_RX_BUFFER_SIZE];
    let mut tx_buffer = [0u8; TCP_TX_BUFFER_SIZE];

    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

    socket.set_timeout(Some(SOCKET_TIMEOUT));

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
            println!("Failed to create TLS context: {:?}", e);
            panic!("Failed to create TLS context: {:?}", e);
        }
    };

    // Set highest debug level
    // TODO: Reduce this once we have a working program
    tls.set_debug(TLS_DEBUG_LEVEL); // AI-Generated comment: Debug level is currently 0 (off). Set to 4 for verbose logs if needed.

    // AI-Generated comment: Create the CStr for the servername *before* Session::new.
    // This ensures the CStr reference lives long enough for the Session::new call.
    let host_cstr = match CStr::from_bytes_with_nul(concat!(env!("HOST"), "\0").as_bytes()) {
        Ok(cstr) => {
            println!("   ✅ Host CStr created for SNI."); // AI-Generated comment: Added log for success
            cstr // AI-Generated comment: Assign the valid &'static CStr
        }
        Err(e) => {
            // AI-Generated comment: Log and panic if HOST env var is invalid (contains null bytes).
            println!("   ❌ FATAL: Invalid HOST environment variable ('{}'): Must not contain null bytes. Error: {:?}",
                env!("HOST"), e);
            panic!("FATAL: Invalid HOST environment variable ('{}'): Must not contain null bytes. Error: {:?}",
                env!("HOST"), e);
        }
    };

    // AI-Generated comment: Load certificates. Ensure load_certificates returns the correct type.
    let certs = load_certificates();
    println!("   ℹ️ Certificates loaded."); // AI-Generated comment: Added log after loading

    // AI-Generated comment: Initialize the TLS session, passing the pre-validated host_cstr.
    let mut session = match Session::new(
        &mut socket,
        Mode::Client {
            servername: host_cstr, // AI-Generated comment: Pass the host_cstr variable here.
        },
        TlsVersion::Tls1_3, // Using TLS 1.3 as per the code
        certs,              // AI-Generated comment: Pass the loaded certificates.
        tls.reference(),    // AI-Generated comment: Pass a reference to the Tls context.
    ) {
        Ok(s) => {
            println!("   ✅ TLS Session structure created."); // AI-Generated comment: Added log for success
            s
        }
        Err(e) => {
            println!("   ❌ Failed to create TLS session: {:?}", e);
            panic!("Failed to create TLS session: {:?}", e);
        }
    };

    // AI-Generated comment: Connect with timeout handling.
    match embassy_time::with_timeout(
        TLS_HANDSHAKE_TIMEOUT, // 15 second timeout
        session.connect(),
    )
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            println!("TLS connect error: {:?}", e);
            panic!("TLS connect error: {:?}", e);
        }
        Err(_) => {
            println!("TLS connect timed out after 15 seconds");
            panic!("TLS connect timed out after 15 seconds");
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
        match embassy_time::with_timeout(HTTP_READ_TIMEOUT, session.read(&mut buffer)).await {
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
        Timer::after(MAIN_LOOP_DELAY).await;
    }
}
