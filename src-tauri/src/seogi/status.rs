//! The red dot: what Seogi is doing, shown where the user looks without
//! opening the window. Usually there is no red at all. Recording a meeting or
//! listening for a dictation shows a clear red dot that blinks every 1.6 s;
//! writing minutes or tidying a dictation shows a small, faint dot.
//!
//! The dot is on the menu bar icon (blinking), on the Dock icon on macOS
//! (steady: a blinking Dock icon would pull the eye all meeting long), and on
//! the taskbar button on Windows (an overlay icon). The indicator draws its
//! own from the same events.
//!
//! The pictures and timings come from `docs/icon/README.md`.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::tray::TrayIconState;

/// What the dot says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    /// Recording a meeting, or listening for a dictation.
    Recording,
    /// Writing minutes, or transcribing and tidying a dictation.
    Busy,
}

static MEETING_RECORDING: AtomicBool = AtomicBool::new(false);
static MEETING_BUSY: AtomicBool = AtomicBool::new(false);
/// The blink's off half is showing.
static DIM: AtomicBool = AtomicBool::new(false);
static BLINKING: AtomicBool = AtomicBool::new(false);
/// The status last shown on the Dock and the taskbar (as a number).
static SHOWN: AtomicU8 = AtomicU8::new(0);
/// Held while a change is worked out and shown.
static CHANGING: Mutex<()> = Mutex::new(());

/// The blink, as the design gives it: fully on for 72% of 1.6 s, then low.
const ON: Duration = Duration::from_millis(1150);
const OFF: Duration = Duration::from_millis(450);

fn code(status: Status) -> u8 {
    match status {
        Status::Idle => 0,
        Status::Recording => 1,
        Status::Busy => 2,
    }
}

/// The status from what the meetings and dictation are doing.
fn combine(meeting_recording: bool, meeting_busy: bool, dictation: TrayIconState) -> Status {
    if meeting_recording || dictation == TrayIconState::Recording {
        Status::Recording
    } else if meeting_busy || dictation == TrayIconState::Transcribing {
        Status::Busy
    } else {
        Status::Idle
    }
}

/// What the menu bar shows for a dictation state, the meetings included.
pub fn tray_state(dictation: TrayIconState) -> TrayIconState {
    match combine(
        MEETING_RECORDING.load(Ordering::SeqCst),
        MEETING_BUSY.load(Ordering::SeqCst),
        dictation,
    ) {
        Status::Recording => TrayIconState::Recording,
        Status::Busy => TrayIconState::Transcribing,
        Status::Idle => TrayIconState::Idle,
    }
}

/// Whether the blink is in its low half now.
pub fn dim() -> bool {
    DIM.load(Ordering::SeqCst)
}

/// A meeting started or stopped being recorded.
pub fn set_meeting_recording(app: &AppHandle, recording: bool) {
    MEETING_RECORDING.store(recording, Ordering::SeqCst);
    changed(app);
}

/// The queue started or finished working on a meeting.
pub fn set_meeting_busy(app: &AppHandle, busy: bool) {
    if MEETING_BUSY.swap(busy, Ordering::SeqCst) != busy {
        crate::tray::refresh_tray_icon(app);
        changed(app);
    }
}

/// Something changed: the Dock and the taskbar follow, and the blink starts
/// or stops. The menu bar is drawn by `tray`, which asks [`tray_state`].
pub fn changed(app: &AppHandle) {
    // One change at a time, from reading the state to asking for the
    // picture: two threads changing at once could otherwise ask for their
    // pictures in the opposite order and leave the older one showing.
    let _one_at_a_time = CHANGING.lock().unwrap_or_else(|e| e.into_inner());
    let dictation = crate::tray::current_state(app);
    let status = combine(
        MEETING_RECORDING.load(Ordering::SeqCst),
        MEETING_BUSY.load(Ordering::SeqCst),
        dictation,
    );
    if SHOWN.swap(code(status), Ordering::SeqCst) != code(status) {
        show_on_dock(app, status);
        show_on_taskbar(app, status);
    }
    if status == Status::Recording && !BLINKING.swap(true, Ordering::SeqCst) {
        let app = app.clone();
        std::thread::spawn(move || blink(app));
    }
}

/// Toggles the menu bar dot while something is being recorded.
fn blink(app: AppHandle) {
    let recording = || SHOWN.load(Ordering::SeqCst) == code(Status::Recording);
    loop {
        loop {
            DIM.store(false, Ordering::SeqCst);
            crate::tray::refresh_tray_icon(&app);
            std::thread::sleep(ON);
            if !recording() {
                break;
            }
            DIM.store(true, Ordering::SeqCst);
            crate::tray::refresh_tray_icon(&app);
            std::thread::sleep(OFF);
            if !recording() {
                break;
            }
        }
        DIM.store(false, Ordering::SeqCst);
        BLINKING.store(false, Ordering::SeqCst);
        crate::tray::refresh_tray_icon(&app);
        // A recording that started just as this one ended found the blink
        // still running and started none: keep blinking for it. When a new
        // blink was started instead, it has taken over.
        if !recording() || BLINKING.swap(true, Ordering::SeqCst) {
            break;
        }
    }
}

#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
fn picture(app: &AppHandle, name: &str) -> Option<std::path::PathBuf> {
    app.path()
        .resolve(
            format!("resources/seogi/{name}"),
            tauri::path::BaseDirectory::Resource,
        )
        .ok()
}

/// The Dock icon with a steady dot while busy; the app's own icon otherwise.
fn show_on_dock(app: &AppHandle, status: Status) {
    #[cfg(target_os = "macos")]
    {
        let path = match status {
            Status::Idle => None,
            Status::Recording => picture(app, "dock-rec.png"),
            Status::Busy => picture(app, "dock-busy.png"),
        };
        let _ = app.run_on_main_thread(move || {
            use objc2::{AllocAnyThread, MainThreadMarker};
            use objc2_app_kit::{NSApplication, NSImage};
            use objc2_foundation::NSString;
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let ns_app = NSApplication::sharedApplication(mtm);
            let image = path.and_then(|path| {
                let path = NSString::from_str(&path.to_string_lossy());
                NSImage::initWithContentsOfFile(NSImage::alloc(), &path)
            });
            // SAFETY: on the main thread; `None` puts the bundle's icon back.
            unsafe { ns_app.setApplicationIconImage(image.as_deref()) };
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, status);
    }
}

/// The taskbar button's overlay dot on Windows.
fn show_on_taskbar(app: &AppHandle, status: Status) {
    #[cfg(target_os = "windows")]
    {
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        let icon = match status {
            Status::Idle => None,
            Status::Recording => picture(app, "overlay-rec.png"),
            Status::Busy => picture(app, "overlay-busy.png"),
        }
        .and_then(|path| tauri::image::Image::from_path(path).ok());
        if let Err(e) = window.set_overlay_icon(icon) {
            log::warn!("The taskbar dot could not be shown: {e}");
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_wins_over_busy_and_nothing_is_red_when_idle() {
        assert_eq!(combine(false, false, TrayIconState::Idle), Status::Idle);
        assert_eq!(combine(true, true, TrayIconState::Idle), Status::Recording);
        assert_eq!(
            combine(false, true, TrayIconState::Recording),
            Status::Recording
        );
        assert_eq!(combine(false, true, TrayIconState::Idle), Status::Busy);
        assert_eq!(
            combine(false, false, TrayIconState::Transcribing),
            Status::Busy
        );
    }
}
