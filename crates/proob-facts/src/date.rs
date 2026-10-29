//! Calendar dates as TOML writes them (`2026-12-31`, a bare date with no time
//! and no offset), and their conversion to and from seconds since the Unix
//! epoch at 00:00:00 UTC; and the RFC 3339 timestamps GitHub writes
//! (`2026-10-28T12:34:56Z`), to seconds since the epoch.
//!
//! A `Date` exists only between 1970-01-01 and 9999-12-31 in the proleptic
//! Gregorian calendar, so `to_secs` is total.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

pub const SECS_PER_DAY: u64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateError {
    /// Not a day of the calendar within 1970-01-01 and 9999-12-31.
    OutOfRange { year: u16, month: u8, day: u8 },
    /// A TOML datetime with a time or an offset, or a time alone.
    NotABareDate,
    /// Not `YYYY-MM-DDThh:mm:ss` followed by `Z` or `±hh:mm` (fractional
    /// seconds allowed).
    NotATimestamp,
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DateError::OutOfRange { year, month, day } => {
                write!(f, "{year:04}-{month:02}-{day:02} is not a date between 1970-01-01 and 9999-12-31")
            }
            DateError::NotABareDate => write!(f, "expected a bare date such as 2026-12-31, without time or offset"),
            DateError::NotATimestamp => {
                write!(f, "expected an RFC 3339 timestamp such as 2026-10-28T12:34:56Z")
            }
        }
    }
}

impl std::error::Error for DateError {}

fn is_leap(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

pub fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    pub const EPOCH: Date = Date { year: 1970, month: 1, day: 1 };

    pub fn new(year: u16, month: u8, day: u8) -> Result<Date, DateError> {
        let in_range = (1970..=9999).contains(&year)
            && (1..=12).contains(&month)
            && (1..=days_in_month(year, month)).contains(&day);
        if in_range { Ok(Date { year, month, day }) } else { Err(DateError::OutOfRange { year, month, day }) }
    }

    pub fn year(self) -> u16 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }

    /// Seconds since the epoch at this date's 00:00:00 UTC.
    pub fn to_secs(self) -> u64 {
        days_from_civil(self) * SECS_PER_DAY
    }

    /// The date containing an instant; `None` past 9999-12-31.
    pub fn from_secs(secs: u64) -> Option<Date> {
        civil_from_days(secs / SECS_PER_DAY)
    }
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm). Unsigned throughout:
/// `Date::new` keeps the year at or after 1970, so the final subtraction is
/// exact and the saturation never applies.
fn days_from_civil(d: Date) -> u64 {
    let y = u64::from(d.year) - u64::from(d.month <= 2);
    let era = y / 400;
    let yoe = y - era * 400;
    let m = u64::from(d.month);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + u64::from(d.day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe).saturating_sub(719_468)
}

/// The inverse of `days_from_civil`; `None` past 9999-12-31.
fn civil_from_days(days: u64) -> Option<Date> {
    let z = days.checked_add(719_468)?;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    Date::new(u16::try_from(year).ok()?, u8::try_from(month).ok()?, u8::try_from(day).ok()?).ok()
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl TryFrom<toml::value::Datetime> for Date {
    type Error = DateError;

    fn try_from(dt: toml::value::Datetime) -> Result<Date, DateError> {
        match (dt.date, dt.time, dt.offset) {
            (Some(d), None, None) => Date::new(d.year, d.month, d.day),
            _ => Err(DateError::NotABareDate),
        }
    }
}

impl From<Date> for toml::value::Datetime {
    fn from(d: Date) -> Self {
        toml::value::Datetime {
            date: Some(toml::value::Date { year: d.year, month: d.month, day: d.day }),
            time: None,
            offset: None,
        }
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        toml::value::Datetime::from(*self).serialize(s)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Date, D::Error> {
        let dt = toml::value::Datetime::deserialize(d)?;
        Date::try_from(dt).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Timestamps
// ---------------------------------------------------------------------------

fn digits(text: &str, range: std::ops::Range<usize>) -> Option<u32> {
    let part = text.get(range)?;
    if part.bytes().all(|b| b.is_ascii_digit()) { part.parse().ok() } else { None }
}

fn at(text: &str, index: usize, byte: u8) -> bool {
    text.as_bytes().get(index) == Some(&byte)
}

/// The offset in seconds named by `Z`, `z`, or `±hh:mm`, as a signed value.
fn offset_secs(tail: &str) -> Option<i64> {
    match tail.as_bytes().first()? {
        b'Z' | b'z' if tail.len() == 1 => Some(0),
        sign @ (b'+' | b'-') if tail.len() == 6 && at(tail, 3, b':') => {
            let secs = i64::from(digits(tail, 1..3)?) * 3600 + i64::from(digits(tail, 4..6)?) * 60;
            Some(if *sign == b'-' { -secs } else { secs })
        }
        _ => None,
    }
}

/// Seconds since the epoch of an RFC 3339 timestamp. Fractional seconds are
/// dropped; an offset is applied; the result is clamped at the epoch.
pub fn parse_timestamp(text: &str) -> Result<u64, DateError> {
    let parsed = (|| {
        let date_ok = at(text, 4, b'-') && at(text, 7, b'-') && (at(text, 10, b'T') || at(text, 10, b't'));
        let time_ok = at(text, 13, b':') && at(text, 16, b':');
        if !(date_ok && time_ok) {
            return None;
        }
        let date = Date::new(
            u16::try_from(digits(text, 0..4)?).ok()?,
            u8::try_from(digits(text, 5..7)?).ok()?,
            u8::try_from(digits(text, 8..10)?).ok()?,
        )
        .ok()?;
        let (h, m, s) = (digits(text, 11..13)?, digits(text, 14..16)?, digits(text, 17..19)?);
        if h > 23 || m > 59 || s > 60 {
            return None;
        }
        let rest = text.get(19..)?;
        let tail = match rest.as_bytes().first() {
            Some(b'.') => rest.get(1 + rest[1..].bytes().take_while(u8::is_ascii_digit).count()..)?,
            _ => rest,
        };
        let offset = offset_secs(tail)?;
        let local = i64::try_from(date.to_secs()).ok()? + i64::from(h) * 3600 + i64::from(m) * 60 + i64::from(s);
        Some(u64::try_from(local - offset).unwrap_or(0))
    })();
    parsed.ok_or(DateError::NotATimestamp)
}
