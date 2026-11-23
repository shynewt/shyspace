//! Telling a working microphone from one that delivers nothing. A real room
//! is never exactly silent, so a microphone giving only zeros for a few
//! seconds is blocked or broken, even though it opened fine.

use std::time::{Duration, Instant};

use shy_core::huddle::media::MicState;

/// How long exact silence lasts before the microphone counts as silent.
pub(crate) const SILENT_AFTER: Duration = Duration::from_secs(3);

/// Follows one opening of the microphone, from `Opening`.
#[derive(Debug)]
pub(crate) struct SilenceWatch {
    state: MicState,
    zeros_since: Instant,
}

impl SilenceWatch {
    pub(crate) fn opened(now: Instant) -> Self {
        Self {
            state: MicState::Opening,
            zeros_since: now,
        }
    }

    /// Takes the peak of the samples captured at `now`, before any
    /// processing; returns the new state if it changed.
    pub(crate) fn observe(&mut self, peak: f32, now: Instant) -> Option<MicState> {
        let state = if peak != 0.0 {
            self.zeros_since = now;
            MicState::Live
        } else if now.saturating_duration_since(self.zeros_since) >= SILENT_AFTER {
            MicState::Silent
        } else {
            self.state
        };
        if state == self.state {
            return None;
        }
        self.state = state;
        Some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    #[test]
    fn the_first_sound_makes_it_live() {
        let start = Instant::now();
        let mut watch = SilenceWatch::opened(start);
        assert_eq!(watch.observe(0.0, at(start, 10)), None, "still opening");
        assert_eq!(watch.observe(0.0004, at(start, 20)), Some(MicState::Live));
        assert_eq!(watch.observe(0.2, at(start, 40)), None, "said once");
    }

    #[test]
    fn exact_silence_from_the_start_is_reported() {
        let start = Instant::now();
        let mut watch = SilenceWatch::opened(start);
        assert_eq!(watch.observe(0.0, at(start, 2990)), None);
        assert_eq!(watch.observe(0.0, at(start, 3000)), Some(MicState::Silent));
        assert_eq!(watch.observe(0.0, at(start, 5000)), None, "said once");
    }

    #[test]
    fn a_live_microphone_going_silent_is_reported_and_recovers() {
        let start = Instant::now();
        let mut watch = SilenceWatch::opened(start);
        watch.observe(0.1, start);
        assert_eq!(watch.observe(0.0, at(start, 2000)), None);
        assert_eq!(watch.observe(0.0, at(start, 3000)), Some(MicState::Silent));
        assert_eq!(watch.observe(0.01, at(start, 3100)), Some(MicState::Live));
    }

    #[test]
    fn short_runs_of_zeros_are_fine() {
        let start = Instant::now();
        let mut watch = SilenceWatch::opened(start);
        watch.observe(0.1, start);
        for second in 1..10 {
            assert_eq!(watch.observe(0.0, at(start, second * 1000)), None);
            assert_eq!(watch.observe(0.1, at(start, second * 1000 + 500)), None);
        }
    }
}
