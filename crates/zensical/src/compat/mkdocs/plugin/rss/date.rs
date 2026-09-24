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

//! Feed dates from metadata, Git history, and the current feed build.

use anyhow::{Context, Result};
use jiff::civil::{Date, DateTime, Time};
use jiff::fmt::temporal::DateTimeParser;
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use crate::config::plugins::RssDateConfig;
use crate::structure::dynamic::Dynamic;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Comparable time and both feed serializations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    /// Unix timestamp used for ordering.
    pub epoch: i64,
    /// JSON Feed date with an explicit offset.
    pub json: String,
    /// RFC 822 date used by RSS.
    pub rss: String,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Stamp {
    /// Creates both feed representations for a zoned instant.
    fn new(zoned: &Zoned) -> Self {
        Self {
            epoch: zoned.timestamp().as_second(),
            json: zoned.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string(),
            rss: zoned.strftime("%a, %d %b %Y %H:%M:%S %z").to_string(),
        }
    }

    /// Captures the current instant in UTC.
    pub fn now() -> Self {
        Self::new(&Timestamp::now().in_tz("UTC").expect("UTC exists"))
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Returns latest and earliest commits for one physical source.
pub fn git_dates(root: &Path, source: &Path) -> Option<(i64, i64)> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["log", "--follow", "--format=%at", "--"])
        .arg(source)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let values = String::from_utf8(output.stdout).ok()?;
    let mut commits =
        values.lines().filter_map(|value| value.parse::<i64>().ok());
    let updated = commits.next()?;
    let created = commits.next_back().unwrap_or(updated);
    Some((created, updated))
}

/// Resolves one feed date, preserving metadata precedence.
/// An absent date uses the feed build time during ranking and rendering.
pub fn resolve(
    meta: &BTreeMap<String, Dynamic>, key: &str, settings: &RssDateConfig,
    git: Option<i64>,
) -> Result<Option<Stamp>> {
    if key != "git"
        && let Some(value) = dot_key(meta, key)
        && let Some(date) = metadata_date(value, settings)?
    {
        return Ok(Some(date));
    }
    if let Some(seconds) = git {
        let timestamp = Timestamp::from_second(seconds)?;
        let zone = &settings.default_timezone;
        return Ok(Some(Stamp::new(&timestamp.in_tz(zone)?)));
    }
    Ok(None)
}

/// Follows a dotted metadata key through nested maps.
fn dot_key<'a>(
    meta: &'a BTreeMap<String, Dynamic>, key: &str,
) -> Option<&'a Dynamic> {
    let mut parts = key.split('.');
    let mut value = meta.get(parts.next()?)?;
    for part in parts {
        value = match value {
            Dynamic::Map(values) => values.get(part)?,
            _ => return None,
        };
    }
    Some(value)
}

/// Parses a metadata date using the configured format and fallback time.
fn metadata_date(
    value: &Dynamic, settings: &RssDateConfig,
) -> Result<Option<Stamp>> {
    let Dynamic::String(value) = value else {
        return Ok(None);
    };
    // YAML timestamps become strings at the native metadata boundary. Keep
    // their explicit offset, as MkDocs does for parsed datetime values.
    if let Ok(timestamp) = value.parse::<Timestamp>() {
        let offset = DateTimeParser::new()
            .parse_pieces(value)?
            .to_numeric_offset()
            .unwrap_or(jiff::tz::Offset::UTC);
        return Ok(Some(Stamp::new(
            &timestamp.to_zoned(TimeZone::fixed(offset)),
        )));
    }
    let date = if let Ok(date) =
        DateTime::strptime(&settings.datetime_format, value)
    {
        date
    } else if let Ok(date) = Date::strptime("%Y-%m-%d", value) {
        let time = Time::strptime("%H:%M", &settings.default_time)
            .context("invalid rss default_time")?;
        date.at(time.hour(), time.minute(), 0, 0)
    } else if let Ok(date) = value.parse::<DateTime>() {
        date
    } else {
        return Ok(None);
    };
    Ok(Some(Stamp::new(&date.in_tz(&settings.default_timezone)?)))
}
