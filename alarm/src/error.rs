//! Error handling.

use std::io::Error as IoError;

/// User-facing errors.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("alarm with id {0:?} exists already")]
    AlarmExists(String),
    #[error("no alarm found with id {0:?}")]
    AlarmNotFound(String),
    #[error("audio playback error: {0}")]
    AudioPlayback(#[from] rodio::PlayError),
    #[error("audio stream error: {0}")]
    AudioStream(#[from] rodio::StreamError),
    #[error("dbus error: {0}")]
    DBus(#[from] zbus::Error),
    #[error("io error: {0}")]
    Io(#[from] IoError),
}
