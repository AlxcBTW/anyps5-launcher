//! Wall-clock helpers: the local time for the top bar and an RFC 3339 stamp for `lastPlayed`.

use std::time::{SystemTime, UNIX_EPOCH};

/// Hour and minute of the local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockTime {
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl ClockTime {
    /// `HH:mm`.
    pub fn text(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

/// The local time of day.
#[cfg(windows)]
pub fn local_time() -> ClockTime {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut now = SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    // SAFETY: `now` is a valid, writable SYSTEMTIME.
    unsafe { GetLocalTime(&mut now) };
    ClockTime {
        hour: u32::from(now.wHour),
        minute: u32::from(now.wMinute),
        second: u32::from(now.wSecond),
    }
}

/// The time of day in UTC (no time zone database without a Windows API).
#[cfg(not(windows))]
pub fn local_time() -> ClockTime {
    let secs = unix_seconds().rem_euclid(86_400) as u32;
    ClockTime {
        hour: secs / 3600,
        minute: secs % 3600 / 60,
        second: secs % 60,
    }
}

/// Seconds until the minute changes (for repaint scheduling), at least 0.05.
pub fn seconds_to_next_minute() -> f32 {
    let second = local_time().second;
    (60 - second) as f32 + 0.05
}

/// Whole seconds since the Unix epoch.
pub fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `2026-10-22T09:30:00Z` for a Unix time.
pub fn rfc3339_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (year + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_known_dates() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_782_120_600), "2026-06-22T09:30:00Z");
        assert_eq!(rfc3339_utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn clock_text_pads() {
        let t = ClockTime {
            hour: 7,
            minute: 5,
            second: 0,
        };
        assert_eq!(t.text(), "07:05");
    }
}
