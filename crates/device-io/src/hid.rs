//! The HID devices of the system and their input reports, through `async-hid`, whose futures
//! are run to their end with `futures-lite` on the calling thread or on a reading thread of
//! this module. No type of either leaves this module. Behaviour note:
//! `docs/behavior/hid-controllers.md`, section *Devices*.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread;

use async_hid::{AsyncHidRead, Device, HidBackend};
use futures_lite::StreamExt;
use futures_lite::future::block_on;

/// Longest report kept, in bytes; the rest of a longer one is cut. The same bound as the
/// mappings read: `platter_control::MAX_REPORT_BYTES`.
pub const MAX_REPORT_BYTES: usize = 1024;

/// A HID device, or one interface of a device that has several, as the system lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HidDevice {
    pub name: String,
    pub vendor: u16,
    pub product: u16,
    /// What the interface is for, by the usage tables of the HID specification.
    pub usage_page: u16,
    pub usage: u16,
}

impl HidDevice {
    fn of(device: &Device) -> Self {
        Self {
            name: device.name.clone(),
            vendor: device.vendor_id,
            product: device.product_id,
            usage_page: device.usage_page,
            usage: device.usage_id,
        }
    }

    /// How it is named in errors: its name, vendor and product.
    fn label(&self) -> String {
        format!("{} ({:04x}:{:04x})", self.name, self.vendor, self.product)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HidError {
    /// The HID system of the computer cannot be used.
    System,
    /// No device is that one now.
    NoSuchDevice(String),
    /// The device exists but cannot be opened, for instance because the system keeps it.
    Busy(String),
}

impl fmt::Display for HidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => f.write_str("the HID devices of the system cannot be listed"),
            Self::NoSuchDevice(name) => write!(f, "there is no HID device {name}"),
            Self::Busy(name) => write!(f, "the HID device {name} cannot be opened"),
        }
    }
}

impl std::error::Error for HidError {}

/// The HID devices of the system, in the order it gives them.
pub fn hid_devices() -> Result<Vec<HidDevice>, HidError> {
    block_on(async {
        let backend = HidBackend::default();
        let devices = backend.enumerate().await.map_err(|_| HidError::System)?;
        Ok(devices.map(|device| HidDevice::of(&device)).collect().await)
    })
}

/// The side of the queue the reading thread holds.
struct Sender {
    queue: SyncSender<Vec<u8>>,
    counts: Arc<Counts>,
}

#[derive(Debug, Default)]
struct Counts {
    /// Reports lost because the queue was full.
    dropped: AtomicU64,
    /// The device stopped sending, usually because it was unplugged.
    gone: AtomicBool,
}

impl Sender {
    /// Never waits: a report that does not fit is counted and lost. Returns whether the report
    /// is in the queue.
    fn deliver(&self, report: &[u8]) -> bool {
        if report.is_empty() {
            return false;
        }
        let report = report[..report.len().min(MAX_REPORT_BYTES)].to_vec();
        match self.queue.try_send(report) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.counts.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

/// The reports of an open device, in the order they arrived.
#[derive(Debug)]
pub struct Reports {
    queue: Receiver<Vec<u8>>,
    counts: Arc<Counts>,
}

impl Reports {
    /// The reports waiting now, without waiting for more. A device that numbers its reports
    /// puts the report ID first.
    pub fn take(&self) -> impl Iterator<Item = Vec<u8>> + '_ {
        self.queue.try_iter()
    }

    /// Reports lost because they were not taken in time.
    pub fn dropped(&self) -> u64 {
        self.counts.dropped.load(Ordering::Relaxed)
    }

    /// Whether the device stopped sending: it was unplugged or failed.
    pub fn gone(&self) -> bool {
        self.counts.gone.load(Ordering::Relaxed)
    }
}

fn queue(capacity: usize) -> (Sender, Reports) {
    let (sender, receiver) = sync_channel(capacity.max(1));
    let counts = Arc::new(Counts::default());
    (
        Sender {
            queue: sender,
            counts: Arc::clone(&counts),
        },
        Reports {
            queue: receiver,
            counts,
        },
    )
}

/// An open HID device. When it is dropped its thread stops reading at the next report or
/// error: a read that waits for the device cannot be cut short.
#[derive(Debug)]
pub struct HidInput {
    device: HidDevice,
    stop: Arc<AtomicBool>,
}

impl HidInput {
    pub fn device(&self) -> &HidDevice {
        &self.device
    }
}

impl Drop for HidInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Opens the first device that is `wanted` and reads its input reports on a thread of its own
/// into a queue of `capacity` reports; those that arrive while it is full are counted and lost.
/// `wake` is called, on that thread, after each report that enters the queue; it must return at
/// once.
pub fn open_hid(
    wanted: &HidDevice,
    capacity: usize,
    wake: impl Fn() + Send + 'static,
) -> Result<(HidInput, Reports), HidError> {
    let reader = block_on(async {
        let backend = HidBackend::default();
        let mut devices = backend.enumerate().await.map_err(|_| HidError::System)?;
        let mut found = None;
        while let Some(device) = devices.next().await {
            if HidDevice::of(&device) == *wanted {
                found = Some(device);
                break;
            }
        }
        let device = found.ok_or_else(|| HidError::NoSuchDevice(wanted.label()))?;
        device
            .open_readable()
            .await
            .map_err(|_| HidError::Busy(wanted.label()))
    })?;
    let (sender, reports) = queue(capacity);
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    thread::Builder::new()
        .name("platter-hid".to_owned())
        .spawn(move || {
            let mut reader = reader;
            let mut buffer = vec![0; MAX_REPORT_BYTES];
            while !stopped.load(Ordering::Relaxed) {
                match block_on(reader.read_input_report(&mut buffer)) {
                    Ok(length) => {
                        if sender.deliver(&buffer[..length.min(buffer.len())]) {
                            wake();
                        }
                    }
                    Err(_) => {
                        sender.counts.gone.store(true, Ordering::Relaxed);
                        wake();
                        break;
                    }
                }
            }
        })
        .map_err(|_| HidError::System)?;
    Ok((
        HidInput {
            device: wanted.clone(),
            stop,
        },
        reports,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(reports: &Reports) -> Vec<Vec<u8>> {
        reports.take().collect()
    }

    #[test]
    fn reports_arrive_in_order_and_a_full_queue_counts_what_it_loses() {
        let (sender, reports) = queue(2);
        let queued: Vec<bool> = (0..4).map(|byte| sender.deliver(&[1, byte])).collect();
        assert_eq!(queued, [true, true, false, false]);
        assert_eq!(reports.dropped(), 2);
        assert_eq!(taken(&reports), [vec![1, 0], vec![1, 1]]);
        assert!(!reports.gone());
    }

    #[test]
    fn a_long_report_is_cut_and_an_empty_one_left_out() {
        let (sender, reports) = queue(4);
        assert!(!sender.deliver(&[]));
        assert!(sender.deliver(&vec![7; MAX_REPORT_BYTES + 5]));
        let taken = taken(&reports);
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].len(), MAX_REPORT_BYTES);
    }

    #[test]
    fn a_closed_queue_is_not_an_error_for_the_reading_thread() {
        let (sender, reports) = queue(1);
        drop(reports);
        assert!(!sender.deliver(&[1, 2]));
    }
}
