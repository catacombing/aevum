//! Tokio-compatible realtime clock.
//!
//! This module is based on the [tokio_walltime] crate, which is licensed under
//! [MIT].
//!
//! [tokio_walltime]: https://crates.io/crates/tokio-walltime
//! [MIT]: https://git.sr.ht/~pounce/tokio-walltime/tree/main/item/LICENSE

use std::io::Error as IoError;
use std::mem::MaybeUninit;
use std::ptr;
use std::time::SystemTime;

use tokio::signal::unix::{SignalKind, signal};

/// Create a new timer.
unsafe fn add_timer(seconds: i64) -> Result<libc::timer_t, IoError> {
    unsafe {
        // Get current time.
        let mut now = MaybeUninit::<libc::timespec>::uninit();
        if libc::clock_gettime(libc::CLOCK_REALTIME, now.as_mut_ptr()) != 0 {
            return Err(IoError::last_os_error());
        }

        // Calculate target wakeup time.
        let mut time = now.assume_init();
        time.tv_sec += seconds as libc::time_t;

        // Create the timer.
        let mut timer = MaybeUninit::<libc::timer_t>::uninit();
        let mut event = MaybeUninit::<libc::sigevent>::zeroed().assume_init();
        event.sigev_signo = SignalKind::alarm().as_raw_value();
        event.sigev_notify = libc::SIGEV_SIGNAL;
        if libc::timer_create(libc::CLOCK_REALTIME, &mut event, timer.as_mut_ptr()) != 0 {
            return Err(IoError::last_os_error());
        }
        let timer = timer.assume_init();

        // Activate the timer.
        let timerspec = libc::itimerspec {
            it_interval: libc::timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: time,
        };
        let result = libc::timer_settime(timer, libc::TIMER_ABSTIME, &timerspec, ptr::null_mut());
        match result {
            0 => Ok(timer),
            _ => {
                remove_timer(timer)?;
                Err(IoError::last_os_error())
            },
        }
    }
}

/// Delete an existing timer.
unsafe fn remove_timer(timer: libc::timer_t) -> Result<(), IoError> {
    match unsafe { libc::timer_delete(timer) } {
        0 => Ok(()),
        _ => Err(IoError::last_os_error()),
    }
}

/// Wait until the specified instant.
///
/// `tokio::time::sleep` uses the monotonic clock, so if the system is suspended
/// while the timer is active, the timer is delayed by a period equal to the
/// amount of time the system was suspended.
///
/// This timer operates using the realtime clock as a reference instead. If the
/// system is suspended at the time that the timer would expire, the timer
/// expires immediately after the system resumes from sleep.
///
/// # Errors
///
/// Returns an error if:
///  - Setting a underlying signal handler fails for any reason (see
///    [`signal#errors`]).
///  - Getting the current time (via `clock_gettime(2)`) fails.
///  - Creating the timer (via `timer_create(2)`) fails.
///  - Setting the timer (via `timer_settime(2)`) fails.
///  - Cleaning up the timer after it has triggered (via `timer_delete(2)`)
///    fails.
pub async fn sleep_until(target: SystemTime) -> Result<(), IoError> {
    // We must schedule our signal handler before the first signal appears.
    let mut alarm = signal(SignalKind::alarm())?;

    loop {
        let now = SystemTime::now();
        let remaining = match target.duration_since(now) {
            Ok(remaining) => remaining,
            Err(_) => return Ok(()),
        };

        // Set a timer for the specified time.
        let timer = unsafe { add_timer(remaining.as_secs() as i64)? };

        // Wait for the signal.
        alarm.recv().await;

        unsafe { remove_timer(timer)? }
    }
}
