//! The menu bar on macOS: the usual menus a Mac app has, so the usual keys
//! work (⌘Z, ⌘C, ⌘V and ⌘A in a text field, ⌘, for the settings, ⌘W, ⌘M,
//! ⌘Q) and moving about the app has keys too (⌘[ back, ⌘] forward, ⌘1 the
//! meetings, ⌘F finding). The edit items are the system's own, so a text
//! field gets them as in any other app. The items that move about the app are
//! passed to the window as `seogi-menu` with their id.
//!
//! Windows has no menu bar: its keys (Ctrl+Z, Ctrl+C, Alt+←, Ctrl+, …) are
//! the web view's own or handled by the window itself.

use tauri::menu::{
    AboutMetadata, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::{AppHandle, Emitter};

/// The event the window gets when an item that moves about the app is
/// chosen; its payload is the item's id (`back`, `forward`, `home`,
/// `settings`, `find`, `dictation`).
pub const EVENT: &str = "seogi-menu";

/// Which menu items the window acts on.
const WINDOW_ITEMS: &[&str] = &["back", "forward", "home", "settings", "find", "dictation"];

fn korean() -> bool {
    sys_locale::get_locale().is_some_and(|locale| locale.starts_with("ko"))
}

/// The Korean words or the English ones, as the system's language is.
fn say(korean_words: &'static str, english: &'static str) -> &'static str {
    if korean() {
        korean_words
    } else {
        english
    }
}

/// The whole menu bar.
pub fn build(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let item = |id: &str, text: &str, keys: &str| {
        MenuItemBuilder::with_id(id, text)
            .accelerator(keys)
            .build(app)
    };
    let name = say("서기", "Seogi");
    let about = AboutMetadata {
        name: Some(name.to_string()),
        version: Some(app.package_info().version.to_string()),
        website: Some(crate::brand::REPO_URL.to_string()),
        ..Default::default()
    };

    let seogi = SubmenuBuilder::new(app, name)
        .item(&PredefinedMenuItem::about(
            app,
            Some(say("서기에 관하여", "About Seogi")),
            Some(about),
        )?)
        .separator()
        .item(&item("settings", say("설정…", "Settings…"), "CmdOrCtrl+,")?)
        .separator()
        .item(&PredefinedMenuItem::services(
            app,
            Some(say("서비스", "Services")),
        )?)
        .separator()
        .item(&PredefinedMenuItem::hide(
            app,
            Some(say("서기 가리기", "Hide Seogi")),
        )?)
        .item(&PredefinedMenuItem::hide_others(
            app,
            Some(say("기타 가리기", "Hide Others")),
        )?)
        .item(&PredefinedMenuItem::show_all(
            app,
            Some(say("모두 보기", "Show All")),
        )?)
        .separator()
        .item(&PredefinedMenuItem::quit(
            app,
            Some(say("서기 종료", "Quit Seogi")),
        )?)
        .build()?;

    let edit = SubmenuBuilder::new(app, say("편집", "Edit"))
        .item(&PredefinedMenuItem::undo(
            app,
            Some(say("실행 취소", "Undo")),
        )?)
        .item(&PredefinedMenuItem::redo(
            app,
            Some(say("실행 복귀", "Redo")),
        )?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, Some(say("오려두기", "Cut")))?)
        .item(&PredefinedMenuItem::copy(
            app,
            Some(say("복사하기", "Copy")),
        )?)
        .item(&PredefinedMenuItem::paste(
            app,
            Some(say("붙여넣기", "Paste")),
        )?)
        .item(&PredefinedMenuItem::select_all(
            app,
            Some(say("전체 선택", "Select All")),
        )?)
        .separator()
        .item(&item("find", say("찾기", "Find"), "CmdOrCtrl+F")?)
        .build()?;

    let view = SubmenuBuilder::new(app, say("보기", "View"))
        .item(&PredefinedMenuItem::fullscreen(
            app,
            Some(say("전체 화면 시작", "Enter Full Screen")),
        )?)
        .build()?;

    let go = SubmenuBuilder::new(app, say("이동", "Go"))
        .item(&item("back", say("뒤로", "Back"), "CmdOrCtrl+[")?)
        .item(&item("forward", say("앞으로", "Forward"), "CmdOrCtrl+]")?)
        .separator()
        .item(&item("home", say("회의함", "Meetings"), "CmdOrCtrl+1")?)
        .item(&item(
            "dictation",
            say("받아쓰기 설정", "Dictation Settings"),
            "CmdOrCtrl+2",
        )?)
        .build()?;

    let window = SubmenuBuilder::new(app, say("윈도우", "Window"))
        .item(&PredefinedMenuItem::minimize(
            app,
            Some(say("최소화", "Minimize")),
        )?)
        .item(&PredefinedMenuItem::maximize(
            app,
            Some(say("확대/축소", "Zoom")),
        )?)
        .separator()
        // Closing hides the window; Seogi stays in the Dock.
        .item(&PredefinedMenuItem::close_window(
            app,
            Some(say("윈도우 닫기", "Close Window")),
        )?)
        .build()?;

    let help = SubmenuBuilder::new(app, say("도움말", "Help"))
        .item(&MenuItemBuilder::with_id("help", say("서기 도움말", "Seogi Help")).build(app)?)
        .item(
            &MenuItemBuilder::with_id("report", say("문제 알리기", "Report a Problem"))
                .build(app)?,
        )
        .build()?;

    MenuBuilder::new(app)
        .items(&[&seogi, &edit, &view, &go, &window, &help])
        .build()
}

/// Does what a menu item asks, for the items that are Seogi's own.
pub fn chosen(app: &AppHandle, id: &str) {
    if WINDOW_ITEMS.contains(&id) {
        crate::show_main_window(app);
        let _ = app.emit(EVENT, id);
        return;
    }
    let url = match id {
        "help" => crate::brand::REPO_URL.to_string(),
        "report" => format!("{}/issues", crate::brand::REPO_URL),
        _ => return,
    };
    use tauri_plugin_opener::OpenerExt;
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        log::warn!("Could not open the help page: {e}");
    }
}
