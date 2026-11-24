//! Seogi's name and links, in one place.

/// The name people see: window title, tray, notifications.
pub const APP_NAME: &str = "서기";
/// ASCII form for places that can't carry Hangul (HTTP headers, file names).
pub const APP_NAME_ASCII: &str = "Seogi";
pub const REPO_URL: &str = "https://github.com/lokim-dj/seogi";
pub const USER_AGENT: &str = "Seogi/0.1 (+https://github.com/lokim-dj/seogi)";

/// Seogi publishes no signed update feed yet. While this is false the updater
/// never runs.
pub const SELF_UPDATE_ENABLED: bool = false;
