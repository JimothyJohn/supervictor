//! ESP-NOW beside the wired uplink — the radio + EMAC coexistence spike
//! (GO, 2026-10-25) and the module the mesh in docs/MESH.md builds on.
//!
//! The WiFi radio comes up in station mode without joining any network and
//! ESP-NOW runs on it: a broadcast beacon every
//! [`ESPNOW_BEACON_INTERVAL`](crate::config::ESPNOW_BEACON_INTERVAL) and a
//! listener that logs every frame heard with the sender's MAC and RSSI.
//! The wired uplink is untouched and keeps feeding the watchdog, so if the
//! radio wedges the EMAC or the executor the board reboots and the fleet
//! row goes stale — the failure is visible from the cloud, not just serial.
//!
//! Go: both boards keep uplinking to the fleet AND hear each other's beacons
//! for the whole soak. No-go: lost uplinks, reboots, or one-way silence.

use core::fmt::Write;

use embassy_time::{Instant, Timer};
use esp_hal::peripherals::WIFI;
use esp_println::println;
use esp_radio::esp_now::{EspNow, EspNowError, EspNowReceiver, EspNowSender, BROADCAST_ADDRESS};
use esp_radio::wifi::{self, WifiController, WifiMode};
use esp_radio::Controller;

use crate::config::{ESPNOW_BEACON_INTERVAL, ESPNOW_CHANNEL, ESPNOW_REPORT_INTERVAL};

/// Beacon payload prefix. Anything else heard is logged but not a beacon.
pub const BEACON_MAGIC: &str = "SVB1";

/// Bring the radio up in station mode (no AP join), pin the channel and hand
/// back ESP-NOW. The controller must outlive the program: dropping it
/// deinitialises the radio under the running tasks.
pub fn init(
    radio: &'static Controller<'static>,
    wifi: WIFI<'static>,
) -> Result<(WifiController<'static>, EspNow<'static>), EspNowError> {
    let (mut controller, interfaces) = wifi::new(radio, wifi, wifi::Config::default())?;
    controller.set_mode(WifiMode::Sta)?;
    controller.start()?;
    let esp_now = interfaces.esp_now;
    esp_now.set_channel(ESPNOW_CHANNEL)?;
    println!("ESP-NOW version {:?}", esp_now.version());
    Ok((controller, esp_now))
}

/// Broadcasts `SVB1 <device> seq=<n> up=<s>` every beacon interval and prints
/// a send/heap summary every report interval.
#[embassy_executor::task]
pub async fn espnow_beacon_task(mut sender: EspNowSender<'static>, device_id: &'static str) {
    let mut seq: u32 = 0;
    let (mut ok, mut failed) = (0u32, 0u32);
    let mut last_report = Instant::now();
    loop {
        Timer::after(ESPNOW_BEACON_INTERVAL).await;
        seq = seq.wrapping_add(1);
        let mut payload: heapless::String<96> = heapless::String::new();
        // Truncation is impossible: a 64-byte device id plus the fixed fields fits.
        let _ = write!(
            payload,
            "{} {} seq={} up={}",
            BEACON_MAGIC,
            device_id,
            seq,
            Instant::now().as_secs()
        );
        match sender
            .send_async(&BROADCAST_ADDRESS, payload.as_bytes())
            .await
        {
            Ok(()) => ok += 1,
            Err(e) => {
                failed += 1;
                println!("ESP-NOW send failed: {:?}", e);
            }
        }
        if last_report.elapsed() >= ESPNOW_REPORT_INTERVAL {
            last_report = Instant::now();
            println!(
                "ESP-NOW beacons: {} ok, {} failed; heap used {} free {}",
                ok,
                failed,
                esp_alloc::HEAP.used(),
                esp_alloc::HEAP.free()
            );
        }
    }
}

/// Logs every ESP-NOW frame heard: counter, sender MAC, RSSI, payload.
#[embassy_executor::task]
pub async fn espnow_listen_task(mut receiver: EspNowReceiver<'static>) {
    let mut heard: u32 = 0;
    loop {
        let frame = receiver.receive_async().await;
        heard += 1;
        let m = frame.info.src_address;
        // The blob's rx_ctrl.rssi is a signed 8-bit bitfield that the binding
        // reads unsigned (observed 175 for a bench neighbour): wrap it to dBm.
        let rssi = frame.info.rx_control.rssi;
        let rssi_dbm = if rssi > 127 { rssi - 256 } else { rssi };
        let data = frame.data();
        let text = core::str::from_utf8(data).unwrap_or("<binary>");
        println!(
            "ESP-NOW rx #{} from {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} rssi={} len={} {}",
            heard,
            m[0],
            m[1],
            m[2],
            m[3],
            m[4],
            m[5],
            rssi_dbm,
            data.len(),
            text
        );
    }
}
