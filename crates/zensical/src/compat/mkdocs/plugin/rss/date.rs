// Copyright (c) 2025-2026 Zensical and contributors
// SPDX-License-Identifier: MIT

//! Feed dates from metadata, Git history, and a stable build instant.

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

/// Comparable time and both feed serializations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Stamp {
    pub epoch: i64,
    pub json: String,
    pub rss: String,
}

impl Stamp {
    fn new(zoned: &Zoned) -> Self {
        Self {
            epoch: zoned.timestamp().as_second(),
            json: zoned.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string(),
            rss: zoned.strftime("%a, %d %b %Y %H:%M:%S %z").to_string(),
        }
    }

    pub(super) fn now() -> Self {
        Self::new(&Timestamp::now().in_tz("UTC").expect("UTC exists"))
    }
}

/// Returns latest and earliest commits for one physical source.
pub(super) fn git_dates(root: &Path, source: &Path) -> Option<(i64, i64)> {
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
    let created = commits.last().unwrap_or(updated);
    Some((created, updated))
}

/// Resolves one feed date, preserving metadata precedence.
pub(super) fn resolve(
    meta: &BTreeMap<String, Dynamic>, key: &str, settings: &RssDateConfig,
    git: Option<i64>, fallback: &Stamp,
) -> Result<Stamp> {
    if key != "git"
        && let Some(value) = dot_key(meta, key)
    {
        if let Some(date) = metadata_date(value, settings)? {
            return Ok(date);
        }
    }
    if let Some(seconds) = git {
        let timestamp = Timestamp::from_second(seconds)?;
        let zone = &settings.default_timezone;
        return Ok(Stamp::new(&timestamp.in_tz(zone)?));
    }
    Ok(fallback.clone())
}

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
    } else if let Ok(date) = value.parse::<DateTime>() {
        date
    } else if let Ok(date) = Date::strptime("%Y-%m-%d", value) {
        let time = Time::strptime("%H:%M", &settings.default_time)
            .context("invalid rss default_time")?;
        date.at(time.hour(), time.minute(), 0, 0)
    } else {
        return Ok(None);
    };
    Ok(Some(Stamp::new(&date.in_tz(&settings.default_timezone)?)))
}
