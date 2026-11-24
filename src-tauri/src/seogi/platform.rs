//! Small differences between macOS and Windows that several modules share.

use std::path::PathBuf;
use std::process::Command;

/// Keeps a helper program from flashing a console window on Windows. A GUI
/// app has no console, so Windows gives every console program it starts a
/// new, visible one unless told not to.
pub fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// Where the program `name` is, if it is installed. An app started from the
/// Dock or the Start menu does not see the shell's PATH, so the places
/// package managers install into are checked as well, after `first`.
pub fn find_program(name: &str, first: &[PathBuf]) -> Option<PathBuf> {
    let file_names: Vec<String> = if cfg!(windows) {
        vec![format!("{name}.exe"), format!("{name}.cmd")]
    } else {
        vec![name.to_string()]
    };

    let mut places: Vec<PathBuf> = first.to_vec();
    if let Some(path) = std::env::var_os("PATH") {
        places.extend(std::env::split_paths(&path));
    }
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    if cfg!(windows) {
        if let Some(local) = var("LOCALAPPDATA") {
            places.push(local.join("Microsoft").join("WinGet").join("Links"));
            places.push(local.join("Programs").join("Ollama"));
        }
        if let Some(roaming) = var("APPDATA") {
            places.push(roaming.join("npm"));
        }
        if let Some(home) = var("USERPROFILE") {
            places.push(home.join("scoop").join("shims"));
            places.push(home.join(".local").join("bin"));
        }
        if let Some(data) = var("ProgramData") {
            places.push(data.join("chocolatey").join("bin"));
        }
    } else {
        places.extend(
            [
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/opt/local/bin",
                "/usr/bin",
            ]
            .iter()
            .map(PathBuf::from),
        );
        if let Some(home) = var("HOME") {
            places.push(home.join(".local").join("bin"));
        }
    }
    places
        .iter()
        .flat_map(|dir| file_names.iter().map(move |file| dir.join(file)))
        .find(|path| path.is_file())
}
