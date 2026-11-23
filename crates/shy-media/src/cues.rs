//! The join and leave sounds, made here rather than shipped as files: two
//! soft notes, rising when someone arrives and falling when someone goes.
//! Your own come and go is a fifth; others' a fourth, higher and quieter.

use std::f32::consts::TAU;

use shy_core::huddle::media::Cue;

const NOTE_SECONDS: f32 = 0.11;
/// Short fades at both ends of each note, so it doesn't click.
const FADE_SECONDS: f32 = 0.012;
const OWN_VOLUME: f32 = 0.25;
const OTHERS_VOLUME: f32 = 0.18;

const C5: f32 = 523.25;
const E5: f32 = 659.26;
const G5: f32 = 783.99;
const A5: f32 = 880.0;

/// Mono samples of `cue` at `sample_rate`.
pub(crate) fn samples(cue: Cue, sample_rate: u32) -> Vec<f32> {
    let (first, second, volume) = match cue {
        Cue::Joined => (C5, G5, OWN_VOLUME),
        Cue::Left => (G5, C5, OWN_VOLUME),
        Cue::PeerJoined => (E5, A5, OTHERS_VOLUME),
        Cue::PeerLeft => (A5, E5, OTHERS_VOLUME),
    };
    let mut samples = note(first, volume, sample_rate);
    samples.extend(note(second, volume, sample_rate));
    samples
}

fn note(frequency: f32, volume: f32, sample_rate: u32) -> Vec<f32> {
    let rate = sample_rate as f32;
    let length = (NOTE_SECONDS * rate) as usize;
    let fade = FADE_SECONDS * rate;
    (0..length)
        .map(|index| {
            let from_edge = index.min(length - 1 - index) as f32;
            let envelope = (from_edge / fade).min(1.0);
            volume * envelope * (TAU * frequency * index as f32 / rate).sin()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    const ALL: [Cue; 4] = [Cue::Joined, Cue::PeerJoined, Cue::PeerLeft, Cue::Left];

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0, |peak, sample| peak.max(sample.abs()))
    }

    /// Roughly the pitch, from how often the wave crosses zero.
    fn crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
            .count()
    }

    #[test]
    fn every_cue_is_short_soft_and_starts_and_ends_quietly() {
        for cue in ALL {
            let samples = samples(cue, RATE);
            let seconds = samples.len() as f32 / RATE as f32;
            assert!((0.2..0.25).contains(&seconds), "{cue:?} lasts {seconds}s");
            assert!(peak(&samples) <= OWN_VOLUME + f32::EPSILON, "{cue:?} too loud");
            assert!(peak(&samples) > 0.1, "{cue:?} too quiet");
            assert!(samples[0].abs() < 0.01 && samples[samples.len() - 1].abs() < 0.01);
        }
    }

    #[test]
    fn arrivals_rise_and_departures_fall() {
        for (cue, rises) in [
            (Cue::Joined, true),
            (Cue::PeerJoined, true),
            (Cue::PeerLeft, false),
            (Cue::Left, false),
        ] {
            let samples = samples(cue, RATE);
            let (first, second) = samples.split_at(samples.len() / 2);
            assert_eq!(crossings(second) > crossings(first), rises, "{cue:?}");
        }
    }

    #[test]
    fn others_are_quieter_than_your_own() {
        assert!(peak(&samples(Cue::PeerJoined, RATE)) < peak(&samples(Cue::Joined, RATE)));
    }
}
