/// Convert days since Unix epoch (1970-01-01) to a YYYY-MM-DD date string.
/// Uses the civil date algorithm (Howard Hinnant).
pub fn epoch_days_to_date(total_days: u64) -> String {
    let z = total_days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Task 8: epoch_days_to_date tests
    #[test]
    fn test_epoch_days_zero() {
        // Unix epoch start: 1970-01-01
        assert_eq!(epoch_days_to_date(0), "1970-01-01");
    }

    #[test]
    fn test_epoch_days_one() {
        // 1970-01-02
        assert_eq!(epoch_days_to_date(1), "1970-01-02");
    }

    #[test]
    fn test_epoch_days_1971() {
        // 1970 had 365 days, so day 365 = 1971-01-01
        assert_eq!(epoch_days_to_date(365), "1971-01-01");
    }

    #[test]
    fn test_epoch_days_leap_1972() {
        // 1972 was a leap year, day 730 = 1972-01-01
        assert_eq!(epoch_days_to_date(730), "1972-01-01");
    }

    #[test]
    fn test_epoch_days_millennium() {
        // 2000-01-01 (millennium)
        // Days from 1970-01-01 to 2000-01-01 = 10957
        assert_eq!(epoch_days_to_date(10957), "2000-01-01");
    }

    #[test]
    fn test_epoch_days_2024_leap() {
        // 2024 is a leap year, 2024-01-01
        assert_eq!(epoch_days_to_date(19723), "2024-01-01");
    }

    #[test]
    fn test_epoch_days_today() {
        // 2026-05-01
        assert_eq!(epoch_days_to_date(20574), "2026-05-01");
    }

    #[test]
    fn test_epoch_days_far_future() {
        // Far future - just verify it produces a valid date string
        let result = epoch_days_to_date(50000);
        assert!(result.len() == 10); // YYYY-MM-DD format
        assert!(result.starts_with("20") || result.starts_with("21"));
    }
}
