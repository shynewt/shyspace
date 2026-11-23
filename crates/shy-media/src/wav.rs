//! Reads the bundled sounds: plain PCM WAV files, mono, 48 kHz, 16-bit.
//! Anything else is refused, so a re-rendered sound in another format
//! fails its test instead of playing wrong.

use anyhow::{Context, Result, bail, ensure};

pub(crate) const RATE: u32 = 48_000;

const PCM: u16 = 1;
const BITS: u16 = 16;

/// The samples of `file`, from -1 to 1.
pub(crate) fn mono_samples(file: &[u8]) -> Result<Vec<f32>> {
    ensure!(
        file.get(..4) == Some(b"RIFF") && file.get(8..12) == Some(b"WAVE"),
        "not a WAV file"
    );
    let mut rest = &file[12..];
    let mut has_format = false;
    while !rest.is_empty() {
        let (id, body, after) = chunk(rest)?;
        match id {
            b"fmt " => {
                check_format(body)?;
                has_format = true;
            }
            b"data" => {
                ensure!(has_format, "a WAV file's samples come before their format");
                ensure!(body.len() % 2 == 0, "a WAV file ends halfway through a sample");
                let (pairs, _) = body.as_chunks::<2>();
                return Ok(pairs
                    .iter()
                    .map(|pair| f32::from(i16::from_le_bytes(*pair)) / 32_768.0)
                    .collect());
            }
            _ => {}
        }
        rest = after;
    }
    bail!("a WAV file without samples")
}

/// One chunk's id and body, and what follows it (bodies are padded to even lengths).
fn chunk(bytes: &[u8]) -> Result<(&[u8], &[u8], &[u8])> {
    let header = bytes.get(..8).context("a WAV chunk is cut short")?;
    let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let body = bytes.get(8..8 + length).context("a WAV chunk is cut short")?;
    let after = bytes.get(8 + length + length % 2..).unwrap_or_default();
    Ok((&header[..4], body, after))
}

fn check_format(body: &[u8]) -> Result<()> {
    let field = |at: usize| -> Result<u16> {
        let bytes = body.get(at..at + 2).context("a WAV format is cut short")?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    };
    let rate_bytes = body.get(4..8).context("a WAV format is cut short")?;
    let rate = u32::from_le_bytes([rate_bytes[0], rate_bytes[1], rate_bytes[2], rate_bytes[3]]);
    ensure!(field(0)? == PCM, "a sound must be plain PCM");
    ensure!(field(2)? == 1, "a sound must be mono");
    ensure!(rate == RATE, "a sound must be at {RATE} Hz, not {rate}");
    ensure!(field(14)? == BITS, "a sound must be 16-bit");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(format: u16, channels: u16, rate: u32, bits: u16, samples: &[i16]) -> Vec<u8> {
        let block = channels * bits / 8;
        let data: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
        let mut file = b"RIFF".to_vec();
        file.extend((36 + data.len() as u32).to_le_bytes());
        file.extend(b"WAVEfmt ");
        file.extend(16u32.to_le_bytes());
        file.extend(format.to_le_bytes());
        file.extend(channels.to_le_bytes());
        file.extend(rate.to_le_bytes());
        file.extend((rate * u32::from(block)).to_le_bytes());
        file.extend(block.to_le_bytes());
        file.extend(bits.to_le_bytes());
        file.extend(b"data");
        file.extend((data.len() as u32).to_le_bytes());
        file.extend(data);
        file
    }

    #[test]
    fn mono_16_bit_48_khz_reads_as_samples() {
        let samples = mono_samples(&wav(PCM, 1, RATE, 16, &[0, 16_384, -32_768])).unwrap();
        assert_eq!(samples, [0.0, 0.5, -1.0]);
    }

    #[test]
    fn any_other_format_is_refused() {
        for (format, channels, rate, bits) in [
            (3, 1, RATE, 16),
            (PCM, 2, RATE, 16),
            (PCM, 1, 44_100, 16),
            (PCM, 1, RATE, 24),
        ] {
            assert!(
                mono_samples(&wav(format, channels, rate, bits, &[0, 1])).is_err(),
                "{format} {channels} {rate} {bits}"
            );
        }
    }

    #[test]
    fn a_broken_file_is_refused_not_misread() {
        let good = wav(PCM, 1, RATE, 16, &[1, 2, 3]);
        assert!(mono_samples(&good[..good.len() - 1]).is_err(), "cut short");
        assert!(mono_samples(b"RIFF").is_err());
        assert!(mono_samples(&[0; 64]).is_err());
        let mut no_samples = good.clone();
        no_samples.truncate(44 - 8);
        assert!(mono_samples(&no_samples).is_err());
    }
}
