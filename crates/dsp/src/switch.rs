//! Changing the model of a block while audio plays. The outgoing and the incoming model run
//! side by side for a short fade, so the change does not click; the incoming model starts
//! from a cleared state.

use platter_core::SampleRate;

/// A model chosen among several, with the fade that follows a change.
#[derive(Clone, Copy, Debug)]
pub struct ModelSwitch<T> {
    active: T,
    /// The model fading out, while a fade runs.
    previous: Option<T>,
    /// A model asked for during a fade, which starts when the fade ends.
    pending: Option<T>,
    /// A model whose fade has just started and whose state the owner must clear.
    started: Option<T>,
    /// Weight of the active model during a fade, rising from zero to one.
    weight: f32,
    step: f32,
}

impl<T: Copy + PartialEq> ModelSwitch<T> {
    /// Starts on `active`, with fades of `milliseconds`.
    pub fn new(active: T, rate: SampleRate, milliseconds: f32) -> Self {
        let samples = (rate.as_f32() * milliseconds / 1_000.0).round();
        let samples = if samples.is_finite() {
            samples.clamp(1.0, 1_000_000.0)
        } else {
            1.0
        };
        Self {
            active,
            previous: None,
            pending: None,
            started: None,
            weight: 1.0,
            step: 1.0 / samples,
        }
    }

    /// The model asked for last, faded in or fading in.
    pub fn active(&self) -> T {
        self.pending.unwrap_or(self.active)
    }

    /// Asks for `model`. During a fade it waits for the fade to end, so no more than two
    /// models ever run at once.
    pub fn request(&mut self, model: T) {
        if self.previous.is_some() {
            self.pending = (model != self.active).then_some(model);
        } else if model != self.active {
            self.begin(model);
        }
    }

    fn begin(&mut self, model: T) {
        self.previous = Some(self.active);
        self.active = model;
        self.weight = 0.0;
        self.started = Some(model);
    }

    /// Advances one sample. Returns the model to run at full weight, or the incoming model,
    /// its weight and the outgoing model during a fade.
    pub fn advance(&mut self) -> (T, f32, Option<T>) {
        if self.previous.is_none()
            && let Some(next) = self.pending.take()
        {
            self.begin(next);
        }
        let Some(previous) = self.previous else {
            return (self.active, 1.0, None);
        };
        self.weight += self.step;
        if self.weight >= 1.0 {
            self.weight = 1.0;
            self.previous = None;
            return (self.active, 1.0, None);
        }
        (self.active, self.weight, Some(previous))
    }

    /// The model whose fade has just started, once: its owner clears its state before it
    /// runs.
    pub fn take_started(&mut self) -> Option<T> {
        self.started.take()
    }
}

/// `previous` blended into `active` by the weight of `active`.
pub fn blend(active: f32, previous: f32, weight: f32) -> f32 {
    previous + (active - previous) * weight
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;

    use super::ModelSwitch;

    fn switch() -> ModelSwitch<u8> {
        // Four samples of fade.
        ModelSwitch::new(0, SampleRate::new(8_000).unwrap(), 0.5)
    }

    #[test]
    fn a_change_fades_the_new_model_in_over_the_ramp_and_clears_it_first() {
        let mut switch = switch();
        assert_eq!(switch.advance(), (0, 1.0, None));
        switch.request(1);
        assert_eq!(switch.take_started(), Some(1));
        assert_eq!(switch.take_started(), None);
        let weights: Vec<_> = (0..5).map(|_| switch.advance()).collect();
        assert_eq!(
            weights,
            [
                (1, 0.25, Some(0)),
                (1, 0.5, Some(0)),
                (1, 0.75, Some(0)),
                (1, 1.0, None),
                (1, 1.0, None)
            ]
        );
    }

    #[test]
    fn a_change_during_a_fade_waits_for_it_and_the_same_model_does_nothing() {
        let mut switch = switch();
        switch.request(0);
        assert_eq!(switch.take_started(), None);
        switch.request(1);
        switch.advance();
        switch.request(2);
        assert_eq!(switch.active(), 2);
        let mut seen = Vec::new();
        for _ in 0..10 {
            let (active, _, previous) = switch.advance();
            seen.push((active, previous));
        }
        assert_eq!(seen.last(), Some(&(2, None)));
        assert!(seen.contains(&(2, Some(1))), "{seen:?}");
        // Asking for the model that is fading in drops what waited.
        switch.request(1);
        switch.advance();
        switch.request(1);
        for _ in 0..10 {
            switch.advance();
        }
        assert_eq!(switch.active(), 1);
    }
}
