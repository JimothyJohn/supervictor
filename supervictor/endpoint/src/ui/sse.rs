//! Live uplink stream: `GET /ui/events` (Server-Sent Events).
//!
//! The ingest path publishes to a `tokio::sync::broadcast` channel — a
//! non-blocking send, so a slow or absent dashboard can never wedge ingest.
//! Each SSE client gets a forwarder task bridging broadcast → bounded mpsc;
//! `try_send` drops events for a client that stops reading (lag is the
//! documented behavior, not backpressure). On Lambda, API Gateway buffers
//! responses, so the page's script falls back to periodic reload there.

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};

use crate::live;
use crate::models::UplinkRecord;
use crate::routes::AppState;
use crate::time::parse_rfc3339_unix_ms;
use supervictor_common::fields;

/// A single uplink, as pushed to dashboard clients: identity and time for
/// the fleet table, plus the 8-bit register samples for the live strips
/// (`crate::live`) — the burst when the device sent one, otherwise the
/// scalar fields quantised to a single sample.
#[derive(Debug, Clone, Serialize)]
pub struct UplinkEvent {
    /// Device that sent the uplink.
    pub device_id: String,
    /// Current sensor reading.
    pub current: i32,
    /// ISO 8601 receive timestamp.
    pub received_at: String,
    /// Receive time, Unix milliseconds.
    pub t_ms: u64,
    /// Flat levels, `levels::COUNT` per sample, oldest first.
    pub levels: Vec<u8>,
    /// Milliseconds between samples; `0` for a single sample.
    pub levels_ms: u32,
}

impl UplinkEvent {
    /// Build the event for a record the ingest path just persisted.
    pub fn from_record(record: &UplinkRecord) -> Self {
        let current = record
            .payload
            .get(fields::UPLINK_CURRENT)
            .and_then(serde_json::Value::as_i64)
            .and_then(|v| i32::try_from(v).ok())
            .unwrap_or_default();
        let (levels, levels_ms) = match live::burst_of(&record.payload) {
            Some(burst) => (burst.levels, burst.step_ms),
            None => (live::scalar_levels(&record.payload).to_vec(), 0),
        };
        UplinkEvent {
            device_id: record.device_id.clone(),
            current,
            received_at: record.received_at.clone(),
            t_ms: parse_rfc3339_unix_ms(&record.received_at).unwrap_or(0),
            levels,
            levels_ms,
        }
    }
}

/// Shared publisher handle stored in [`AppState`].
pub type EventSender = broadcast::Sender<UplinkEvent>;

/// Capacity of the broadcast ring; readers that fall further behind skip ahead.
const BROADCAST_CAPACITY: usize = 64;
/// Per-client buffer; a client that stops reading silently drops events.
const CLIENT_BUFFER: usize = 16;

/// Create the process-wide uplink event channel.
pub fn channel() -> EventSender {
    broadcast::channel(BROADCAST_CAPACITY).0
}

/// mpsc receiver adapted to `Stream` for axum's `Sse` (mpsc exposes
/// `poll_recv`; broadcast does not, hence the forwarder task).
pub struct EventStream {
    rx: mpsc::Receiver<Event>,
}

impl futures_core::Stream for EventStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx).map(|opt| opt.map(Ok))
    }
}

/// `GET /ui/events`
pub async fn events(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    let mut source = state.events.subscribe();
    let (tx, rx) = mpsc::channel(CLIENT_BUFFER);

    tokio::spawn(async move {
        loop {
            match source.recv().await {
                Ok(ev) => {
                    let data = match serde_json::to_string(&ev) {
                        Ok(d) => d,
                        Err(_) => continue,
                    };
                    match tx.try_send(Event::default().event("uplink").data(data)) {
                        // Client went away — stop forwarding, drop the task.
                        Err(mpsc::error::TrySendError::Closed(_)) => break,
                        // Full buffer: slow client loses this event; ingest is
                        // unaffected either way.
                        Err(mpsc::error::TrySendError::Full(_)) | Ok(()) => {}
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    Sse::new(EventStream { rx }).keep_alive(KeepAlive::default())
}
