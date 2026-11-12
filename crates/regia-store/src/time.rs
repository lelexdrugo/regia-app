//! Wall-clock instants as the prototype writes and reads them: UTC, ISO 8601, milliseconds.
//!
//! The prototype writes `datetime.isoformat(timespec="milliseconds")` and reads with
//! `datetime.fromisoformat`, whose C implementation in Python 3.12 accepts far more than it writes. A
//! reader that refused a string the prototype accepts would treat a live lock as expired and take it,
//! so [`WallTime::parse_iso`] follows that parser, quirks included, and a golden recorded from the
//! prototype pins the two together.

use std::time::{SystemTime, UNIX_EPOCH};

const MICROS_PER_SECOND: i64 = 1_000_000;
const SECONDS_PER_DAY: i64 = 86_400;
/// Days from 0001-01-01 (Python's ordinal 1) to 1970-01-01.
const UNIX_EPOCH_ORDINAL: i64 = 719_163;
/// 0001-01-01T00:00:00 and 9999-12-31T23:59:59.999999: the range of Python's `datetime`.
const MIN_MICROS: i64 = -62_135_596_800 * MICROS_PER_SECOND;
const MAX_MICROS: i64 = 253_402_300_799 * MICROS_PER_SECOND + 999_999;

/// A UTC instant, in microseconds since the Unix epoch: the precision of Python's `datetime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WallTime(i64);

impl WallTime {
    /// The instant `micros` microseconds after the Unix epoch, if a `datetime` can hold it.
    #[must_use]
    pub fn from_unix_micros(micros: i64) -> Option<Self> {
        (MIN_MICROS..=MAX_MICROS).contains(&micros).then_some(Self(micros))
    }

    #[must_use]
    pub fn unix_micros(self) -> i64 {
        self.0
    }

    /// The system's wall clock.
    ///
    /// # Panics
    ///
    /// If the system clock is set outside the years 1 to 9999.
    #[must_use]
    pub fn now() -> Self {
        let micros = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_micros()).unwrap_or(i64::MAX),
            Err(before) => -i64::try_from(before.duration().as_micros()).unwrap_or(i64::MAX),
        };
        Self::from_unix_micros(micros).unwrap_or_else(|| panic!("the system clock is out of range: {micros} µs"))
    }

    /// `self + timedelta(seconds=secs)`, rounded to the microsecond as Python rounds it, or `None` past
    /// the range of a `datetime`.
    #[must_use]
    pub fn checked_add_secs(self, secs: f64) -> Option<Self> {
        let whole = secs.trunc();
        let micros = whole * 1e6 + ((secs - whole) * 1e6).round_ties_even();
        if !micros.is_finite() || micros.abs() > (MAX_MICROS - MIN_MICROS) as f64 {
            return None;
        }
        // In range by the check above, and integral by construction.
        Self::from_unix_micros(self.0.checked_add(micros as i64)?)
    }

    /// `(later - self).total_seconds()`.
    #[must_use]
    pub fn seconds_until(self, later: Self) -> f64 {
        (later.0 - self.0) as f64 / 1e6
    }

    /// The prototype's format: `2026-11-10T02:47:39.123+00:00`, milliseconds truncated.
    #[must_use]
    pub fn to_iso(self) -> String {
        let (date, time) = self.civil();
        let millis = self.0.rem_euclid(MICROS_PER_SECOND) / 1000;
        format!("{date}T{time}.{millis:03}+00:00")
    }

    /// The instant of a step and of the events cursor: `strftime("%Y-%m-%dT%H:%M:%SZ")` in UTC, the
    /// second truncated, so in doubt more events count as new.
    #[must_use]
    pub fn to_iso_seconds(self) -> String {
        let (date, time) = self.civil();
        format!("{date}T{time}Z")
    }

    /// `datetime.isoformat()` in UTC: the microseconds only when they are not zero, and `+00:00`. It is
    /// the date a step's end records for a report, read back by the precheck to the microsecond.
    #[must_use]
    pub fn to_python_iso(self) -> String {
        let (date, time) = self.civil();
        match self.0.rem_euclid(MICROS_PER_SECOND) {
            0 => format!("{date}T{time}+00:00"),
            micros => format!("{date}T{time}.{micros:06}+00:00"),
        }
    }

    /// The date and the time of day to the second, in UTC.
    fn civil(self) -> (String, String) {
        let seconds = self.0.div_euclid(MICROS_PER_SECOND);
        let (year, month, day) = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY));
        let of_day = seconds.rem_euclid(SECONDS_PER_DAY);
        let (hour, minute, second) = (of_day / 3600, of_day / 60 % 60, of_day % 60);
        (format!("{year:04}-{month:02}-{day:02}"), format!("{hour:02}:{minute:02}:{second:02}"))
    }

    /// What the prototype reads from an expiry: `datetime.fromisoformat(text.replace("Z", "+00:00"))`,
    /// in UTC when the text has no offset. `None` where Python raises.
    #[must_use]
    pub fn parse_iso(text: &str) -> Option<Self> {
        let text = text.replace('Z', "+00:00");
        // Python counts characters here, and bytes from here on.
        if text.chars().count() < 7 {
            return None;
        }
        let parser = Parser { bytes: text.as_bytes() };
        let separator = parser.date_separator()?;
        let (year, month, day) = parser.date(separator)?;
        let mut time = Time::default();
        if parser.bytes.len() > separator {
            // Any one character separates date and time, even a non-ASCII one.
            let width = text.get(separator..)?.chars().next()?.len_utf8();
            time = parser.time(separator + width)?;
        }
        let naive = civil_micros(year, month, day, &time)?;
        Some(Self(naive - time.offset_micros))
    }

    /// The instant `secs` seconds after the epoch, rounded to the microsecond as Python's
    /// `datetime.fromtimestamp` rounds it, or `None` outside the range of a `datetime`.
    #[must_use]
    pub fn from_unix_secs_f64(secs: f64) -> Option<Self> {
        if !secs.is_finite() {
            return None;
        }
        Self(0).checked_add_secs(secs)
    }

    /// Seconds since the epoch, as `datetime.timestamp()` gives them.
    #[must_use]
    pub fn unix_secs_f64(self) -> f64 {
        self.0 as f64 / 1e6
    }

    /// `astimezone().isoformat(timespec=…)` at `offset_s` seconds east of UTC, as the prototype's
    /// probes write a reset or an expiry: `2026-11-11T07:39+01:00` to the minute,
    /// `2026-11-11T07:39:12+01:00` to the second. What is finer is cut, not rounded.
    #[must_use]
    pub fn to_local_iso(self, offset_s: i32, precision: Precision) -> String {
        let local = Local::of(self, offset_s);
        let time = match precision {
            Precision::Minutes => format!("{:02}:{:02}", local.hour, local.minute),
            Precision::Seconds => format!("{:02}:{:02}:{:02}", local.hour, local.minute, local.second),
        };
        format!("{:04}-{:02}-{:02}T{time}{}", local.year, local.month, local.day, offset_text(offset_s))
    }

    /// `astimezone().strftime("%d/%m %H:%M")`: the day and time a reason names, `11/11 15:37`.
    #[must_use]
    pub fn to_local_day_time(self, offset_s: i32) -> String {
        let local = Local::of(self, offset_s);
        format!("{:02}/{:02} {:02}:{:02}", local.day, local.month, local.hour, local.minute)
    }
}

/// How finely [`WallTime::to_local_iso`] writes the time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    Minutes,
    Seconds,
}

/// The civil fields of an instant at an offset.
struct Local {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
}

impl Local {
    fn of(time: WallTime, offset_s: i32) -> Self {
        let seconds = time.0.div_euclid(MICROS_PER_SECOND) + i64::from(offset_s);
        let (year, month, day) = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY));
        let of_day = seconds.rem_euclid(SECONDS_PER_DAY);
        Self { year, month, day, hour: of_day / 3600, minute: of_day / 60 % 60, second: of_day % 60 }
    }
}

/// Python's `_format_offset`: `+01:00`, and the seconds only when there are some.
fn offset_text(offset_s: i32) -> String {
    let sign = if offset_s < 0 { '-' } else { '+' };
    let total = offset_s.unsigned_abs();
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if seconds == 0 { format!("{sign}{hours:02}:{minutes:02}") } else { format!("{sign}{hours:02}:{minutes:02}:{seconds:02}") }
}

#[derive(Debug, Default)]
struct Time {
    hour: i64,
    minute: i64,
    second: i64,
    micros: i64,
    offset_micros: i64,
}

/// A port of `_datetimemodule.c` (Python 3.12): `fromisoformat` and the helpers it calls. Reads past
/// the end see `\0`, as the C code reads its terminator.
struct Parser<'text> {
    bytes: &'text [u8],
}

/// `parse_hh_mm_ss_ff` succeeded; `trailing` is its "not at the end of the string" return value.
struct Clock4 {
    values: [i64; 4],
    trailing: bool,
}

impl Parser<'_> {
    fn at(&self, index: usize) -> u8 {
        self.bytes.get(index).copied().unwrap_or(0)
    }

    fn digits(&self, at: &mut usize, count: usize) -> Option<i64> {
        let mut value = 0;
        for _ in 0..count {
            let byte = self.at(*at);
            *at += 1;
            if !byte.is_ascii_digit() {
                return None;
            }
            value = value * 10 + i64::from(byte - b'0');
        }
        Some(value)
    }

    /// `_find_isoformat_datetime_separator`.
    fn date_separator(&self) -> Option<usize> {
        let len = self.bytes.len();
        if len == 7 {
            return Some(7);
        }
        if self.at(4) == b'-' {
            if self.at(5) != b'W' {
                return Some(10);
            }
            if len < 8 {
                return None;
            }
            if len > 8 && self.at(8) == b'-' {
                if len == 9 {
                    return None;
                }
                if len > 10 && self.at(10).is_ascii_digit() {
                    return Some(8);
                }
                return Some(10);
            }
            return Some(8);
        }
        if self.at(4) != b'W' {
            return Some(8);
        }
        let mut index = 7;
        while index < len && self.at(index).is_ascii_digit() {
            index += 1;
        }
        if index < 9 {
            return Some(index);
        }
        Some(if index % 2 == 0 { 7 } else { 8 })
    }

    /// `parse_isoformat_date`, then the checks of the `datetime` constructor.
    fn date(&self, len: usize) -> Option<(i64, i64, i64)> {
        let mut at = 0;
        let year = self.digits(&mut at, 4)?;
        let dashed = self.at(at) == b'-';
        if dashed {
            at += 1;
        }
        if self.at(at) == b'W' {
            at += 1;
            let week = self.digits(&mut at, 2)?;
            let mut weekday = 1;
            if at < len {
                if dashed {
                    let byte = self.at(at);
                    at += 1;
                    if byte != b'-' {
                        return None;
                    }
                }
                weekday = self.digits(&mut at, 1)?;
            }
            return iso_week_date(year, week, weekday);
        }
        let month = self.digits(&mut at, 2)?;
        if dashed {
            let byte = self.at(at);
            at += 1;
            if byte != b'-' {
                return None;
            }
        }
        let day = self.digits(&mut at, 2)?;
        Some((year, month, day))
    }

    /// `parse_isoformat_time`, from `start` to the end of the text.
    fn time(&self, start: usize) -> Option<Time> {
        let end = self.bytes.len();
        let mut zone = start;
        loop {
            if matches!(self.at(zone), b'Z' | b'+' | b'-') {
                break;
            }
            zone += 1;
            if zone >= end {
                break;
            }
        }
        let clock = self.hh_mm_ss_ff(start, zone)?;
        let [hour, minute, second, micros] = clock.values;
        let mut time = Time { hour, minute, second, micros, offset_micros: 0 };
        if zone == end {
            // No offset, so anything left over is an error.
            return (!clock.trailing).then_some(time);
        }
        // Left over before an offset is ignored, as in the C code.
        if self.at(zone) == b'Z' {
            return (self.at(zone + 1) == 0).then_some(time);
        }
        let sign = if self.at(zone) == b'-' { -1 } else { 1 };
        let offset = self.hh_mm_ss_ff(zone + 1, end)?;
        if offset.trailing {
            return None;
        }
        let [hours, minutes, seconds, offset_micros] = offset.values;
        let offset_seconds = sign * (hours * 3600 + minutes * 60 + seconds);
        // A zero offset in seconds is UTC: the C code drops its microseconds.
        if offset_seconds != 0 {
            let total = offset_seconds * MICROS_PER_SECOND + sign * offset_micros;
            if total.abs() >= SECONDS_PER_DAY * MICROS_PER_SECOND {
                return None;
            }
            time.offset_micros = total;
        }
        Some(time)
    }

    /// `parse_hh_mm_ss_ff` between `start` and `end`.
    fn hh_mm_ss_ff(&self, start: usize, end: usize) -> Option<Clock4> {
        const CORRECTION: [i64; 5] = [100_000, 10_000, 1000, 100, 10];
        let mut values = [0; 4];
        let mut at = start;
        let mut separated = true;
        for index in 0..3 {
            values[index] = self.digits(&mut at, 2)?;
            let byte = self.at(at);
            at += 1;
            if index == 0 {
                separated = byte == b':';
            }
            if at >= end {
                return Some(Clock4 { values, trailing: byte != 0 });
            } else if separated && byte == b':' {
                continue;
            } else if byte == b'.' || byte == b',' {
                break;
            } else if !separated {
                at -= 1;
            } else {
                return None;
            }
        }
        // The fraction, also after a third field that ended in `:` or in a digit, as in the C code.
        let count = (end - at).min(6);
        let mut micros = self.digits(&mut at, count)?;
        if count < 6 {
            micros *= CORRECTION[count - 1];
        }
        values[3] = micros;
        while self.at(at).is_ascii_digit() {
            at += 1;
        }
        Some(Clock4 { values, trailing: self.at(at) != 0 })
    }
}

/// `iso_to_ymd`: the Monday-based ISO week date to a civil date.
fn iso_week_date(year: i64, week: i64, weekday: i64) -> Option<(i64, i64, i64)> {
    if !(1..=9999).contains(&year) {
        return None;
    }
    let first_day = ordinal(year, 1, 1);
    // 0 for a Monday.
    let first_weekday = (first_day + 6) % 7;
    if !(1..53).contains(&week) {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        if week != 53 || !(first_weekday == 3 || (first_weekday == 2 && leap)) {
            return None;
        }
    }
    if !(1..8).contains(&weekday) {
        return None;
    }
    let mut monday = first_day - first_weekday;
    if first_weekday > 3 {
        monday += 7;
    }
    let (y, m, d) = civil_from_days(monday + (week - 1) * 7 + weekday - 1 - UNIX_EPOCH_ORDINAL);
    Some((y, m, d))
}

/// Python's proleptic Gregorian ordinal: 0001-01-01 is 1.
fn ordinal(year: i64, month: i64, day: i64) -> i64 {
    days_from_civil(year, month, day) + UNIX_EPOCH_ORDINAL
}

/// The checks of Python's `datetime` constructor, then the instant as if it were UTC.
fn civil_micros(year: i64, month: i64, day: i64, time: &Time) -> Option<i64> {
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    if time.hour > 23 || time.minute > 59 || time.second > 59 {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * SECONDS_PER_DAY + time.hour * 3600 + time.minute * 60 + time.second;
    Some(seconds * MICROS_PER_SECOND + time.micros)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn micros(text: &str) -> Option<i64> {
        WallTime::parse_iso(text).map(WallTime::unix_micros)
    }

    #[test]
    fn writes_what_the_prototype_writes() {
        assert_eq!(WallTime(1_794_278_859_123_456).to_iso(), "2026-11-10T02:47:39.123+00:00");
        assert_eq!(WallTime(-1000).to_iso(), "1969-12-31T23:59:59.999+00:00");
        assert_eq!(WallTime(MIN_MICROS).to_iso(), "0001-01-01T00:00:00.000+00:00");
        assert_eq!(WallTime(MAX_MICROS).to_iso(), "9999-12-31T23:59:59.999+00:00");
    }

    /// `datetime.fromtimestamp(ts, timezone.utc).isoformat()` on Python 3.12, for the date a step's end
    /// records for a report: the timestamps as Python's `repr` prints them, so each is the double it read.
    #[test]
    fn a_file_s_date_is_written_as_python_writes_it() {
        let vectors = [
            (1_768_032_000.0, "2026-01-10T08:00:00+00:00"),
            (1_768_032_000.25, "2026-01-10T08:00:00.250000+00:00"),
            (1_768_032_000.123_456_5, "2026-01-10T08:00:00.123456+00:00"),
            (1_768_032_000.000_000_5, "2026-01-10T08:00:00+00:00"),
            (1_768_032_000.000_001_4, "2026-01-10T08:00:00.000001+00:00"),
            (1_768_032_000.999_999_5, "2026-01-10T08:00:01+00:00"),
            (1_762_918_212.345_678_8, "2025-11-12T03:30:12.345679+00:00"),
        ];
        for (timestamp, python) in vectors {
            assert_eq!(WallTime::from_unix_secs_f64(timestamp).map(WallTime::to_python_iso).as_deref(), Some(python), "{timestamp}");
        }
        // The instant of a step: to the second, truncated.
        assert_eq!(WallTime(1_768_033_800_999_999).to_iso_seconds(), "2026-01-10T08:30:00Z");
    }

    #[test]
    fn reads_back_what_it_writes() {
        for value in [0, -1000, 1_794_278_859_123_000, MIN_MICROS, MAX_MICROS - 999] {
            let time = WallTime(value);
            assert_eq!(micros(&time.to_iso()), Some(value), "{}", time.to_iso());
        }
    }

    #[test]
    fn civil_dates_round_trip_over_the_whole_range() {
        for days in (days_from_civil(1, 1, 1)..=days_from_civil(9999, 12, 31)).step_by(997) {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
        }
    }

    #[test]
    fn adds_seconds_as_timedelta_does() {
        let start = WallTime(1_000_000);
        assert_eq!(start.checked_add_secs(0.05).map(WallTime::unix_micros), Some(1_050_000));
        assert_eq!(start.checked_add_secs(600.0).map(WallTime::unix_micros), Some(601_000_000));
        assert_eq!(start.checked_add_secs(1e300), None);
        assert_eq!(WallTime(MAX_MICROS).checked_add_secs(1.0), None);
    }

    /// The common forms; the full vector comes from the prototype (`store_golden.rs`).
    #[test]
    fn reads_the_forms_the_prototype_reads() {
        assert_eq!(micros("2026-11-10T02:47:39.123+00:00"), Some(1_794_278_859_123_000));
        assert_eq!(micros("2026-11-10T02:47:39Z"), Some(1_794_278_859_000_000));
        assert_eq!(micros("2026-11-10T02:47:39"), Some(1_794_278_859_000_000));
        assert_eq!(micros("2026-11-10T02:47:39-05:30"), Some(1_794_298_659_000_000));
        assert_eq!(micros("2026-W45-2"), Some(1_793_664_000_000_000));
        assert_eq!(micros("2026-11-10T02:47.5"), Some(1_794_278_820_500_000));
        assert_eq!(micros("2026-11-10T02:47:39z"), None);
        assert_eq!(micros("2026-11-10T24:00:00"), None);
        assert_eq!(micros(" 2026-11-10T02:47:39"), None);
    }

    /// As `datetime.fromtimestamp(t, tz).isoformat(timespec=…)`, recorded from Python 3.12.
    #[test]
    fn writes_local_times_as_the_probes_do() {
        let reset = WallTime::from_unix_secs_f64(1_794_380_399.999_999_7).expect("in range");
        assert_eq!(reset.unix_micros(), 1_794_380_400_000_000, "rounded to the microsecond, half to even");
        assert_eq!(reset.to_local_iso(3600, Precision::Minutes), "2026-11-11T08:00+01:00");
        let expiry = WallTime::from_unix_secs_f64(1_794_407_822.75).expect("in range");
        assert_eq!(expiry.to_local_iso(3600, Precision::Seconds), "2026-11-11T15:37:02+01:00");
        assert_eq!(expiry.to_local_iso(0, Precision::Seconds), "2026-11-11T14:37:02+00:00");
        assert_eq!(expiry.to_local_iso(-5 * 3600 - 1800, Precision::Minutes), "2026-11-11T09:07-05:30");
        assert_eq!(expiry.to_local_iso(3600 + 30, Precision::Minutes), "2026-11-11T15:37+01:00:30");
        assert_eq!(expiry.to_local_day_time(3600), "11/11 15:37");
        assert_eq!(WallTime::parse_iso(&expiry.to_local_iso(3600, Precision::Seconds)).map(WallTime::unix_micros), Some(1_794_407_822_000_000));
        assert_eq!(WallTime::from_unix_secs_f64(f64::NAN), None);
        assert_eq!(WallTime::from_unix_secs_f64(1e20), None);
    }
}
