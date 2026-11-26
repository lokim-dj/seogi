//! Seogi's name and links, in one place.

/// The name people see: window title, tray, notifications.
pub const APP_NAME: &str = "서기";
/// ASCII form for places that can't carry Hangul (HTTP headers, file names).
pub const APP_NAME_ASCII: &str = "Seogi";
pub const REPO_URL: &str = "https://github.com/lokim-dj/seogi";
pub const USER_AGENT: &str = "Seogi/0.1 (+https://github.com/lokim-dj/seogi)";

/// The installed app's identifier (`tauri.conf.json`). Its data folder,
/// keychain entries and macOS permissions are filed under it; changing it
/// would make every installed copy start over.
pub const RELEASE_IDENTIFIER: &str = "io.github.lokimdj.seogi";
/// A development build's identifier (`tauri.dev.conf.json`), so a debug build
/// never reads or changes the installed app's data, keys or permissions, and
/// can run while the installed app is open.
pub const DEV_IDENTIFIER: &str = "io.github.lokimdj.seogi.dev";

/// The identifier this build files its data, keys and permissions under.
pub const fn identifier() -> &'static str {
    if cfg!(debug_assertions) {
        DEV_IDENTIFIER
    } else {
        RELEASE_IDENTIFIER
    }
}

/// Seogi publishes no signed update feed yet. While this is false the updater
/// never runs.
pub const SELF_UPDATE_ENABLED: bool = false;

#[cfg(test)]
mod tests {
    use super::*;

    fn identifier_in(config: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(config);
        let text = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json["identifier"].as_str().unwrap().to_string()
    }

    #[test]
    fn identifiers_agree_with_the_configs() {
        assert_eq!(identifier_in("tauri.conf.json"), RELEASE_IDENTIFIER);
        assert_eq!(identifier_in("tauri.dev.conf.json"), DEV_IDENTIFIER);
        assert_ne!(DEV_IDENTIFIER, RELEASE_IDENTIFIER);
    }

    #[test]
    fn a_test_build_uses_the_development_identifier() {
        // Tests are debug builds.
        assert_eq!(identifier(), DEV_IDENTIFIER);
    }
}
