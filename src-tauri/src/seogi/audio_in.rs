//! Reads a recording file into the audio the pipeline works on: 16 kHz mono.
//!
//! The built-in decoder covers what phone and desktop recorders usually write
//! (m4a/AAC, mp3, wav, flac, ogg/opus, and the sound track of mp4/mov/webm).
//! Formats it cannot read, such as AMR from older call recorders, go through
//! ffmpeg when one is installed. The file itself is only ever read.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Result};
use log::{info, warn};

use super::platform::{find_program, hide_console};

/// The whole of `path`'s audio as 16 kHz mono samples.
pub fn decode(path: &Path) -> Result<Vec<f32>> {
    if !path.is_file() {
        bail!("{} is not a file", path.display());
    }
    // A malformed file can make the decoder panic; that is a failed read.
    let built_in = std::panic::catch_unwind(|| crate::audio_toolkit::decode_audio_file(path))
        .unwrap_or_else(|_| Err(anyhow!("the decoder crashed on this file")));
    let built_in_error = match built_in {
        Ok(samples) if !samples.is_empty() => return Ok(samples),
        Ok(_) => anyhow!("the file has no audio"),
        Err(e) => e,
    };

    // On a Mac the system's own converter reads everything Apple's recorders
    // write, including what nothing else does, and it is always there.
    #[cfg(target_os = "macos")]
    let built_in_error = match decode_with_afconvert(path) {
        Ok(samples) if !samples.is_empty() => {
            info!(
                "Built-in decoder could not read {} ({built_in_error}); the system's converter did",
                path.display()
            );
            return Ok(samples);
        }
        Ok(_) => built_in_error,
        Err(system_error) => anyhow!("{built_in_error}; afconvert: {system_error}"),
    };

    let Some(ffmpeg) = find_ffmpeg() else {
        bail!(
            "{built_in_error}. This format needs ffmpeg, which was not found \
             (macOS: brew install ffmpeg, Windows: winget install Gyan.FFmpeg)"
        );
    };
    info!(
        "Built-in decoder could not read {} ({built_in_error}); using ffmpeg",
        path.display()
    );
    decode_with_ffmpeg(&ffmpeg, path)
        .map_err(|ffmpeg_error| anyhow!("{built_in_error}; ffmpeg: {ffmpeg_error}"))
}

/// Decodes with macOS's `afconvert` into a temporary WAV and reads that.
/// The temporary file is removed; the original is only read.
#[cfg(target_os = "macos")]
fn decode_with_afconvert(path: &Path) -> Result<Vec<f32>> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let wav = std::env::temp_dir().join(format!("seogi-decode-{}-{stamp}.wav", std::process::id()));
    let output = Command::new("/usr/bin/afconvert")
        .args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"])
        .arg(path)
        .arg(&wav)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| anyhow!("could not run afconvert: {e}"));
    let samples = output.and_then(|output| {
        if !output.status.success() {
            let reason = String::from_utf8_lossy(&output.stderr);
            bail!("{}", reason.lines().last().unwrap_or("it failed").trim());
        }
        let mut reader = hound::WavReader::open(&wav)?;
        reader
            .samples::<i16>()
            .map(|sample| Ok(f32::from(sample?) / 32_768.0))
            .collect::<Result<Vec<f32>>>()
    });
    let _ = std::fs::remove_file(&wav);
    samples
}

fn decode_with_ffmpeg(ffmpeg: &Path, path: &Path) -> Result<Vec<f32>> {
    let mut command = Command::new(ffmpeg);
    command
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "f32le", "-"])
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
    let samples: Vec<f32> = output
        .stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect();
    if samples.is_empty() {
        bail!("the file has no audio");
    }
    Ok(samples)
}

/// Where ffmpeg is, if it is installed: `SEOGI_FFMPEG`, beside the app, or
/// the usual install places.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SEOGI_FFMPEG").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
        warn!(
            "SEOGI_FFMPEG points at {}, which is not a file",
            path.display()
        );
    }
    let mut first: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        first.push(dir.join("resources"));
        first.push(dir);
    }
    if cfg!(windows) {
        first.push(PathBuf::from(r"C:\ffmpeg\bin"));
    }
    find_program("ffmpeg", &first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn the_system_converter_gives_sixteen_kilohertz_mono() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("스테레오.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        // Two seconds of a 440 Hz tone on both channels.
        for index in 0..88_200u32 {
            let value =
                ((index as f32 * 440.0 * std::f32::consts::TAU / 44_100.0).sin() * 8_000.0) as i16;
            writer.write_sample(value).unwrap();
            writer.write_sample(value).unwrap();
        }
        writer.finalize().unwrap();

        let samples = decode_with_afconvert(&path).unwrap();
        // Two seconds at 16 kHz, one channel, and not silence.
        assert!(
            (31_000..=33_000).contains(&samples.len()),
            "{}",
            samples.len()
        );
        assert!(samples.iter().any(|sample| sample.abs() > 0.1));
        // Nothing is left behind, and what is not audio is refused.
        let text = dir.path().join("메모.txt");
        std::fs::write(&text, "소리가 아님").unwrap();
        assert!(decode_with_afconvert(&text).is_err());
    }
}
