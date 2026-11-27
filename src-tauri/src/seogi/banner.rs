//! The banner: a small window above everything else, at the top of the
//! screen, that says a meeting is starting or that minutes are ready.
//!
//! It never takes the keyboard away from what the user is doing: on macOS
//! it is a panel that cannot become the key window, elsewhere a window that
//! is shown without being activated. What it shows is asked for by the page
//! inside it (`seogi_banner`); this module only makes, places, shows and
//! hides the window.

use anyhow::{anyhow, Result};
use tauri::{AppHandle, Manager, WebviewUrl};

pub const LABEL: &str = "seogi_banner";
const PAGE: &str = "src/seogi/banner/index.html";

/// The window is larger than what is drawn in it, to leave room for the
/// shadow: (width, height) of a meeting starting, and of minutes ready.
/// What is drawn is sized like a system notification; the ready window
/// keeps room for its middle line to wrap once.
const STARTING_SIZE: (f64, f64) = (584.0, 76.0);
const READY_SIZE: (f64, f64) = (312.0, 112.0);
/// Space kept from the screen's edges.
const EDGE: f64 = 4.0;

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(SeogiBannerPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

/// Where the banner goes: the top middle of the main screen when a meeting
/// is starting, its top right corner when minutes are ready.
fn place(app: &AppHandle, starting: bool) -> Option<((f64, f64), (f64, f64))> {
    let monitor = app.primary_monitor().ok().flatten()?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let left = area.position.x as f64 / scale;
    let top = area.position.y as f64 / scale;
    let width = area.size.width as f64 / scale;
    let size = if starting { STARTING_SIZE } else { READY_SIZE };
    let x = if starting {
        left + (width - size.0) / 2.0
    } else {
        left + width - size.0 - EDGE
    };
    Some((size, (x, top + EDGE)))
}

#[cfg(target_os = "macos")]
fn create(app: &AppHandle, size: (f64, f64), at: (f64, f64)) -> Result<()> {
    PanelBuilder::<_, SeogiBannerPanel>::new(app, LABEL)
        .url(WebviewUrl::App(PAGE.into()))
        .title("Seogi")
        .position(tauri::Position::Logical(tauri::LogicalPosition {
            x: at.0,
            y: at.1,
        }))
        .level(PanelLevel::Status)
        .size(tauri::Size::Logical(tauri::LogicalSize {
            width: size.0,
            height: size.1,
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
fn create(app: &AppHandle, size: (f64, f64), at: (f64, f64)) -> Result<()> {
    let mut builder = tauri::WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(PAGE.into()))
        .title("Seogi")
        .inner_size(size.0, size.1)
        .position(at.0, at.1)
        .resizable(false)
        .shadow(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .focused(false)
        .visible(false);
    if let Some(data_dir) = crate::portable::data_dir() {
        builder = builder.data_directory(data_dir.join("webview"));
    }
    builder.build().map(|_| ()).map_err(|e| anyhow!("{e}"))
}

/// Makes the banner window, hidden, when the app starts. A window made at
/// the moment it is first needed does not appear: it has to exist for a
/// while before it can be shown, so it is made ahead of time.
pub fn prepare(app: &AppHandle) {
    let Some((size, at)) = place(app, true) else {
        return;
    };
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if handle.get_webview_window(LABEL).is_none() {
            if let Err(e) = create(&handle, size, at) {
                log::warn!("The banner window could not be made: {e:#}");
            }
        }
    });
}

/// Shows the banner window. Returns once the request has been handed to
/// the main thread, where windows live.
pub fn show(app: &AppHandle, starting: bool) -> Result<()> {
    let (size, at) = place(app, starting).ok_or_else(|| anyhow!("there is no screen"))?;
    if app.get_webview_window(LABEL).is_none() {
        // Not made at start for some reason: make it now and give it a
        // moment before showing it.
        prepare(app);
        std::thread::sleep(std::time::Duration::from_millis(800));
    }
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window(LABEL) else {
            log::warn!("There is no banner window to show");
            return;
        };
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: size.0,
            height: size.1,
        }));
        let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition {
            x: at.0,
            y: at.1,
        }));
        match window.show() {
            Ok(()) => log::info!(
                "Banner shown at ({:.0}, {:.0}), {:.0} x {:.0}",
                at.0,
                at.1,
                size.0,
                size.1
            ),
            Err(e) => log::warn!("The banner window could not be shown: {e}"),
        }
    })
    .map_err(|e| anyhow!("{e}"))
}

pub fn hide(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(LABEL) {
            let _ = window.hide();
        }
    });
}
