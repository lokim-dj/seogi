//! The user's own API keys, kept in the operating system's keychain
//! (Keychain on macOS, Credential Manager on Windows). A key is never
//! written to a settings file, a log or this repository.

use anyhow::{anyhow, Result};

/// The name the keys are filed under in the keychain.
const SERVICE: &str = "io.github.lokimdj.seogi";

fn entry(name: &str) -> Result<keyring::Entry> {
    if name.trim().is_empty() {
        return Err(anyhow!("the key has no name"));
    }
    keyring::Entry::new(SERVICE, name.trim())
        .map_err(|e| anyhow!("the keychain could not be opened ({e})"))
}

/// Saves `value` as the key called `name` (`openai`, `anthropic`, `gemini`),
/// replacing an earlier one.
pub fn set(name: &str, value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("the key is empty"));
    }
    entry(name)?
        .set_password(value)
        .map_err(|e| anyhow!("the key could not be saved ({e})"))
}

/// The key called `name`, if one is saved.
pub fn get(name: &str) -> Option<String> {
    entry(name).ok()?.get_password().ok()
}

pub fn has(name: &str) -> bool {
    get(name).is_some()
}

/// Forgets the key called `name`. Forgetting one that is not there is fine.
pub fn remove(name: &str) -> Result<()> {
    match entry(name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow!("the key could not be removed ({e})")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes to the real keychain, so it runs only when asked for:
    /// `cargo test --lib seogi::secrets -- --ignored`.
    #[test]
    #[ignore = "touches the operating system's keychain"]
    fn a_key_is_saved_read_back_and_forgotten() {
        let name = "seogi-selftest";
        remove(name).unwrap();
        assert!(!has(name));
        set(name, "  not-a-real-key  ").unwrap();
        assert_eq!(get(name).as_deref(), Some("not-a-real-key"));
        set(name, "replaced").unwrap();
        assert_eq!(get(name).as_deref(), Some("replaced"));
        remove(name).unwrap();
        assert!(!has(name));
        // Forgetting twice is fine; an empty key or name is refused.
        remove(name).unwrap();
        assert!(set(name, "  ").is_err());
        assert!(set("", "x").is_err());
    }
}
