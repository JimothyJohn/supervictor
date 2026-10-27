//! Staleness watchdog: periodic fleet-health logging for long-running
//! deployments (staging box, containers). Emits a structured summary each
//! tick and a WARN per device that newly went dark — journalctl alerts
//! key off these lines.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use crate::fleet;
use crate::store::DeviceStore;

/// Spawn the watchdog loop. `interval_secs == 0` disables it.
pub fn spawn(store: Arc<dyn DeviceStore>, interval_secs: u64) {
    if interval_secs == 0 {
        tracing::info!("staleness watchdog disabled (WATCHDOG_INTERVAL_SECS=0)");
        return;
    }
    tokio::spawn(run(store, interval_secs));
}

async fn run(store: Arc<dyn DeviceStore>, interval_secs: u64) {
    let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
    // First tick fires immediately; use it to seed known_dark without
    // alerting on devices that were already dark at boot.
    ticker.tick().await;
    let mut known_dark: HashSet<String> =
        match fleet::snapshot_now(store.as_ref(), fleet::DEFAULT_POWER_THRESHOLD_MA) {
            Ok(devices) => devices
                .iter()
                .filter(|d| d.staleness == fleet::Staleness::Dark)
                .map(|d| d.device_id.clone())
                .collect(),
            Err(e) => {
                tracing::error!(error = %e, "watchdog: initial fleet snapshot failed");
                HashSet::new()
            }
        };
    tracing::info!(interval_secs, "staleness watchdog started");

    loop {
        ticker.tick().await;
        let devices = match fleet::snapshot_now(store.as_ref(), fleet::DEFAULT_POWER_THRESHOLD_MA) {
            Ok(devices) => devices,
            Err(e) => {
                tracing::error!(error = %e, "watchdog: fleet snapshot failed");
                continue;
            }
        };

        let summary = fleet::summarize(&devices);
        tracing::info!(
            total = summary.total,
            active = summary.active,
            fresh = summary.fresh,
            stale = summary.stale,
            dark = summary.dark,
            never = summary.never,
            "fleet health"
        );

        let dark_now: HashSet<String> = devices
            .iter()
            .filter(|d| d.staleness == fleet::Staleness::Dark)
            .map(|d| d.device_id.clone())
            .collect();

        for device_id in dark_now.difference(&known_dark) {
            // device_id comes from our own store, but sanitize anyway.
            tracing::warn!(
                device_id = %crate::middleware::sanitize_for_log(device_id),
                "device went dark"
            );
        }
        for device_id in known_dark.difference(&dark_now) {
            tracing::info!(
                device_id = %crate::middleware::sanitize_for_log(device_id),
                "device recovered from dark"
            );
        }
        known_dark = dark_now;
    }
}
