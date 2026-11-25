//! Signing in with a subscription: whether Claude Code and Codex are on this
//! computer and signed in, and opening each tool's own sign-in.
//!
//! Seogi never signs in for the user and never installs a tool. It asks the
//! tool whether it is signed in (`claude auth status`, `codex login status`:
//! both read what is stored and answer at once), opens the tool's own
//! sign-in in a terminal window, and opens the tool's installation page in
//! the browser.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::llm;
use super::platform::hide_console;

/// How long a status question may take. Both tools answer in a tenth of a
/// second from what they have stored; one that hangs counts as signed out.
const STATUS_TIMEOUT: Duration = Duration::from_secs(8);

/// A tool that signs in with the user's own plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    /// Claude Code, for a Claude plan.
    Claude,
    /// Codex, for a ChatGPT plan.
    Codex,
}

/// Where a tool stands on this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SignIn {
    /// The tool is not installed.
    Missing,
    /// Installed, not signed in.
    SignedOut,
    /// Installed and signed in.
    SignedIn,
}

/// Where both tools stand.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SignIns {
    pub claude: SignIn,
    pub codex: SignIn,
}

impl Tool {
    fn find(self) -> Option<PathBuf> {
        match self {
            Tool::Claude => llm::find_claude_cli(),
            Tool::Codex => llm::find_codex_cli(),
        }
    }

    /// The tool's own question "am I signed in?". Both exit 0 when signed
    /// in and 1 when not (checked against Claude Code 2.1 and Codex 0.171).
    fn status_args(self) -> &'static [&'static str] {
        match self {
            Tool::Claude => &["auth", "status"],
            Tool::Codex => &["login", "status"],
        }
    }

    /// The tool's own sign-in, which opens the browser (`claude auth login`
    /// signs in with a Claude plan by default; `codex login` with ChatGPT).
    fn sign_in_args(self) -> &'static [&'static str] {
        match self {
            Tool::Claude => &["auth", "login"],
            Tool::Codex => &["login"],
        }
    }

    /// The official page that says how to install the tool, Windows
    /// included.
    pub fn install_page(self) -> &'static str {
        match self {
            Tool::Claude => "https://code.claude.com/docs/en/setup",
            Tool::Codex => "https://developers.openai.com/codex/cli",
        }
    }
}

/// Where `tool` stands. Blocks for as long as the tool takes to answer.
pub fn status(tool: Tool) -> SignIn {
    match tool.find() {
        None => SignIn::Missing,
        Some(path) if signed_in(&path, tool.status_args()) => SignIn::SignedIn,
        Some(_) => SignIn::SignedOut,
    }
}

/// Where both tools stand.
pub fn statuses() -> SignIns {
    let codex = std::thread::spawn(|| status(Tool::Codex));
    let claude = status(Tool::Claude);
    SignIns {
        claude,
        codex: codex.join().unwrap_or(SignIn::SignedOut),
    }
}

fn signed_in(program: &Path, args: &[&str]) -> bool {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir(std::env::temp_dir());
    hide_console(&mut command);
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if started.elapsed() < STATUS_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Opens `tool`'s own sign-in in a terminal window (Terminal on macOS,
/// PowerShell on Windows), so the user sees what it says and can paste a
/// code if the browser asks for one. `farewell` is printed once it is done.
pub fn open_sign_in(tool: Tool, farewell: &str) -> Result<()> {
    let program = tool.find().context("not_installed")?;
    open_in_terminal(&program, tool.sign_in_args(), farewell)
}

/// The shell script that runs the sign-in in Terminal, then says
/// `farewell`. Every word is quoted, so a path with spaces or quotes runs
/// as it is.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn sh_script(program: &Path, args: &[&str], farewell: &str) -> String {
    let quote = |text: &str| format!("'{}'", text.replace('\'', r"'\''"));
    let mut line = quote(&program.display().to_string());
    for arg in args {
        line.push(' ');
        line.push_str(&quote(arg));
    }
    format!("#!/bin/sh\n{line}\necho\necho {}\n", quote(farewell))
}

/// The same for PowerShell, encoded as `-EncodedCommand` takes it (UTF-16
/// in base64), which no quoting on the command line can spoil.
#[cfg_attr(not(windows), allow(dead_code))]
fn powershell_encoded(program: &Path, args: &[&str], farewell: &str) -> String {
    use base64::Engine;
    // PowerShell takes the curly quotes as single quotes too, so each is
    // doubled like the straight one.
    let quote = |text: &str| {
        let mut quoted = String::from("'");
        for c in text.chars() {
            if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
                quoted.push(c);
            }
            quoted.push(c);
        }
        quoted.push('\'');
        quoted
    };
    let mut line = format!("& {}", quote(&program.display().to_string()));
    for arg in args {
        line.push(' ');
        line.push_str(&quote(arg));
    }
    let script = format!("{line}; Write-Host ''; Write-Host {}", quote(farewell));
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

#[cfg(target_os = "macos")]
fn open_in_terminal(program: &Path, args: &[&str], farewell: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let name = program
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("tool");
    let file = std::env::temp_dir().join(format!("seogi-sign-in-{name}.command"));
    std::fs::write(&file, sh_script(program, args, farewell))
        .context("could not write the sign-in script")?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
        .context("could not make the sign-in script runnable")?;
    // `open` needs no permission to control another app, as telling
    // Terminal what to type would.
    let status = Command::new("open")
        .args(["-a", "Terminal"])
        .arg(&file)
        .status()
        .context("could not open Terminal")?;
    if !status.success() {
        bail!("could not open Terminal");
    }
    Ok(())
}

#[cfg(windows)]
fn open_in_terminal(program: &Path, args: &[&str], farewell: &str) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NoExit",
            "-EncodedCommand",
            &powershell_encoded(program, args, farewell),
        ])
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .context("could not open PowerShell")?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn open_in_terminal(_program: &Path, _args: &[&str], _farewell: &str) -> Result<()> {
    bail!("signing in from Seogi needs macOS or Windows")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tool_has_its_own_commands_and_page() {
        assert_eq!(Tool::Claude.status_args(), ["auth", "status"]);
        assert_eq!(Tool::Codex.status_args(), ["login", "status"]);
        assert_eq!(Tool::Claude.sign_in_args(), ["auth", "login"]);
        assert_eq!(Tool::Codex.sign_in_args(), ["login"]);
        assert!(Tool::Claude.install_page().starts_with("https://"));
        assert!(Tool::Codex.install_page().starts_with("https://"));
    }

    #[test]
    fn the_sign_in_runs_the_path_as_it_is() {
        let program = Path::new("/Users/a b/it's/claude");
        assert_eq!(
            sh_script(program, &["auth", "login"], "done 'ok'"),
            "#!/bin/sh\n'/Users/a b/it'\\''s/claude' 'auth' 'login'\necho\necho 'done '\\''ok'\\'''\n"
        );

        use base64::Engine;
        let encoded = powershell_encoded(
            Path::new("C:\\a b\\it's O\u{2019}Neil\\codex.exe"),
            &["login"],
            "끝",
        );
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("base64");
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(
            String::from_utf16(&units).expect("utf-16"),
            "& 'C:\\a b\\it''s O\u{2019}\u{2019}Neil\\codex.exe' 'login'; Write-Host ''; Write-Host '끝'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_answer_is_the_exit_code() {
        assert!(signed_in(Path::new("/bin/sh"), &["-c", "exit 0"]));
        assert!(!signed_in(Path::new("/bin/sh"), &["-c", "exit 1"]));
        assert!(!signed_in(Path::new("/no/such/tool"), &[]));
    }
}
