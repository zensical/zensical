// Copyright (c) 2025-2026 Zensical and contributors
// SPDX-License-Identifier: MIT

//! Localized date representations shared by metadata and Markdown placeholders.

use anyhow::{Context, Result};
use html_escape::{encode_double_quoted_attribute, encode_text};
use icu_locale_core::Locale;
use jiff::fmt::strtime;
use jiff::{tz::TimeZone, Timestamp, Zoned};
use std::collections::BTreeMap;

use crate::compat::mkdocs::plugin::blog::BlogDate;

#[derive(Clone, Debug)]
pub struct Formatter {
    zone: TimeZone,
    custom: String,
}

impl Formatter {
    pub fn new(zone: &str, custom: &str) -> Result<Self> {
        Ok(Self {
            zone: TimeZone::get(zone).with_context(|| {
                format!("invalid revision date timezone {zone:?}")
            })?,
            custom: custom.into(),
        })
    }

    /// Gregorian dates are localized with the same ICU data as blog dates.
    pub fn formats(
        &self, epoch: i64, locale: &str,
    ) -> Result<BTreeMap<String, String>> {
        locale
            .replace('_', "-")
            .parse::<Locale>()
            .with_context(|| {
                format!("invalid revision date locale {locale:?}")
            })?;
        let zoned = Timestamp::from_second(epoch)?.to_zoned(self.zone.clone());
        let civil =
            BlogDate::parse(&strtime::format("%Y-%m-%dT%H:%M:%S%:z", &zoned)?)?;
        let date = civil.format_display("long", locale)?;
        let time = strtime::format("%H:%M:%S", &zoned)?;
        Ok(BTreeMap::from([
            ("date".into(), date.clone()),
            ("datetime".into(), format!("{date} {time}")),
            ("datetime-timezone".into(), format!("{date} {}", strtime::format("%H:%M:%S %Z", &zoned)?)),
            ("iso_date".into(), strtime::format("%Y-%m-%d", &zoned)?),
            ("iso_datetime".into(), strtime::format("%Y-%m-%d %H:%M:%S", &zoned)?),
            ("timeago".into(), format!("<span class=\"timeago\" datetime=\"{}\" locale=\"{}\"></span>", strtime::format("%Y-%m-%dT%H:%M:%S%:z", &zoned)?, encode_double_quoted_attribute(locale))),
            ("custom".into(), custom(&self.custom, &zoned, civil, locale)?),
        ]))
    }
}

/// A selected representation is wrapped with upstream's CSS classes and title.
pub fn display(formats: &BTreeMap<String, String>, kind: &str) -> String {
    let wrap = |kind: &str| {
        let value = &formats[kind];
        let value = if kind == "timeago" {
            value.clone()
        } else {
            encode_text(value).into_owned()
        };
        format!("<span class=\"git-revision-date-localized-plugin git-revision-date-localized-plugin-{kind}\" title=\"{}\">{value}</span>", encode_double_quoted_attribute(&formats["datetime-timezone"]))
    };
    let mut value = wrap(kind);
    if kind == "timeago" {
        value.push_str(&wrap("iso_date"));
    }
    value
}

fn custom(
    pattern: &str, zoned: &Zoned, civil: BlogDate, locale: &str,
) -> Result<String> {
    let mut chars = pattern.chars().peekable();
    let mut result = String::new();
    while let Some(character) = chars.next() {
        if character != '%' {
            result.push(character);
            continue;
        }
        let mut directive = String::from("%");
        while chars
            .peek()
            .is_some_and(|ch| "-_0^#0123456789:".contains(*ch))
        {
            directive.push(chars.next().expect("peeked character"));
        }
        let code = chars
            .next()
            .context("incomplete revision date custom_format directive")?;
        directive.push(code);
        let named = match code {
            'a' => Some("EEE"),
            'A' => Some("EEEE"),
            'b' | 'h' => Some("MMM"),
            'B' => Some("MMMM"),
            'p' => Some("a"),
            'x' => Some("short"),
            'c' => Some("full"),
            _ => None,
        };
        let value = if let Some(pattern) = named {
            let value = civil.format_display(pattern, locale)?;
            if code == 'c' {
                format!("{value} {}", strtime::format("%H:%M:%S", zoned)?)
            } else {
                value
            }
        } else if code == 'f' {
            "000000".into()
        } else {
            strtime::format(&directive, zoned).with_context(|| format!("invalid revision date custom_format directive {directive:?}"))?
        };
        result.push_str(&value);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{display, Formatter};

    #[test]
    fn dates_are_localized_after_timezone_conversion() {
        // Midnight UTC is still the preceding calendar day in New York.
        let formatter =
            Formatter::new("America/New_York", "%d %B %Y at %H:%M %%").unwrap();

        let formats = formatter.formats(1_704_067_200, "fr_FR").unwrap();

        assert_eq!(formats["iso_datetime"], "2023-12-31 19:00:00");
        assert_eq!(formats["date"], "31 décembre 2023");
        assert_eq!(formats["custom"], "31 décembre 2023 at 19:00 %");
    }

    #[test]
    fn html_is_escaped_and_timeago_has_a_print_fallback() {
        let formatter = Formatter::new("UTC", "<%Y> & \"text\"").unwrap();
        let formats = formatter.formats(1_704_067_200, "en").unwrap();

        assert!(display(&formats, "custom").contains("&lt;2024&gt; &amp;"));
        let relative = display(&formats, "timeago");
        assert!(relative.contains("datetime=\"2024-01-01T00:00:00+00:00\""));
        assert!(
            relative.contains("git-revision-date-localized-plugin-iso_date")
        );
    }
}
