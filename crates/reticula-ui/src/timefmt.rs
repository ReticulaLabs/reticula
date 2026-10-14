//! Formatting of UTC timestamps for the settings screens.
//!
//! Deliberately dependency-free: the UI crate renders onto an embedded display
//! with a 6×10 bitmap font, so pulling in a full date/time library would cost
//! flash and buy nothing. Only what the System page needs is implemented:
//! civil-date arithmetic in both directions, and fixed-width rendering.

/// Unix time below which a clock is certainly unset.
///
/// ESP32-class targets boot with their clock at 0 and it only counts up from
/// boot until something syncs it, so an unsynced device reads as 1970. On
/// Reticulum that is not merely cosmetic: announce timestamps are compared
/// against the last one a peer saw, so a 1970 clock makes every peer discard
/// this node as stale. This is the 2020-09-13 threshold.
pub const PLAUSIBLE_UNIX_TIME: u64 = 1_600_000_000;

/// A broken-down UTC timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 = Sunday.
    pub weekday: u32,
}

/// Split Unix seconds into a UTC date and time.
///
/// Uses Howard Hinnant's `civil_from_days`, valid across the whole proleptic
/// Gregorian range, so it handles pre-epoch and far-future timestamps without
/// special-casing — which matters because an untrusted or wrong clock can hand
/// us either.
pub fn from_unix(unix_secs: u64) -> DateTime {
    // Floor-divide so timestamps before the epoch still land on the right day.
    let days = (unix_secs / 86_400) as i64;
    let secs_of_day = unix_secs % 86_400;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if month <= 2 { y + 1 } else { y };

    DateTime {
        year,
        month,
        day,
        hour: (secs_of_day / 3600) as u32,
        minute: ((secs_of_day % 3600) / 60) as u32,
        second: (secs_of_day % 60) as u32,
        weekday: ((days + 4).rem_euclid(7)) as u32, // 1970-01-01 was a Thursday
    }
}

/// Compose Unix seconds from a UTC date and time, validating each field.
///
/// Returns `None` for impossible dates (e.g. 31 February) rather than rolling
/// them over, so a mistyped manual entry is rejected instead of silently
/// becoming a different day.
pub fn to_unix(
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Option<u64> {
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let secs = days
        .checked_mul(86_400)?
        .checked_add((hour * 3600 + minute * 60 + second) as i64)?;
    u64::try_from(secs).ok()
}

/// Days in `month`, honouring leap years.
pub fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days between 1970-01-01 and the given civil date.
///
/// Uses the same `era`/`yoe`/`doy` decomposition as [`from_unix`] so the two
/// directions cannot drift apart.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let m = month as i64;
    let d = day as i64;
    // Shift the year to start in March, so leap days land at the end of the
    // (now uniform 365-day) year and need no special case in the month loop.
    let y = if m <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// `2026-10-13 20:47:03 UTC` — the fixed-width form the System page shows.
pub fn format_datetime(unix_secs: u64) -> String {
    let dt = from_unix(unix_secs);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
    )
}

/// `Tue`, for a compact secondary line.
pub fn format_weekday(unix_secs: u64) -> &'static str {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    DAYS[(from_unix(unix_secs).weekday % 7) as usize]
}

/// Whether `unix_secs` is too early to be a real time. See
/// [`PLAUSIBLE_UNIX_TIME`].
pub fn is_implausible(unix_secs: u64) -> bool {
    unix_secs < PLAUSIBLE_UNIX_TIME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_timestamp_round_trips() {
        // 2026-10-13 20:47:03 UTC == 1_791_924_423.
        let t = 1_791_924_423;
        let dt = from_unix(t);
        assert_eq!(
            (dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second),
            (2026, 10, 13, 20, 47, 3)
        );
        assert_eq!(to_unix(2026, 10, 13, 20, 47, 3), Some(t));
    }

    /// Both directions must agree, or a hand-typed date lands on the wrong
    /// day. Sweeping every day from the epoch catches an off-by-one that only
    /// shows up on a leap year or a century boundary.
    #[test]
    fn round_trips_every_day_from_the_epoch_to_2100() {
        // 2100-01-01, chosen because it is *not* a leap year: the 100-year rule
        // is the easiest one to get wrong.
        let end = 4_102_444_800u64;

        let mut t = 0u64;
        let mut days = 0u64;
        while t <= end {
            let dt = from_unix(t);
            assert_eq!(
                to_unix(dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second),
                Some(t),
                "round trip failed at {t}"
            );
            days += 1;
            t += 86_400;
        }
        assert_eq!(
            days, 47_483,
            "sweep should cover every day up to 2100 inclusive"
        );
    }

    /// The reverse direction, over the same span: a leap day must not be
    /// accepted in a common year, and 1 March must follow 29 February.
    #[test]
    fn no_leap_day_is_invented_in_common_years() {
        let mut t = 0u64;
        let end = 4_102_444_800u64;
        while t <= end {
            let dt = from_unix(t);
            // Every date from_unix produces must be re-accepted by to_unix, and
            // 29 February may only appear when the year really is a leap year.
            if dt.month == 2 && dt.day == 29 {
                assert!(is_leap(dt.year), "invented 29 Feb in {}", dt.year);
            }
            t += 86_400;
        }
    }

    #[test]
    fn epoch_is_the_start_of_the_range() {
        let dt = from_unix(0);
        assert_eq!((dt.year, dt.month, dt.day, dt.hour), (1970, 1, 1, 0));
        assert_eq!(dt.weekday, 4); // Thursday
        assert_eq!(to_unix(1970, 1, 1, 0, 0, 0), Some(0));
    }

    #[test]
    fn weekday_advances_one_day_at_a_time() {
        // 2026-10-13 is a Tuesday, so the 14th must be a Wednesday.
        assert_eq!(format_weekday(1_791_924_423), "Tue");
        assert_eq!(format_weekday(1_791_924_423 + 86_400), "Wed");
    }

    #[test]
    fn round_trips_across_a_leap_day() {
        // 2024-02-29 exists; the next day must be 1 March.
        let feb29 = to_unix(2024, 2, 29, 12, 0, 0).unwrap();
        assert_eq!(from_unix(feb29).month, 2);
        assert_eq!(from_unix(feb29).day, 29);
        assert_eq!(from_unix(feb29 + 86_400).month, 3);
        assert_eq!(from_unix(feb29 + 86_400).day, 1);
    }

    #[test]
    fn common_years_reject_february_29() {
        assert!(to_unix(2023, 2, 29, 0, 0, 0).is_none());
        assert!(
            to_unix(2100, 2, 29, 0, 0, 0).is_none(),
            "2100 is not a leap year"
        );
        assert!(
            to_unix(2000, 2, 29, 0, 0, 0).is_some(),
            "2000 is a leap year"
        );
    }

    #[test]
    fn out_of_range_fields_are_rejected() {
        assert!(to_unix(2026, 13, 1, 0, 0, 0).is_none(), "month 13");
        assert!(to_unix(2026, 0, 1, 0, 0, 0).is_none(), "month 0");
        assert!(to_unix(2026, 4, 31, 0, 0, 0).is_none(), "31 April");
        assert!(to_unix(2026, 1, 32, 0, 0, 0).is_none(), "day 32");
        assert!(to_unix(2026, 1, 1, 24, 0, 0).is_none(), "hour 24");
        assert!(to_unix(2026, 1, 1, 0, 60, 0).is_none(), "minute 60");
        assert!(to_unix(2026, 1, 1, 0, 0, 60).is_none(), "second 60");
        assert!(to_unix(2026, 1, 0, 0, 0, 0).is_none(), "day 0");
    }

    #[test]
    fn month_lengths_are_right() {
        assert_eq!(days_in_month(2026, 1), 31);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
    }

    #[test]
    fn implausibility_matches_the_documented_threshold() {
        assert!(is_implausible(0));
        assert!(is_implausible(PLAUSIBLE_UNIX_TIME - 1));
        assert!(!is_implausible(PLAUSIBLE_UNIX_TIME));
        assert!(!is_implausible(1_791_924_423));
    }

    #[test]
    fn formatting_is_fixed_width() {
        assert_eq!(format_datetime(1_791_924_423), "2026-10-13 20:47:03");
        // A 1970-era clock still renders, it just reads as 1970.
        assert_eq!(format_datetime(0), "1970-01-01 00:00:00");
    }
}
