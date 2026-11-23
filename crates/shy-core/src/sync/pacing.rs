//! How long the sync loop waits between checks. Every upload sends a signed
//! hint and every admin commit a nudge, so news normally wakes a check at
//! once; the timer is the safety net. It waits longer after each check that
//! found nothing gossip had missed, and starts short again after one that
//! did.

use std::time::Duration;

/// The wait after a check that found something gossip had missed.
pub(super) const SHORT_WAIT: Duration = Duration::from_secs(8);
/// The longest wait: how long a silent gossip split can go unnoticed.
const LONGEST_WAIT: Duration = Duration::from_secs(60);
/// The shortest wait of a workspace checking in the background.
const BACKGROUND_WAIT: Duration = Duration::from_secs(5 * 60);
/// The wall clock running this far ahead of the monotonic clock (which
/// stops while the computer sleeps) means it slept.
const SLEEP_GAP: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct Pacing {
    empty_checks: u32,
}

impl Pacing {
    /// The wait before the next check: `SHORT_WAIT` after one that found
    /// news, then half as long again after each empty one, up to
    /// `LONGEST_WAIT`.
    pub fn after_check(&mut self, has_found_new: bool) -> Duration {
        if has_found_new {
            self.empty_checks = 0;
            return SHORT_WAIT;
        }
        let mut wait = SHORT_WAIT;
        for _ in 0..self.empty_checks {
            wait = wait.mul_f64(1.5);
            if wait >= LONGEST_WAIT {
                break;
            }
        }
        self.empty_checks = self.empty_checks.saturating_add(1);
        wait.min(LONGEST_WAIT)
    }

    /// Something happened (a hint naming ops we lack, a nudge, a wake from
    /// sleep): the next waits start short again.
    pub fn wake(&mut self) {
        self.empty_checks = 0;
    }
}

/// How often a workspace checks the bucket when nothing wakes it. One of
/// several open in the app runs in the background once others were used
/// more recently: gossip still delivers live and wakes a check at once,
/// but the timer only checks every few minutes, so idle workspaces cost
/// few requests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Cadence {
    #[default]
    Live,
    Background,
}

impl Cadence {
    pub(super) fn stretch(self, wait: Duration) -> Duration {
        match self {
            Self::Live => wait,
            Self::Background => wait.max(BACKGROUND_WAIT),
        }
    }
}

/// Whether the computer slept while we waited: the wall clock moved on
/// much further than the monotonic one. A wall clock set back never counts.
pub(super) fn has_slept(wall_elapsed: Duration, monotonic_elapsed: Duration) -> bool {
    wall_elapsed > monotonic_elapsed.saturating_add(SLEEP_GAP)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seconds(waits: impl IntoIterator<Item = Duration>) -> Vec<u64> {
        waits.into_iter().map(|wait| wait.as_secs()).collect()
    }

    #[test]
    fn a_workspace_in_the_background_checks_every_few_minutes() {
        let mut pacing = Pacing::default();
        let wait = pacing.after_check(true);

        assert_eq!(Cadence::Live.stretch(wait), SHORT_WAIT);
        assert_eq!(Cadence::Background.stretch(wait), BACKGROUND_WAIT);
    }

    #[test]
    fn empty_checks_wait_longer_up_to_a_minute() {
        let mut pacing = Pacing::default();
        let waits = (0..9).map(|_| pacing.after_check(false));
        assert_eq!(seconds(waits), [8, 12, 18, 27, 40, 60, 60, 60, 60]);
    }

    #[test]
    fn news_gossip_missed_or_a_wake_starts_the_short_wait_again() {
        let mut pacing = Pacing::default();
        for _ in 0..6 {
            pacing.after_check(false);
        }
        assert_eq!(pacing.after_check(true), SHORT_WAIT);
        assert_eq!(pacing.after_check(false), SHORT_WAIT);

        for _ in 0..6 {
            pacing.after_check(false);
        }
        pacing.wake();
        assert_eq!(pacing.after_check(false), SHORT_WAIT);
    }

    #[test]
    fn the_wall_clock_running_ahead_means_the_computer_slept() {
        let waited = Duration::from_secs(8);
        assert!(has_slept(Duration::from_secs(600), waited));
        assert!(!has_slept(Duration::from_secs(9), waited));
        assert!(
            !has_slept(Duration::ZERO, waited),
            "a wall clock set back isn't sleep"
        );
    }
}
