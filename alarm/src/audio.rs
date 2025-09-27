//! Audio playback.

use std::io::Cursor;
use std::time::Duration;

use libpulse_binding::context::{Context, FlagSet as ContextFlagSet, State as PulseState};
use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};
use libpulse_binding::volume::{ChannelVolumes, Volume};
use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};
use tracing::error;

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
        // Ensure volume is at 100% before playing alarm.
        if let Err(err) = Pulseaudio::connect().and_then(|mut pa| pa.set_volume(100)) {
            error!("Pulseaudio error: {err}");
        }

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

struct Pulseaudio {
    mainloop: Mainloop,
    context: Context,
}

impl Pulseaudio {
    /// Connect to the pulseaudio server.
    fn connect() -> Result<Self, Error> {
        // Connect with pulseaudio's standard event loop.
        let crate_name = env!("CARGO_PKG_NAME");
        let mainloop = Mainloop::new().ok_or(Error::PulseaudioConnection)?;
        let mut context = Context::new(&mainloop, crate_name).ok_or(Error::PulseaudioConnection)?;
        context.connect(None, ContextFlagSet::NOFLAGS, None)?;

        let mut pulseaudio = Self { mainloop, context };

        // Wait for connection to be established.
        loop {
            pulseaudio.dispatch()?;

            match pulseaudio.context.get_state() {
                PulseState::Ready => break,
                PulseState::Failed | PulseState::Terminated => {
                    return Err(Error::PulseaudioConnection);
                },
                _ => (),
            }
        }

        Ok(pulseaudio)
    }

    /// Set audio volume percentage.
    fn set_volume(&mut self, volume: u8) -> Result<(), Error> {
        let volume = Volume(Volume::NORMAL.0 * volume as u32 / 100);
        let mut volumes = ChannelVolumes::default();
        volumes.set(ChannelVolumes::CHANNELS_MAX, volume);

        let mut introspect = self.context.introspect();
        introspect.set_sink_volume_by_index(0, &volumes, None);

        self.dispatch()?;
        self.dispatch()?;
        self.dispatch()
    }

    /// Blockingly dispatch the next pulseaudio event.
    fn dispatch(&mut self) -> Result<(), Error> {
        match self.mainloop.iterate(true) {
            IterateResult::Quit(_) => Err(Error::PulseaudioConnection),
            IterateResult::Err(err) => Err(err.into()),
            IterateResult::Success(_) => Ok(()),
        }
    }
}

impl Drop for Pulseaudio {
    fn drop(&mut self) {
        self.context.disconnect();
    }
}
