//! The processor: two decks, the mixer and the master section.

use std::sync::Arc;

use platter_core::{DeckId, SampleRate};
use platter_dsp::Frame;
use platter_dsp::crossfade::CrossfaderCurve;
use platter_dsp::limiter::Limiter;
use platter_dsp::meter::PeakMeter;
use platter_dsp::smooth::Smoothed;
use rtrb::{Consumer, Producer};

use super::deck::DeckPlayer;
use super::strip::{ChannelStrip, PARAMETER_RAMP_MS};
use super::{COMMAND_CAPACITY, Retired, RtCommand, Shared};

struct Channel {
    player: DeckPlayer,
    strip: ChannelStrip,
    /// Gain the crossfader gives this side.
    crossfade: Smoothed,
}

pub struct Processor {
    commands: Consumer<RtCommand>,
    retired: Producer<Retired>,
    shared: Arc<Shared>,
    channels: [Channel; 2],
    crossfader_position: f32,
    crossfader_curve: CrossfaderCurve,
    master_gain: Smoothed,
    limiter: Limiter,
    master_meter: PeakMeter,
}

impl Processor {
    pub(super) fn new(
        rate: SampleRate,
        commands: Consumer<RtCommand>,
        retired: Producer<Retired>,
        shared: Arc<Shared>,
    ) -> Self {
        let position = 0.5;
        let curve = CrossfaderCurve::default();
        let (left, right) = curve.gains(position);
        let channel = |crossfade| Channel {
            player: DeckPlayer::new(rate),
            strip: ChannelStrip::new(rate),
            crossfade: Smoothed::new(crossfade, rate, PARAMETER_RAMP_MS),
        };
        Self {
            commands,
            retired,
            shared,
            channels: [channel(left), channel(right)],
            crossfader_position: position,
            crossfader_curve: curve,
            master_gain: Smoothed::new(1.0, rate, PARAMETER_RAMP_MS),
            limiter: Limiter::new(rate),
            master_meter: PeakMeter::new(rate),
        }
    }

    /// Fills `output`, interleaved with `channels` channels per frame, with the next block
    /// of the mix. The master goes to the first two channels; one channel gets the sum of
    /// both sides at half level; further channels are silent. This is the function the
    /// audio callback calls, and the deviceless render calls the same one.
    pub fn process(&mut self, output: &mut [f32], channels: usize) {
        self.take_commands();
        if channels == 0 {
            output.fill(0.0);
            return;
        }
        let mut frames = output.chunks_exact_mut(channels);
        for frame in &mut frames {
            let [left, right] = self.next_frame();
            match frame {
                [only] => *only = 0.5 * (left + right),
                [first, second, rest @ ..] => {
                    *first = left;
                    *second = right;
                    rest.fill(0.0);
                }
                [] => {}
            }
        }
        frames.into_remainder().fill(0.0);
        self.publish();
    }

    fn take_commands(&mut self) {
        // Bounded by the capacity of the queue, even if the control side keeps sending.
        for _ in 0..COMMAND_CAPACITY {
            let Ok(command) = self.commands.pop() else {
                break;
            };
            self.apply(command);
        }
    }

    fn apply(&mut self, command: RtCommand) {
        match command {
            RtCommand::Swap { deck, audio } => {
                let channel = &mut self.channels[deck.index()];
                channel.player.swap(audio, &mut self.retired);
            }
            RtCommand::Play { deck } => self.player(deck).set_playing(true),
            RtCommand::Pause { deck } => self.player(deck).set_playing(false),
            RtCommand::TogglePlay { deck } => self.player(deck).toggle_playing(),
            RtCommand::CueSet { deck } => self.player(deck).set_cue(),
            RtCommand::CueGoto { deck } => self.player(deck).goto_cue(),
            RtCommand::Seek { deck, seconds } => self.player(deck).seek_seconds(seconds),
            RtCommand::Tempo { deck, ratio } => self.player(deck).set_tempo(ratio),
            RtCommand::Nudge { deck, amount } => self.player(deck).set_nudge(amount),
            RtCommand::Gain { deck, linear } => self.strip(deck).set_gain(linear),
            RtCommand::Eq { deck, band, gain } => self.strip(deck).set_eq(band, gain),
            RtCommand::Filter { deck, position } => self.strip(deck).set_filter(position),
            RtCommand::Fader { deck, gain } => self.strip(deck).set_fader(gain),
            RtCommand::Crossfader { position } => {
                self.crossfader_position = position;
                self.retarget_crossfader();
            }
            RtCommand::CrossfaderCurve { curve } => {
                self.crossfader_curve = curve;
                self.retarget_crossfader();
            }
            RtCommand::MasterGain { linear } => self.master_gain.set(linear),
        }
    }

    fn player(&mut self, deck: DeckId) -> &mut DeckPlayer {
        &mut self.channels[deck.index()].player
    }

    fn strip(&mut self, deck: DeckId) -> &mut ChannelStrip {
        &mut self.channels[deck.index()].strip
    }

    /// The gains move along a ramp, so neither the crossfader nor a change of curve clicks.
    fn retarget_crossfader(&mut self) {
        let (left, right) = self.crossfader_curve.gains(self.crossfader_position);
        self.channels[0].crossfade.set(left);
        self.channels[1].crossfade.set(right);
    }

    fn next_frame(&mut self) -> Frame {
        let mut mix = [0.0f32; 2];
        for channel in &mut self.channels {
            let played = channel.player.next(&mut self.retired);
            let strip = channel.strip.process(played);
            let side = channel.crossfade.advance();
            mix[0] += strip[0] * side;
            mix[1] += strip[1] * side;
        }
        let gain = self.master_gain.advance();
        let output = self.limiter.process([mix[0] * gain, mix[1] * gain]);
        self.master_meter.feed(output);
        output
    }

    fn publish(&self) {
        for (channel, shared) in self.channels.iter().zip(&self.shared.decks) {
            shared.loaded.set(channel.player.is_loaded());
            shared.playing.set(channel.player.is_playing());
            shared.position.set(channel.player.position_seconds());
            shared.cue.set(channel.player.cue_seconds());
            shared.level.set(channel.strip.level());
        }
        self.shared.master_level.set(self.master_meter.level());
        self.shared.limiter_gain.set(self.limiter.gain());
    }
}
