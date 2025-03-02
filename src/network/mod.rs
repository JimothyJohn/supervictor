use embassy_net::Runner;
use embassy_time::{Duration, Timer};
use esp_println::println;
use esp_wifi::wifi::{
    ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState,
};

#[embassy_executor::task]
pub async fn connection(
    mut controller: WifiController<'static>,
    ssid: &'static str,
    password: &'static str,
) {
    // println!("Device capabilities: {:?}", controller.capabilities());

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

pub fn get_request(host: &str) -> heapless::String<128> {
    let mut request = heapless::String::<128>::new();
    const USER_AGENT: &str = "Uplink/0.1.0 (Platform; ESP32-C3)";

    request.push_str("GET / HTTP/1.0\r\nHost: ").unwrap();
    request.push_str(host).unwrap();
    request.push_str("\r\n").unwrap();
    request.push_str("User-Agent: ").unwrap();
    request.push_str(USER_AGENT).unwrap();
    request.push_str("\r\n").unwrap();
    request.push_str("Accept: */*\r\n").unwrap();
    request
}

/*
pub fn post_request(host: &str, body: &str) -> heapless::String<128> {
    let mut request = heapless::String::<128>::new();
    request.push_str("POST / HTTP/1.0\r\nHost: ").unwrap();
    request.push_str(host).unwrap();
    request.push_str("\r\n\r\n").unwrap();
    request
}
*/
