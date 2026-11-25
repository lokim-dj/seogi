// Derived from Handy (https://github.com/cjpais/Handy), Copyright (c) 2025 CJ Pais, MIT License. See THIRD_PARTY_NOTICES.md.

//! Launch-at-login (autostart) handling.
//!
//! All platforms apply the setting through tauri-plugin-autostart, except
//! macOS 13+ where the app registers itself as a login item via
//! `SMAppService`. The plugin's launch agent plist carries no app
//! association, so the System Settings Login Items pane attributes it to the
//! code-signing certificate's developer name instead of the app (#337).
//! `SMAppService` login items are attributed to the app bundle itself and
//! appear under "Open at Login" with the app's name and icon.

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

/// Apply the user's autostart preference using the best mechanism for the
/// current platform.
///
/// A failure is logged and returned. At launch the caller ignores it: the
/// preference is re-applied on every launch, so a transient failure heals and
/// must not block startup. The settings screen shows it.
pub fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    // A dev build is a bare executable in the build directory. Registering it
    // adds a login item named after the binary that points into `target/`, so
    // a dev build only ever clears its registration.
    let enabled = enabled && !cfg!(debug_assertions);

    #[cfg(target_os = "macos")]
    if macos::login_item_api_available() {
        macos::remove_plugin_launch_agent(app);
        return macos::set_login_item(enabled);
    }

    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|e| {
        log::warn!(
            "Failed to apply autostart setting (enabled={}): {}",
            enabled,
            e
        );
        e.to_string()
    })
}

/// Whether the system will open the app at login, as the system says, not
/// as the setting says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum LoginItem {
    On,
    Off,
    /// Registered, but switched off in System Settings › Login Items (or
    /// waiting for the user's approval there). Only the user can turn it on.
    NeedsApproval,
    /// The system could not be asked.
    Unknown,
}

/// The login item as the system has it now.
pub fn login_item(app: &AppHandle) -> LoginItem {
    #[cfg(target_os = "macos")]
    if macos::login_item_api_available() {
        return macos::login_item();
    }
    match app.autolaunch().is_enabled() {
        Ok(true) => LoginItem::On,
        Ok(false) => LoginItem::Off,
        Err(e) => {
            log::warn!("Could not read the autostart setting: {e}");
            LoginItem::Unknown
        }
    }
}

/// Opens the system's list of apps that open at login (macOS 13+; does
/// nothing elsewhere).
pub fn open_login_items() {
    #[cfg(target_os = "macos")]
    if macos::login_item_api_available() {
        // SAFETY: a class method without arguments.
        unsafe { objc2_service_management::SMAppService::openSystemSettingsLoginItems() };
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};

    use objc2::runtime::AnyClass;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    use tauri::{AppHandle, Manager};

    /// `SMAppService` requires macOS 13. The ServiceManagement framework is
    /// linked unconditionally (it has existed since 10.6), so looking up the
    /// class doubles as the OS version check: present exactly when the API is
    /// usable.
    pub fn login_item_api_available() -> bool {
        AnyClass::get(c"SMAppService").is_some()
    }

    /// Register or unregister the app as a login item, skipping the call when
    /// the service is already in the requested state (unregistering a
    /// never-registered service returns an error on every launch otherwise).
    pub fn set_login_item(enabled: bool) -> Result<(), String> {
        let service = unsafe { SMAppService::mainAppService() };
        let status = unsafe { service.status() };

        if enabled {
            if status == SMAppServiceStatus::Enabled {
                return Ok(());
            }
            match unsafe { service.registerAndReturnError() } {
                Ok(()) => {
                    log::info!("Registered login item via SMAppService");
                    Ok(())
                }
                // Fails in dev (no signed app bundle) and when the user has
                // switched the item off in System Settings, which apps are
                // not allowed to override.
                Err(e) => {
                    log::warn!("Failed to register login item: {}", e);
                    Err(e.to_string())
                }
            }
        } else {
            if status == SMAppServiceStatus::NotRegistered || status == SMAppServiceStatus::NotFound
            {
                return Ok(());
            }
            match unsafe { service.unregisterAndReturnError() } {
                Ok(()) => {
                    log::info!("Unregistered login item via SMAppService");
                    Ok(())
                }
                Err(e) => {
                    log::warn!("Failed to unregister login item: {}", e);
                    Err(e.to_string())
                }
            }
        }
    }

    /// The login item as `SMAppService` reports it.
    pub fn login_item() -> super::LoginItem {
        let service = unsafe { SMAppService::mainAppService() };
        let status = unsafe { service.status() };
        if status == SMAppServiceStatus::Enabled {
            super::LoginItem::On
        } else if status == SMAppServiceStatus::RequiresApproval {
            super::LoginItem::NeedsApproval
        } else {
            super::LoginItem::Off
        }
    }

    /// Remove the launch agent plist that tauri-plugin-autostart (via the
    /// auto-launch crate) wrote on older versions, so login doesn't start the
    /// app twice after migrating to `SMAppService`. Runs on every launch;
    /// missing file is the normal case.
    pub fn remove_plugin_launch_agent(app: &AppHandle) {
        let Ok(home) = app.path().home_dir() else {
            return;
        };
        remove_launch_agent_file(&plugin_launch_agent_path(&home, &app.package_info().name));
    }

    /// Path of the plist the auto-launch crate writes:
    /// `~/Library/LaunchAgents/{app name}.plist`.
    fn plugin_launch_agent_path(home: &Path, app_name: &str) -> PathBuf {
        home.join("Library")
            .join("LaunchAgents")
            .join(format!("{}.plist", app_name))
    }

    fn remove_launch_agent_file(path: &Path) {
        match std::fs::remove_file(path) {
            Ok(()) => log::info!("Removed legacy autostart launch agent {:?}", path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("Failed to remove legacy launch agent {:?}: {}", path, e),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Validates the assumption `login_item_api_available` rests on: the
        /// ServiceManagement framework is linked into the binary, so the
        /// class lookup finds `SMAppService` whenever the host is macOS 13+
        /// (which anything able to build this crate is).
        #[test]
        fn sm_app_service_class_resolves() {
            assert!(login_item_api_available());
        }

        #[test]
        fn launch_agent_path_matches_auto_launch_crate() {
            let path = plugin_launch_agent_path(Path::new("/Users/someone"), "Handy");
            assert_eq!(
                path,
                Path::new("/Users/someone/Library/LaunchAgents/Handy.plist")
            );
        }

        #[test]
        fn removes_existing_launch_agent() {
            let dir = tempfile::tempdir().unwrap();
            let plist = dir.path().join("Handy.plist");
            std::fs::write(&plist, "<plist/>").unwrap();

            remove_launch_agent_file(&plist);
            assert!(!plist.exists());
        }

        #[test]
        fn missing_launch_agent_is_a_no_op() {
            let dir = tempfile::tempdir().unwrap();
            remove_launch_agent_file(&dir.path().join("Handy.plist"));
        }
    }
}
