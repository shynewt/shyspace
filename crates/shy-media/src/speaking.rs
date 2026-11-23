//! Whether someone is speaking, from the level of their audio over time.
//! Speech has short gaps between words, so "stopped" waits a moment, and
//! the indicator doesn't flicker on every pause.

use std::time::{Duration, Instant};

/// The level (RMS of samples in -1..1) above which audio counts as speech:
/// about -36 dBFS, above a quiet room and below a soft voice.
pub(crate) const SPEECH_LEVEL: f32 = 0.016;
/// How long the level stays under `SPEECH_LEVEL` before speaking ends.
pub(crate) const HOLD: Duration = Duration::from_millis(600);

/// Follows one voice. At most one change per `HOLD` in each direction, so
/// its reports are throttled by construction.
#[derive(Debug, Default)]
pub(crate) struct SpeakingDetector {
    is_speaking: bool,
    quiet_since: Option<Instant>,
}

impl SpeakingDetector {
    /// Takes the level measured at `now`; returns the new state if it changed.
    pub(crate) fn observe(&mut self, level: f32, now: Instant) -> Option<bool> {
        if level >= SPEECH_LEVEL {
            self.quiet_since = None;
            return self.change_to(true);
        }
        if !self.is_speaking {
            return None;
        }
        let quiet_since = *self.quiet_since.get_or_insert(now);
        if now.saturating_duration_since(quiet_since) >= HOLD {
            self.quiet_since = None;
            return self.change_to(false);
        }
        None
    }

    /// The voice stopped arriving, so it isn't speaking whatever its last
    /// level was. Returns whether it was speaking.
    pub(crate) fn stop(&mut self) -> bool {
        self.quiet_since = None;
        std::mem::replace(&mut self.is_speaking, false)
    }

    fn change_to(&mut self, is_speaking: bool) -> Option<bool> {
        if self.is_speaking == is_speaking {
            return None;
        }
        self.is_speaking = is_speaking;
        Some(is_speaking)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOUD: f32 = 0.1;
    const QUIET: f32 = 0.001;

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    #[test]
    fn speech_starts_at_once() {
        let start = Instant::now();
        let mut detector = SpeakingDetector::default();
        assert_eq!(detector.observe(QUIET, start), None);
        assert_eq!(detector.observe(LOUD, at(start, 20)), Some(true));
        assert_eq!(detector.observe(LOUD, at(start, 40)), None, "said once");
    }

    #[test]
    fn a_pause_between_words_doesnt_end_speaking() {
        let start = Instant::now();
        let mut detector = SpeakingDetector::default();
        detector.observe(LOUD, start);
        for millis in (20..500).step_by(20) {
            assert_eq!(detector.observe(QUIET, at(start, millis)), None);
        }
        assert_eq!(detector.observe(LOUD, at(start, 520)), None);
    }

    #[test]
    fn speaking_ends_after_the_hold() {
        let start = Instant::now();
        let mut detector = SpeakingDetector::default();
        detector.observe(LOUD, start);
        assert_eq!(detector.observe(QUIET, at(start, 20)), None);
        assert_eq!(detector.observe(QUIET, at(start, 600)), None);
        assert_eq!(detector.observe(QUIET, at(start, 620)), Some(false));
        assert_eq!(detector.observe(QUIET, at(start, 2000)), None, "said once");
    }

    #[test]
    fn the_hold_restarts_with_each_word() {
        let start = Instant::now();
        let mut detector = SpeakingDetector::default();
        detector.observe(LOUD, start);
        detector.observe(QUIET, at(start, 100));
        detector.observe(LOUD, at(start, 500));
        assert_eq!(detector.observe(QUIET, at(start, 520)), None);
        assert_eq!(detector.observe(QUIET, at(start, 1000)), None);
        assert_eq!(detector.observe(QUIET, at(start, 1120)), Some(false));
    }

    #[test]
    fn a_voice_that_stops_arriving_stops_speaking_at_once() {
        let start = Instant::now();
        let mut detector = SpeakingDetector::default();
        assert!(!detector.stop(), "wasn't speaking");
        detector.observe(LOUD, start);
        assert!(detector.stop());
        assert!(!detector.stop(), "said once");
        assert_eq!(detector.observe(LOUD, at(start, 20)), Some(true), "starts again");
    }
}
