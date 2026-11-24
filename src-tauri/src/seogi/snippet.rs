//! A few seconds of a recording, for listening to the words an entry of the
//! minutes rests on. Checking a doubtful line should take ten seconds, not a
//! search through a two-hour file.
//!
//! The snippet is made from the original each time it is asked for and is
//! never kept: the original stays the only audio there is.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};

use super::audio_in;
use super::platform::hide_console;

/// The longest snippet handed out.
const MAX_MS: u64 = 60_000;

/// The part of `path` from `start_ms` to `end_ms` as a WAV file in memory
/// (16-bit, one channel).
pub fn wav(path: &Path, start_ms: u64, end_ms: u64) -> Result<Vec<u8>> {
    if end_ms <= start_ms {
        bail!("the snippet has no length");
    }
    let length_ms = (end_ms - start_ms).min(MAX_MS);

    // A WAV can be read from the middle directly.
    if let Some(found) = from_wav(path, start_ms, length_ms) {
        return found;
    }
    // Other formats: ffmpeg seeks straight to the place when it is there.
    if let Some(ffmpeg) = audio_in::find_ffmpeg() {
        match from_ffmpeg(&ffmpeg, path, start_ms, length_ms) {
            Ok(found) => return Ok(found),
            Err(e) => log::warn!("ffmpeg could not cut a snippet ({e:#}); decoding the whole file"),
        }
    }
    // Last resort: decode all of it and keep the part asked for.
    let pcm = audio_in::decode(path)?;
    let rate = super::chunks::RATE as u64;
    let from = ((start_ms * rate / 1_000) as usize).min(pcm.len());
    let to = (((start_ms + length_ms) * rate / 1_000) as usize).min(pcm.len());
    let samples: Vec<i16> = pcm[from..to]
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
        .collect();
    Ok(wav_file(&samples, rate as u32))
}

/// `None` when the file is not a WAV this can read.
fn from_wav(path: &Path, start_ms: u64, length_ms: u64) -> Option<Result<Vec<u8>>> {
    let is_wav = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"));
    if !is_wav {
        return None;
    }
    let mut reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
        return None;
    }
    let rate = u64::from(spec.sample_rate);
    let channels = usize::from(spec.channels.max(1));
    let from = (start_ms * rate / 1_000).min(u64::from(reader.duration()));
    let frames = (length_ms * rate / 1_000) as usize;
    Some((|| {
        reader
            .seek(from as u32)
            .context("could not find the place in the recording")?;
        let mut samples: Vec<i16> = Vec::with_capacity(frames);
        let mut frame: Vec<i32> = Vec::with_capacity(channels);
        for sample in reader.samples::<i16>().take(frames * channels) {
            frame.push(i32::from(sample.context("could not read the recording")?));
            if frame.len() == channels {
                // Several channels become one.
                samples.push((frame.iter().sum::<i32>() / channels as i32) as i16);
                frame.clear();
            }
        }
        Ok(wav_file(&samples, spec.sample_rate))
    })())
}

fn from_ffmpeg(ffmpeg: &Path, path: &Path, start_ms: u64, length_ms: u64) -> Result<Vec<u8>> {
    const RATE: u32 = 16_000;
    let seconds = |ms: u64| format!("{:.3}", ms as f64 / 1_000.0);
    let mut command = Command::new(ffmpeg);
    command
        .args(["-nostdin", "-v", "error", "-ss", &seconds(start_ms), "-t"])
        .arg(seconds(length_ms))
        .arg("-i")
        .arg(path)
        .args([
            "-vn",
            "-ac",
            "1",
            "-ar",
            &RATE.to_string(),
            "-f",
            "s16le",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut command);
    let output = command
        .output()
        .map_err(|e| anyhow!("could not run {}: {e}", ffmpeg.display()))?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr);
        bail!("{}", reason.lines().last().unwrap_or("it failed").trim());
    }
    let samples: Vec<i16> = output
        .stdout
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    if samples.is_empty() {
        bail!("there is no audio at that time");
    }
    Ok(wav_file(&samples, RATE))
}

/// A complete WAV file: 16-bit, one channel.
fn wav_file(samples: &[i16], rate: u32) -> Vec<u8> {
    let data_bytes = (samples.len() * 2) as u32;
    let mut file = Vec::with_capacity(44 + samples.len() * 2);
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes()); // PCM
    file.extend_from_slice(&1u16.to_le_bytes()); // one channel
    file.extend_from_slice(&rate.to_le_bytes());
    file.extend_from_slice(&(rate * 2).to_le_bytes()); // bytes a second
    file.extend_from_slice(&2u16.to_le_bytes()); // bytes a frame
    file.extend_from_slice(&16u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in samples {
        file.extend_from_slice(&sample.to_le_bytes());
    }
    file
}

/// Bytes as base64, for handing a snippet to the screen as text.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                out.push(ALPHABET[(group >> shift) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snippet_of_a_wav_is_exactly_the_part_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("녹음.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        // Five seconds; each sample says which hundredth of a second it is in.
        for index in 0..80_000u32 {
            writer.write_sample((index / 160) as i16).unwrap();
        }
        writer.finalize().unwrap();

        let snippet = wav(&path, 1_000, 2_500).unwrap();
        let mut reader = hound::WavReader::new(std::io::Cursor::new(snippet)).unwrap();
        assert_eq!(reader.spec(), spec);
        let samples: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
        assert_eq!(samples.len(), 24_000);
        assert_eq!(samples[0], 100);
        assert_eq!(*samples.last().unwrap(), 249);

        // Past the end there is simply less; a snippet with no length is refused.
        let tail = wav(&path, 4_500, 9_000).unwrap();
        let reader = hound::WavReader::new(std::io::Cursor::new(tail)).unwrap();
        assert_eq!(reader.duration(), 8_000);
        assert!(wav(&path, 2_000, 2_000).is_err());
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"\xff\xfe\xfd\xfc"), "//79/A==");
    }
}
