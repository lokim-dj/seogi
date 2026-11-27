//! The floating microphone key: a small round key above every other app
//! that starts and stops a dictation.
//!
//! Like the banner it never takes the keyboard: on macOS it is a panel that
//! cannot become the key window, so a press leaves the cursor where it was
//! and the words go there. The user drags it where they like and it stays
//! there. What it looks like is the page inside it (`src/seogi/mic/`).
//!
//! While it is dragged a second small window shows a × at the bottom centre
//! of that screen; letting go of the key over it hides the key. The drag is
//! the system's own (the page asks for it with [`drag`]), so the page sees
//! no pointer while it lasts: the end is found by asking the system whether
//! the button is still down, and the key's place is compared with the ×.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl};

use super::app::Secretary;
use super::dictation::Spot;

pub const LABEL: &str = "seogi_mic";
const PAGE: &str = "src/seogi/mic/index.html";
/// The × the key is dropped on to hide it, shown only while it is dragged.
const TARGET_LABEL: &str = "seogi_mic_target";
const TARGET_PAGE: &str = "src/seogi/mic/target.html";
/// The ×'s window (logical px) and how far it sits above the screen's foot.
const TARGET_SIZE: f64 = 72.0;
const TARGET_FROM_BOTTOM: f64 = 36.0;
/// How far around the ×'s window the key's centre still counts as on it.
const TARGET_SLACK: f64 = 8.0;
/// The window: the key (34 px) and room around it for the listening ring.
const SIZE: f64 = 48.0;
/// Where the key starts: this far in from the bottom right corner.
const FROM_CORNER: (f64, f64) = (72.0, 132.0);

/// The key is on the screen.
static SHOWN: AtomicBool = AtomicBool::new(false);
/// Bumped on each move, so only the last of a drag is saved.
static MOVES: AtomicU64 = AtomicU64::new(0);
/// A drag is going on (so a second ask for one is ignored).
static DRAGGING: AtomicBool = AtomicBool::new(false);

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

    panel!(SeogiMicTargetPanel {
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

#[cfg(target_os = "macos")]
fn create_target(app: &AppHandle) -> Result<()> {
    PanelBuilder::<_, SeogiMicTargetPanel>::new(app, TARGET_LABEL)
        .url(WebviewUrl::App(TARGET_PAGE.into()))
        .title("Seogi")
        .level(PanelLevel::Status)
        .size(tauri::Size::Logical(tauri::LogicalSize {
            width: TARGET_SIZE,
            height: TARGET_SIZE,
        }))
        .has_shadow(false)
        .transparent(true)
        .no_activate(true)
        .corner_radius(0.0)
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .with_window(|w| w.decorations(false).transparent(true))
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
fn create_target(app: &AppHandle) -> Result<()> {
    let mut builder =
        tauri::WebviewWindowBuilder::new(app, TARGET_LABEL, WebviewUrl::App(TARGET_PAGE.into()))
            .title("Seogi")
            .inner_size(TARGET_SIZE, TARGET_SIZE)
            .resizable(false)
            .shadow(false)
            .maximizable(false)
            .minimizable(false)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .transparent(true)
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
        if handle.get_webview_window(TARGET_LABEL).is_none() {
            if let Err(e) = create_target(&handle) {
                log::warn!("The microphone key's × could not be made: {e:#}");
            } else if let Some(target) = handle.get_webview_window(TARGET_LABEL) {
                // It is only something to aim at: clicks go through it.
                let _ = target.set_ignore_cursor_events(true);
            }
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
        if secretary.config().dictation.button_at == Some(spot) {
            return;
        }
        if let Err(e) = secretary.update_dictation(|dictation| dictation.button_at = Some(spot)) {
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
            let app = app.clone();
            std::thread::spawn(move || {
                if let Err(e) = super::dictation::show_key(&app, false, None) {
                    log::warn!("Could not hide the microphone key: {e}");
                }
            });
        }
        "mic_settings" => {
            crate::show_main_window(app);
            let _ = app.emit("seogi-open-dictation", ());
        }
        _ => {}
    }
}

// ----- dragging, and the × to drop it on ------------------------------------

/// A rectangle on the screen in physical pixels: left, top, right, bottom.
type Rect = (f64, f64, f64, f64);

/// Whether a point is inside `rect` grown by `slack` on every side.
fn inside(rect: Rect, slack: f64, point: (f64, f64)) -> bool {
    point.0 >= rect.0 - slack
        && point.0 <= rect.2 + slack
        && point.1 >= rect.1 - slack
        && point.1 <= rect.3 + slack
}

/// Where the × goes on a screen's work area (physical px, with its scale):
/// at the bottom centre, a little above the edge.
fn target_rect(area: Rect, scale: f64) -> Rect {
    let size = TARGET_SIZE * scale;
    let left = area.0 + ((area.2 - area.0) - size) / 2.0;
    let top = area.3 - size - TARGET_FROM_BOTTOM * scale;
    (left, top, left + size, top + size)
}

/// Whether the main mouse button is held down now, as far as the system
/// says; `None` where there is no way to ask.
fn button_down() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        // Bit 0 is the primary button, whichever finger the user set it to.
        Some(objc2_app_kit::NSEvent::pressedMouseButtons() & 1 != 0)
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON,
        };
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
        // The key codes name the physical buttons; with them swapped the
        // primary one is the right.
        let primary = if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 {
            VK_RBUTTON
        } else {
            VK_LBUTTON
        };
        let state = unsafe { GetAsyncKeyState(i32::from(primary.0)) };
        Some((state as u16) & 0x8000 != 0)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// The key's window as a rectangle, in physical px.
fn rect_of(window: &tauri::WebviewWindow) -> Option<Rect> {
    let at = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    let (x, y) = (f64::from(at.x), f64::from(at.y));
    Some((x, y, x + f64::from(size.width), y + f64::from(size.height)))
}

/// Lights the × while the key is over it.
fn light_target(app: &AppHandle, over: bool) {
    if let Some(target) = app.get_webview_window(TARGET_LABEL) {
        let _ = target.eval(format!(
            "document.documentElement.toggleAttribute('data-over', {over})"
        ));
    }
}

fn hide_target(app: &AppHandle) {
    light_target(app, false);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(target) = handle.get_webview_window(TARGET_LABEL) {
            let _ = target.hide();
        }
    });
}

/// The key is being dragged: the system moves its window, the × shows at the
/// bottom centre of the screen the key is on until the button is let go, and
/// a drop on the × hides the key (it comes back where the drag began).
/// Called from a thread that may wait for the main one.
pub fn drag(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    if DRAGGING.swap(true, Ordering::SeqCst) {
        return;
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    let from = window.outer_position().ok().map(|at| Spot {
        x: (f64::from(at.x) / scale).round() as i32,
        y: (f64::from(at.y) / scale).round() as i32,
    });
    let target = window.current_monitor().ok().flatten().map(|monitor| {
        let area = monitor.work_area();
        let (x, y) = (f64::from(area.position.x), f64::from(area.position.y));
        target_rect(
            (
                x,
                y,
                x + f64::from(area.size.width),
                y + f64::from(area.size.height),
            ),
            monitor.scale_factor(),
        )
    });
    if let Some(rect) = target {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let Some(target) = handle.get_webview_window(TARGET_LABEL) else {
                return;
            };
            let _ = target.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                x: rect.0.round() as i32,
                y: rect.1.round() as i32,
            }));
            let _ = target.show();
        });
    }
    // The × is asked for first: while the system drags a window it may keep
    // the main thread busy until the button is let go.
    if let Err(e) = window.start_dragging() {
        log::warn!("The microphone key could not be dragged: {e}");
        hide_target(app);
        DRAGGING.store(false, Ordering::SeqCst);
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        let mut over = false;
        let mut last_rect = None;
        let mut still_since = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(30));
            let rect = rect_of(&window);
            if rect != last_rect {
                last_rect = rect;
                still_since = Instant::now();
            }
            let now_over = match (target, rect) {
                (Some(t), Some(r)) => inside(
                    t,
                    TARGET_SLACK * scale,
                    ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0),
                ),
                _ => false,
            };
            if now_over != over {
                over = now_over;
                light_target(&app, over);
            }
            let let_go = match button_down() {
                Some(down) => !down && started.elapsed() > Duration::from_millis(60),
                // Nothing to ask: the drag is taken as over once the key has
                // stood still for a moment.
                None => still_since.elapsed() > Duration::from_millis(500),
            };
            if let_go || started.elapsed() > Duration::from_secs(120) {
                break;
            }
        }
        // The window's last move may land just after the button is let go.
        std::thread::sleep(Duration::from_millis(60));
        let dropped = match (target, rect_of(&window)) {
            (Some(t), Some(r)) => inside(
                t,
                TARGET_SLACK * scale,
                ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0),
            ),
            _ => false,
        };
        hide_target(&app);
        DRAGGING.store(false, Ordering::SeqCst);
        if dropped {
            dropped_on_target(&app, &window, from);
        }
    });
}

/// The key was let go on the ×: it is hidden (saved, as the settings' switch
/// would), and put back where the drag began so it shows there next time.
fn dropped_on_target(app: &AppHandle, window: &tauri::WebviewWindow, from: Option<Spot>) {
    // Whatever place the drag was about to save is not where it should come
    // back.
    MOVES.fetch_add(1, Ordering::SeqCst);
    if let Err(e) = super::dictation::show_key(app, false, from) {
        log::warn!("Could not hide the microphone key: {e}");
        return;
    }
    if let Some(Spot { x, y }) = from {
        let window = window.clone();
        let _ = app.run_on_main_thread(move || {
            let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition {
                x: f64::from(x),
                y: f64::from(y),
            }));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_sits_at_the_bottom_centre_of_the_work_area() {
        // A 1440 x 875 work area at 2x, starting below a 25 pt menu bar.
        let area = (0.0, 50.0, 2880.0, 1800.0);
        let (left, top, right, bottom) = target_rect(area, 2.0);
        assert_eq!(right - left, TARGET_SIZE * 2.0);
        assert_eq!((left + right) / 2.0, 1440.0);
        assert_eq!(bottom, 1800.0 - TARGET_FROM_BOTTOM * 2.0);
        assert!(top > 50.0);
    }

    #[test]
    fn the_key_counts_as_on_the_target_only_near_it() {
        let target = (100.0, 100.0, 172.0, 172.0);
        assert!(inside(target, 8.0, (136.0, 136.0)));
        assert!(inside(target, 8.0, (95.0, 136.0)));
        assert!(!inside(target, 8.0, (90.0, 136.0)));
        assert!(!inside(target, 8.0, (136.0, 300.0)));
    }
}
