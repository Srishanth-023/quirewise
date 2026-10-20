//! Dates: seconds since 1970 to text and back, without a calendar library.

/// Year, month, day, hour, minute and second in UTC.
pub type Civil = (i64, u32, u32, u32, u32, u32);

/// Splits seconds since 1970 UTC into calendar parts.
pub fn civil(unix: i64) -> Civil {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (
        year,
        m,
        d,
        (secs / 3600) as u32,
        (secs % 3600 / 60) as u32,
        (secs % 60) as u32,
    )
}

/// Seconds since 1970 UTC of calendar parts.
pub fn unix_of(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from((month + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second)
}

/// `D:YYYYMMDDHHmmSS+00'00'`, the date of a PDF dictionary.
pub fn pdf_date(unix: i64) -> String {
    let (y, mo, d, h, mi, s) = civil(unix);
    format!("D:{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}+00'00'")
}

/// `YYYY-MM-DD HH:MM:SS UTC`.
pub fn utc(unix: i64) -> String {
    let (y, mo, d, h, mi, s) = civil(unix);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}

/// `YYYY.MM.DD HH:MM:SS UTC`, the way signature appearances show the time.
pub fn signature_date(unix: i64) -> String {
    let (y, mo, d, h, mi, s) = civil(unix);
    format!("{y:04}.{mo:02}.{d:02} {h:02}:{mi:02}:{s:02} UTC")
}

/// `YYYY-MM-DD`.
pub fn day(unix: i64) -> String {
    let (y, mo, d, ..) = civil(unix);
    format!("{y:04}-{mo:02}-{d:02}")
}

/// Reads a PDF date such as `D:20261020120000+02'00'` or `D:20261020120000Z`.
pub fn parse_pdf_date(text: &str) -> Option<i64> {
    let body = text.strip_prefix("D:").unwrap_or(text);
    let digits =
        |from: usize, len: usize| -> Option<u32> { body.get(from..from + len)?.parse().ok() };
    let year = i64::from(digits(0, 4)?);
    let month = digits(4, 2).unwrap_or(1);
    let day = digits(6, 2).unwrap_or(1);
    let hour = digits(8, 2).unwrap_or(0);
    let minute = digits(10, 2).unwrap_or(0);
    let second = digits(12, 2).unwrap_or(0);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let local = unix_of(year, month, day, hour, minute, second.min(59));
    let rest = body.get(14..).unwrap_or("");
    let offset = match rest.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let numbers: String = rest[1..].chars().filter(char::is_ascii_digit).collect();
            let hours: i64 = numbers.get(0..2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let minutes: i64 = numbers.get(2..4).and_then(|s| s.parse().ok()).unwrap_or(0);
            let total = hours * 3600 + minutes * 60;
            if sign == '+' { total } else { -total }
        }
        _ => 0,
    };
    Some(local - offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_dates_convert_both_ways() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil(951_782_400), (2000, 2, 29, 0, 0, 0), "a leap day");
        assert_eq!(civil(1_800_000_000), (2027, 1, 15, 8, 0, 0));
        for t in [
            0,
            86_399,
            951_782_400,
            1_234_567_890,
            1_800_000_000,
            4_102_444_800,
        ] {
            let (y, mo, d, h, mi, s) = civil(t);
            assert_eq!(unix_of(y, mo, d, h, mi, s), t, "{t}");
        }
    }

    #[test]
    fn dates_are_formatted() {
        assert_eq!(pdf_date(1_800_000_000), "D:20270115080000+00'00'");
        assert_eq!(utc(1_800_000_000), "2027-01-15 08:00:00 UTC");
        assert_eq!(signature_date(1_800_000_000), "2027.01.15 08:00:00 UTC");
        assert_eq!(day(1_800_000_000), "2027-01-15");
    }

    #[test]
    fn pdf_dates_are_read_with_their_offsets() {
        assert_eq!(
            parse_pdf_date("D:20270115080000+00'00'"),
            Some(1_800_000_000)
        );
        assert_eq!(
            parse_pdf_date("D:20270115100000+02'00'"),
            Some(1_800_000_000)
        );
        assert_eq!(parse_pdf_date("D:20270115080000Z"), Some(1_800_000_000));
        assert_eq!(
            parse_pdf_date("D:20270114230000-09'00'"),
            Some(1_800_000_000)
        );
        assert_eq!(parse_pdf_date("D:2027"), Some(unix_of(2027, 1, 1, 0, 0, 0)));
        assert_eq!(parse_pdf_date("garbage"), None);
        assert_eq!(parse_pdf_date("D:20271315000000"), None);
    }
}
