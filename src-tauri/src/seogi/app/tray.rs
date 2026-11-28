//! The banner and the tray menu.

use super::*;

impl Secretary {
    // ----- the banner -------------------------------------------------------

    /// Shows the banner window with `banner`. If the window cannot be made,
    /// the system's own notification says it instead.
    pub(super) fn show_banner(&self, banner: Banner) {
        if self.quiet {
            return;
        }
        let (title, body, starting) = match &banner {
            Banner::Starting { event } => (
                format!(
                    "{} {}",
                    say(&self.app, "곧 시작합니다:", "Starting soon:"),
                    event.title
                ),
                String::new(),
                true,
            ),
            Banner::Ready {
                title,
                actions,
                to_check,
                ..
            } => (
                say(&self.app, "회의록이 준비됐습니다", "The minutes are ready").to_string(),
                if is_korean(&self.app) {
                    format!("{title} · 할 일 {actions}개 · 확인 필요 {to_check}곳")
                } else {
                    format!("{title} · {actions} tasks · {to_check} to check")
                },
                false,
            ),
        };
        *lock(&self.banner) = Some(banner);
        if let Err(e) = super::banner::show(&self.app, starting) {
            warn!("The banner could not be shown ({e:#}); using a notification");
            *lock(&self.banner) = None;
            self.notify(&title, &body);
            return;
        }
        let _ = BannerChanged {}.emit(&self.app);
    }

    pub fn banner(&self) -> Option<Banner> {
        lock(&self.banner).clone()
    }

    /// What a button of the banner does: `record`, `mute`, `open`, or
    /// anything else to put it away.
    pub fn banner_action(self: &Arc<Self>, action: &str) -> Result<()> {
        let banner = lock(&self.banner).take();
        super::banner::hide(&self.app);
        let _ = BannerChanged {}.emit(&self.app);
        match (action, banner) {
            ("record", Some(Banner::Starting { event })) => self
                .start_recording(None, Some((event.uid, event.start_ms)))
                .map(|_| ()),
            ("mute", Some(Banner::Starting { event })) => self.mute_event(&event.uid, true),
            ("open", Some(Banner::Ready { id, .. })) => {
                crate::show_main_window(&self.app);
                let _ = OpenMeeting { id }.emit(&self.app);
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// The tray menu: recording first, dictation second, the window last.
pub fn tray_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
    let secretary = Secretary::of(app);
    let recording = secretary.as_ref().is_some_and(|s| s.is_recording());
    let dictating = secretary
        .as_ref()
        .is_some_and(|s| s.config().dictation.enabled);
    let item = |id: &str, korean: &str, english: &str| {
        MenuItem::with_id(app, id, say(app, korean, english), true, None::<&str>)
    };
    let menu = Menu::new(app)?;
    if recording {
        menu.append(&item("seogi_stop", "녹음 끝내기", "Stop recording")?)?;
        menu.append(&item("seogi_mark", "중요 표시", "Mark this moment")?)?;
    } else {
        menu.append(&item("seogi_record", "녹음 시작", "Start recording")?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&CheckMenuItem::with_id(
        app,
        "seogi_dictation",
        say(app, "말로 입력 켜 두기", "Keep voice typing on"),
        // Until the secretary is ready a click could not be saved.
        secretary.is_some(),
        dictating,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&item("settings", "서기 열기", "Open Seogi")?)?;
    menu.append(&item("quit", "끝내기", "Quit")?)?;
    Ok(menu)
}

/// Does what a tray menu item asks. Returns whether the item was Seogi's.
pub fn tray_action(app: &AppHandle, id: &str) -> bool {
    let Some(secretary) = Secretary::of(app) else {
        return false;
    };
    match id {
        "seogi_record" | "seogi_stop" => {
            std::thread::spawn(move || secretary.toggle_recording());
            true
        }
        "seogi_mark" => {
            let _ = secretary.mark();
            true
        }
        "seogi_dictation" => {
            // The same path as the settings' switch, so the shortcut, the
            // tray and an open settings screen all follow.
            let app = app.clone();
            std::thread::spawn(move || {
                let mut wanted = super::dictation::read(&app, &secretary.config().dictation);
                wanted.enabled = !wanted.enabled;
                if let Err(e) = super::dictation::write(&app, wanted) {
                    warn!("Could not turn dictation on or off from the tray: {e}");
                    // The item ticked itself; show the state as it is.
                    crate::tray::rebuild_tray_menu(&app);
                }
            });
            true
        }
        _ => false,
    }
}
