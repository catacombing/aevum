use std::cmp::Ordering;
use std::fs::File;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use zbus::zvariant::{OwnedValue, Type, Value};

use crate::ioctl::{RtcTime, RtcWkalm};

mod ioctl;

/// Primary RTC path, should always exist for systems with RTC.
const RTC_PATH: &str = "/dev/rtc";

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    InvalidTime(#[from] time::error::ComponentRange),
    #[error("{0}")]
    InvalidInt(#[from] std::num::TryFromIntError),
    #[error("{0}")]
    Nix(#[from] nix::Error),
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Set a new RTC wakeup time.
pub fn set_wakeup(mut time: OffsetDateTime) -> Result<(), Error> {
    let rtc_file = File::open(RTC_PATH)?;

    // Get current RTC time.
    let mut rtc_time: MaybeUninit<RtcTime> = MaybeUninit::uninit();
    let rtc_time: OffsetDateTime = unsafe {
        ioctl::rtc_rd_time(rtc_file.as_raw_fd(), rtc_time.as_mut_ptr())?;
        rtc_time.assume_init().try_into()?
    };

    // Apply delta between RTC and current time.
    let delta = OffsetDateTime::now_utc() - rtc_time;
    time -= delta;

    unsafe { ioctl::rtc_wkalm_set(rtc_file.as_raw_fd(), &time.into() as *const _)? };

    Ok(())
}

pub fn get_wakeup() -> Result<Option<OffsetDateTime>, Error> {
    let rtc_file = File::open(RTC_PATH)?;
    let mut time: MaybeUninit<RtcWkalm> = MaybeUninit::uninit();
    let time = unsafe {
        ioctl::rtc_wkalm_rd(rtc_file.as_raw_fd(), time.as_mut_ptr())?;
        time.assume_init()
    };
    Ok(time.into())
}

/// Clear all current wakeup times.
pub fn clear_wakeup() -> Result<(), Error> {
    let rtc_file = File::open(RTC_PATH)?;
    unsafe { ioctl::rtc_wkalm_set(rtc_file.as_raw_fd(), &ioctl::RESET_ALARM as *const _)? };
    Ok(())
}

/// Single alarm.
#[derive(Deserialize, Serialize, Type, Value, OwnedValue, Clone, PartialEq, Eq, Debug)]
pub struct Alarm {
    pub id: String,
    pub unix_time: i64,
    pub ring_seconds: u32,
}

impl Alarm {
    pub fn new(id: impl Into<String>, unix_time: i64, ring_seconds: u32) -> Self {
        Self { id: id.into(), unix_time, ring_seconds }
    }
}

impl PartialOrd for Alarm {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Alarm {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.unix_time.cmp(&other.unix_time) {
            Ordering::Equal => match self.id.cmp(&other.id) {
                Ordering::Equal => self.ring_seconds.cmp(&other.ring_seconds),
                ordering => ordering,
            },
            ordering => ordering,
        }
    }
}
