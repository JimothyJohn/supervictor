//! Settings in the `nvs` partition: find it, read both slots, seed an
//! empty one from the baked values, write a new record.
//!
//! The flash driver is borrowed per call rather than owned: at boot the
//! OTA state machine takes the flash peripheral for good, so the settings
//! load runs first on a reborrow, and a later write (portal, dashboard
//! push) will go through whoever holds the driver then.

use embedded_storage::{ReadStorage, Storage};
use esp_bootloader_esp_idf::partitions::{
    self, DataPartitionSubType, PartitionType, PARTITION_TABLE_MAX_LEN,
};
use esp_println::println;
use esp_storage::FlashStorage;

use super::record::{self, Found, Slot, LEN};
use super::Settings;

/// Why the partition could not be read or written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// The partition table is the factory one, or unreadable.
    NoPartition,
    /// The `nvs` partition is smaller than the two slots need.
    TooSmall,
    /// The flash driver failed.
    Flash,
}

impl From<partitions::Error> for StoreError {
    fn from(_: partitions::Error) -> Self {
        StoreError::Flash
    }
}

/// Where the record lives and what it says now; the handle for writes.
pub struct Store {
    /// Newest valid record, if any.
    current: Option<Found>,
}

impl Store {
    /// Read the partition. `None` settings means nothing valid is there.
    pub fn open(
        flash: &mut FlashStorage<'_>,
        pt_buf: &mut [u8; PARTITION_TABLE_MAX_LEN],
    ) -> Result<Store, StoreError> {
        let mut a = [0u8; LEN];
        let mut b = [0u8; LEN];
        {
            let mut region = nvs_region(flash, pt_buf)?;
            region
                .read(Slot::A.offset(), &mut a)
                .map_err(|_| StoreError::Flash)?;
            region
                .read(Slot::B.offset(), &mut b)
                .map_err(|_| StoreError::Flash)?;
        }
        Ok(Store {
            current: record::choose(&a, &b),
        })
    }

    /// The settings on flash, if any.
    pub fn current(&self) -> Option<&Settings> {
        self.current.as_ref().map(|f| &f.settings)
    }

    /// Write `settings` as the next record into the other slot.
    pub fn write(
        &mut self,
        flash: &mut FlashStorage<'_>,
        pt_buf: &mut [u8; PARTITION_TABLE_MAX_LEN],
        settings: &Settings,
    ) -> Result<(), StoreError> {
        let (slot, seq) = record::next(self.current.as_ref());
        let mut frame = [0u8; LEN];
        record::encode(settings, seq, &mut frame);
        {
            let mut region = nvs_region(flash, pt_buf)?;
            region
                .write(slot.offset(), &frame)
                .map_err(|_| StoreError::Flash)?;
            // Read back: a write that did not take must not become "current".
            let mut check = [0u8; LEN];
            region
                .read(slot.offset(), &mut check)
                .map_err(|_| StoreError::Flash)?;
            if check != frame {
                return Err(StoreError::Flash);
            }
        }
        self.current = Some(Found {
            slot,
            seq,
            settings: settings.clone(),
        });
        Ok(())
    }
}

fn nvs_region<'a, 'd>(
    flash: &'a mut FlashStorage<'d>,
    pt_buf: &'a mut [u8; PARTITION_TABLE_MAX_LEN],
) -> Result<partitions::FlashRegion<'a, FlashStorage<'d>>, StoreError> {
    let table =
        partitions::read_partition_table(flash, pt_buf).map_err(|_| StoreError::NoPartition)?;
    let entry = table
        .find_partition(PartitionType::Data(DataPartitionSubType::Nvs))?
        .ok_or(StoreError::NoPartition)?;
    if entry.len() < record::SPAN {
        return Err(StoreError::TooSmall);
    }
    Ok(entry.as_embedded_storage(flash))
}

/// What boot runs: the record on flash, else the baked values written
/// there so the next boot does not depend on the image again. Never
/// fails — a board with no usable partition runs on the baked values and
/// says so.
pub fn boot(flash: &mut FlashStorage<'_>, pt_buf: &mut [u8; PARTITION_TABLE_MAX_LEN]) -> Settings {
    let mut store = match Store::open(flash, pt_buf) {
        Ok(s) => s,
        Err(e) => {
            println!(
                "settings: nvs partition unavailable ({:?}); running on the baked values",
                e
            );
            return Settings::baked();
        }
    };
    if let Some(found) = store.current.as_ref() {
        println!(
            "settings: record {} from slot {:?} (host {}, device {}, ssid {})",
            found.seq,
            found.slot,
            found.settings.host.as_str(),
            found.settings.device_name.as_str(),
            if found.settings.ssid.is_empty() {
                "-"
            } else {
                found.settings.ssid.as_str()
            }
        );
        return found.settings.clone();
    }
    let baked = Settings::baked();
    if baked.is_blank() {
        println!("settings: nothing on flash and nothing baked in — unprovisioned");
        return baked;
    }
    match store.write(flash, pt_buf, &baked) {
        Ok(()) => println!(
            "settings: empty partition seeded from the image (host {}, device {})",
            baked.host.as_str(),
            baked.device_name.as_str()
        ),
        Err(e) => println!(
            "settings: seeding failed ({:?}); running on the baked values",
            e
        ),
    }
    baked
}
