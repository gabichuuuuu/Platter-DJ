//! Filter: a high-pass and a low-pass in series with a shared resonance. Knob 1 raises the low
//! cut, knob 2 is the resonance, knob 3 lowers the high cut. Key 1 rejects the band between
//! the two corners instead of keeping it. Key 2 makes knob 3 a one-knob filter: low-pass to
//! the left of the middle, high-pass to the right, nothing at the middle.

use platter_core::SampleRate;

use super::Controls;
use super::shared::{StateVariable, Tuning, exponential};
use crate::Frame;

const LOW_CUT_HZ: (f32, f32) = (20.0, 10_000.0);
const HIGH_CUT_HZ: (f32, f32) = (20_000.0, 60.0);
const RESONANCE_Q: (f32, f32) = (0.6, 8.0);
/// Below this a cut knob leaves the signal alone.
const OPEN: f32 = 0.005;
/// Half the width of the middle of the one-knob filter, where it leaves the signal alone.
const MIDDLE: f32 = 0.02;

#[derive(Clone, Copy, PartialEq)]
struct Setting {
    knobs: [f32; 3],
    keys: [bool; 2],
}

/// What the filter does for one setting of its controls.
#[derive(Clone, Copy)]
struct Plan {
    high_pass: Option<Tuning>,
    low_pass: Option<Tuning>,
    reject: bool,
}

pub(super) struct Filter {
    rate: f32,
    stages: [[StateVariable; 2]; 2],
    setting: Option<Setting>,
    plan: Plan,
}

impl Filter {
    pub(super) fn new(rate: SampleRate) -> Self {
        Self {
            rate: rate.as_f32(),
            stages: [[StateVariable::default(); 2]; 2],
            setting: None,
            plan: Plan {
                high_pass: None,
                low_pass: None,
                reject: false,
            },
        }
    }

    fn plan(&self, setting: Setting) -> Plan {
        let [low_cut, resonance, high_cut] = setting.knobs.map(|knob| {
            if knob.is_finite() {
                knob.clamp(0.0, 1.0)
            } else {
                0.0
            }
        });
        let [reject, one_knob] = setting.keys;
        let q = exponential(resonance, RESONANCE_Q.0, RESONANCE_Q.1);
        let tuning = |hz| Tuning::new(hz, q, self.rate);
        if one_knob {
            let offset = high_cut - 0.5;
            let travel = ((offset.abs() - MIDDLE) / (0.5 - MIDDLE)).max(0.0);
            return Plan {
                high_pass: (offset > MIDDLE)
                    .then(|| tuning(exponential(travel, LOW_CUT_HZ.0, LOW_CUT_HZ.1))),
                low_pass: (offset < -MIDDLE)
                    .then(|| tuning(exponential(travel, HIGH_CUT_HZ.0, HIGH_CUT_HZ.1))),
                reject: false,
            };
        }
        Plan {
            high_pass: (low_cut > OPEN)
                .then(|| tuning(exponential(low_cut, LOW_CUT_HZ.0, LOW_CUT_HZ.1))),
            low_pass: (high_cut > OPEN)
                .then(|| tuning(exponential(high_cut, HIGH_CUT_HZ.0, HIGH_CUT_HZ.1))),
            reject,
        }
    }

    pub(super) fn process(&mut self, input: Frame, controls: &Controls) -> Frame {
        let setting = Setting {
            knobs: controls.knobs,
            keys: controls.keys,
        };
        if self.setting != Some(setting) {
            self.plan = self.plan(setting);
            self.setting = Some(setting);
        }
        let plan = self.plan;
        let mut output = input;
        for (side, sample) in output.iter_mut().enumerate() {
            let [high_stage, low_stage] = &mut self.stages[side];
            let mut kept = *sample;
            if let Some(tuning) = &plan.high_pass {
                kept = high_stage.process(kept, tuning).high;
            }
            if let Some(tuning) = &plan.low_pass {
                kept = low_stage.process(kept, tuning).low;
            }
            *sample = if plan.reject && plan.high_pass.is_some() && plan.low_pass.is_some() {
                *sample - kept
            } else {
                kept
            };
        }
        output
    }

    pub(super) fn reset(&mut self) {
        self.stages = [[StateVariable::default(); 2]; 2];
    }
}
