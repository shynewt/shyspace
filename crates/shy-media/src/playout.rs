//! Plays a whole sound through a sink that holds only a moment: writes a
//! little at a time, keeping just ahead of the speaker.

use std::time::Duration;

use anyhow::{Result, bail};
use tokio::time::Instant;

use crate::{backend::Sink, wav};

/// How far ahead of real time the sound is written: under the sink's own
/// 50 ms queue, so it rarely has to refuse any.
const LEAD: Duration = Duration::from_millis(20);
/// 10 ms of mono samples at 48 kHz.
const CHUNK: usize = 480;
const TICK: Duration = Duration::from_millis(5);
/// How long the speaker may take nothing before the sound is given up.
const STALL_LIMIT: Duration = Duration::from_secs(2);

/// Writes mono `samples` at [`wav::RATE`] into `sink` as fast as they play.
///
/// Paced by the clock from the first sample the speaker takes, not by what
/// the sink reports as buffered: a real one counts its own latency there,
/// which never drops below a fixed mark.
pub(crate) async fn play<K: Sink>(sink: &mut K, samples: &[f32]) -> Result<()> {
    let mut rest = samples;
    let mut started: Option<Instant> = None;
    let mut written = Duration::ZERO;
    let mut stalled_since = Instant::now();
    while !rest.is_empty() {
        if let Some(started) = started
            && written >= started.elapsed() + LEAD
        {
            tokio::time::sleep(TICK).await;
            continue;
        }
        let taken = sink.write(&rest[..rest.len().min(CHUNK)])?;
        rest = &rest[taken.min(rest.len())..];
        if taken > 0 {
            started.get_or_insert_with(Instant::now);
            written += Duration::from_secs_f64(taken as f64 / f64::from(wav::RATE));
            stalled_since = Instant::now();
            continue;
        }
        if stalled_since.elapsed() >= STALL_LIMIT {
            bail!("the speaker isn't playing");
        }
        tokio::time::sleep(TICK).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tokio::time::Instant;

    use super::*;

    const RATE: f32 = 48_000.0;

    /// A speaker that starts reading after `starts_after` and holds 50 ms.
    struct Device {
        started: Instant,
        starts_after: Duration,
        written: usize,
        heard: Vec<f32>,
    }

    impl Device {
        fn new(starts_after: Duration) -> Self {
            Self {
                started: Instant::now(),
                starts_after,
                written: 0,
                heard: Vec::new(),
            }
        }

        fn is_reading(&self) -> bool {
            self.started.elapsed() >= self.starts_after
        }

        fn played(&self) -> usize {
            let reading_for = self.started.elapsed().saturating_sub(self.starts_after);
            ((reading_for.as_secs_f32() * RATE) as usize).min(self.written)
        }
    }

    impl Sink for Device {
        fn write(&mut self, samples: &[f32]) -> Result<usize> {
            if !self.is_reading() {
                return Ok(0);
            }
            let room = (0.05 * RATE) as usize - (self.written - self.played());
            let taken = samples.len().min(room);
            self.heard.extend_from_slice(&samples[..taken]);
            self.written += taken;
            Ok(taken)
        }

        async fn finish(self) {}
    }

    fn sound(seconds: f32) -> Vec<f32> {
        (0..(seconds * RATE) as usize).map(|index| index as f32).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn a_sound_longer_than_the_sink_plays_whole_and_in_order() {
        let mut device = Device::new(Duration::ZERO);
        let sound = sound(1.0);
        play(&mut device, &sound).await.unwrap();
        assert_eq!(device.heard, sound);
    }

    #[tokio::test(start_paused = true)]
    async fn nothing_is_lost_while_the_speaker_starts() {
        let mut device = Device::new(Duration::from_millis(300));
        let sound = sound(0.5);
        play(&mut device, &sound).await.unwrap();
        assert_eq!(device.heard, sound);
    }

    #[tokio::test(start_paused = true)]
    async fn a_sound_plays_in_about_its_own_length() {
        let mut device = Device::new(Duration::ZERO);
        let sound = sound(1.0);
        let started = Instant::now();
        play(&mut device, &sound).await.unwrap();
        let took = started.elapsed();
        assert!(
            took < Duration::from_millis(1_100),
            "took {took:?}, as if it waited on the sink"
        );
        assert_eq!(device.heard, sound);
    }

    #[tokio::test(start_paused = true)]
    async fn a_speaker_that_never_plays_is_given_up() {
        let mut device = Device::new(Duration::from_secs(3600));
        assert!(play(&mut device, &sound(0.5)).await.is_err());
    }
}
