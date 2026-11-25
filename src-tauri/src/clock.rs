//! Timestamps stored in the database: RFC 3339, UTC, millisecond precision.

use time::OffsetDateTime;
use time::macros::format_description;

/// The current time as `2026-11-19T21:00:00.123Z`.
pub fn now() -> String {
    format(OffsetDateTime::now_utc())
}

/// Formats a UTC instant the way the database stores it.
pub fn format(instant: OffsetDateTime) -> String {
    let description =
        format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");
    instant
        .to_offset(time::UtcOffset::UTC)
        .format(&description)
        .expect("a UTC timestamp always formats")
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn formats_utc_with_milliseconds() {
        assert_eq!(
            format(datetime!(2026-11-19 21:05:09.123_456 UTC)),
            "2026-11-19T21:05:09.123Z"
        );
        assert_eq!(
            format(datetime!(2026-11-19 18:05:09 -3)),
            "2026-11-19T21:05:09.000Z"
        );
    }
}
