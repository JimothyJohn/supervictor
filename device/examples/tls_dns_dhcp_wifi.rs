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

use embassy_net::tcp::TcpSocket;
use embassy_net::{Runner, StackResources};
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::{clock::CpuClock, rng::Rng, timer::timg::TimerGroup};
use mbedtls_rs::Tls;
use mbedtls_rs::{asynch::Session, Mode, TlsVersion};
use esp_println::println;
use esp_wifi::wifi::{
    ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState,
};
use esp_wifi::{init, EspWifiController};

// use supervictor::models::UplinkMessage;
// use supervictor::network::http::post_request;
//use supervictor::network::tls::load_certificates;
// use supervictor::network::utils::{connection, net_task};

macro_rules! make_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

// AI-Generated comment: Import necessary items from heapless and serde.
use core::fmt::Write;
use heapless::{FnvIndexMap, String as HString};
use serde::Serialize; // Import write macro trait for converting numbers to string

// AI-Generated comment: Function to create a map with device data and serialize it to JSON.
// Returns a heapless String containing the JSON payload.
// Capacities are set here: Key=8 bytes, Value=16 bytes, Map entries=2, Output JSON=128 bytes.
// Adjust these capacities based on actual data needs.
fn create_and_serialize_map() -> HString<128> {
    println!("   ℹ️ Creating and serializing data map...");

    // AI-Generated comment: Define the map type alias within the function scope.
    type DataMap<'a> = FnvIndexMap<HString<8>, HString<16>, 2>;

    // AI-Generated comment: Create an instance of the map.
    let mut data_map = DataMap::new();

    // AI-Generated comment: Insert 'id' field. Using expect for brevity, handle Result properly in production.
    data_map
        .insert(
            "id".try_into().expect("id key too long"),
            "1234567890".try_into().expect("id value too long"),
        )
        .map_err(|_| println!("   ❌ Failed to insert 'id' into map (capacity full?)"))
        .ok(); // AI-Generated comment: .ok() converts Result to Option, discarding error value but allowing chaining/logging.

    // AI-Generated comment: Convert the integer value to a heapless String.
    let mut current_val_str: HString<16> = HString::new();
    // AI-Generated comment: Use write! macro to format the integer into the string buffer.
    write!(current_val_str, "{}", 100).expect("Failed to format integer into HString<16>");
    // AI-Generated comment: Insert 'current' field.
    data_map
        .insert(
            "current".try_into().expect("current key too long"),
            current_val_str,
        )
        .map_err(|_| println!("   ❌ Failed to insert 'current' into map (capacity full?)"))
        .ok();

    // AI-Generated comment: Serialize the map to a JSON string using serde_json_core.
    let json_body: HString<128> = match serde_json_core::to_string(&data_map) {
        Ok(body) => {
            println!("   ✅ Map serialized to JSON: {}", body);
            body
        }
        Err(e) => {
            // AI-Generated comment: Log serialization error and provide a default empty JSON object string.
            println!("   ❌ Error serializing map to JSON: {:?}", e);
            // AI-Generated comment: Use expect for the default value conversion, ensuring it fits.
            "{ }"
                .try_into()
                .expect("Default JSON string '{ }' doesn't fit HString<128>")
        }
    };

    json_body // AI-Generated comment: Return the serialized JSON string.
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

    // AI-Generated comment: Call the function to create and serialize the data map.
    let json_body: heapless::String<128> = create_and_serialize_map();

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
        env!("PORT").parse::<u16>().unwrap(),
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

    // https://github.com/esp-rs/mbedtls-rs/blob/main/examples/async_client.rs
    let mut tls = match Tls::new(peripherals.SHA) {
        Ok(t) => t,
        Err(e) => {
            println!("Failed to create TLS context: {:?}", e);
            panic!("Failed to create TLS context: {:?}", e);
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
                    println!("Invalid HOST environment variable ('{}'): Must not contain null bytes. Error: {:?}",
                env!("HOST"), e);
                    panic!("Invalid HOST environment variable ('{}'): Must not contain null bytes. Error: {:?}",
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
            println!("Failed to create TLS session: {:?}", e);
            panic!("Failed to create TLS session: {:?}", e);
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

#[embassy_executor::task]
pub async fn connection(
    mut controller: WifiController<'static>,
    ssid: &'static str,
    password: &'static str,
) {
    loop {
        if esp_wifi::wifi::wifi_state() == WifiState::StaConnected {
            // wait until we're no longer connected
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            Timer::after(Duration::from_millis(5000)).await
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: ssid.try_into().unwrap(),
                password: password.try_into().unwrap(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            controller.start_async().await.unwrap();
        }

        match controller.connect_async().await {
            Ok(_) => println!("Connected to Wifi!"),
            Err(e) => {
                println!("Failed to connect to Wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

use mbedtls_rs::{Certificates, X509};

pub fn load_certificates() -> Certificates<'static> {
    // AI-Generated comment: Load the CA chain certificate data at compile time.
    // AI-Generated comment: The concat! macro appends a null byte, required by X509::pem.
    // AI-Generated comment: as_bytes() gets a reference to the static byte slice.
    let ca_chain_bytes = concat!(include_str!("../../aws/letsencrypt.pem"), "\0").as_bytes();
    // AI-Generated comment: Create the X509 object, borrowing the static data. Returns Option<X509<'static>>.
    let ca_chain = X509::pem(ca_chain_bytes);

    // AI-Generated comment: Load the client certificate data at compile time.
    let client_cert_bytes = concat!(include_str!("../../aws/debian.cert.pem"), "\0").as_bytes();
    // AI-Generated comment: Create the X509 object, borrowing the static data.
    let client_cert = X509::pem(client_cert_bytes);

    // AI-Generated comment: Load the private key data at compile time.
    let private_key_bytes = concat!(include_str!("../../aws/debian.private.key"), "\0").as_bytes();
    // AI-Generated comment: Create the X509 object, borrowing the static data.
    let private_key = X509::pem(private_key_bytes);

    // AI-Generated comment: Construct the Certificates struct.
    // AI-Generated comment: Use .ok() to convert Result<X509, _> to Option<X509>.
    // AI-Generated comment: The resulting Certificates struct has a 'static lifetime.
    Certificates {
        ca_chain: ca_chain.ok(),
        certificate: client_cert.ok(),
        private_key: private_key.ok(),
        password: None, // AI-Generated comment: No password needed for these keys.
    }
}

/// Create an HTTP POST request with JSON body
pub fn post_request<T>(host: &str, data: &T, path: Option<&str>) -> HString<512>
where
    T: Serialize,
{
    let mut request = HString::<512>::new();

    // Format the request path
    let endpoint = path.unwrap_or("/");

    // Start building the request
    request.push_str("POST ").unwrap();
    request.push_str(endpoint).unwrap();
    request.push_str(" HTTP/1.1\r\n").unwrap();
    request.push_str("Host: ").unwrap();
    request.push_str(host).unwrap();
    request.push_str("\r\n").unwrap();
    request
        .push_str("Content-Type: application/json\r\n")
        .unwrap();

    // Serialize the data to JSON
    let json_result = serde_json_core::to_string::<T, 256>(data);

    match json_result {
        Ok(json) => {
            // Add Content-Length header
            request.push_str("Content-Length: ").unwrap();

            // Convert length to string - simplified approach
            let len = json.len();
            // For most HTTP requests, content length will be small
            // This handles up to 5 digits (lengths up to 99999)
            let mut buffer = [0u8; 5];
            let mut i = 0;

            // Handle zero case
            if len == 0 {
                request.push_str("0").unwrap();
            } else {
                // Convert number to digits
                let mut n = len;
                while n > 0 {
                    buffer[i] = (n % 10) as u8 + b'0';
                    n /= 10;
                    i += 1;
                }

                // Add digits in reverse order
                while i > 0 {
                    i -= 1;
                    request.push(buffer[i] as char).unwrap();
                }
            }

            request.push_str("\r\n\r\n").unwrap();

            // Add the JSON body
            request.push_str(&json).unwrap();
        }
        Err(_) => {
            // Handle serialization error
            request.push_str("Content-Length: 0\r\n\r\n").unwrap();
        }
    }

    request
}
