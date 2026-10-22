//! UTC timestamps (`YYYY-MM-DDTHH:MM:SSZ`) shown as dates in the user's time zone.

/// The local calendar date of `utc`, `YYYY-MM-DD`; `None` when `utc` is not in the
/// expected form or the time zone cannot convert it.
pub fn local_date(utc: &str) -> Option<String> {
    imp::local_date(utc, None)
}

/// `YYYY-MM-DDTHH:MM:SSZ` -> `[year, month, day, hour, minute, second]`.
fn parse(utc: &str) -> Option<[u16; 6]> {
    let b = utc.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' {
        return None;
    }
    if b[16] != b':' || b[19] != b'Z' {
        return None;
    }
    let num = |from: usize, to: usize| utc.get(from..to)?.parse::<u16>().ok();
    Some([
        num(0, 4)?,
        num(5, 7)?,
        num(8, 10)?,
        num(11, 13)?,
        num(14, 16)?,
        num(17, 19)?,
    ])
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::Time::{SystemTimeToTzSpecificLocalTime, TIME_ZONE_INFORMATION};

    /// `zone` is the machine's current time zone when `None` (daylight saving time
    /// as it applies on that date).
    pub fn local_date(utc: &str, zone: Option<&TIME_ZONE_INFORMATION>) -> Option<String> {
        let [year, month, day, hour, minute, second] = super::parse(utc)?;
        let utc = SYSTEMTIME {
            wYear: year,
            wMonth: month,
            wDay: day,
            wHour: hour,
            wMinute: minute,
            wSecond: second,
            ..Default::default()
        };
        let mut local = SYSTEMTIME::default();
        // SAFETY: both SYSTEMTIMEs and the optional zone are valid for the call.
        unsafe { SystemTimeToTzSpecificLocalTime(zone.map(|z| z as *const _), &utc, &mut local) }
            .ok()?;
        Some(format!(
            "{:04}-{:02}-{:02}",
            local.wYear, local.wMonth, local.wDay
        ))
    }
}

#[cfg(not(windows))]
mod imp {
    /// No time zone support yet off Windows: the UTC date.
    pub fn local_date(utc: &str, _zone: Option<&()>) -> Option<String> {
        super::parse(utc)?;
        utc.get(..10).map(str::to_owned)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use windows::Win32::System::Time::TIME_ZONE_INFORMATION;

    use super::imp::local_date;

    /// A zone without daylight saving time, `minutes` ahead of UTC.
    fn zone(minutes: i32) -> TIME_ZONE_INFORMATION {
        TIME_ZONE_INFORMATION {
            Bias: -minutes,
            ..Default::default()
        }
    }

    #[test]
    fn dates_follow_the_given_zone_not_the_machine() {
        let china = zone(8 * 60);
        // Before 08:00 in UTC+8 it is already the next day there.
        assert_eq!(
            local_date("2026-10-21T23:30:00Z", Some(&china)).as_deref(),
            Some("2026-10-22")
        );
        assert_eq!(
            local_date("2026-12-31T16:00:00Z", Some(&china)).as_deref(),
            Some("2027-01-01")
        );
        assert_eq!(
            local_date("2026-10-22T08:33:03Z", Some(&china)).as_deref(),
            Some("2026-10-22")
        );
        assert_eq!(
            local_date("2026-03-01T03:00:00Z", Some(&zone(-5 * 60))).as_deref(),
            Some("2026-02-28")
        );
        assert_eq!(
            local_date("2026-10-21T23:30:00Z", Some(&zone(0))).as_deref(),
            Some("2026-10-21")
        );
    }

    #[test]
    fn malformed_timestamps_give_none() {
        for bad in [
            "",
            "2026-10-22",
            "2026-10-22 08:33:03Z",
            "2026-13-40T08:33:03Z",
        ] {
            assert_eq!(local_date(bad, Some(&zone(0))), None, "{bad}");
        }
        assert_eq!(super::local_date("garbage"), None);
    }
}
