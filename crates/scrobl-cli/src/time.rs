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

#[cfg(test)]
mod tests {
    use super::utc;

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
