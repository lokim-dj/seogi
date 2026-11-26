//! Seogi's own backend: recordings, the processing pipeline and minutes.
//!
//! Nothing here knows about the UI. The frontend reaches this module through
//! its commands and listens to its events, so the screens can be replaced
//! without touching what happens to a recording.

pub mod amounts;
pub mod app;
pub mod app_menu;
pub mod ask;
pub mod audio_in;
pub mod banner;
pub mod calendar;
pub mod chunks;
pub mod cli;
pub mod commands;
pub mod config;
pub mod diarize;
pub mod dictation;
pub mod documents;
pub mod evidence;
pub mod facts;
pub mod feedback;
pub mod focus;
pub mod llm;
pub mod mac_calendar;
pub mod mic_button;
pub mod minutes;
pub mod numbers;
pub mod pipeline;
pub mod platform;
pub mod power;
pub mod recorder;
pub mod render;
pub mod secrets;
pub mod sign_in;
pub mod snippet;
pub mod status;
pub mod store;
pub mod talk;
pub mod templates;
pub mod transcript;
pub mod vocab;
pub mod watch;
