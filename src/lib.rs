use std::time::{Duration, SystemTime};

use rezz::Alarm;
use tokio_stream::StreamExt;
use zbus::{Connection, PropertyStream};

use crate::dbus::RezzProxy;
use crate::error::Error;

pub mod audio;
mod dbus;
pub mod error;
mod timer;

/// Primary alarm interface.
pub struct Alarms;

impl Alarms {
    /// Add a new alarm.
    pub async fn add(&self, alarm: Alarm) -> Result<(), Error> {
        let connection = Connection::system().await?;
        let rezz = RezzProxy::new(&connection).await?;
        rezz.add_alarm(alarm).await?;
        Ok(())
    }

    /// Remove an existing alarm.
    pub async fn remove(&self, id: String) -> Result<(), Error> {
        let connection = Connection::system().await?;
        let rezz = RezzProxy::new(&connection).await?;
        rezz.remove_alarm(id).await?;
        Ok(())
    }

    /// Load the alarm database.
    ///
    /// This will create the database, to simplify inotify usage.
    pub async fn load(&self) -> Result<Vec<Alarm>, Error> {
        let connection = Connection::system().await?;
        let rezz = RezzProxy::new(&connection).await?;
        let alarms = rezz.alarms().await?;
        Ok(alarms)
    }
}

/// Subscriber for alarm events.
pub struct Subscriber<'a> {
    alarms_stream: PropertyStream<'a, Vec<Alarm>>,
    alarms: Vec<Alarm>,
}

impl Subscriber<'static> {
    /// Create a new DBus alarm subscription.
    pub async fn new() -> Result<Self, Error> {
        // Setup DBus connection.
        let connection = Connection::system().await?;
        let rezz = RezzProxy::new(&connection).await?;

        // Create listener for alarms change.
        let alarms = rezz.alarms().await?;
        let alarms_stream = rezz.receive_alarms_changed().await;

        Ok(Self { alarms_stream, alarms })
    }

    /// Get the next alarm event.
    pub async fn next(&mut self) -> Option<Event<'_>> {
        let next_alarm = Self::next_alarm(&mut self.alarms);

        tokio::select! {
            // Handle alarm updates.
            Some(new_alarms) = self.alarms_stream.next() => {
                if let Ok(alarms) = new_alarms.get().await {
                    self.alarms = alarms;
                    return Some(Event::AlarmsChanged(&self.alarms));
                }
            },
            // Ring the alarm.
            _ = Self::wait_alarm(next_alarm) => {
                if let Some(alarm) = next_alarm {
                    return Some(Event::Ring(alarm.clone()));
                }
            },
        }

        None
    }

    /// Get all alarms.
    pub fn alarms(&self) -> &[Alarm] {
        self.alarms.as_slice()
    }

    /// Get the next alarm.
    ///
    /// This will ignore all elapsed alarms and sort the array to ensure optimal
    /// performance.
    fn next_alarm(alarms: &mut [Alarm]) -> Option<&Alarm> {
        // Get seconds since unix epoch.
        let current_secs =
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();

        // Get the next non-elapsed alarm.
        alarms.sort_by(|a, b| a.unix_time.cmp(&b.unix_time));
        alarms.iter().find(|alarm| alarm.unix_time as u64 > current_secs)
    }

    /// Convert alarm to tokio async sleep.
    async fn wait_alarm(alarm: Option<&Alarm>) -> Result<(), Error> {
        // Get time until alarm.
        let target = match alarm {
            Some(alarm) => SystemTime::UNIX_EPOCH + Duration::from_secs(alarm.unix_time as u64),
            // Default to an hour without alarm present.
            None => SystemTime::now() + Duration::from_secs(60 * 60),
        };

        // Wait for timer to elapse.
        timer::sleep_until(target).await?;

        Ok(())
    }
}

/// Alarm subscription events.
pub enum Event<'a> {
    AlarmsChanged(&'a [Alarm]),
    Ring(Alarm),
}
