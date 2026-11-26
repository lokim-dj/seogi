//! Room on the disk for a model download: checked before it starts, so a
//! download that cannot fit says why instead of failing part way.

use std::path::Path;

/// Room left free beyond the download itself: the system and the
/// recordings need some too.
pub const SPARE_BYTES: u64 = 1_000_000_000;

/// The bytes free for this user on the disk that holds `path`, or the
/// nearest folder above it that exists. `None` when the system will not
/// say.
pub fn free_bytes(path: &Path) -> Option<u64> {
    let mut at = path;
    while !at.exists() {
        at = at.parent()?;
    }
    available(at)
}

/// How many bytes are missing to download `needed` more bytes and still
/// keep [`SPARE_BYTES`] free; `None` when they fit.
pub fn shortfall(free: u64, needed: u64) -> Option<u64> {
    let wanted = needed.saturating_add(SPARE_BYTES);
    (free < wanted).then(|| wanted - free)
}

/// Whether a download's error says the disk filled up.
pub fn is_disk_full(error: &str) -> bool {
    let error = error.to_lowercase();
    // macOS and Linux: ENOSPC (28); Windows: ERROR_DISK_FULL (112) and
    // ERROR_HANDLE_DISK_FULL (39).
    error.contains("no space left")
        || error.contains("os error 28)")
        || error.contains("not enough space on the disk")
        || error.contains("os error 112)")
        || error.contains("os error 39)")
}

#[cfg(unix)]
fn available(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is a valid NUL-terminated string and `stat` a valid,
    // writable statvfs the call fills in.
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)]
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(windows)]
fn available(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut free: u64 = 0;
    // SAFETY: `wide` is NUL-terminated and outlives the call; `free` is a
    // valid u64 the call writes to.
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut free as *mut u64),
            None,
            None,
        )
    }
    .ok()?;
    Some(free)
}

#[cfg(not(any(unix, windows)))]
fn available(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_download_must_fit_with_room_to_spare() {
        let gb = 1_000_000_000;
        assert_eq!(shortfall(10 * gb, 2 * gb), None);
        assert_eq!(shortfall(3 * gb, 2 * gb), None);
        assert_eq!(shortfall(2 * gb, 2 * gb), Some(gb));
        assert_eq!(shortfall(0, 2 * gb), Some(3 * gb));
        assert_eq!(shortfall(u64::MAX, u64::MAX), None);
    }

    #[test]
    fn a_full_disk_is_recognised_in_the_errors_each_system_gives() {
        assert!(is_disk_full("No space left on device (os error 28)"));
        assert!(is_disk_full(
            "There is not enough space on the disk. (os error 112)"
        ));
        assert!(!is_disk_full("connection reset by peer (os error 54)"));
        assert!(!is_disk_full("HTTP 404"));
    }

    #[test]
    fn free_space_is_read_from_the_nearest_folder_that_exists() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("not/yet/made");
        assert!(free_bytes(&deep).is_some_and(|free| free > 0));
    }
}
