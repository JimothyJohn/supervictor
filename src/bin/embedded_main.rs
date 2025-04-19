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
use esp_mbedtls::{asynch::Session, Certificates, Mode, TlsVersion};
use esp_mbedtls::{Tls, X509};
use esp_println::println;
use esp_wifi::{init, EspWifiController};

use supervictor::models::UplinkMessage;
use supervictor::network::embedded::{connection, net_task};
use supervictor::network::http::{get_request, post_request};

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
    let _json_body: heapless::String<128> = match serde_json_core::to_string(&UplinkMessage {
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

    let address = match stack
        .dns_query(env!("HOST"), embassy_net::dns::DnsQueryType::A)
        .await
    {
        Ok(addresses) => {
            if let Some(first_addr) = addresses.first() {
                println!("Resolved address: {:?}", first_addr);
                *first_addr
            } else {
                println!("No addresses returned");
                panic!("No addresses returned from DNS query");
            }
        }
        Err(e) => {
            println!("DNS resolution failed: {:?}", e);
            panic!("Could not resolve hostname");
        }
    };

    // Now you can use the address with port 443 to create your endpoint
    let remote_endpoint = (address, 443);

    let mut rx_buffer = [0u8; 4096];
    let mut tx_buffer = [0u8; 4096];

    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

    socket.set_timeout(Some(Duration::from_secs(10)));

    println!("connecting...");
    let r = socket.connect(remote_endpoint).await;
    if let Err(e) = r {
        println!("connect error: {:?}", e);
        #[allow(clippy::empty_loop)]
        loop {}
    }

    // Add before creating the certificates
    println!("Checking certificate files:");
    let ca_contents = include_str!("../../aws/letsencrypt_chain.pem");
    println!("CA cert: {} bytes", ca_contents.len());
    let cert_contents = include_str!("../../aws/debian.cert.pem");
    println!("Client cert: {} bytes", cert_contents.len());
    let key_contents = include_str!("../../aws/debian.private.key");
    println!("Private key: {} bytes", key_contents.len());

    // Print the first 50 characters of each file to debug format issues
    if !ca_contents.is_empty() {
        let preview = &ca_contents[0..core::cmp::min(50, ca_contents.len())];
        println!("CA cert starts with: {}", preview);
    }
    // Repeat for other files

    // Debug certificate parsing
    println!("Parsing certificates:");
    let ca_chain = X509::pem(concat!(include_str!("../../aws/letsencrypt.pem"), "\0").as_bytes());
    println!("CA chain loaded: {}", ca_chain.is_ok());

    // Load certificate and private key separately
    let client_cert =
        X509::pem(concat!(include_str!("../../aws/debian.cert.pem"), "\0").as_bytes());
    println!("Client certificate loaded: {}", client_cert.is_ok());

    let private_key =
        X509::pem(concat!(include_str!("../../aws/debian.private.key"), "\0").as_bytes());
    println!("Private key loaded: {}", private_key.is_ok());

    let certificates = Certificates {
        ca_chain: ca_chain.ok(),
        certificate: client_cert.ok(),
        private_key: private_key.ok(),
        password: None,
    };

    // Check if certificates were properly loaded
    println!("Certificate check:");
    println!("CA chain: {}", certificates.ca_chain.is_some());
    println!("Certificate: {}", certificates.certificate.is_some());
    println!("Private key: {}", certificates.private_key.is_some());
    println!("   ✅ All certificates present");

    println!("1. Creating TLS context");
    let mut tls = match Tls::new(peripherals.SHA) {
        Ok(t) => {
            println!("   ✅ TLS context created successfully");
            t
        }
        Err(e) => {
            println!("   ❌ Failed to create TLS context: {:?}", e);
            panic!("TLS context creation failed");
        }
    };

    // Set highest debug level
    println!("2. Setting TLS debug level");
    tls.set_debug(4);
    println!("   ✅ Debug level set");

    // Check certificates
    println!("3. Checking certificates");
    if certificates.ca_chain.is_none() {
        println!("   ❌ CA certificate is missing");
        panic!("CA certificate missing");
    }
    if certificates.certificate.is_none() {
        println!("   ❌ Client certificate is missing");
        panic!("Client certificate missing");
    }
    if certificates.private_key.is_none() {
        println!("   ❌ Private key is missing");
        panic!("Private key missing");
    }
    println!("   ✅ All certificates present");

    // Verify hostname
    println!("4. Preparing hostname: {}", env!("HOST"));
    static HOST_BYTES: &[u8] = concat!(env!("HOST"), "\0").as_bytes();
    println!("   Host bytes length: {}", HOST_BYTES.len() - 1); // -1 for null terminator
    let host_cstr = unsafe { CStr::from_bytes_with_nul_unchecked(HOST_BYTES) };
    println!("   ✅ Hostname prepared");

    // Print remote endpoint details
    println!("5. Remote endpoint: {:?}:{}", address, 443);

    // Try multiple TLS versions
    println!("6. Creating TLS session with TLS 1.3");
    let mut session = match Session::new(
        &mut socket,
        Mode::Client {
            servername: host_cstr,
        },
        TlsVersion::Tls1_3, // Using TLS 1.3 as per the code
        certificates,
        tls.reference(),
    ) {
        Ok(s) => {
            println!("   ✅ TLS session created successfully");
            s
        }
        Err(e) => {
            println!("   ❌ Failed to create TLS session: {:?}", e);
            panic!("Session creation failed");
        }
    };

    // Connect with timeout handling
    println!("8. Starting TLS connect");
    match embassy_time::with_timeout(
        Duration::from_secs(15), // 15 second timeout
        session.connect(),
    )
    .await
    {
        Ok(Ok(_)) => {
            println!("   ✅ TLS connected successfully!");
        }
        Ok(Err(e)) => {
            println!("   ❌ TLS connect error: {:?}", e);
            panic!("TLS connect failed");
        }
        Err(_) => {
            println!("   ❌ TLS connect timed out after 15 seconds");
            panic!("TLS connect timeout");
        }
    };

    // Try sending a simple HTTP request to verify the connection
    println!("10. Sending simple HTTP request");
    let request = post_request(env!("HOST"), &_json_body, None);
    match session.write(request.as_bytes()).await {
        Ok(written) => {
            if written == request.len() {
                println!("   ✅ Request sent successfully ({} bytes)", written);
            } else {
                println!("   ⚠️ Only wrote {} of {} bytes", written, request.len());
            }
        }
        Err(e) => println!("   ❌ Failed to send request: {:?}", e),
    };

    // Try to read response
    println!("11. Reading response");
    let mut buffer = [0u8; 1024];
    match embassy_time::with_timeout(Duration::from_secs(5), session.read(&mut buffer)).await {
        Ok(Ok(n)) => {
            println!("   ✅ Read {} bytes", n);
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

    #[allow(clippy::empty_loop)]
    loop {
        /*
        // Use _json_body if/when re-enabling this
        post_request_reqwless(&stack, _tls_seed, env!("HOST"), &_json_body).await;
        Timer::after(Duration::from_millis(3_000)).await;
        */
        println!("Looping!");
        Timer::after(Duration::from_millis(1_000)).await;
    }
}
