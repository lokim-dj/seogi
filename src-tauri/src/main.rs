// Derived from Handy (https://github.com/cjpais/Handy), Copyright (c) 2025 CJ Pais, MIT License. See THIRD_PARTY_NOTICES.md.

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use seogi_lib::CliArgs;

fn main() {
    // Let users pick GTK's display backend even inside the AppImage, whose
    // launch hook forces GDK_BACKEND=x11 over the user's own value (#1729,
    // #2034; hook workaround for tauri-apps/tauri#8541). Runs first so GTK
    // never sees the forced value.
    #[cfg(target_os = "linux")]
    seogi_lib::apply_handy_gdk_backend();

    seogi_lib::webkit_renderer::apply();

    #[cfg(target_os = "windows")]
    {
        // Avoid overlay/capture layer crashes (#2049). Set before backend
        // initialization, preserving user overrides.
        if std::env::var_os("VK_LOADER_LAYERS_DISABLE").is_none()
            && !seogi_lib::env_flag_enabled("HANDY_KEEP_VULKAN_IMPLICIT_LAYERS")
        {
            std::env::set_var("VK_LOADER_LAYERS_DISABLE", "~implicit~");
        }
    }

    #[cfg(target_os = "linux")]
    seogi_lib::disable_gamescope_wsi_layer();

    // Transcription worker: runs transcribe.cpp in isolation and never touches
    // Tauri, the CLI, or single-instance handling. Checked after the env setup
    // above so the worker inherits it (e.g. the Vulkan layer opt-out).
    if seogi_lib::engine_supervisor::is_worker_invocation() {
        std::process::exit(seogi_lib::engine_supervisor::run_worker());
    }

    // Diarization worker: the same isolation for the speaker model.
    if seogi_lib::seogi::diarize::is_worker_invocation() {
        std::process::exit(seogi_lib::seogi::diarize::run_worker());
    }

    // Calendar worker: one read of the Calendar app over a fresh connection,
    // for when the app's own has gone stale.
    if seogi_lib::seogi::mac_calendar::is_worker_invocation() {
        std::process::exit(seogi_lib::seogi::mac_calendar::run_worker());
    }

    let cli_args = CliArgs::parse();
    seogi_lib::run(cli_args)
}
