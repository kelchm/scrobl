//! Unix seconds: the clock, and seconds as text.

use std::time::{SystemTime, UNIX_EPOCH};

/// The system clock as Unix seconds, or 0 if it is set before 1970.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Formats Unix seconds as UTC, to the second: `2023-11-14T22:13:20Z`.
///
/// ```
/// assert_eq!(scrobl_cli::time::utc(1_700_000_000), "2023-11-14T22:13:20Z");
/// ```
pub fn utc(seconds: u64) -> String {
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let (hour, minute, second) = (rest / 3_600, rest % 3_600 / 60, rest % 60);

    // Days since 1970-01-01 to a civil date, counting years from 1 March so
    // that a leap day is the last day of its year.
    let days = days + 719_468;
    let (era, day_of_era) = (days / 146_097, days % 146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let (month, carry) = if shifted_month < 10 {
        (shifted_month + 3, 0)
    } else {
        (shifted_month - 9, 1)
    };
    let year = year_of_era + era * 400 + carry;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Reads the date of an HTTP `Date` header as Unix seconds:
/// `Tue, 14 Nov 2023 22:13:20 GMT`, the one form HTTP allows a server to
/// send. Anything else is `None`: a date before 1970, a day that does not
/// exist, and a day of the week that is not that date's.
///
/// ```
/// use scrobl_cli::time::http_date;
///
/// assert_eq!(http_date("Tue, 14 Nov 2023 22:13:20 GMT"), Some(1_700_000_000));
/// assert_eq!(http_date("2023-11-14T22:13:20Z"), None);
/// ```
pub fn http_date(text: &str) -> Option<u64> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // From the day of the week 1970 began on.
    const WEEKDAYS: [&str; 7] = ["Thu,", "Fri,", "Sat,", "Sun,", "Mon,", "Tue,", "Wed,"];
    let number = |digits: &str, length: usize| -> Option<u64> {
        (digits.len() == length && digits.bytes().all(|b| b.is_ascii_digit()))
            .then(|| digits.parse().ok())
            .flatten()
    };

    let mut parts = text.split(' ');
    let (weekday, day, month, year, clock, zone) = (
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
    );
    if parts.next().is_some() || zone != "GMT" {
        return None;
    }
    let day = number(day, 2)?;
    let month = MONTHS.iter().position(|name| *name == month)? as u64 + 1;
    let year = number(year, 4)?;
    let mut clock = clock.split(':');
    let (hour, minute, second) = (
        number(clock.next()?, 2)?,
        number(clock.next()?, 2)?,
        number(clock.next()?, 2)?,
    );
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    // A leap second is written as second 60; it is taken as the next second.
    if clock.next().is_some()
        || year < 1970
        || day == 0
        || day > days_in_month
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }

    // A civil date to days since 1970-01-01, the inverse of `utc`.
    let shifted_year = if month < 3 { year - 1 } else { year };
    let (era, year_of_era) = (shifted_year / 400, shifted_year % 400);
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    if WEEKDAYS.iter().position(|name| *name == weekday)? as u64 != days % 7 {
        return None;
    }
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

#[cfg(test)]
mod tests {
    use super::{http_date, utc};

    #[test]
    fn reads_the_date_of_an_http_header() {
        for (text, seconds) in [
            ("Thu, 01 Jan 1970 00:00:00 GMT", 0),
            ("Sun, 06 Nov 1994 08:49:37 GMT", 784_111_777),
            ("Tue, 29 Feb 2000 00:00:00 GMT", 951_782_400),
            ("Wed, 01 Mar 2000 00:00:00 GMT", 951_868_800),
            ("Thu, 29 Feb 2024 23:59:59 GMT", 1_709_251_199),
            ("Wed, 01 Jan 2025 00:00:00 GMT", 1_735_689_600),
            ("Thu, 31 Dec 2099 23:59:59 GMT", 4_102_444_799),
            ("Sat, 31 Dec 2016 23:59:60 GMT", 1_483_228_800),
        ] {
            assert_eq!(http_date(text), Some(seconds), "{text}");
        }
    }

    #[test]
    fn refuses_what_is_not_that_date() {
        for text in [
            "",
            "yesterday",
            "2023-11-14T22:13:20Z",
            "Tue, 14 Nov 2023 22:13:20",
            "Tue, 14 Nov 2023 22:13:20 UTC",
            "Tue, 14 Nov 2023 22:13:20 GMT ",
            "Tue 14 Nov 2023 22:13:20 GMT",
            "Tuesday, 14-Nov-23 22:13:20 GMT",
            "Tue Nov 14 22:13:20 2023",
            "Tue, 14 Nov 1969 22:13:20 GMT",
            "Tue, 30 Feb 2023 22:13:20 GMT",
            "Tue, 29 Feb 2023 22:13:20 GMT",
            "Tue, 00 Nov 2023 22:13:20 GMT",
            "Tue, 14 Nov 2023 24:13:20 GMT",
            "Tue, 14 Nov 2023 22:60:20 GMT",
            "Tue, 14 Nov 2023 22:13:61 GMT",
            "Tue, 14 Nov 2023 22:13 GMT",
            "Tue, 14 Nov 2023 22:13:20:00 GMT",
            "Tue, 14 nov 2023 22:13:20 GMT",
            "Tue, +4 Nov 2023 22:13:20 GMT",
            "Tue, 14 Nov 99999 22:13:20 GMT",
            "XYZ, 14 Nov 2023 22:13:20 GMT",
            "\u{e9}x, 14 Nov 2023 22:13:20 GMT",
            "tue, 14 Nov 2023 22:13:20 GMT",
            "Wed, 14 Nov 2023 22:13:20 GMT",
        ] {
            assert_eq!(http_date(text), None, "{text:?}");
        }
    }

    #[test]
    fn formats_known_instants() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(86_399), "1970-01-01T23:59:59Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc(951_868_800), "2000-03-01T00:00:00Z");
        assert_eq!(utc(1_709_251_199), "2024-02-29T23:59:59Z");
        assert_eq!(utc(1_735_689_600), "2025-01-01T00:00:00Z");
        assert_eq!(utc(4_102_444_799), "2099-12-31T23:59:59Z");
    }
}
