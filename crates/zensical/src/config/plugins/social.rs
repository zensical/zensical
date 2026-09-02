// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to
// deal in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
// sell copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
// IN THE SOFTWARE.

// ----------------------------------------------------------------------------

//! Material social plugin configuration.

use pyo3::exceptions::PyValueError;
use pyo3::types::{
    PyAny, PyAnyMethods, PyDict, PyDictMethods, PyList, PyListMethods,
};
use pyo3::{Borrowed, Bound, FromPyObject, PyErr, PyResult};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

use crate::path::SitePath;
use crate::structure::dynamic::Dynamic;

const OPTIONS: &[&str] = &[
    "enabled",
    "concurrency",
    "cache",
    "cache_dir",
    "log",
    "log_level",
    "cards",
    "cards_dir",
    "cards_layout_dir",
    "cards_layout",
    "cards_layout_options",
    "cards_include",
    "cards_exclude",
    "debug",
    "debug_on_build",
    "debug_grid",
    "debug_grid_step",
    "debug_color",
    "cards_color",
    "cards_font",
];

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Material social plugin instances.
#[derive(Clone, Debug, Hash, Serialize)]
pub struct SocialPlugin {
    /// Ordered plugin instances.
    pub config: Vec<SocialPluginInstance>,
}

/// One Material social plugin instance.
#[derive(Clone, Debug, Hash, Serialize)]
pub struct SocialPluginInstance {
    /// Configured plugin name.
    pub name: String,
    /// Fully normalized configuration.
    pub config: SocialPluginConfig,
}

/// Material social plugin configuration.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Hash, Serialize)]
pub struct SocialPluginConfig {
    /// Whether this instance participates in the build.
    pub enabled: bool,
    /// Maximum number of cards rendered concurrently.
    pub concurrency: usize,
    /// Whether the persistent card cache is used.
    pub cache: bool,
    /// Project-relative cache directory.
    pub cache_dir: String,
    /// Whether card errors are logged instead of failing the build.
    pub log: bool,
    /// Log level for recoverable card errors.
    pub log_level: String,
    /// Default page-level card generation switch.
    pub cards: bool,
    /// Site-relative generated card directory.
    pub cards_dir: String,
    /// Project-relative or absolute custom layout directory.
    pub cards_layout_dir: String,
    /// Default layout name.
    pub cards_layout: String,
    /// Arbitrary variables supplied to layouts.
    pub cards_layout_options: BTreeMap<String, Dynamic>,
    /// Source inclusion patterns.
    pub cards_include: Vec<String>,
    /// Source exclusion patterns.
    pub cards_exclude: Vec<String>,
    /// Whether debug overlays are enabled.
    pub debug: bool,
    /// Whether debug overlays are retained for ordinary builds.
    pub debug_on_build: bool,
    /// Whether the debug grid is shown.
    pub debug_grid: bool,
    /// Debug grid spacing in pixels.
    pub debug_grid_step: usize,
    /// Debug overlay color.
    pub debug_color: String,
    /// Whether the deprecated `cards_color` option was supplied.
    #[serde(skip)]
    deprecated_cards_color: bool,
    /// Whether the deprecated `cards_font` option was supplied.
    #[serde(skip)]
    deprecated_cards_font: bool,
}

/// Strict reader for one Python mapping.
struct Reader<'py> {
    value: &'py Bound<'py, PyDict>,
    path: String,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl SocialPluginConfig {
    /// Normalizes and validates one raw plugin mapping.
    fn from_python(value: &Bound<'_, PyAny>, path: String) -> PyResult<Self> {
        let value = value.cast::<PyDict>().map_err(|_| {
            configuration_error(&path, "expected a configuration mapping")
        })?;
        let reader = Reader { value, path };
        reader.reject_unknown(OPTIONS)?;
        let mut config = Self::default();
        config.enabled = reader.bool("enabled", config.enabled)?;
        config.concurrency = reader.usize("concurrency", config.concurrency)?;
        config.cache = reader.bool("cache", config.cache)?;
        config.cache_dir = reader.string("cache_dir", &config.cache_dir)?;
        config.log = reader.bool("log", config.log)?;
        config.log_level = reader.string("log_level", &config.log_level)?;
        config.cards = reader.bool("cards", config.cards)?;
        config.cards_dir = reader.string("cards_dir", &config.cards_dir)?;
        config.cards_layout_dir =
            reader.string("cards_layout_dir", &config.cards_layout_dir)?;
        config.cards_layout =
            reader.string("cards_layout", &config.cards_layout)?;
        config.cards_layout_options = reader.mapping("cards_layout_options")?;
        config.cards_include = reader.strings("cards_include")?;
        config.cards_exclude = reader.strings("cards_exclude")?;
        config.debug = reader.bool("debug", config.debug)?;
        config.debug_on_build =
            reader.bool("debug_on_build", config.debug_on_build)?;
        config.debug_grid = reader.bool("debug_grid", config.debug_grid)?;
        config.debug_grid_step =
            reader.usize("debug_grid_step", config.debug_grid_step)?;
        config.debug_color =
            reader.string("debug_color", &config.debug_color)?;
        config.deprecated_cards_color = reader.get("cards_color")?.is_some();
        config.deprecated_cards_font = reader.get("cards_font")?.is_some();
        validate(&config, &reader)?;
        Ok(config)
    }

    /// Returns whether the deprecated `cards_color` option was supplied.
    pub fn has_deprecated_cards_color(&self) -> bool {
        self.deprecated_cards_color
    }

    /// Returns whether the deprecated `cards_font` option was supplied.
    pub fn has_deprecated_cards_font(&self) -> bool {
        self.deprecated_cards_font
    }
}

impl Default for SocialPluginConfig {
    fn default() -> Self {
        let concurrency = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .saturating_sub(1)
            .max(1);
        Self {
            enabled: true,
            concurrency,
            cache: true,
            cache_dir: ".cache/plugin/social".into(),
            log: true,
            log_level: "warn".into(),
            cards: true,
            cards_dir: "assets/images/social".into(),
            cards_layout_dir: "layouts".into(),
            cards_layout: "default".into(),
            cards_layout_options: BTreeMap::new(),
            cards_include: Vec::new(),
            cards_exclude: Vec::new(),
            debug: false,
            debug_on_build: false,
            debug_grid: true,
            debug_grid_step: 32,
            debug_color: "grey".into(),
            deprecated_cards_color: false,
            deprecated_cards_font: false,
        }
    }
}

impl<'a, 'py> FromPyObject<'a, 'py> for SocialPlugin {
    type Error = PyErr;

    fn extract(obj: Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        let root = obj.cast::<PyDict>().map_err(|_| {
            configuration_error("plugins.social", "expected a mapping")
        })?;
        let entries = root.get_item("config")?.ok_or_else(|| {
            configuration_error("plugins.social", "missing configuration")
        })?;
        let entries = entries.cast::<PyList>().map_err(|_| {
            configuration_error("plugins.social", "expected an instance list")
        })?;
        let mut config = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let entry = entry.cast::<PyDict>().map_err(|_| {
                configuration_error(
                    &format!("plugins.social[{index}]"),
                    "expected an instance mapping",
                )
            })?;
            let name = entry
                .get_item("name")?
                .ok_or_else(|| {
                    configuration_error(
                        &format!("plugins.social[{index}]"),
                        "missing instance name",
                    )
                })?
                .extract::<String>()?;
            let raw = entry.get_item("config")?.ok_or_else(|| {
                configuration_error(
                    &format!("plugins.social[{index}]"),
                    "missing instance configuration",
                )
            })?;
            config.push(SocialPluginInstance {
                config: SocialPluginConfig::from_python(
                    &raw,
                    format!("plugins.{name}"),
                )?,
                name,
            });
        }
        Ok(Self { config })
    }
}

impl<'py> Reader<'py> {
    /// Rejects misspelled options before defaults can hide them.
    fn reject_unknown(&self, allowed: &[&str]) -> PyResult<()> {
        let allowed = allowed.iter().copied().collect::<BTreeSet<_>>();
        for (key, _) in self.value.iter() {
            let key = key.extract::<String>().map_err(|_| {
                configuration_error(&self.path, "option names must be strings")
            })?;
            if !allowed.contains(key.as_str()) {
                return Err(self.error(&key, "is not a supported option"));
            }
        }
        Ok(())
    }

    /// Returns one present non-null option.
    fn get(&self, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        Ok(self.value.get_item(name)?.filter(|value| !value.is_none()))
    }

    fn bool(&self, name: &str, default: bool) -> PyResult<bool> {
        self.get(name)?
            .map(|value| {
                value
                    .extract::<bool>()
                    .map_err(|_| self.error(name, "must be a Boolean"))
            })
            .transpose()
            .map(|value| value.unwrap_or(default))
    }

    fn usize(&self, name: &str, default: usize) -> PyResult<usize> {
        self.get(name)?
            .map(|value| {
                value
                    .extract::<usize>()
                    .map_err(|_| self.error(name, "must be a positive integer"))
            })
            .transpose()
            .map(|value| value.unwrap_or(default))
    }

    fn string(&self, name: &str, default: &str) -> PyResult<String> {
        self.get(name)?
            .map(|value| {
                value
                    .extract::<String>()
                    .map_err(|_| self.error(name, "must be a string"))
            })
            .transpose()
            .map(|value| value.unwrap_or_else(|| default.into()))
    }

    fn strings(&self, name: &str) -> PyResult<Vec<String>> {
        let Some(value) = self.get(name)? else {
            return Ok(Vec::new());
        };
        value
            .cast::<PyList>()
            .map_err(|_| self.error(name, "must be a list"))?
            .iter()
            .enumerate()
            .map(|(index, value)| {
                value.extract::<String>().map_err(|_| {
                    self.error(name, &format!("item {index} must be a string"))
                })
            })
            .collect()
    }

    fn mapping(&self, name: &str) -> PyResult<BTreeMap<String, Dynamic>> {
        self.get(name)?
            .map(|value| {
                value.extract::<BTreeMap<String, Dynamic>>().map_err(|_| {
                    self.error(name, "must be a mapping with string keys")
                })
            })
            .transpose()
            .map(Option::unwrap_or_default)
    }

    fn error(&self, name: &str, reason: &str) -> PyErr {
        configuration_error(&format!("{}.{}", self.path, name), reason)
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

fn validate(config: &SocialPluginConfig, reader: &Reader<'_>) -> PyResult<()> {
    if config.concurrency == 0 {
        return Err(reader.error("concurrency", "must be greater than zero"));
    }
    if config.debug_grid_step == 0 {
        return Err(
            reader.error("debug_grid_step", "must be greater than zero")
        );
    }
    if config.cards_dir.trim().is_empty() {
        return Err(reader.error("cards_dir", "must not be empty"));
    }
    if config.cards_dir.parse::<SitePath>().is_err() {
        return Err(reader.error("cards_dir", "must be a safe site path"));
    }
    if config.cards_layout.trim().is_empty() {
        return Err(reader.error("cards_layout", "must not be empty"));
    }
    if !["warn", "info", "ignore"].contains(&config.log_level.as_str()) {
        return Err(reader.error("log_level", "is not a valid log level"));
    }
    Ok(())
}

fn configuration_error(path: &str, reason: &str) -> PyErr {
    PyValueError::new_err(format!("invalid configuration at {path}: {reason}"))
}
