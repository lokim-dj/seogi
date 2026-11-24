//! Keeps the computer awake while a meeting is being recorded or worked on.
//!
//! A laptop that dozes off in the middle of a two-hour meeting loses the
//! rest of it. While a [`KeepAwake`] is alive the system does not go to
//! sleep on its own; closing the lid still sleeps it, as the user expects.

/// Holds the system awake until dropped.
pub struct KeepAwake {
    #[cfg(target_os = "macos")]
    helper: Option<std::process::Child>,
    #[cfg(windows)]
    release: Option<std::sync::mpsc::Sender<()>>,
}

impl KeepAwake {
    pub fn new() -> Self {
        #[cfg(target_os = "macos")]
        {
            // `caffeinate -i` holds an idle-sleep assertion; `-w` makes it
            // let go when this app exits, however it exits.
            let helper = std::process::Command::new("/usr/bin/caffeinate")
                .args(["-i", "-w", &std::process::id().to_string()])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| log::warn!("Could not keep the computer awake ({e})"))
                .ok();
            Self { helper }
        }
        #[cfg(windows)]
        {
            // The request belongs to the thread that makes it, so a thread
            // makes it, waits, and withdraws it.
            let (release, released) = std::sync::mpsc::channel::<()>();
            let spawned = std::thread::Builder::new()
                .name("seogi-keep-awake".into())
                .spawn(move || {
                    // SAFETY: a plain Win32 call with constant flags.
                    unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
                    // Returns when the sender is dropped.
                    let _ = released.recv();
                    unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
                });
            if let Err(e) = spawned {
                log::warn!("Could not keep the computer awake ({e})");
            }
            Self {
                release: Some(release),
            }
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            Self {}
        }
    }
}

impl Default for KeepAwake {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(mut helper) = self.helper.take() {
            let _ = helper.kill();
            let _ = helper.wait();
        }
        #[cfg(windows)]
        {
            self.release.take();
        }
    }
}

#[cfg(windows)]
const ES_CONTINUOUS: u32 = 0x8000_0000;
#[cfg(windows)]
const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn SetThreadExecutionState(flags: u32) -> u32;
}
