//! Alarm clock CLI interface.

use std::num::ParseIntError;
use std::process::ExitCode;
use std::str::FromStr;
use std::time::{Duration as StdDuration, Instant};

use alarm::audio::AlarmSound;
use alarm::{Alarms, Event, Subscriber};
use clap::{Args, Parser, Subcommand};
use rezz::Alarm;
use time::error::ComponentRange;
use time::format_description::well_known::Rfc2822;
use time::{Duration, Month, OffsetDateTime, Time, UtcOffset};
use uuid::Uuid;

/// Infinite sleep timeout.
const INFINITY: StdDuration = StdDuration::from_secs(60 * 60 * 24 * 365 * 999);

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    subcommand: Subcmd,
}

#[derive(Subcommand, Debug)]
enum Subcmd {
    /// Alarm background daemon.
    #[clap(alias = "d")]
    Daemon(DaemonArgs),
    /// Add a new alarm.
    #[clap(alias = "a")]
    Add(AddArgs),
    /// Remove an existing alarm.
    #[clap(alias = "r")]
    Remove(RemoveArgs),
    /// List all alarms.
    #[clap(alias = "l")]
    List(ListArgs),
}

#[derive(Args, Debug)]
struct DaemonArgs {}

#[derive(Args, Debug)]
struct AddArgs {
    /// ID used to delete the alarm [default: UUID].
    #[clap(long)]
    id: Option<String>,
    /// Alarm time in RFC3339 format.
    time: ClapDateTime,
    /// Seconds to ring the alarm for.
    #[clap(long, short = 's', default_value_t = 600)]
    ring_seconds: u32,
}

#[derive(Args, Debug)]
struct RemoveArgs {
    /// Alarm IDs.
    id: Vec<String>,
}

#[derive(Args, Debug)]
struct ListArgs {}

#[tokio::main(flavor = "current_thread")]
pub async fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.subcommand {
        Subcmd::Add(args) => {
            let id = args.id.unwrap_or_else(|| Uuid::new_v4().to_string());
            let unix_time = (args.time.0 - OffsetDateTime::UNIX_EPOCH).whole_seconds();
            let alarm = Alarm::new(&id, unix_time, args.ring_seconds);

            match Alarms.add(alarm).await {
                Ok(()) => println!("Added alarm with ID {id:?}"),
                Err(err) => {
                    eprintln!("Could not add alarm: {err}");
                    return ExitCode::from(1);
                },
            }
        },
        Subcmd::Remove(args) => {
            for id in &args.id {
                match Alarms.remove(id.clone()).await {
                    Ok(()) => println!("Removed alarm with ID {:?}", args.id),
                    Err(err) => {
                        eprintln!("Could not remove alarm: {err}");
                        return ExitCode::from(2);
                    },
                }
            }
        },
        Subcmd::List(_args) => {
            let alarms = match Alarms.load().await {
                Ok(alarms) => alarms,
                Err(err) => {
                    eprintln!("Could not read alarms database: {err}");
                    return ExitCode::from(3);
                },
            };

            // Early return without any alarms.
            if alarms.is_empty() {
                println!("No alarms set");
                return ExitCode::SUCCESS;
            }

            // Print header.
            println!("\x1b[4;1m{: <36}  {: <31}\x1b[0m", "ID", "Alarm Time");

            // Print each alarm.
            for alarm in alarms {
                // Try to convert unix seconds to local time.
                let mut time = OffsetDateTime::UNIX_EPOCH + Duration::seconds(alarm.unix_time);
                if let Ok(offset) = UtcOffset::current_local_offset() {
                    time = time.to_offset(offset);
                }
                let time_str = time.format(&Rfc2822).unwrap();

                println!("{: <36}  {: <31}", alarm.id, time_str);
            }
        },
        Subcmd::Daemon(_args) => {
            // Setup listener for DBus events.
            let mut subscriber = match Subscriber::new().await {
                Ok(subscriber) => subscriber,
                Err(err) => {
                    eprintln!("Could not subscribe to DBus events: {err}");
                    return ExitCode::from(1);
                },
            };

            println!("Successfully started alarm daemon");

            let mut ringing_alarm: Option<RingingAlarm> = None;
            loop {
                // Convert optional timeout into infinite future on `None`.
                let ringing_timeout = match &ringing_alarm {
                    Some(ringing) => tokio::time::sleep_until(ringing.timeout.into()),
                    None => tokio::time::sleep(INFINITY),
                };

                tokio::select! {
                    Some(event) = subscriber.next() => match event {
                        // Play sound once an alarm becomes active.
                        Event::Ring(alarm) => {
                            // Wait for 3 seconds to allow a UI frontend to pick up the alarm.
                            tokio::time::sleep(StdDuration::from_secs(3)).await;

                            // Get full list of alarms, including alarms currently ringing.
                            let all_alarms = match Alarms.load().await {
                                Ok(all_alarms) => all_alarms,
                                Err(_) => continue,
                            };

                            // Ignore alarm if it has been picked up while waiting.
                            if !all_alarms.iter().any(|a| a.id == alarm.id) {
                                continue;
                            }

                            // Start ringing if the alarm hasn't been picked up by a UI.
                            let sound = match AlarmSound::play() {
                                Ok(sound) => sound,
                                Err(err) => {
                                    eprintln!("Could not play alarm sound: {err}");
                                    continue;
                                },
                            };

                            // Schedule alarm for cancellation.
                            let timeout =
                                Instant::now() + StdDuration::from_secs(alarm.ring_seconds as u64);
                            ringing_alarm = Some(RingingAlarm { alarm, timeout, sound });
                        },
                        // Cancel alarm if it was removed by a third-party client.
                        Event::AlarmsChanged(alarms) => {
                            if let Some(ringing) = &ringing_alarm
                                && !alarms.iter().any(|alarm| alarm.id == ringing.alarm.id)
                            {
                                ringing_alarm = None;
                            }
                        },
                    },
                    // NOTE: This usually isn't hit, since the removal of the Alarm through DBus
                    // after it stops ringing will cancel the alarm automatically.
                    _ = ringing_timeout => {
                        if let Some(ringing) = ringing_alarm.take() {
                            ringing.sound.stop();
                        }
                    },
                }
            }
        },
    }

    ExitCode::SUCCESS
}

/// DateTime wrapper with `FromStr` implementation.
#[derive(Clone, Debug)]
struct ClapDateTime(OffsetDateTime);

impl FromStr for ClapDateTime {
    type Err = DateTimeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Get current time.
        let mut now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());

        // Split date and time.
        let (date, time) = match s.split_once('T') {
            Some((date, time)) => (Some(date), time),
            None => (None, s),
        };

        // Override date.
        if let Some(date) = date {
            let mut components = date.splitn(3, '-');

            let year =
                components.next().ok_or_else(|| DateTimeError::InvalidFormat(date.into()))?;
            now.replace_year(i32::from_str(year)?)?;

            let month =
                components.next().ok_or_else(|| DateTimeError::InvalidFormat(date.into()))?;
            let month_offset = u8::from_str(month)?.saturating_sub(1);
            now.replace_month(Month::January.nth_next(month_offset))?;

            let day = components.next().ok_or_else(|| DateTimeError::InvalidFormat(date.into()))?;
            now.replace_day(u8::from_str(day)?)?;
        }

        // Override time.

        let (hour, rest) =
            time.split_once(':').ok_or_else(|| DateTimeError::InvalidFormat(time.into()))?;
        let (minute, second) = match rest.split_once(':') {
            Some((minute, second)) => (minute, Some(second)),
            None => (rest, None),
        };

        let hour = u8::from_str(hour)?;
        let minute = u8::from_str(minute)?;
        let second = second.map_or(Ok(0), u8::from_str)?;
        let time = Time::from_hms(hour, minute, second)?;

        // Add one day if time has already passed.
        if time < now.time() {
            now += Duration::days(1);
        }

        now = now.replace_time(time);

        Ok(Self(now))
    }
}

/// Actively ringing alarm.
struct RingingAlarm {
    alarm: Alarm,
    timeout: Instant,
    sound: AlarmSound,
}

#[derive(thiserror::Error, Clone, Debug)]
enum DateTimeError {
    #[error("invalid format: {0}")]
    ComponentRange(#[from] ComponentRange),
    #[error("{0}")]
    InvalidFormat(String),
    #[error("{0}")]
    ParseInt(#[from] ParseIntError),
}
