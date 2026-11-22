//! The processor: four decks, the mixer, the master section and the headphone output, or
//! each deck on its own outputs in external mixer mode.

use std::sync::Arc;

use platter_core::{DeckId, SampleRate};
use platter_dsp::Frame;
use platter_dsp::crossfade::{CrossfaderCurve, CrossfaderSide};
use platter_dsp::limiter::Limiter;
use platter_dsp::maximiser::{Aligner, Maximiser};
use platter_dsp::meter::PeakMeter;
use platter_dsp::smooth::Smoothed;
use rtrb::{Consumer, Producer};

use super::clock::{Candidate, Clock};
use super::deck::DeckPlayer;
use super::effects::Effects;
use super::input::{InputReader, InputSource};
use super::loop_recorder::{LoopRecorder, LoopSource};
use super::metronome::Metronome;
use super::midi_clock::MidiClock;
use super::preview::PreviewPlayer;
use super::recorder::Tap;
use super::remix::RemixPlayer;
use super::strip::{ChannelStrip, PARAMETER_RAMP_MS};
use super::tick::Tick;
use super::tone::TestTone;
use super::{
    COMMAND_CAPACITY, CROSSFADER_SIDES, DeckKind, DeckReport, InputTarget, Output, Queues, Retired,
    RtCommand, Shared,
};

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
    /// One while the deck plays its input in the place of its track:
    /// `docs/behavior/live-input.md`.
    live: Smoothed,
    /// One while the deck plays its grid of samples in the place of its track:
    /// `docs/behavior/remix-deck.md`.
    remix: Smoothed,
    /// Boxed: its 64 cells would make the processor large.
    grid: Box<RemixPlayer>,
}

pub struct Processor {
    commands: Consumer<RtCommand>,
    retired: Producer<Retired>,
    reports: Producer<DeckReport>,
    shared: Arc<Shared>,
    channels: [Channel; DeckId::COUNT],
    clock: Clock,
    /// Synced decks keep their beats on those of the clock, not only its tempo.
    keep_beat: bool,
    crossfader_position: f32,
    crossfader_curve: CrossfaderCurve,
    /// The side of the crossfader each channel is on.
    crossfader_sides: [CrossfaderSide; DeckId::COUNT],
    /// The automatic fade the crossfader is in.
    fade: Option<Fade>,
    /// The length of one frame in minutes.
    frame_minutes: f64,
    master_gain: Smoothed,
    /// Before the safety limiter, while the Limiter key is lit: `docs/behavior/master.md`.
    maximiser: Maximiser,
    /// Keeps what the headphones take beside the master in time with it.
    aligner: Aligner,
    limiter: Limiter,
    master_meter: PeakMeter,
    headphone_mix: Smoothed,
    headphone_gain: Smoothed,
    headphone_limiter: Limiter,
    tone: TestTone,
    metronome: Metronome,
    midi_clock: MidiClock,
    /// The mix recorder records the input routed to the recorders: `docs/behavior/recording.md`.
    recorder_external: bool,
    /// The return of the FX Send for the frame, for the third pair of outputs.
    send_return: Frame,
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
    effects: Effects,
    recorder: Tap,
    /// Boxed with its two buffers, so the processor stays small.
    loop_recorder: Box<LoopRecorder>,
    input: Box<InputReader>,
    /// The input each target of the routing takes, in the order of [`InputTarget::ALL`].
    routes: [Option<InputSource>; InputTarget::ALL.len()],
    aux_level: Smoothed,
    /// Beats of the clock since the processor started, with the phase of the latest frame,
    /// and that phase: the remix decks count their boundaries on it.
    beat: f64,
    last_phase: f64,
}

impl Processor {
    pub(super) fn new(rate: SampleRate, queues: Queues, shared: Arc<Shared>) -> Self {
        let position = 0.5;
        let curve = CrossfaderCurve::default();
        let gains = curve.gains(position);
        let parameter = |value| Smoothed::new(value, rate, PARAMETER_RAMP_MS);
        let channel = |side: CrossfaderSide| Channel {
            player: DeckPlayer::new(rate),
            strip: ChannelStrip::new(rate),
            crossfade: parameter(side.gain(gains)),
            pre_listen: parameter(0.0),
            tick: Tick::new(rate),
            live: parameter(0.0),
            remix: parameter(0.0),
            grid: Box::new(RemixPlayer::new(rate)),
        };
        Self {
            commands: queues.commands,
            retired: queues.retired,
            reports: queues.reports,
            shared,
            channels: CROSSFADER_SIDES.map(channel),
            clock: Clock::new(rate),
            keep_beat: true,
            crossfader_position: position,
            crossfader_curve: curve,
            crossfader_sides: CROSSFADER_SIDES,
            fade: None,
            frame_minutes: 1.0 / (rate.as_f64() * 60.0),
            master_gain: parameter(1.0),
            // Off; every command to it carries all of its settings.
            maximiser: Maximiser::new(rate, PARAMETER_RAMP_MS, 1.0, 0.1),
            aligner: Aligner::new(rate),
            limiter: Limiter::new(rate),
            master_meter: PeakMeter::new(rate),
            headphone_mix: parameter(0.0),
            headphone_gain: parameter(1.0),
            headphone_limiter: Limiter::new(rate),
            tone: TestTone::new(rate),
            metronome: Metronome::new(rate),
            midi_clock: MidiClock::new(queues.clock),
            recorder_external: false,
            send_return: [0.0; 2],
            preview: PreviewPlayer::new(rate),
            external: parameter(0.0),
            external_eq_gain: parameter(0.0),
            external_master_volume: parameter(0.0),
            pair_limiters: [Limiter::new(rate), Limiter::new(rate)],
            effects: Effects::new(rate),
            recorder: Tap::new(queues.recorded),
            loop_recorder: Box::new(LoopRecorder::new(rate)),
            input: Box::new(InputReader::new(rate, queues.input)),
            routes: [None; InputTarget::ALL.len()],
            aux_level: parameter(0.0),
            beat: 0.0,
            last_phase: 0.0,
        }
    }

    /// Fills `output`, interleaved with `channels` channels per frame, with the next block
    /// of the mix. The master goes to the first two channels and, when there are four or
    /// more, the headphones go to the third and the fourth; in external mode decks A and C
    /// take the first pair and decks B and D the second instead. One channel gets the sum
    /// of both sides of the first pair at half level; further channels are silent. This is
    /// the function the audio callback calls, and the deviceless render calls the same one.
    pub fn process(&mut self, output: &mut [f32], channels: usize) {
        // The sender of the MIDI clock times the frames by when it sees this change.
        self.shared.block_frame.set(self.midi_clock.frame());
        self.take_commands();
        self.loop_recorder.prepare();
        self.clock.choose(self.candidates());
        if channels == 0 {
            output.fill(0.0);
            return;
        }
        self.input.prepare(output.len() / channels);
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
                    if let [third, fourth, rest @ ..] = rest {
                        *third = pair[0];
                        *fourth = pair[1];
                        if let [fifth, sixth, ..] = rest {
                            *fifth = self.send_return[0];
                            *sixth = self.send_return[1];
                        }
                    }
                }
                [] => {}
            }
        }
        frames.into_remainder().fill(0.0);
        self.effects.collect(&mut self.retired);
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
            // A live deck has no transport: its track stays paused until it is a track deck
            // again.
            RtCommand::Play { deck }
            | RtCommand::TogglePlay { deck }
            | RtCommand::CueButton { deck, .. }
            | RtCommand::CupButton { deck, .. }
                if self.is_live(deck) => {}
            RtCommand::Play { deck } => self.player(deck).set_playing(true),
            RtCommand::Pause { deck } => self.player(deck).set_playing(false),
            RtCommand::TogglePlay { deck } => self.player(deck).toggle_playing(),
            RtCommand::CueSet { deck } => self.player(deck).set_cue(),
            RtCommand::CueGoto { deck } => self.player(deck).goto_cue(),
            RtCommand::CueButton { deck, pressed } => self.player(deck).cue_button(pressed),
            RtCommand::CupButton { deck, pressed } => self.player(deck).cup_button(pressed),
            RtCommand::DeckType { deck, kind } => {
                if kind != DeckKind::Track {
                    self.player(deck).set_playing(false);
                }
                let channel = &mut self.channels[deck.index()];
                channel
                    .live
                    .set(if kind == DeckKind::Live { 1.0 } else { 0.0 });
                channel
                    .remix
                    .set(if kind == DeckKind::Remix { 1.0 } else { 0.0 });
            }
            RtCommand::RemixLoad {
                deck,
                at,
                audio,
                settings,
            } => {
                let grid = &mut self.channels[deck.index()].grid;
                grid.load(at, audio, settings, &mut self.retired);
            }
            RtCommand::RemixSettings { deck, at, settings } => {
                self.channels[deck.index()].grid.set_settings(at, settings);
            }
            RtCommand::RemixTrigger { deck, at } => {
                let beat = self.beat;
                self.channels[deck.index()].grid.trigger(at, beat);
            }
            RtCommand::RemixStop { deck, slot } => {
                let beat = self.beat;
                self.channels[deck.index()].grid.stop(slot, beat);
            }
            RtCommand::RemixVolume { deck, slot, level } => {
                self.channels[deck.index()].grid.set_volume(slot, level);
            }
            RtCommand::RemixMute { deck, slot, on } => {
                self.channels[deck.index()].grid.set_mute(slot, on);
            }
            RtCommand::RemixQuantize { deck, beats } => {
                self.channels[deck.index()].grid.set_quantize(beats);
            }
            RtCommand::InputRoute { target, source } => self.routes[target.index()] = source,
            RtCommand::AuxLevel { level } => self.aux_level.set(level),
            RtCommand::InputLatency { frames } => self.input.set_latency(frames),
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
            RtCommand::MidiClock { on } => self.midi_clock.set_sending(on),
            RtCommand::MidiClockRestart => self.midi_clock.restart(),
            RtCommand::ClockFollow { bpm, phase } => {
                // The count of the clock sent no longer marks the beat: start it again there.
                if self.clock.follow(bpm, phase) {
                    self.midi_clock.restart();
                }
            }
            RtCommand::Record { on, recording } => self.recorder.set(on, recording),
            RtCommand::RecorderSource { external } => self.recorder_external = external,
            RtCommand::LoopRecord => {
                let bpm = self.clock.pulse().bpm;
                self.loop_recorder.record(bpm);
            }
            RtCommand::LoopPlay => self.loop_recorder.toggle_play(),
            RtCommand::LoopUndo => self.loop_recorder.undo(),
            RtCommand::LoopRedo => self.loop_recorder.redo(),
            RtCommand::LoopDelete => self.loop_recorder.delete(),
            RtCommand::LoopBars { bars } => self.loop_recorder.set_bars(bars),
            RtCommand::LoopSource { source } => self.loop_recorder.set_source(source),
            RtCommand::LoopDryWet { amount } => self.loop_recorder.set_dry_wet(amount),
            RtCommand::LoopDecay { decay } => self.loop_recorder.set_decay(decay),
            RtCommand::FxValues { unit, values } => self.effects.set_values(unit, values),
            RtCommand::FxRouting { unit, routing } => self.effects.set_routing(unit, routing),
            RtCommand::FxAssign { deck, unit, on } => self.effects.set_assigned(deck, unit, on),
            RtCommand::FxPlace {
                deck,
                unit,
                place,
                effect,
            } => self
                .effects
                .place(deck, unit, place, effect, &mut self.retired),
            RtCommand::FxSendPlace {
                unit,
                place,
                effect,
            } => self
                .effects
                .place_send(unit, place, effect, &mut self.retired),
            RtCommand::MixerEffectPlace { deck, effect } => {
                self.effects.mixer_place(deck, effect, &mut self.retired);
            }
            RtCommand::MixerEffectAmount { deck, amount } => {
                self.effects.set_mixer_amount(deck, amount);
            }
            mixer => self.apply_mixer(mixer),
        }
    }

    fn candidates(&self) -> [Candidate; DeckId::COUNT] {
        self.channels.each_ref().map(|channel| {
            // A live or a remix deck has no tempo of its own, so it is never the tempo master.
            let candidate = channel.player.candidate();
            if channel.live.target() > 0.0 || channel.remix.target() > 0.0 {
                Candidate {
                    eligible: false,
                    live: false,
                    ..candidate
                }
            } else {
                candidate
            }
        })
    }

    /// Counts the beats of the clock from its phase, as the effect units do.
    fn count_beats(&mut self, phase: f64) {
        let mut whole = self.beat.floor();
        if phase < self.last_phase - 0.5 {
            whole += 1.0;
        } else if phase > self.last_phase + 0.5 {
            whole -= 1.0;
        }
        self.beat = whole + phase;
        self.last_phase = phase;
    }

    /// The deck plays its input or its grid: its track has no transport.
    fn is_live(&self, deck: DeckId) -> bool {
        let channel = &self.channels[deck.index()];
        channel.live.target() > 0.0 || channel.remix.target() > 0.0
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
            RtCommand::EqModel { model } => {
                for channel in &mut self.channels {
                    channel.strip.set_eq_model(model);
                }
            }
            RtCommand::FilterModel { model } => {
                for channel in &mut self.channels {
                    channel.strip.set_filter_model(model);
                    channel.player.set_stem_filter_model(model);
                }
            }
            RtCommand::Stem {
                deck,
                stem,
                settings,
            } => self.channels[deck.index()].player.set_stem(stem, settings),
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
            RtCommand::Maximiser {
                on,
                drive,
                ceiling,
                release_seconds,
            } => {
                self.maximiser.set_on(on);
                self.maximiser.set_drive(drive);
                self.maximiser.set_ceiling(ceiling);
                self.maximiser.set_release(release_seconds);
            }
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
    /// path is skipped, parameter ramps included. In external mode they are decks A and C
    /// and decks B and D, or all the decks on the first pair; during the ramp between the
    /// modes both paths run and their outputs are blended.
    fn next_frame(&mut self, two_pairs: bool) -> (Frame, Frame) {
        let mut mix = [0.0f32; 2];
        let mut pre_listened = [0.0f32; 2];
        let mut decks = [[0.0f32; 2]; DeckId::COUNT];
        let mut before_faders = [[0.0f32; 2]; DeckId::COUNT];
        let mut ticks = [0.0f32; DeckId::COUNT];
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
        self.midi_clock.next(pulse.phase);
        self.advance_fade(pulse.bpm);
        self.effects.follow(pulse);
        self.count_beats(pulse.phase);
        let beat = self.beat;
        let input = self.input.next();
        let routed_input = |target: InputTarget, routes: &[Option<InputSource>]| {
            routes[target.index()].map_or([0.0; 2], |source| source.take(&input))
        };
        let effects = &mut self.effects;
        let channels = DeckId::ALL.into_iter().zip(&mut self.channels);
        for (index, (deck, channel)) in channels.enumerate() {
            // The tempo master follows nothing: the clock follows it.
            let clock = (master != Some(deck)).then_some(pulse);
            channel.player.follow(clock, self.keep_beat);
            let stems = channel.player.has_stems();
            let mut next = channel.player.next(&mut self.retired);
            let live = channel.live.advance();
            if live > 0.0 {
                let heard = routed_input(InputTarget::Deck(deck), &self.routes);
                for ((sent, around), heard) in next.sent.iter_mut().zip(&mut next.around).zip(heard)
                {
                    *sent = *sent * (1.0 - live) + heard * live;
                    *around *= 1.0 - live;
                }
            }
            // The grid plays on unheard on a deck of another type, to stay in time.
            let grid = channel.grid.next(beat, pulse.bpm, &mut self.retired);
            let remix = channel.remix.advance();
            if remix > 0.0 {
                for ((sent, around), grid) in next.sent.iter_mut().zip(&mut next.around).zip(grid) {
                    *sent = *sent * (1.0 - remix) + grid * remix;
                    *around *= 1.0 - remix;
                }
            }
            let played = [0, 1].map(|side| next.sent[side] + next.around[side]);
            let strip = channel.strip.process(next, stems, |stage, frame| {
                effects.process(deck, stage, frame, metered_outside)
            });
            before_faders[index] = strip.pre;
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
                // The effects of the insert point act whether the equaliser does or not.
                let source = played[side]
                    + (strip.equalised[side] - played[side]) * eq_gain
                    + (strip.inserted[side] - strip.equalised[side]);
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
        // The loop is taken before it joins the mix, so it never records itself:
        // `docs/behavior/recording.md`, *Loop recorder*.
        let source = match self.loop_recorder.source() {
            LoopSource::Master => mix,
            LoopSource::Deck(deck) => before_faders[deck.index()],
            LoopSource::External => routed_input(InputTarget::Recorder, &self.routes),
        };
        let (dry, looped) = self.loop_recorder.next(source, pulse.phase);
        // The units in send routing take the FX Send and return to the third pair, in
        // external mode only: `docs/behavior/effects.md`, *Send*.
        self.send_return = if routed {
            let sent = routed_input(InputTarget::FxSend, &self.routes);
            let wet = self.effects.process_send(sent);
            [wet[0] * external, wet[1] * external]
        } else {
            [0.0; 2]
        };
        // The auxiliary input joins after the channels; it is silent in external mode, where
        // the master is not heard: `docs/behavior/live-input.md`.
        let aux = self.aux_level.advance() * (1.0 - external);
        let heard = routed_input(InputTarget::Aux, &self.routes);
        let mix = [
            mix[0] * dry + looped[0] + heard[0] * aux,
            mix[1] * dry + looped[1] + heard[1] * aux,
        ];
        let tone = self.tone.next(Output::Master);
        // The test tone comes after the maximiser, so its drive does not change its level.
        let maximised = self.maximiser.process([mix[0] * gain, mix[1] * gain]);
        let master = self
            .limiter
            .process([maximised[0] + tone, maximised[1] + tone]);
        // The internal mix, in either mode: `docs/behavior/recording.md`.
        self.recorder.feed(if self.recorder_external {
            routed_input(InputTarget::Recorder, &self.routes)
        } else {
            master
        });
        // The preview plays on, heard or not, so its position moves as it would.
        let preview = self.preview.next();
        // The pre-listened decks and the ticks wait as long as the master does, so a deck
        // heard in both does not comb in the headphones. The aligner runs every frame, so it
        // holds nothing stale when the headphones come back.
        let clicks = if two_pairs {
            click + ticks.iter().sum::<f32>()
        } else {
            0.0
        };
        let [left, right, clicks] = self.aligner.process(
            [pre_listened[0], pre_listened[1], clicks],
            self.maximiser.weight(),
        );
        let headphones = if two_pairs {
            self.headphones([left, right], master, clicks, preview)
        } else {
            [0.0; 2]
        };
        if !routed {
            self.master_meter.feed(master);
            return (master, headphones);
        }
        // Each deck goes to the pair of its column, with its beat tick; one pair takes all.
        let mut pairs = [[0.0f32; 2]; 2];
        for (index, (deck, tick)) in decks.into_iter().zip(ticks).enumerate() {
            let pair = &mut pairs[if two_pairs { index % 2 } else { 0 }];
            pair[0] += deck[0] + tick;
            pair[1] += deck[1] + tick;
        }
        let [a, b] = pairs;
        let first = self.pair_limiters[0].process([a[0] + tone, a[1] + tone]);
        let second = if two_pairs {
            self.pair_limiters[1].process(b)
        } else {
            [0.0; 2]
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
        self.shared.midi_clock.set(self.midi_clock.is_sending());
        self.shared.crossfader.set(self.crossfader_position);
        for (channel, shared) in self.channels.iter().zip(&self.shared.decks) {
            shared.sync.set(channel.player.is_synced());
            for (slot, (playing, waiting)) in shared
                .remix_playing
                .iter()
                .zip(&shared.remix_waiting)
                .enumerate()
            {
                let state = channel.grid.state(slot);
                let code = |cell: Option<usize>| cell.map_or(0, |cell| cell as u64 + 1);
                playing.set(code(state.playing));
                waiting.set(code(state.waiting));
            }
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
        self.shared.maximiser_gain.set(self.maximiser.gain());
        self.shared.recorded_frames.set(self.recorder.frames());
        self.shared.recorder_dropped.set(self.recorder.dropped());
        let looper = &self.loop_recorder;
        self.shared.loop_phase.set(u64::from(looper.phase().code()));
        self.shared.loop_progress.set(looper.progress());
        self.shared.loop_seconds.set(looper.length_seconds());
        self.shared
            .loop_bars
            .set(u64::from(looper.fitting_bars(pulse.bpm)));
        self.shared.loop_can_undo.set(looper.can_undo());
        self.shared.loop_can_redo.set(looper.can_redo());
        for (shared, level) in self.shared.input_levels.iter().zip(self.input.levels()) {
            shared.set(level);
        }
        self.shared.input_gaps.set(self.input.gaps());
        self.shared.input_reading.set(self.input.is_reading());
        self.shared.input_ratio.set(self.input.ratio());
    }
}

/// An automatic fade of the crossfader: where it goes and how many beats the whole travel
/// takes.
#[derive(Clone, Copy)]
struct Fade {
    to: f32,
    beats: f32,
}

#[cfg(test)]
mod tests {
    use super::Processor;

    /// The processor is built on the stack of the thread that starts the engine, 1 MB on the
    /// main thread of Windows. At 70 KB a debug build overflowed it
    /// (`docs/decisions/0033-effect-chain.md`); what grows goes in a box.
    #[test]
    fn the_processor_stays_small_enough_to_build_on_any_stack() {
        let size = std::mem::size_of::<Processor>();
        assert!(size <= 64 * 1024, "the processor takes {size} bytes");
    }
}
