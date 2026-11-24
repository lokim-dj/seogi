//! The original recording as evidence.
//!
//! The original file is never changed, re-encoded or deleted by the app. Its
//! SHA-256 is taken once, when the recording ends or the file is brought in,
//! and written beside it together with when and how it was recorded. Anyone
//! can later check that the file is the one the minutes were made from.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;

/// The sidecar's file name: "information about the original".
pub const SIDECAR_NAME: &str = "원본정보.json";

/// What is recorded about an original. Written as `원본정보.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Evidence {
    /// The original's file name, in this folder.
    pub file: String,
    pub sha256: String,
    pub size_bytes: u64,
    /// When the recording started and ended, ISO 8601 with the local offset.
    pub recorded_start: String,
    pub recorded_end: String,
    /// How the start time is known: `recorder` (recorded in this app),
    /// `file_name` (the recorder put it in the name), `file_metadata` (the
    /// recorder wrote it inside the file), or `file_modified_minus_length`
    /// (an estimate from the file's own time).
    #[serde(default)]
    pub time_source: String,
    pub duration_ms: u64,
    /// How it came to be here: `recorded` in the app, `imported` by hand, or
    /// `watched` (picked up from a watched folder).
    pub origin: String,
    /// Where an imported file was, when it was brought in.
    pub original_path: String,
    /// The microphone, for a recording made in the app.
    pub input_device: String,
    /// Set when the recording did not end as it should have: `app_stopped`
    /// (the app went down; what had reached the disk was recovered),
    /// `microphone_lost` or `file_incomplete`.
    #[serde(default)]
    pub capture_note: String,
    /// The calendar event it was matched to, if any.
    pub event_title: String,
    pub event_start: String,
    pub app: String,
    /// When this record was written.
    pub written_at: String,
}

/// The SHA-256 of a file, as lowercase hex. Reads it in pieces, so a
/// three-hour recording does not have to fit in memory.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("could not open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("could not read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Seconds between 1904, where MP4 files count from, and 1970.
const MP4_EPOCH_OFFSET: u64 = 2_082_844_800;

/// When an MP4-family recording (m4a, mp4, mov, qta, 3gp) says it was made,
/// in milliseconds since 1970 UTC: the time the recorder wrote into the file
/// itself, which stays true when the file is copied, uploaded or renamed
/// later. `None` for other formats, or when the file does not say.
pub fn created_in_file(path: &Path) -> Option<i64> {
    use std::io::{Seek, SeekFrom};

    /// Reads a box header at the current position: its type and how many
    /// bytes of content follow the header (`None` content for "to the end").
    fn header(file: &mut std::fs::File) -> Option<([u8; 4], Option<u64>)> {
        let mut head = [0u8; 8];
        file.read_exact(&mut head).ok()?;
        let size = u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]));
        let kind = [head[4], head[5], head[6], head[7]];
        match size {
            0 => Some((kind, None)),
            1 => {
                let mut large = [0u8; 8];
                file.read_exact(&mut large).ok()?;
                Some((kind, Some(u64::from_be_bytes(large).checked_sub(16)?)))
            }
            size => Some((kind, Some(size.checked_sub(8)?))),
        }
    }

    let mut file = std::fs::File::open(path).ok()?;
    // The movie box may come after hours of audio; boxes are skipped, not read.
    let movie_end = loop {
        let (kind, content) = header(&mut file)?;
        if &kind == b"moov" {
            let here = file.stream_position().ok()?;
            break content.map_or(u64::MAX, |content| here + content);
        }
        file.seek(SeekFrom::Current(i64::try_from(content?).ok()?))
            .ok()?;
    };
    while file.stream_position().ok()? < movie_end {
        let (kind, content) = header(&mut file)?;
        if &kind != b"mvhd" {
            file.seek(SeekFrom::Current(i64::try_from(content?).ok()?))
                .ok()?;
            continue;
        }
        // version (1 byte), flags (3), then the creation time: 32 bits in
        // version 0, 64 in version 1.
        let mut start = [0u8; 12];
        file.read_exact(&mut start).ok()?;
        let seconds = if start[0] == 1 {
            u64::from_be_bytes(start[4..12].try_into().ok()?)
        } else {
            u64::from(u32::from_be_bytes(start[4..8].try_into().ok()?))
        };
        // Zero means "not set"; so does anything before 1970.
        let unix = seconds
            .checked_sub(MP4_EPOCH_OFFSET)
            .filter(|_| seconds != 0)?;
        return i64::try_from(unix.checked_mul(1_000)?).ok();
    }
    None
}

/// Writes the sidecar into `folder`, replacing an earlier one.
pub fn write_sidecar(folder: &Path, evidence: &Evidence) -> Result<()> {
    let json = serde_json::to_string_pretty(evidence)?;
    super::store::write_atomically(&folder.join(SIDECAR_NAME), json.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_the_standard_sha256_of_the_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::write(&path, b"").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert!(sha256_file(&dir.path().join("없는파일")).is_err());
    }

    /// A box: its size, its type, its content.
    fn boxed(kind: &[u8; 4], content: &[u8]) -> Vec<u8> {
        let mut out = ((content.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(content);
        out
    }

    #[test]
    fn the_time_a_recorder_wrote_into_the_file_is_read() {
        let dir = tempfile::tempdir().unwrap();
        // 2026-11-24 01:00:00 UTC, as seconds since 1904.
        let made: u64 = 1_795_482_000 + MP4_EPOCH_OFFSET;

        // The usual layout: the audio first, the movie header at the end.
        let mut old = vec![0u8; 4];
        old.extend_from_slice(&(made as u32).to_be_bytes());
        old.extend_from_slice(&[0u8; 88]);
        let mut file = boxed(b"ftyp", b"M4A isom");
        file.extend(boxed(b"mdat", &[7u8; 5_000]));
        file.extend(boxed(
            b"moov",
            &[boxed(b"udta", b"x"), boxed(b"mvhd", &old)].concat(),
        ));
        let path = dir.path().join("녹음.m4a");
        std::fs::write(&path, &file).unwrap();
        assert_eq!(created_in_file(&path), Some(1_795_482_000_000));

        // The 64-bit form.
        let mut new = vec![1u8, 0, 0, 0];
        new.extend_from_slice(&made.to_be_bytes());
        new.extend_from_slice(&[0u8; 100]);
        let wide = [
            boxed(b"ftyp", b"qt  "),
            boxed(b"moov", &boxed(b"mvhd", &new)),
        ]
        .concat();
        std::fs::write(&path, &wide).unwrap();
        assert_eq!(created_in_file(&path), Some(1_795_482_000_000));

        // A time that was never set, a file that is not one of these, and
        // one cut off in the middle say nothing rather than something wrong.
        let unset = [
            boxed(b"ftyp", b"M4A "),
            boxed(b"moov", &boxed(b"mvhd", &[0u8; 100])),
        ]
        .concat();
        std::fs::write(&path, &unset).unwrap();
        assert_eq!(created_in_file(&path), None);
        std::fs::write(&path, b"RIFF....WAVEfmt ").unwrap();
        assert_eq!(created_in_file(&path), None);
        std::fs::write(&path, &file[..2_000]).unwrap();
        assert_eq!(created_in_file(&path), None);
    }
}
