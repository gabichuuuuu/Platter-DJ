//! Render without a sound card: runs a list of timed commands through the same processor
//! the live audio uses and hands over the resulting stereo audio.
//!
//! The render is deterministic. Commands are applied at exact frame positions, and a load
//! is waited for before time moves on, so the output does not depend on how fast the
//! machine decodes.

use std::fmt;

use platter_core::{DeckId, SampleRate, ValueError};

use crate::command::Command;
use crate::control::{Engine, EngineError, Event};

pub const MAX_RENDER_SECONDS: f64 = 6.0 * 60.0 * 60.0;
const BLOCK_FRAMES: u64 = 512;

#[derive(Clone, Debug, PartialEq)]
pub struct TimedCommand {
    /// Time from the start of the render at which the command is applied.
    pub at_seconds: f64,
    pub command: Command,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderPlan {
    pub sample_rate: SampleRate,
    /// Length of the output.
    pub seconds: f64,
    /// Commands in any order; those at the same time keep the order given here.
    pub commands: Vec<TimedCommand>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderReport {
    pub frames: u64,
    /// Highest absolute sample of the output.
    pub peak: f32,
    /// Lowest gain the safety limiter applied; one means it never worked.
    pub lowest_limiter_gain: f32,
}

#[derive(Debug)]
pub enum RenderError {
    Value(ValueError),
    Command {
        at_seconds: f64,
        id: &'static str,
        error: EngineError,
    },
    Load {
        deck: DeckId,
        message: String,
    },
    Output(std::io::Error),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(error) => error.fmt(f),
            Self::Command {
                at_seconds,
                id,
                error,
            } => write!(f, "`{id}` at {at_seconds} s: {error}"),
            Self::Load { deck, message } => write!(f, "deck {deck}: {message}"),
            Self::Output(error) => write!(f, "cannot write the output: {error}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<ValueError> for RenderError {
    fn from(error: ValueError) -> Self {
        Self::Value(error)
    }
}

/// Runs the plan and passes the output to `sink` block by block, as interleaved stereo.
pub fn render(
    plan: &RenderPlan,
    mut sink: impl FnMut(&[f32]) -> std::io::Result<()>,
) -> Result<RenderReport, RenderError> {
    let rate = plan.sample_rate.as_f64();
    let seconds = ValueError::check("render length", plan.seconds, 0.0, MAX_RENDER_SECONDS)?;
    let total_frames = (seconds * rate).round() as u64;

    let mut schedule = Vec::with_capacity(plan.commands.len());
    for timed in &plan.commands {
        let at = ValueError::check("command time", timed.at_seconds, 0.0, MAX_RENDER_SECONDS)?;
        schedule.push(((at * rate).round() as u64, timed));
    }
    // A stable sort keeps the given order among commands at the same frame.
    schedule.sort_by_key(|(frame, _)| *frame);

    let (mut engine, mut processor) = Engine::new(plan.sample_rate);
    let mut block = vec![0.0f32; BLOCK_FRAMES as usize * 2];
    let mut report = RenderReport {
        frames: total_frames,
        peak: 0.0,
        lowest_limiter_gain: 1.0,
    };
    let mut pending = schedule.into_iter().peekable();
    let mut now = 0u64;

    loop {
        while let Some((_, timed)) = pending.next_if(|(frame, _)| *frame <= now) {
            apply(&mut engine, timed)?;
        }
        if now >= total_frames {
            break;
        }
        let until = pending
            .peek()
            .map_or(total_frames, |(frame, _)| (*frame).min(total_frames));
        let frames = (until - now).min(BLOCK_FRAMES);
        let samples = &mut block[..frames as usize * 2];
        processor.process(samples, 2);
        report.peak = samples
            .iter()
            .fold(report.peak, |peak, s| peak.max(s.abs()));
        report.lowest_limiter_gain = report
            .lowest_limiter_gain
            .min(engine.snapshot().limiter_gain);
        sink(samples).map_err(RenderError::Output)?;
        now += frames;
    }
    Ok(report)
}

fn apply(engine: &mut Engine, timed: &TimedCommand) -> Result<(), RenderError> {
    engine
        .dispatch(timed.command.clone())
        .map_err(|error| RenderError::Command {
            at_seconds: timed.at_seconds,
            id: timed.command.id(),
            error,
        })?;
    for event in engine.wait_for_loads() {
        if let Event::LoadFailed { deck, message } = event {
            return Err(RenderError::Load { deck, message });
        }
    }
    Ok(())
}
