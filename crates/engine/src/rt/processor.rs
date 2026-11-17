//! The processor: two decks, the mixer, the master section and the headphone output, or
//! each deck on its own outputs in external mixer mode.

use std::sync::Arc;

use platter_core::{DeckId, SampleRate};
use platter_dsp::Frame;
use platter_dsp::crossfade::{CrossfaderCurve, CrossfaderSide};
use platter_dsp::limiter::Limiter;
use platter_dsp::meter::PeakMeter;
use platter_dsp::smooth::Smoothed;
use rtrb::{Consumer, Producer};

use super::clock::{Candidate, Clock};
use super::deck::DeckPlayer;
use super::metronome::Metronome;
use super::preview::PreviewPlayer;
use super::strip::{ChannelStrip, PARAMETER_RAMP_MS};
use super::tick::Tick;
use super::tone::TestTone;
use super::{COMMAND_CAPACITY, DeckReport, Output, Queues, Retired, RtCommand, Shared};

/// Channels a device needs for a second pair: the headphones, or deck B in external mode.
const CHANNELS_WITH_TWO_PAIRS: usize = 4;

struct Channel {
    player: DeckPlayer,
    strip: ChannelStrip,
    /// Gain the crossfader gives this side.
    crossfade: Smoothed,
    /// One while the deck is sent to the headphones, zero otherwise.
    pre_listen: Smoothed,
    tick: Tick,
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
    /// The side of the crossfader each channel is on.
    crossfader_sides: [CrossfaderSide; 2],
    /// The automatic fade the crossfader is in.
    fade: Option<Fade>,
    /// The length of one frame in minutes.
    frame_minutes: f64,
    master_gain: Smoothed,
    limiter: Limiter,
    master_meter: PeakMeter,
    headphone_mix: Smoothed,
    headphone_gain: Smoothed,
    headphone_limiter: Limiter,
    tone: TestTone,
    metronome: Metronome,
    preview: PreviewPlayer,
    /// Zero mixes inside, one sends each deck to its own pair:
    /// `docs/behavior/external-mixer.md`.
    external: Smoothed,
    /// One while the gain and the equaliser act in external mode.
    external_eq_gain: Smoothed,
    /// One while the master volume acts in external mode.
    external_master_volume: Smoothed,
    /// The limiters of the first and the second pair in external mode.
    pair_limiters: [Limiter; 2],
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
            tick: Tick::new(rate),
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
            crossfader_sides: [CrossfaderSide::Left, CrossfaderSide::Right],
            fade: None,
            frame_minutes: 1.0 / (rate.as_f64() * 60.0),
            master_gain: parameter(1.0),
            limiter: Limiter::new(rate),
            master_meter: PeakMeter::new(rate),
            headphone_mix: parameter(0.0),
            headphone_gain: parameter(1.0),
            headphone_limiter: Limiter::new(rate),
            tone: TestTone::new(rate),
            metronome: Metronome::new(rate),
            preview: PreviewPlayer::new(rate),
            external: parameter(0.0),
            external_eq_gain: parameter(0.0),
            external_master_volume: parameter(0.0),
            pair_limiters: [Limiter::new(rate), Limiter::new(rate)],
        }
    }

    /// Fills `output`, interleaved with `channels` channels per frame, with the next block
    /// of the mix. The master goes to the first two channels and, when there are four or
    /// more, the headphones go to the third and the fourth; in external mode deck A and
    /// deck B take those pairs instead. One channel gets the sum of both sides of the first
    /// pair at half level; further channels are silent. This is the function the audio
    /// callback calls, and the deviceless render calls the same one.
    pub fn process(&mut self, output: &mut [f32], channels: usize) {
        self.take_commands();
        self.clock.choose(self.candidates());
        if channels == 0 {
            output.fill(0.0);
            return;
        }
        let two_pairs = channels >= CHANNELS_WITH_TWO_PAIRS;
        let mut frames = output.chunks_exact_mut(channels);
        for frame in &mut frames {
            let ([left, right], pair) = self.next_frame(two_pairs);
            match frame {
                [only] => *only = 0.5 * (left + right),
                [first, second, rest @ ..] => {
                    *first = left;
                    *second = right;
                    rest.fill(0.0);
                    if let [third, fourth, ..] = rest {
                        *third = pair[0];
                        *fourth = pair[1];
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
            RtCommand::PreviewSwap { track, play } => {
                self.preview.swap(track, play, &mut self.retired);
            }
            RtCommand::PreviewToggle => self.preview.toggle(),
            RtCommand::PreviewSeek { seconds } => self.preview.seek(seconds),
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
            RtCommand::BeatTickToggle { deck } => self.player(deck).toggle_beat_tick(),
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
                self.fade = None;
                self.crossfader_position = position;
                self.retarget_crossfader();
            }
            RtCommand::CrossfaderCurve { curve } => {
                self.crossfader_curve = curve;
                self.retarget_crossfader();
            }
            RtCommand::CrossfaderFade { to, beats } => {
                self.fade = (to != self.crossfader_position).then_some(Fade { to, beats });
            }
            RtCommand::CrossfaderAssign { deck, side } => {
                self.crossfader_sides[deck.index()] = side;
                self.retarget_crossfader();
            }
            RtCommand::MasterGain { linear } => self.master_gain.set(linear),
            RtCommand::HeadphoneMix { position } => self.headphone_mix.set(position),
            RtCommand::HeadphoneGain { linear } => self.headphone_gain.set(linear),
            RtCommand::TestTone { output } => self.tone.play(output),
            RtCommand::ExternalMixer {
                on,
                eq_gain,
                master_volume,
            } => {
                let weight = |on: bool| if on { 1.0 } else { 0.0 };
                self.external.set(weight(on));
                self.external_eq_gain.set(weight(eq_gain));
                self.external_master_volume.set(weight(master_volume));
            }
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

    /// The gains move along a ramp, so neither the crossfader, a change of curve nor a new
    /// assignment clicks.
    fn retarget_crossfader(&mut self) {
        let gains = self.crossfader_curve.gains(self.crossfader_position);
        for (channel, side) in self.channels.iter_mut().zip(self.crossfader_sides) {
            channel.crossfade.set(side.gain(gains));
        }
    }

    /// Moves the crossfader one frame of its automatic fade, at the tempo of the clock.
    fn advance_fade(&mut self, bpm: f64) {
        let Some(fade) = self.fade else {
            return;
        };
        let step = (bpm * self.frame_minutes) as f32 / fade.beats;
        let distance = fade.to - self.crossfader_position;
        if distance.abs() <= step {
            self.crossfader_position = fade.to;
            self.fade = None;
        } else {
            self.crossfader_position += step.copysign(distance);
        }
        self.retarget_crossfader();
    }

    /// The next frame of the first and the second pair of outputs. In internal mode they
    /// are the master and the headphones; without a second pair the headphone part of the
    /// path is skipped, parameter ramps included. In external mode they are deck A and
    /// deck B, or both decks on the first pair; during the ramp between the modes both
    /// paths run and their outputs are blended.
    fn next_frame(&mut self, two_pairs: bool) -> (Frame, Frame) {
        let mut mix = [0.0f32; 2];
        let mut pre_listened = [0.0f32; 2];
        let mut decks = [[0.0f32; 2]; 2];
        let mut ticks = [0.0f32; 2];
        let external = self.external.advance();
        let routed = external > 0.0;
        let metered_outside = self.external.target() > 0.0;
        let eq_gain = self.external_eq_gain.advance();
        let master_volume = self.external_master_volume.advance();
        let gain = self.master_gain.advance();
        let external_gain = 1.0 + (gain - 1.0) * master_volume;
        let master = self.clock.master();
        let lead = master.and_then(|deck| self.channels[deck.index()].player.lead());
        let pulse = self.clock.tick(lead);
        let click = self.metronome.next(pulse.phase);
        self.advance_fade(pulse.bpm);
        let channels = DeckId::ALL.into_iter().zip(&mut self.channels);
        for (index, (deck, channel)) in channels.enumerate() {
            // The tempo master follows nothing: the clock follows it.
            let clock = (master != Some(deck)).then_some(pulse);
            channel.player.follow(clock, self.keep_beat);
            let played = channel.player.next(&mut self.retired);
            let strip = channel.strip.process(played);
            let side = channel.crossfade.advance();
            mix[0] += strip.post[0] * side;
            mix[1] += strip.post[1] * side;
            if two_pairs || routed {
                if channel.player.on_beat() {
                    channel.tick.play();
                }
                ticks[index] = channel.tick.next();
            }
            if two_pairs {
                let send = channel.pre_listen.advance();
                pre_listened[0] += strip.pre[0] * send;
                pre_listened[1] += strip.pre[1] * send;
            }
            let sent = |side: usize| {
                let source = played[side] + (strip.equalised[side] - played[side]) * eq_gain;
                source * external_gain
            };
            decks[index] = [sent(0), sent(1)];
            // The meter reads the signal the mode sends out.
            channel.strip.feed_meter(if metered_outside {
                decks[index]
            } else {
                strip.post
            });
        }
        let tone = self.tone.next(Output::Master);
        let master = self
            .limiter
            .process([mix[0] * gain + tone, mix[1] * gain + tone]);
        // The preview plays on, heard or not, so its position moves as it would.
        let preview = self.preview.next();
        let headphones = if two_pairs {
            self.headphones(pre_listened, master, click + ticks[0] + ticks[1], preview)
        } else {
            [0.0; 2]
        };
        if !routed {
            self.master_meter.feed(master);
            return (master, headphones);
        }
        let with = |deck: Frame, added: f32| [deck[0] + added, deck[1] + added];
        let [a, b] = decks;
        let (first, second) = if two_pairs {
            (
                self.pair_limiters[0].process(with(a, ticks[0] + tone)),
                self.pair_limiters[1].process(with(b, ticks[1])),
            )
        } else {
            let both = [a[0] + b[0], a[1] + b[1]];
            let first = self.pair_limiters[0].process(with(both, ticks[0] + ticks[1] + tone));
            (first, [0.0; 2])
        };
        let blend = |inside: Frame, outside: Frame| {
            [
                inside[0] * (1.0 - external) + outside[0] * external,
                inside[1] * (1.0 - external) + outside[1] * external,
            ]
        };
        let first = blend(master, first);
        self.master_meter.feed(first);
        (first, blend(headphones, second))
    }

    /// The headphones: the pre-listened decks and the master, weighted by the mix knob, the
    /// metronome, the beat ticks and the preview player, through their own limiter.
    fn headphones(
        &mut self,
        pre_listened: Frame,
        master: Frame,
        clicks: f32,
        preview: Frame,
    ) -> Frame {
        let balance = self.headphone_mix.advance();
        let gain = self.headphone_gain.advance();
        let tone = self.tone.next(Output::Headphones);
        // The beat tick, the metronome and the preview are heard whatever the mix, and
        // follow the volume, which sets their level.
        let blend = |index: usize| {
            pre_listened[index] * (1.0 - balance)
                + master[index] * balance
                + clicks
                + preview[index]
        };
        self.headphone_limiter
            .process([blend(0) * gain + tone, blend(1) * gain + tone])
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
        self.shared.crossfader.set(self.crossfader_position);
        for (channel, shared) in self.channels.iter().zip(&self.shared.decks) {
            shared.sync.set(channel.player.is_synced());
            shared.key_lock.set(channel.player.is_key_locked());
            shared.beat_tick.set(channel.player.has_beat_tick());
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
        self.shared.preview_playing.set(self.preview.is_playing());
        self.shared
            .preview_position
            .set(self.preview.position_seconds());
        let levels = self.master_meter.levels();
        self.shared.master_levels[0].set(levels[0]);
        self.shared.master_levels[1].set(levels[1]);
        let limiter = if self.external.target() > 0.0 {
            self.pair_limiters[0]
                .gain()
                .min(self.pair_limiters[1].gain())
        } else {
            self.limiter.gain()
        };
        self.shared.limiter_gain.set(limiter);
    }
}

/// An automatic fade of the crossfader: where it goes and how many beats the whole travel
/// takes.
#[derive(Clone, Copy)]
struct Fade {
    to: f32,
    beats: f32,
}
