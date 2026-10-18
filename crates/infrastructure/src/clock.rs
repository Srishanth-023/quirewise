//! Wall clock adapter.

use application::ports::Clock;
use domain::naming::DateTimeParts;
use time::OffsetDateTime;

/// Local time from the operating system, falling back to UTC when the offset is unavailable.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTimeParts {
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        DateTimeParts {
            year: u16::try_from(now.year()).unwrap_or(1970),
            month: u8::from(now.month()),
            day: now.day(),
            hour: now.hour(),
            minute: now.minute(),
            second: now.second(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_plausible_date() {
        let t = SystemClock.now();
        assert!(t.year >= 2026);
        assert!((1..=12).contains(&t.month));
        assert!((1..=31).contains(&t.day));
        assert!(t.hour < 24 && t.minute < 60 && t.second < 61);
    }
}
