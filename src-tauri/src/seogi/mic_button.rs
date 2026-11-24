//! The floating microphone key: a small round key above every other app
//! that starts and stops a dictation.
//!
//! Like the banner it never takes the keyboard: on macOS it is a panel that
//! cannot become the key window, so a press leaves the cursor where it was
//! and the words go there. The user drags it where they like and it stays
//! there. What it looks like is the page inside it (`src/seogi/mic/`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{anyhow, Result};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl};

use super::app::Secretary;
use super::dictation::Spot;

pub const LABEL: &str = "seogi_mic";
const PAGE: &str = "src/seogi/mic/index.html";
/// The window: the key (34 px) and room around it for the listening ring.
const SIZE: f64 = 48.0;
/// Where the key starts: this far in from the bottom right corner.
const FROM_CORNER: (f64, f64) = (72.0, 132.0);

/// The key is on the screen.
static SHOWN: AtomicBool = AtomicBool::new(false);
/// Bumped on each move, so only the last of a drag is saved.
static MOVES: AtomicU64 = AtomicU64::new(0);

pub fn is_shown() -> bool {
    SHOWN.load(Ordering::Relaxed)
}

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(SeogiMicPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

/// Where the key goes: where the user left it, if that is still on a
/// screen, else near the bottom right corner of the main screen.
fn place(app: &AppHandle) -> Option<(f64, f64)> {
    let saved = Secretary::of(app).and_then(|s| s.config().dictation.button_at);
    let monitors = app.available_monitors().ok().unwrap_or_default();
    if let Some(Spot { x, y }) = saved {
        let (x, y) = (f64::from(x), f64::from(y));
        let on_a_screen = monitors.iter().any(|monitor| {
            let scale = monitor.scale_factor();
            let area = monitor.work_area();
            let left = area.position.x as f64 / scale;
            let top = area.position.y as f64 / scale;
            let right = left + area.size.width as f64 / scale;
            let bottom = top + area.size.height as f64 / scale;
            x >= left && x + SIZE <= right && y >= top && y + SIZE <= bottom
        });
        if on_a_screen {
            return Some((x, y));
        }
    }
    let monitor = app.primary_monitor().ok().flatten()?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let left = area.position.x as f64 / scale;
    let top = area.position.y as f64 / scale;
    let width = area.size.width as f64 / scale;
    let height = area.size.height as f64 / scale;
    Some((left + width - FROM_CORNER.0, top + height - FROM_CORNER.1))
}

#[cfg(target_os = "macos")]
fn create(app: &AppHandle, at: (f64, f64)) -> Result<()> {
    PanelBuilder::<_, SeogiMicPanel>::new(app, LABEL)
        .url(WebviewUrl::App(PAGE.into()))
        .title("Seogi")
        .position(tauri::Position::Logical(tauri::LogicalPosition {
            x: at.0,
            y: at.1,
        }))
        .level(PanelLevel::Status)
        .size(tauri::Size::Logical(tauri::LogicalSize {
            width: SIZE,
            height: SIZE,
        }))
        .has_shadow(false)
        .transparent(true)
        .no_activate(true)
        .corner_radius(0.0)
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .with_window(|w| {
            w.decorations(false)
                .transparent(true)
                .accept_first_mouse(true)
        })
        .collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary(),
        )
        .build()
        .map(|panel| panel.hide())
        .map_err(|e| anyhow!("{e}"))
}

#[cfg(not(target_os = "macos"))]
fn create(app: &AppHandle, at: (f64, f64)) -> Result<()> {
    let mut builder = tauri::WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(PAGE.into()))
        .title("Seogi")
        .inner_size(SIZE, SIZE)
        .position(at.0, at.1)
        .resizable(false)
        .shadow(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .accept_first_mouse(true)
        .focusable(false)
        .focused(false)
        .visible(false);
    if let Some(data_dir) = crate::portable::data_dir() {
        builder = builder.data_directory(data_dir.join("webview"));
    }
    builder.build().map(|_| ()).map_err(|e| anyhow!("{e}"))
}

/// Makes the key's window, hidden, when the app starts: a window has to
/// exist for a moment before it can be shown.
pub fn prepare(app: &AppHandle) {
    let Some(at) = place(app) else {
        return;
    };
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if handle.get_webview_window(LABEL).is_some() {
            return;
        }
        if let Err(e) = create(&handle, at) {
            log::warn!("The microphone key could not be made: {e:#}");
            return;
        }
        if let Some(window) = handle.get_webview_window(LABEL) {
            let app = handle.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Moved(position) = event {
                    moved(&app, *position);
                }
            });
            let app = handle.clone();
            window.on_menu_event(move |_, event| menu_chosen(&app, event.id().as_ref()));
        }
    });
}

/// Shows or hides the key.
pub fn set_shown(app: &AppHandle, shown: bool) {
    if SHOWN.swap(shown, Ordering::SeqCst) == shown {
        return;
    }
    if shown && app.get_webview_window(LABEL).is_none() {
        prepare(app);
        std::thread::sleep(Duration::from_millis(800));
    }
    let at = if shown { place(app) } else { None };
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window(LABEL) else {
            return;
        };
        if shown {
            if let Some((x, y)) = at {
                let _ =
                    window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
            }
            if let Err(e) = window.show() {
                log::warn!("The microphone key could not be shown: {e}");
            }
        } else {
            let _ = window.hide();
        }
    });
}

/// Remembers where the key was dragged, once the drag is over.
fn moved(app: &AppHandle, position: tauri::PhysicalPosition<i32>) {
    if !is_shown() {
        return;
    }
    let scale = app
        .get_webview_window(LABEL)
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or(1.0);
    let spot = Spot {
        x: (f64::from(position.x) / scale).round() as i32,
        y: (f64::from(position.y) / scale).round() as i32,
    };
    let generation = MOVES.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        if MOVES.load(Ordering::SeqCst) != generation {
            return;
        }
        let Some(secretary) = Secretary::of(&app) else {
            return;
        };
        let mut dictation = secretary.config().dictation;
        if dictation.button_at == Some(spot) {
            return;
        }
        dictation.button_at = Some(spot);
        if let Err(e) = secretary.set_dictation(dictation) {
            log::warn!("Where the microphone key is could not be saved: {e:#}");
        }
    });
}

/// The key's own menu, on a right click: hide it, or open the settings.
pub fn show_menu(app: &AppHandle, hide: &str, settings: &str) -> Result<()> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| anyhow!("there is no microphone key"))?;
    let menu = MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id("mic_hide", hide).build(app)?)
        .item(&MenuItemBuilder::with_id("mic_settings", settings).build(app)?)
        .build()?;
    window.popup_menu(&menu)?;
    Ok(())
}

fn menu_chosen(app: &AppHandle, id: &str) {
    match id {
        "mic_hide" => {
            if let Some(secretary) = Secretary::of(app) {
                let mut dictation = secretary.config().dictation;
                dictation.button = false;
                if let Err(e) = secretary.set_dictation(dictation) {
                    log::warn!("Could not hide the microphone key: {e:#}");
                }
            }
            set_shown(app, false);
        }
        "mic_settings" => {
            crate::show_main_window(app);
            let _ = app.emit("seogi-open-dictation", ());
        }
        _ => {}
    }
}
