// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to
// deal in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
// sell copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
// IN THE SOFTWARE.

// ----------------------------------------------------------------------------

//! Native ISO date parsing and route-date formatting.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Normalized post date with a UTC ordering key and original civil fields.
#[derive(
    Clone,
    Copy,
    Debug,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub struct BlogDate {
    /// UTC ordering key with microsecond precision.
    timestamp_micros: i64,
    /// Original civil year.
    year: i32,
    /// Original civil month in the range 1 through 12.
    month: u8,
    /// Original civil day of the month.
    day: u8,
    /// Original hour in the range 0 through 23.
    hour: u8,
    /// Original minute in the range 0 through 59.
    minute: u8,
    /// Original second in the range 0 through 59.
    second: u8,
    /// Original fractional second normalized to microseconds.
    microsecond: u32,
    /// Original UTC offset in minutes.
    offset_minutes: i16,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl BlogDate {
    /// Parses a YAML-compatible ISO date or datetime.
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        let value = value.trim();
        let (date, time) = value
            .split_once(['T', ' '])
            .map_or((value, None), |(date, time)| (date, Some(time)));
        let (year, month, day) = parse_date(date)?;
        let (hour, minute, second, microsecond, offset_minutes) =
            time.map(parse_time).transpose()?.unwrap_or((0, 0, 0, 0, 0));
        validate_date(year, month, day)?;
        let timestamp = days_from_civil(year, month, day) * 86_400
            + i64::from(hour) * 3_600
            + i64::from(minute) * 60
            + i64::from(second)
            - i64::from(offset_minutes) * 60;
        let timestamp_micros = timestamp * 1_000_000 + i64::from(microsecond);
        Ok(Self {
            timestamp_micros,
            year,
            month,
            day,
            hour,
            minute,
            second,
            microsecond,
            offset_minutes,
        })
    }

    /// Returns the UTC ordering key in microseconds.
    pub const fn timestamp_micros(self) -> i64 {
        self.timestamp_micros
    }

    /// Returns Material's string representation of a timezone-aware datetime.
    pub fn template_value(self) -> String {
        let sign = if self.offset_minutes < 0 { '-' } else { '+' };
        let minutes = self.offset_minutes.unsigned_abs();
        let offset = format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60);
        let fraction = if self.microsecond == 0 {
            String::new()
        } else {
            format!(".{:06}", self.microsecond)
        };
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}{fraction}{offset}",
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second
        )
    }

    /// Formats Material's default English `long` display date.
    pub fn long_en(self) -> String {
        format!(
            "{} {}, {}",
            month_name(self.month, false),
            self.day,
            self.year
        )
    }

    /// Formats a Material/Babel date pattern using native English names.
    pub fn format_display(self, pattern: &str) -> anyhow::Result<String> {
        match pattern {
            "full" => Ok(format!(
                "{}, {} {}, {}",
                weekday_name(self, false),
                month_name(self.month, false),
                self.day,
                self.year
            )),
            "long" => Ok(self.long_en()),
            "medium" => Ok(format!(
                "{} {}, {}",
                month_name(self.month, true),
                self.day,
                self.year
            )),
            "short" => Ok(format!(
                "{}/{}/{:02}",
                self.month,
                self.day,
                self.year.rem_euclid(100)
            )),
            pattern => self.format_pattern(pattern),
        }
    }

    /// Formats the subset of Unicode date patterns used in blog URL defaults.
    pub fn format_url(self, pattern: &str) -> anyhow::Result<String> {
        self.format_pattern(pattern)
    }

    fn format_pattern(self, pattern: &str) -> anyhow::Result<String> {
        let mut output = String::new();
        let mut chars = pattern.chars().peekable();
        let mut quoted = false;
        while let Some(character) = chars.next() {
            if character == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    output.push('\'');
                } else {
                    quoted = !quoted;
                }
                continue;
            }
            if quoted
                || !matches!(
                    character,
                    'y' | 'M' | 'd' | 'E' | 'H' | 'h' | 'm' | 's' | 'a'
                )
            {
                output.push(character);
                continue;
            }
            let mut width = 1;
            while chars.peek() == Some(&character) {
                chars.next();
                width += 1;
            }
            match character {
                'y' if width == 2 => {
                    write!(&mut output, "{:02}", self.year.rem_euclid(100))
                        .expect("writing to a string cannot fail");
                }
                'y' => write!(&mut output, "{:0width$}", self.year)
                    .expect("writing to a string cannot fail"),
                'M' if width <= 2 => {
                    write!(&mut output, "{:0width$}", self.month)
                        .expect("writing to a string cannot fail");
                }
                'M' if width == 3 => output.push_str(month_name(self.month, true)),
                'M' if width == 4 => output.push_str(month_name(self.month, false)),
                'd' if width <= 2 => {
                    write!(&mut output, "{:0width$}", self.day)
                        .expect("writing to a string cannot fail");
                }
                'E' if width <= 3 => output.push_str(weekday_name(self, true)),
                'E' if width == 4 => output.push_str(weekday_name(self, false)),
                'H' if width <= 2 => {
                    write!(&mut output, "{:0width$}", self.hour)
                        .expect("writing to a string cannot fail");
                }
                'h' if width <= 2 => {
                    let hour = match self.hour % 12 {
                        0 => 12,
                        hour => hour,
                    };
                    write!(&mut output, "{hour:0width$}")
                        .expect("writing to a string cannot fail");
                }
                'm' if width <= 2 => {
                    write!(&mut output, "{:0width$}", self.minute)
                        .expect("writing to a string cannot fail");
                }
                's' if width <= 2 => {
                    write!(&mut output, "{:0width$}", self.second)
                        .expect("writing to a string cannot fail");
                }
                'a' => output.push_str(if self.hour < 12 { "AM" } else { "PM" }),
                _ => bail!(
                    "unsupported date field '{character}' in URL format '{pattern}'"
                ),
            }
        }
        if quoted {
            bail!("unterminated quote in URL date format '{pattern}'")
        }
        Ok(output)
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

fn month_name(month: u8, short: bool) -> &'static str {
    const LONG: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const SHORT: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct",
        "Nov", "Dec",
    ];
    let index = usize::from(month - 1);
    if short {
        SHORT[index]
    } else {
        LONG[index]
    }
}

fn weekday_name(date: BlogDate, short: bool) -> &'static str {
    const LONG: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    const SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let days = days_from_civil(date.year, date.month, date.day);
    let index =
        usize::try_from((days + 4).rem_euclid(7)).expect("weekday range");
    if short {
        SHORT[index]
    } else {
        LONG[index]
    }
}

fn parse_date(value: &str) -> anyhow::Result<(i32, u8, u8)> {
    let mut parts = value.split('-');
    let year = number(parts.next(), "year")?;
    let month = number(parts.next(), "month")?;
    let day = number(parts.next(), "day")?;
    if parts.next().is_some() {
        bail!("date must use YYYY-MM-DD syntax")
    }
    Ok((year, month, day))
}

fn parse_time(value: &str) -> anyhow::Result<(u8, u8, u8, u32, i16)> {
    let (time, offset) = if let Some(time) = value.strip_suffix(['Z', 'z']) {
        (time, 0)
    } else {
        let at = value.char_indices().skip(1).find_map(|(index, character)| {
            matches!(character, '+' | '-').then_some(index)
        });
        match at {
            Some(at) => {
                let sign = if value.as_bytes()[at] == b'-' { -1 } else { 1 };
                let (time, zone) = value.split_at(at);
                let zone = &zone[1..];
                let (hours, minutes) = zone
                    .split_once(':')
                    .context("timezone must use +HH:MM syntax")?;
                let hours =
                    hours.parse::<i16>().context("invalid timezone hour")?;
                let minutes = minutes
                    .parse::<i16>()
                    .context("invalid timezone minute")?;
                if hours > 23 || minutes > 59 {
                    bail!("timezone offset is out of range")
                }
                (time, sign * (hours * 60 + minutes))
            }
            None => (value, 0),
        }
    };
    let (time, microsecond) = match time.split_once('.') {
        Some((time, fraction)) => (time, parse_fraction(fraction)?),
        None => (time, 0),
    };
    let mut parts = time.split(':');
    let hour = number(parts.next(), "hour")?;
    let minute = number(parts.next(), "minute")?;
    let second = parts
        .next()
        .map(|value| value.parse::<u8>().context("invalid second"))
        .transpose()?
        .unwrap_or(0);
    if parts.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        bail!("time is out of range")
    }
    Ok((hour, minute, second, microsecond, offset))
}

fn parse_fraction(value: &str) -> anyhow::Result<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("invalid fractional second")
    }
    let digits = &value[..value.len().min(6)];
    let fraction =
        digits.parse::<u32>().context("invalid fractional second")?;
    Ok(fraction * 10_u32.pow(u32::try_from(6 - digits.len())?))
}

fn number<T>(value: Option<&str>, name: &str) -> anyhow::Result<T>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    value
        .context(format!("missing {name}"))?
        .parse()
        .with_context(|| format!("invalid {name}"))
}

fn validate_date(year: i32, month: u8, day: u8) -> anyhow::Result<()> {
    if !(1..=9999).contains(&year) {
        bail!("year is out of range")
    }
    let leap = year.rem_euclid(4) == 0
        && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => bail!("month is out of range"),
    };
    if day == 0 || day > days {
        bail!("day is out of range")
    }
    Ok(())
}

// Howard Hinnant's proleptic Gregorian civil-date conversion.
fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = i32::from(month);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5
        + i32::from(day)
        - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    i64::from(era * 146_097 + doe - 719_468)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::BlogDate;

    #[test]
    fn parses_dates_datetimes_and_offsets() {
        let date = BlogDate::parse("2026-09-03").unwrap();
        assert_eq!(date.format_url("yyyy/MM/dd").unwrap(), "2026/09/03");
        let utc = BlogDate::parse("2026-09-03T10:30:00Z").unwrap();
        let offset = BlogDate::parse("2026-09-03 12:30:00+02:00").unwrap();
        assert_eq!(utc.timestamp_micros(), offset.timestamp_micros());
    }

    #[test]
    fn preserves_material_fractional_second_precision() {
        let short = BlogDate::parse("2026-09-03T10:30:00.1Z").unwrap();
        let long = BlogDate::parse("2026-09-03T10:30:00.123456789Z").unwrap();
        assert_eq!(short.template_value(), "2026-09-03 10:30:00.100000+00:00");
        assert_eq!(long.template_value(), "2026-09-03 10:30:00.123456+00:00");
        assert!(long.timestamp_micros() > short.timestamp_micros());
    }

    #[test]
    fn validates_leap_days_and_url_patterns() {
        assert!(BlogDate::parse("2024-02-29").is_ok());
        assert!(BlogDate::parse("2025-02-29").is_err());
        let date = BlogDate::parse("2026-09-03").unwrap();
        assert_eq!(date.format_url("yy-M-d").unwrap(), "26-9-3");
        assert_eq!(date.format_url("yyyy'year'MM").unwrap(), "2026year09");
        assert_eq!(
            date.format_display("MMMM d, yyyy").unwrap(),
            "September 3, 2026"
        );
        assert_eq!(
            date.format_display("full").unwrap(),
            "Thursday, September 3, 2026"
        );
        let time = BlogDate::parse("2026-09-03T14:05:09Z").unwrap();
        assert_eq!(
            time.format_display("MMM d, yyyy h:mm a").unwrap(),
            "Sep 3, 2026 2:05 PM"
        );
    }
}
