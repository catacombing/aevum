//! RTC system interface.

use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

use crate::Error;

nix::ioctl_write_ptr!(rtc_wkalm_set, 'p', 0x0f, RtcWkalm);
nix::ioctl_read!(rtc_wkalm_rd, 'p', 0x10, RtcWkalm);
nix::ioctl_read!(rtc_rd_time, 'p', 0x09, RtcTime);

pub const RESET_ALARM: RtcWkalm = RtcWkalm {
    enabled: false,
    pending: false,
    // 1970-01-01T00:00Z
    time: RtcTime {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 1,
        tm_mon: 0,
        tm_year: 70,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
    },
};

/// Scheduled RTC wakeup.
#[repr(C)]
#[derive(Debug)]
pub struct RtcWkalm {
    /// Whether alarm interrupt should be enabled.
    enabled: bool,
    /// Pending interrupt for reading.
    pending: bool,
    /// Wakeup time.
    time: RtcTime,
}

impl From<OffsetDateTime> for RtcWkalm {
    fn from(time: OffsetDateTime) -> Self {
        Self { time: time.into(), enabled: true, pending: false }
    }
}

impl From<RtcWkalm> for Option<OffsetDateTime> {
    fn from(wkalm: RtcWkalm) -> Self {
        if wkalm.enabled { OffsetDateTime::try_from(wkalm.time).ok() } else { None }
    }
}

/// RTC wakeup time.
#[repr(C)]
#[derive(Debug)]
pub struct RtcTime {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    // Unused.
    tm_wday: i32,
    // Unused.
    tm_yday: i32,
    // Unused.
    tm_isdst: i32,
}

impl From<OffsetDateTime> for RtcTime {
    fn from(time: OffsetDateTime) -> Self {
        let time = time.to_offset(UtcOffset::UTC);

        Self {
            tm_sec: time.second() as i32,
            tm_min: time.minute() as i32,
            tm_hour: time.hour() as i32,
            tm_mday: time.day() as i32,
            tm_mon: time.month() as i32 - 1,
            tm_year: time.year() - 1900,
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
        }
    }
}

impl TryFrom<RtcTime> for OffsetDateTime {
    type Error = Error;

    fn try_from(time: RtcTime) -> Result<Self, Self::Error> {
        let year = time.tm_year + 1900;
        let month = Month::try_from(u8::try_from(time.tm_mon + 1)?)?;
        let day = u8::try_from(time.tm_mday)?;
        let hour = u8::try_from(time.tm_hour)?;
        let minute = u8::try_from(time.tm_min)?;
        let second = u8::try_from(time.tm_sec)?;

        let date = Date::from_calendar_date(year, month, day)?;
        let time = Time::from_hms(hour, minute, second)?;

        let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
        Ok(PrimitiveDateTime::new(date, time).assume_utc().to_offset(offset))
    }
}
