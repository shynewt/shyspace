//! The join and leave sounds and the ring, bundled into the app. They're
//! rendered by `sounds/render.py`; `sounds/README.md` says what each is.
//! Played at unity gain: their quietness is intended.

use anyhow::Result;
use shy_core::huddle::media::Cue;

use crate::wav;

/// One cycle of the incoming-huddle ring. It starts and ends at silence,
/// so it loops without a click.
const RING: &[u8] = include_bytes!("../sounds/ring.wav");

fn file(cue: Cue) -> &'static [u8] {
    match cue {
        Cue::Joined => include_bytes!("../sounds/joined.wav"),
        Cue::PeerJoined => include_bytes!("../sounds/peer-joined.wav"),
        Cue::PeerLeft => include_bytes!("../sounds/peer-left.wav"),
        Cue::Left => include_bytes!("../sounds/left.wav"),
    }
}

/// Mono samples at [`wav::RATE`].
pub(crate) fn samples(cue: Cue) -> Result<Vec<f32>> {
    wav::mono_samples(file(cue))
}

/// One ring cycle's mono samples at [`wav::RATE`].
pub(crate) fn ring() -> Result<Vec<f32>> {
    wav::mono_samples(RING)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Cue; 4] = [Cue::Joined, Cue::PeerJoined, Cue::PeerLeft, Cue::Left];

    fn seconds(samples: &[f32]) -> f32 {
        samples.len() as f32 / wav::RATE as f32
    }

    #[test]
    fn every_sound_is_bundled_in_the_one_format_played() {
        for cue in ALL {
            let samples = samples(cue).unwrap_or_else(|error| panic!("{cue:?}: {error:#}"));
            assert!(
                (0.5..1.5).contains(&seconds(&samples)),
                "{cue:?} is a short sound"
            );
        }
        assert!((2.0..3.0).contains(&seconds(&ring().unwrap())), "one ring cycle");
    }

    #[test]
    fn the_ring_loops_without_a_click() {
        let ring = ring().unwrap();
        assert!(ring[0].abs() < 0.001 && ring[ring.len() - 1].abs() < 0.001);
    }
}
