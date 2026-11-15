//! MIDI 1.0 channel messages, after the summary of MIDI 1.0 messages of the MIDI Association:
//! <https://midi.org/summary-of-midi-1-0-messages>. The low four bits of the status byte are
//! the channel, 0 to 15 for channels 1 to 16; every data byte is below 128.

/// The messages a mapping listens to. Channels count from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        note: u8,
    },
    ControlChange {
        channel: u8,
        number: u8,
        value: u8,
    },
    /// The 14 bits of the bend, 0 to 16 383, with the centre at 8 192.
    PitchBend {
        channel: u8,
        value: u16,
    },
}

const NOTE_OFF: u8 = 0x80;
const NOTE_ON: u8 = 0x90;
const CONTROL_CHANGE: u8 = 0xB0;
const PITCH_BEND: u8 = 0xE0;

impl Message {
    /// The message in `bytes`, or `None` for one of another kind, cut short, or with a data
    /// byte of 128 or more. Bytes after the message are ignored.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let (&status, data) = bytes.split_first()?;
        let channel = status & 0x0F;
        let [first, second, ..] = *data else {
            return None;
        };
        if first > 0x7F || second > 0x7F {
            return None;
        }
        match status & 0xF0 {
            NOTE_OFF => Some(Self::NoteOff {
                channel,
                note: first,
            }),
            NOTE_ON => Some(Self::NoteOn {
                channel,
                note: first,
                velocity: second,
            }),
            CONTROL_CHANGE => Some(Self::ControlChange {
                channel,
                number: first,
                value: second,
            }),
            // The least significant seven bits come first.
            PITCH_BEND => Some(Self::PitchBend {
                channel,
                value: u16::from(second) << 7 | u16::from(first),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_messages_are_read() {
        assert_eq!(
            Message::parse(&[0x9F, 36, 100]),
            Some(Message::NoteOn {
                channel: 15,
                note: 36,
                velocity: 100
            })
        );
        assert_eq!(
            Message::parse(&[0x80, 36, 0]),
            Some(Message::NoteOff {
                channel: 0,
                note: 36
            })
        );
        assert_eq!(
            Message::parse(&[0xB1, 7, 127]),
            Some(Message::ControlChange {
                channel: 1,
                number: 7,
                value: 127
            })
        );
        assert_eq!(
            Message::parse(&[0xE0, 0x00, 0x40]),
            Some(Message::PitchBend {
                channel: 0,
                value: 8192
            })
        );
        assert_eq!(
            Message::parse(&[0xE0, 0x7F, 0x7F]),
            Some(Message::PitchBend {
                channel: 0,
                value: 16_383
            })
        );
    }

    #[test]
    fn other_or_damaged_messages_are_ignored() {
        for bytes in [
            &[][..],
            &[0x90, 36],
            &[0x90, 0x80, 1],
            &[0x90, 1, 0x80],
            &[0xC0, 5, 0],
            &[0xA0, 36, 10],
            &[0xF8, 0, 0],
            &[36, 100, 0],
        ] {
            assert_eq!(Message::parse(bytes), None, "{bytes:?}");
        }
    }
}
