//! Audio playback.

use std::io::Cursor;
use std::time::Duration;

use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};

use crate::error::Error;

/// Alarm sound.
///
/// Created as `service-login.oga` by the Pidgin developers under GPLv2:
/// https://cgit.freedesktop.org/sound-theme-freedesktop.
const ALARM_AUDIO: &[u8] = include_bytes!("../../alarm.flac");

/// Length of the alarm audio file.
///
/// The default `service-login.oga` is a bit long to be played on repeat as an
/// alarm, so we shorten it by 680ms.
const ALARM_AUDIO_LENGTH: Duration = Duration::from_millis(1500);

/// Alarm audio playback.
pub struct AlarmSound {
    _stream: OutputStream,
    sink: Sink,
}

impl AlarmSound {
    /// Play the alarm sound.
    ///
    /// This will start playing the alarm sound immediately and only stop after
    /// the returned [`AlarmSound`] is dropped or [`AlarmSound::stop`] is called
    /// on it.
    pub fn play() -> Result<Self, Error> {
        // Parse the audio source file.
        let stream = OutputStreamBuilder::open_default_stream()?;
        let audio_buffer = Cursor::new(ALARM_AUDIO);
        let source = Decoder::new(audio_buffer).unwrap();

        // Adjust length and repeat infinitely.
        let source = source.take_duration(ALARM_AUDIO_LENGTH).repeat_infinite();

        // Create a sink to allow playback control.
        let sink = Sink::connect_new(stream.mixer());
        sink.append(source);

        Ok(Self { _stream: stream, sink })
    }

    /// Stop the alarm playback.
    pub fn stop(self) {
        self.sink.stop();
    }
}
