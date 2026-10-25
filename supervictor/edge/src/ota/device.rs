//! Flash, bootloader and network glue for the OTA core (embedded builds).
//!
//! [`Ota`] owns the SPI-flash driver and runs three things: the boot
//! check (count an unconfirmed image's boots, revert when the budget is
//! gone), the confirmation after the first good uplink, and the update
//! itself — manifest, ranged image download straight into the inactive
//! slot through [`Installer`], activate, reboot. Progress lives in an
//! RTC-persistent record so a reset mid-download resumes.

use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_time::{with_timeout, Duration, Instant};
use embedded_storage::{ReadStorage, Storage};
use esp_bootloader_esp_idf::ota::OtaImageState;
use esp_bootloader_esp_idf::ota_updater::OtaUpdater;
use esp_bootloader_esp_idf::partitions::{
    self, AppPartitionSubType, FlashRegion, PARTITION_TABLE_MAX_LEN,
};
use esp_println::println;
use esp_storage::FlashStorage;
use mbedtls_rs::{Session, SessionConfig, TlsReference};
use supervictor_common::firmware::{ChipId, Version};

use super::http::{self, HeadError, MAX_HEAD_LEN};
use super::install::{InstallError, Installer, Slot, SECTOR};
use super::manifest::{self, Manifest, ManifestError};
use super::policy::{self, BootDecision, State};
use crate::app::tasks::{API_PATH, DEVICE_ID_ON_WIRE};
use crate::config::*;
use crate::models::uplink::FIRMWARE_VERSION;
use crate::network::tls::load_certificates;

/// Chip this build runs on; a manifest for anything else is refused.
#[cfg(feature = "chip-esp32")]
pub const THIS_CHIP: ChipId = ChipId::Esp32;
/// Chip this build runs on; a manifest for anything else is refused.
#[cfg(feature = "chip-esp32c3")]
pub const THIS_CHIP: ChipId = ChipId::Esp32C3;

/// Per-read timeout while streaming the image.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Whole-download budget. Well inside `WATCHDOG_TIMEOUT_S` (300 s): the
/// watchdog is not fed during the download, and must not be — a wedged
/// download is exactly what it exists to reset. A reset here resumes.
const UPDATE_DEADLINE: Duration = Duration::from_secs(200);
/// Buffer for the response head and the body reads.
const READ_BUF: usize = 2048;

/// The record that survives resets (RTC fast memory, see `policy`).
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut OTA_RTC: [u32; policy::WORDS] = [0; policy::WORDS];

fn rtc_load() -> State {
    // SAFETY: single-core firmware; the only accesses are these two
    // helpers, each short-lived and never overlapping.
    let words = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(OTA_RTC)) };
    policy::load(&words)
}

fn rtc_store(state: State) {
    let mut words = [0u32; policy::WORDS];
    policy::store(&mut words, state);
    // SAFETY: see `rtc_load`.
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(OTA_RTC), words) };
}

/// Why an update attempt stopped. The uplink loop logs it and carries on;
/// the next uplink that still asks for the version tries again.
#[derive(Debug)]
pub enum OtaError {
    /// `desired_fw` is not a version, or is not newer than what runs.
    NotNewer,
    /// The partition table has no OTA slots (factory layout).
    NoOtaTable,
    /// DNS, TCP or TLS failure reaching the endpoint.
    Connect,
    /// The server's reply could not be framed.
    Head(HeadError),
    /// Unexpected status for the manifest or the image.
    Status(u16),
    /// Manifest rejected.
    Manifest(ManifestError),
    /// Manifest is for another chip or another version than asked.
    WrongImage,
    /// The server ignored `Range`, or framed the body inconsistently.
    Framing,
    /// Timed out reading, or the whole-download deadline passed.
    Timeout,
    /// The installer refused the bytes (digest, header, overrun, flash).
    Install(InstallError<partitions::Error>),
    /// otadata could not be updated.
    Partition(partitions::Error),
}

impl From<InstallError<partitions::Error>> for OtaError {
    fn from(e: InstallError<partitions::Error>) -> Self {
        OtaError::Install(e)
    }
}

impl Slot for FlashRegion<'_, FlashStorage<'static>> {
    type Error = partitions::Error;
    fn capacity(&self) -> u32 {
        self.partition_size() as u32
    }
    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), Self::Error> {
        Storage::write(self, offset, data)
    }
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), Self::Error> {
        ReadStorage::read(self, offset, buf)
    }
}

/// `ota_N` slot number for an app partition subtype.
fn slot_number(sub: AppPartitionSubType) -> Option<u8> {
    match sub {
        AppPartitionSubType::Ota0 => Some(0),
        AppPartitionSubType::Ota1 => Some(1),
        _ => None,
    }
}

/// App partition subtype for an `ota_N` slot number.
fn slot_subtype(n: u8) -> Option<AppPartitionSubType> {
    match n {
        0 => Some(AppPartitionSubType::Ota0),
        1 => Some(AppPartitionSubType::Ota1),
        _ => None,
    }
}

/// OTA state machine owner. One per board; lives in a `StaticCell`, and
/// its two buffers (partition table 3 KB, sector 4 KB) are statics handed
/// in by the caller: the boot stack on the classic ESP32 is whatever the
/// linker leaves (~25-50 KB) and neither this struct nor the task future
/// that owns it may carry them.
pub struct Ota {
    flash: FlashStorage<'static>,
    pt_buf: &'static mut [u8; PARTITION_TABLE_MAX_LEN],
    sector: &'static mut [u8; SECTOR],
    confirming: bool,
}

impl Ota {
    /// Take the flash peripheral. `None` (with a console line) when the
    /// partition table has no OTA slots — a board still on the factory
    /// layout keeps uplinking, it just cannot update until its one more
    /// USB flash.
    pub fn new(
        flash: esp_hal::peripherals::FLASH<'static>,
        pt_buf: &'static mut [u8; PARTITION_TABLE_MAX_LEN],
        sector: &'static mut [u8; SECTOR],
    ) -> Option<Ota> {
        let mut me = Ota {
            flash: FlashStorage::new(flash),
            pt_buf,
            sector,
            confirming: false,
        };
        match OtaUpdater::new(&mut me.flash, &mut *me.pt_buf) {
            Ok(mut updater) => {
                let selected = updater.selected_partition();
                println!(
                    "OTA: slots ready, booted from {:?}, running {}",
                    selected, FIRMWARE_VERSION
                );
                Some(me)
            }
            Err(e) => {
                println!(
                    "OTA: unavailable ({:?}) — factory partition table; reflash over USB with partitions.csv",
                    e
                );
                None
            }
        }
    }

    /// Count this boot against an unconfirmed image; revert and reset when
    /// the budget is spent. Call before anything that could wedge.
    pub fn boot_check(&mut self) -> BootDecision {
        let (decision, next) = policy::on_boot(rtc_load());
        rtc_store(next);
        match decision {
            BootDecision::Revert { previous_slot } => {
                println!(
                    "OTA: image never confirmed after {} boots — reverting to ota_{}",
                    policy::MAX_PENDING_BOOTS,
                    previous_slot
                );
                if let Some(sub) = slot_subtype(previous_slot) {
                    let result = OtaUpdater::new(&mut self.flash, &mut *self.pt_buf).and_then(
                        |mut updater| {
                            let mut data = updater.ota_data()?;
                            // Mark the slot we are leaving so nothing boots it on purpose.
                            let _ = data.set_current_ota_state(OtaImageState::Invalid);
                            data.set_current_app_partition(sub)
                        },
                    );
                    if let Err(e) = result {
                        println!("OTA: revert failed: {:?} — booting as is", e);
                    }
                }
                esp_hal::system::software_reset();
            }
            BootDecision::Confirming { boots } => {
                self.confirming = true;
                println!(
                    "OTA: running an unconfirmed image (boot {}/{}); confirms on the first uplink",
                    boots,
                    policy::MAX_PENDING_BOOTS
                );
            }
            BootDecision::Normal => {}
        }
        decision
    }

    /// The running image completed an uplink round-trip: it works. Mark
    /// the slot valid and forget the boot count. No-op otherwise.
    pub fn confirm(&mut self) {
        if !self.confirming {
            return;
        }
        let result = OtaUpdater::new(&mut self.flash, &mut *self.pt_buf)
            .and_then(|mut updater| updater.set_current_ota_state(OtaImageState::Valid));
        match result {
            Ok(()) => println!("OTA: {} confirmed", FIRMWARE_VERSION),
            Err(e) => println!("OTA: confirm: otadata write failed: {:?}", e),
        }
        rtc_store(State::Idle);
        self.confirming = false;
    }

    /// Whether `desired` is a version this board should move to.
    pub fn wants(desired: &str) -> bool {
        match (Version::parse(desired), Version::parse(FIRMWARE_VERSION)) {
            (Some(want), Some(have)) => want.is_newer_than(&have),
            _ => false,
        }
    }

    /// Fetch and install `desired`, then reboot into it. Returns only on
    /// failure. `rx`/`tx` are the TCP buffers the uplink loop lends while
    /// its own session is closed.
    pub async fn update(
        &mut self,
        stack: Stack<'static>,
        tls: TlsReference<'_>,
        rx: &mut [u8],
        tx: &mut [u8],
        desired: &str,
    ) -> Result<core::convert::Infallible, OtaError> {
        if !Self::wants(desired) {
            return Err(OtaError::NotNewer);
        }
        let started = Instant::now();
        println!("OTA: {} -> {} requested", FIRMWARE_VERSION, desired);

        // 1. Manifest.
        let mut buf = [0u8; READ_BUF];
        let manifest = {
            let request = http::manifest_request(HOST, API_PATH, DEVICE_ID_ON_WIRE, desired)
                .map_err(|_| OtaError::Framing)?;
            let mut session = connect(stack, tls, rx, tx).await?;
            write_all(&mut session, request.as_bytes()).await?;
            let (head, body_len) = read_head_and_body(&mut session, &mut buf, started).await?;
            let _ = session.close().await;
            if head.status != 200 {
                return Err(OtaError::Status(head.status));
            }
            Manifest::parse(&buf[..body_len]).map_err(OtaError::Manifest)?
        };
        if manifest.chip != THIS_CHIP || manifest.version.as_str() != desired {
            return Err(OtaError::WrongImage);
        }
        let sha_hex = manifest::encode_hex32(&manifest.sha256);
        println!(
            "OTA: manifest {} bytes sha256 {}…",
            manifest.size,
            &sha_hex.as_str()[..16]
        );

        // 2. Slot + resume point.
        let mut updater = OtaUpdater::new(&mut self.flash, &mut *self.pt_buf)
            .map_err(|_| OtaError::NoOtaTable)?;
        let previous = updater
            .selected_partition()
            .ok()
            .and_then(slot_number)
            .unwrap_or(0);
        let (region, target) = updater.next_partition().map_err(OtaError::Partition)?;
        let resume_at = policy::resume_offset(rtc_load(), manifest.size, &manifest.sha256);
        let mut installer = if resume_at > 0 {
            println!("OTA: resuming at {} into {:?}", resume_at, target);
            match Installer::resume(region, &mut *self.sector, &manifest, resume_at) {
                Ok(i) => i,
                Err(e) => {
                    // Whatever is in the slot is not what the record says:
                    // forget it and start over next time.
                    rtc_store(State::Idle);
                    return Err(e.into());
                }
            }
        } else {
            println!("OTA: writing into {:?}", target);
            Installer::new(region, &mut *self.sector, &manifest)?
        };
        let mut committed = installer.committed();

        // 3. Image, from `committed` to the end, pinned to this digest.
        let request = http::image_request(
            HOST,
            API_PATH,
            DEVICE_ID_ON_WIRE,
            desired,
            committed,
            sha_hex.as_str(),
        )
        .map_err(|_| OtaError::Framing)?;
        let mut session = connect(stack, tls, rx, tx).await?;
        write_all(&mut session, request.as_bytes()).await?;
        let (head, first_body) = read_head(&mut session, &mut buf, started).await?;
        let expected_status = if committed == 0 { 200 } else { 206 };
        match head.status {
            s if s == expected_status || (committed == 0 && s == 206) => {}
            // The image changed under us, or our offset is nonsense:
            // start over on the next attempt.
            412 | 416 => {
                rtc_store(State::Idle);
                return Err(OtaError::Status(head.status));
            }
            // A 200 to a ranged request: the server ignored Range. The
            // slot prefix is still good, but this body starts at 0.
            200 => {
                rtc_store(State::Idle);
                return Err(OtaError::Framing);
            }
            s => return Err(OtaError::Status(s)),
        }
        if let Some((start, total)) = head.content_range {
            if start != committed || total != manifest.size {
                rtc_store(State::Idle);
                return Err(OtaError::Framing);
            }
        }
        if let Some(len) = head.content_length {
            if len != manifest.size - committed {
                return Err(OtaError::Framing);
            }
        }

        // 4. Stream.
        let mut pending = &buf[first_body.0..first_body.1];
        loop {
            if !pending.is_empty() {
                if installer.feed(pending)? > 0 {
                    committed = installer.committed();
                    rtc_store(policy::resume_state(
                        committed,
                        manifest.size,
                        &manifest.sha256,
                    ));
                }
            }
            if installer.remaining() == 0 {
                break;
            }
            if started.elapsed() > UPDATE_DEADLINE {
                return Err(OtaError::Timeout);
            }
            let n = match with_timeout(READ_TIMEOUT, session.read(&mut buf)).await {
                Ok(Ok(0)) => break, // EOF: the installer decides if it was enough
                Ok(Ok(n)) => n,
                Ok(Err(_)) => return Err(OtaError::Connect),
                Err(_) => return Err(OtaError::Timeout),
            };
            pending = &buf[..n];
        }
        let _ = session.close().await;

        // 5. Verify (size, digest, read-back) and activate.
        let verified = installer.finish()?;
        drop(updater);
        let mut updater = OtaUpdater::new(&mut self.flash, &mut *self.pt_buf)
            .map_err(|_| OtaError::NoOtaTable)?;
        updater
            .activate_next_partition()
            .map_err(OtaError::Partition)?;
        let _ = updater.set_current_ota_state(OtaImageState::New);
        rtc_store(State::Pending {
            boots: 0,
            previous_slot: previous,
        });
        println!(
            "OTA: {} verified ({} bytes) in {:?}; rebooting into it (fallback ota_{})",
            desired, verified.size, target, previous
        );
        esp_hal::system::software_reset()
    }
}

/// DNS + TCP + TLS to the endpoint, same path as the uplink.
async fn connect<'a>(
    stack: Stack<'static>,
    tls: TlsReference<'a>,
    rx: &'a mut [u8],
    tx: &'a mut [u8],
) -> Result<Session<'a, TcpSocket<'a>>, OtaError> {
    let addresses = stack
        .dns_query(HOST, embassy_net::dns::DnsQueryType::A)
        .await
        .map_err(|_| OtaError::Connect)?;
    let address = *addresses.first().ok_or(OtaError::Connect)?;
    let mut socket = TcpSocket::new(stack, rx, tx);
    socket.set_timeout(Some(SOCKET_TIMEOUT));
    socket
        .connect((address, AWS_IOT_PORT))
        .await
        .map_err(|_| OtaError::Connect)?;
    let config = load_certificates();
    let mut session = Session::new(tls, socket, &SessionConfig::Client(config.clone()))
        .map_err(|_| OtaError::Connect)?;
    match with_timeout(TLS_HANDSHAKE_TIMEOUT, session.connect()).await {
        Ok(Ok(())) => Ok(session),
        _ => Err(OtaError::Connect),
    }
}

/// `Session::write` may be partial: push until done.
async fn write_all<T>(session: &mut Session<'_, T>, mut data: &[u8]) -> Result<(), OtaError>
where
    T: embedded_io_async::Read + embedded_io_async::Write,
{
    while !data.is_empty() {
        let n = session.write(data).await.map_err(|_| OtaError::Connect)?;
        if n == 0 {
            return Err(OtaError::Connect);
        }
        data = &data[n..];
    }
    session.flush().await.map_err(|_| OtaError::Connect)
}

/// Read until the head is complete. Returns the head and the `(start,
/// end)` of whatever body bytes arrived with it in `buf`.
async fn read_head<T>(
    session: &mut Session<'_, T>,
    buf: &mut [u8; READ_BUF],
    started: Instant,
) -> Result<(http::ResponseHead, (usize, usize)), OtaError>
where
    T: embedded_io_async::Read + embedded_io_async::Write,
{
    let mut filled = 0;
    loop {
        match http::parse_head(&buf[..filled]) {
            Ok((head, head_len)) => return Ok((head, (head_len, filled))),
            Err(HeadError::Incomplete) => {}
            Err(e) => return Err(OtaError::Head(e)),
        }
        if filled >= MAX_HEAD_LEN.min(READ_BUF) {
            return Err(OtaError::Head(HeadError::Malformed));
        }
        if started.elapsed() > UPDATE_DEADLINE {
            return Err(OtaError::Timeout);
        }
        let n = match with_timeout(READ_TIMEOUT, session.read(&mut buf[filled..])).await {
            Ok(Ok(0)) => return Err(OtaError::Head(HeadError::Incomplete)),
            Ok(Ok(n)) => n,
            Ok(Err(_)) => return Err(OtaError::Connect),
            Err(_) => return Err(OtaError::Timeout),
        };
        filled += n;
    }
}

/// Read a small response (the manifest) whole: head, then body to
/// `Content-Length` or EOF, compacted to the start of `buf`. Returns the
/// head and the body length.
async fn read_head_and_body<T>(
    session: &mut Session<'_, T>,
    buf: &mut [u8; READ_BUF],
    started: Instant,
) -> Result<(http::ResponseHead, usize), OtaError>
where
    T: embedded_io_async::Read + embedded_io_async::Write,
{
    let (head, (body_start, mut filled)) = read_head(session, buf, started).await?;
    let want = head.content_length.map(|n| n as usize);
    loop {
        let have = filled - body_start;
        if want.is_some_and(|w| have >= w) || filled == buf.len() {
            break;
        }
        if started.elapsed() > UPDATE_DEADLINE {
            return Err(OtaError::Timeout);
        }
        match with_timeout(READ_TIMEOUT, session.read(&mut buf[filled..])).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => filled += n,
            Ok(Err(_)) => return Err(OtaError::Connect),
            Err(_) => return Err(OtaError::Timeout),
        }
    }
    let body_len = match want {
        Some(w) => (filled - body_start).min(w),
        None => filled - body_start,
    };
    buf.copy_within(body_start..body_start + body_len, 0);
    Ok((head, body_len))
}
