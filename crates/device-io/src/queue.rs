//! What the callback of a port does with a message: copy it into a bounded queue without
//! waiting, and count what does not fit.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

/// Longest message kept, in bytes: a channel message is at most three.
pub const MAX_MESSAGE_BYTES: usize = 3;

/// One message as the port gave it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawMessage {
    bytes: [u8; MAX_MESSAGE_BYTES],
    len: u8,
}

impl RawMessage {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

#[derive(Debug, Default)]
struct Counts {
    /// Messages lost because the queue was full.
    dropped: AtomicU64,
    /// Messages longer than [`MAX_MESSAGE_BYTES`], left out.
    oversized: AtomicU64,
}

/// The side of the queue the callback of the port holds.
#[derive(Debug)]
pub(crate) struct Sender {
    queue: SyncSender<RawMessage>,
    counts: Arc<Counts>,
}

impl Sender {
    /// Never waits: a message that does not fit is counted and lost. Returns whether the
    /// message is in the queue.
    pub(crate) fn deliver(&self, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return false;
        }
        if bytes.len() > MAX_MESSAGE_BYTES {
            self.counts.oversized.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let mut message = RawMessage {
            bytes: [0; MAX_MESSAGE_BYTES],
            len: bytes.len() as u8,
        };
        message.bytes[..bytes.len()].copy_from_slice(bytes);
        match self.queue.try_send(message) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.counts.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

/// The messages of an open port, in the order they arrived.
#[derive(Debug)]
pub struct Messages {
    queue: Receiver<RawMessage>,
    counts: Arc<Counts>,
}

impl Messages {
    /// The messages waiting now, without waiting for more.
    pub fn take(&self) -> impl Iterator<Item = RawMessage> + '_ {
        self.queue.try_iter()
    }

    /// Messages lost because they were not taken in time.
    pub fn dropped(&self) -> u64 {
        self.counts.dropped.load(Ordering::Relaxed)
    }

    /// Messages left out because they were longer than a channel message.
    pub fn oversized(&self) -> u64 {
        self.counts.oversized.load(Ordering::Relaxed)
    }
}

/// A queue that holds up to `capacity` messages; at least one.
pub(crate) fn queue(capacity: usize) -> (Sender, Messages) {
    let (sender, receiver) = sync_channel(capacity.max(1));
    let counts = Arc::new(Counts::default());
    (
        Sender {
            queue: sender,
            counts: Arc::clone(&counts),
        },
        Messages {
            queue: receiver,
            counts,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(messages: &Messages) -> Vec<Vec<u8>> {
        messages
            .take()
            .map(|message| message.bytes().to_vec())
            .collect()
    }

    #[test]
    fn messages_arrive_in_order_with_their_length() {
        let (sender, messages) = queue(8);
        sender.deliver(&[0x90, 36, 100]);
        sender.deliver(&[0xC0, 5]);
        sender.deliver(&[0xF8]);
        assert_eq!(
            taken(&messages),
            [vec![0x90, 36, 100], vec![0xC0, 5], vec![0xF8]]
        );
        assert!(taken(&messages).is_empty());
    }

    #[test]
    fn a_full_queue_counts_what_it_loses_and_never_waits() {
        let (sender, messages) = queue(2);
        let queued: Vec<bool> = (0..5)
            .map(|note| sender.deliver(&[0x90, note, 1]))
            .collect();
        // Only the messages that entered the queue wake whoever reads it.
        assert_eq!(queued, [true, true, false, false, false]);
        assert_eq!(messages.dropped(), 3);
        assert_eq!(taken(&messages), [vec![0x90, 0, 1], vec![0x90, 1, 1]]);
        sender.deliver(&[0x80, 9, 0]);
        assert_eq!(taken(&messages), [vec![0x80, 9, 0]]);
    }

    #[test]
    fn long_and_empty_messages_are_left_out() {
        let (sender, messages) = queue(4);
        assert!(!sender.deliver(&[0xF0, 1, 2, 3, 0xF7]));
        assert!(!sender.deliver(&[]));
        assert!(taken(&messages).is_empty());
        assert_eq!((messages.oversized(), messages.dropped()), (1, 0));
    }

    #[test]
    fn a_closed_queue_is_not_an_error_for_the_port() {
        let (sender, messages) = queue(0);
        drop(messages);
        assert!(!sender.deliver(&[0x90, 1, 1]));
    }
}
