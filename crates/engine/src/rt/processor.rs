//! The processor: two decks, the mixer, the master section and the headphone output.

use std::sync::Arc;

use platter_core::{DeckId, SampleRate};
use platter_dsp::Frame;
use platter_dsp::crossfade::CrossfaderCurve;
use platter_dsp::limiter::Limiter;
use platter_dsp::meter::PeakMeter;
use platter_dsp::smooth::Smoothed;
use rtrb::{Consumer, Producer};

use super::clock::{Candidate, Clock};
use super::deck::DeckPlayer;
use super::metronome::Metronome;
use super::strip::{ChannelStrip, PARAMETER_RAMP_MS};
use super::tone::TestTone;
use super::{COMMAND_CAPACITY, DeckReport, Output, Queues, Retired, RtCommand, Shared};

/// Channels a device needs for the headphones to have their own pair.
const CHANNELS_WITH_HEADPHONES: usize = 4;

struct Channel {
    player: DeckPlayer,
    strip: ChannelStrip,
    /// Gain the crossfader gives this side.
    crossfade: Smoothed,
    /// One while the deck is sent to the headphones, zero otherwise.
    pre_listen: Smoothed,
}

pub struct Processor {
    commands: Consumer<RtCommand>,
    retired: Producer<Retired>,
    reports: Producer<DeckReport>,
    shared: Arc<Shared>,
    channels: [Channel; 2],
    clock: Clock,
    /// Synced decks keep their beats on those of the clock, not only its tempo.
    keep_beat: bool,
    crossfader_position: f32,
    crossfader_curve: CrossfaderCurve,
    master_gain: Smoothed,
    limiter: Limiter,
    master_meter: PeakMeter,
    headphone_mix: Smoothed,
    headphone_gain: Smoothed,
    headphone_limiter: Limiter,
    tone: TestTone,
    metronome: Metronome,
}

impl Processor {
    pub(super) fn new(rate: SampleRate, queues: Queues, shared: Arc<Shared>) -> Self {
        let position = 0.5;
        let curve = CrossfaderCurve::default();
        let (left, right) = curve.gains(position);
        let parameter = |value| Smoothed::new(value, rate, PARAMETER_RAMP_MS);
        let channel = |crossfade| Channel {
            player: DeckPlayer::new(rate),
            strip: ChannelStrip::new(rate),
            crossfade: parameter(crossfade),
            pre_listen: parameter(0.0),
        };
        Self {
            commands: queues.commands,
            retired: queues.retired,
            reports: queues.reports,
            shared,
            channels: [channel(left), channel(right)],
            clock: Clock::new(rate),
            keep_beat: true,
            crossfader_position: position,
            crossfader_curve: curve,
            master_gain: parameter(1.0),
            limiter: Limiter::new(rate),
            master_meter: PeakMeter::new(rate),
            headphone_mix: parameter(0.0),
            headphone_gain: parameter(1.0),
            headphone_limiter: Limiter::new(rate),
            tone: TestTone::new(rate),
            metronome: Metronome::new(rate),
        }
    }

    /// Fills `output`, interleaved with `channels` channels per frame, with the next block
    /// of the mix. The master goes to the first two channels and, when there are four or
    /// more, the headphones go to the third and the fourth; one channel gets the sum of
    /// both sides of the master at half level; further channels are silent. This is the
    /// function the audio callback calls, and the deviceless render calls the same one.
    pub fn process(&mut self, output: &mut [f32], channels: usize) {
        self.take_commands();
        self.clock.choose(self.candidates());
        if channels == 0 {
            output.fill(0.0);
            return;
        }
        let with_headphones = channels >= CHANNELS_WITH_HEADPHONES;
        let mut frames = output.chunks_exact_mut(channels);
        for frame in &mut frames {
            let ([left, right], headphones) = self.next_frame(with_headphones);
            match frame {
                [only] => *only = 0.5 * (left + right),
                [first, second, rest @ ..] => {
                    *first = left;
                    *second = right;
                    rest.fill(0.0);
                    if let [third, fourth, ..] = rest {
                        *third = headphones[0];
                        *fourth = headphones[1];
                    }
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
            RtCommand::Swap { deck, track } => {
                let channel = &mut self.channels[deck.index()];
                channel.player.swap(track, &mut self.retired);
            }
            RtCommand::Play { deck } => self.player(deck).set_playing(true),
            RtCommand::Pause { deck } => self.player(deck).set_playing(false),
            RtCommand::TogglePlay { deck } => self.player(deck).toggle_playing(),
            RtCommand::CueSet { deck } => self.player(deck).set_cue(),
            RtCommand::CueGoto { deck } => self.player(deck).goto_cue(),
            RtCommand::CueButton { deck, pressed } => self.player(deck).cue_button(pressed),
            RtCommand::CupButton { deck, pressed } => self.player(deck).cup_button(pressed),
            RtCommand::Seek { deck, seconds } => self.player(deck).seek_seconds(seconds),
            RtCommand::Tempo { deck, ratio } => {
                // The clock sets the speed of a deck that follows it.
                let follows = self.clock.master() != Some(deck) && self.player(deck).can_follow();
                if !follows {
                    self.player(deck).set_tempo(ratio);
                }
            }
            RtCommand::Nudge { deck, amount } => self.player(deck).set_nudge(amount),
            RtCommand::Grid { deck, edit } => self.player(deck).edit_grid(edit),
            RtCommand::Loop { deck, edit } => self.player(deck).edit_loop(edit),
            RtCommand::Jump { deck, by } => self.player(deck).jump(by),
            RtCommand::Cues { deck, edit } => self.player(deck).edit_cues(edit),
            RtCommand::SyncToggle { deck } => self.player(deck).toggle_sync(),
            RtCommand::KeyLockToggle { deck } => self.player(deck).toggle_key_lock(),
            RtCommand::KeyStep { deck, up } => self.player(deck).step_key(up),
            RtCommand::KeySet { deck, semitones } => self.player(deck).set_key(semitones),
            RtCommand::KeyPersist { deck } => self.player(deck).persist_key(),
            RtCommand::FluxToggle { deck } => self.player(deck).toggle_flux(),
            RtCommand::Reverse { deck, pressed } => self.player(deck).reverse(pressed),
            RtCommand::Master { deck } => {
                let candidate = self.player(deck).candidate();
                self.clock.set_master(deck, candidate);
            }
            RtCommand::ClockAuto { on } => {
                let decks = self.candidates();
                self.clock.set_auto(on, decks);
            }
            RtCommand::ClockTempo { bpm } => self.clock.set_bpm(bpm),
            RtCommand::ClockStep { amount } => self.clock.step_bpm(amount),
            RtCommand::ClockBend { amount } => self.clock.set_bend(amount),
            RtCommand::KeepBeat { on } => self.keep_beat = on,
            RtCommand::Snap { on } => {
                for channel in &mut self.channels {
                    channel.player.set_snap(on);
                }
            }
            RtCommand::Quantize { on } => {
                for channel in &mut self.channels {
                    channel.player.set_quantize(on);
                }
            }
            RtCommand::Metronome { on } => self.metronome.set_on(on),
            mixer => self.apply_mixer(mixer),
        }
    }

    fn candidates(&self) -> [Candidate; 2] {
        [
            self.channels[0].player.candidate(),
            self.channels[1].player.candidate(),
        ]
    }

    fn apply_mixer(&mut self, command: RtCommand) {
        match command {
            RtCommand::Gain { deck, linear } => self.strip(deck).set_gain(linear),
            RtCommand::Eq { deck, band, gain } => self.strip(deck).set_eq(band, gain),
            RtCommand::EqCut { deck, band, cut } => self.strip(deck).set_eq_cut(band, cut),
            RtCommand::Filter { deck, position } => self.strip(deck).set_filter(position),
            RtCommand::Balance { deck, position } => self.strip(deck).set_balance(position),
            RtCommand::Fader { deck, gain } => self.strip(deck).set_fader(gain),
            RtCommand::PreListen { deck, on } => {
                let target = if on { 1.0 } else { 0.0 };
                self.channels[deck.index()].pre_listen.set(target);
            }
            RtCommand::Crossfader { position } => {
                self.crossfader_position = position;
                self.retarget_crossfader();
            }
            RtCommand::CrossfaderCurve { curve } => {
                self.crossfader_curve = curve;
                self.retarget_crossfader();
            }
            RtCommand::MasterGain { linear } => self.master_gain.set(linear),
            RtCommand::HeadphoneMix { position } => self.headphone_mix.set(position),
            RtCommand::HeadphoneGain { linear } => self.headphone_gain.set(linear),
            RtCommand::TestTone { output } => self.tone.play(output),
            // The deck commands are handled by `apply`.
            _ => {}
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

    /// The next frame of the master and of the headphones. Without headphones their frame
    /// is silence and their part of the path is skipped, parameter ramps included.
    fn next_frame(&mut self, with_headphones: bool) -> (Frame, Frame) {
        let mut mix = [0.0f32; 2];
        let mut pre_listened = [0.0f32; 2];
        let master = self.clock.master();
        let lead = master.and_then(|deck| self.channels[deck.index()].player.lead());
        let pulse = self.clock.tick(lead);
        let click = self.metronome.next(pulse.phase);
        for (deck, channel) in DeckId::ALL.into_iter().zip(&mut self.channels) {
            // The tempo master follows nothing: the clock follows it.
            let clock = (master != Some(deck)).then_some(pulse);
            channel.player.follow(clock, self.keep_beat);
            let played = channel.player.next(&mut self.retired);
            let strip = channel.strip.process(played);
            let side = channel.crossfade.advance();
            mix[0] += strip.post[0] * side;
            mix[1] += strip.post[1] * side;
            if with_headphones {
                let send = channel.pre_listen.advance();
                pre_listened[0] += strip.pre[0] * send;
                pre_listened[1] += strip.pre[1] * send;
            }
        }
        let gain = self.master_gain.advance();
        let tone = self.tone.next(Output::Master);
        let master = self
            .limiter
            .process([mix[0] * gain + tone, mix[1] * gain + tone]);
        self.master_meter.feed(master);
        if !with_headphones {
            return (master, [0.0; 2]);
        }
        let balance = self.headphone_mix.advance();
        let gain = self.headphone_gain.advance();
        let tone = self.tone.next(Output::Headphones);
        // The headphone volume sets the level of the metronome; the cue mix does not.
        let blend =
            |index: usize| pre_listened[index] * (1.0 - balance) + master[index] * balance + click;
        let headphones = self
            .headphone_limiter
            .process([blend(0) * gain + tone, blend(1) * gain + tone]);
        (master, headphones)
    }

    fn publish(&mut self) {
        for (deck, channel) in DeckId::ALL.into_iter().zip(&mut self.channels) {
            let report = DeckReport {
                deck,
                grid: channel.player.unreported_grid().copied(),
                looping: channel.player.unreported_loop(),
                cues: channel.player.unreported_cues(),
            };
            let (grid, looping) = (report.grid.is_some(), report.looping.is_some());
            let cues = report.cues.is_some();
            // A full queue keeps what changed for the next block.
            if (grid || looping || cues) && self.reports.push(report).is_ok() {
                if cues {
                    channel.player.cues_reported();
                }
                if grid {
                    channel.player.grid_reported();
                }
                if looping {
                    channel.player.loop_reported();
                }
            }
        }
        let pulse = self.clock.pulse();
        self.shared.master.set(self.clock.master());
        self.shared.clock_bpm.set(pulse.bpm);
        for (channel, shared) in self.channels.iter().zip(&self.shared.decks) {
            shared.sync.set(channel.player.is_synced());
            shared.key_lock.set(channel.player.is_key_locked());
            shared.key_shift.set(channel.player.key_shift());
            shared.pitch.set(channel.player.pitch_semitones());
            shared.flux.set(channel.player.is_flux_on());
            shared.reverse.set(channel.player.is_reversed());
            shared
                .timeline
                .set(channel.player.timeline_seconds().unwrap_or(f64::NAN));
            shared.speed.set(f64::from(channel.player.speed()));
            let ahead = channel.player.beats_ahead(pulse);
            shared
                .phase
                .set(ahead.map_or(f32::NAN, |beats| beats as f32));
            shared.loaded.set(channel.player.is_loaded());
            shared.playing.set(channel.player.is_playing());
            shared.position.set(channel.player.position_seconds());
            shared.cue.set(channel.player.cue_seconds());
            let levels = channel.strip.levels();
            shared.levels[0].set(levels[0]);
            shared.levels[1].set(levels[1]);
        }
        let levels = self.master_meter.levels();
        self.shared.master_levels[0].set(levels[0]);
        self.shared.master_levels[1].set(levels[1]);
        self.shared.limiter_gain.set(self.limiter.gain());
    }
}
