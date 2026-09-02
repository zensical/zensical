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

//! Native configuration for Material tags compatibility.

use pyo3::exceptions::PyValueError;
use pyo3::types::{
    PyAny, PyAnyMethods, PyDict, PyDictMethods, PyList, PyListMethods,
};
use pyo3::{Borrowed, Bound, FromPyObject, PyErr, PyResult};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

use crate::structure::dynamic::Dynamic;

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Complete supported configuration surface after deprecated keys are removed.
const OPTIONS: &[&str] = &[
    "enabled",
    "filters",
    "tags",
    "tags_slugify",
    "tags_slugify_separator",
    "tags_slugify_format",
    "tags_hierarchy",
    "tags_hierarchy_separator",
    "tags_sort_by",
    "tags_sort_reverse",
    "tags_name_property",
    "tags_name_variable",
    "tags_allowed",
    "listings",
    "listings_map",
    "listings_sort_by",
    "listings_sort_reverse",
    "listings_tags_sort_by",
    "listings_tags_sort_reverse",
    "listings_directive",
    "listings_layout",
    "listings_toc",
    "shadow",
    "shadow_on_serve",
    "shadow_tags",
    "shadow_tags_prefix",
    "shadow_tags_suffix",
    "export",
    "export_file",
    "export_only",
];

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Finite native behavior selected from a compatibility callable.
#[derive(Clone, Copy)]
enum Strategy {
    /// Tag slug construction.
    Slug,
    /// Tag ordering.
    Tag,
    /// Listing item ordering.
    Item,
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Material tags plugins.
#[derive(Clone, Debug, Hash, Serialize)]
pub struct TagsPlugin {
    /// Ordered plugin instances.
    pub config: Vec<TagsPluginInstance>,
}

// ----------------------------------------------------------------------------

/// One Material tags plugin instance.
#[derive(Clone, Debug, Hash, Serialize)]
pub struct TagsPluginInstance {
    /// Canonical plugin name.
    pub name: String,
    /// Plugin configuration.
    pub config: TagsPluginConfig,
}

// ----------------------------------------------------------------------------

/// Source admission filters used by one tags instance.
#[derive(Clone, Debug, Hash, Serialize)]
pub struct TagsFilterConfig {
    /// Inclusion patterns.
    pub include: Vec<String>,
    /// Exclusion patterns.
    pub exclude: Vec<String>,
}

// ----------------------------------------------------------------------------

/// Per-listing tags configuration.
#[derive(Clone, Debug, Default, Hash, Serialize)]
pub struct TagsListingConfig {
    /// Whether membership is restricted to the listing directory.
    pub scope: Option<bool>,
    /// Whether shadow tags are rendered.
    pub shadow: Option<bool>,
    /// Fragment layout name.
    pub layout: Option<String>,
    /// Whether listing anchors are added to the page table of contents.
    pub toc: Option<bool>,
    /// Included tag names.
    pub include: Option<Vec<String>>,
    /// Excluded tag names.
    pub exclude: Option<Vec<String>>,
}

// ----------------------------------------------------------------------------

/// Material tags plugin configuration.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Hash, Serialize)]
pub struct TagsPluginConfig {
    /// Whether the instance is enabled.
    pub enabled: bool,
    /// Source admission filters.
    pub filters: TagsFilterConfig,
    /// Whether page tag references are exposed to templates.
    pub tags: bool,
    /// Built-in tag slug strategy.
    pub tags_slugify: String,
    /// Separator supplied to the slug strategy.
    pub tags_slugify_separator: String,
    /// Format containing the generated `{slug}` placeholder.
    pub tags_slugify_format: String,
    /// Whether slash-separated tags form a hierarchy.
    pub tags_hierarchy: bool,
    /// Hierarchy separator.
    pub tags_hierarchy_separator: String,
    /// Built-in page-tag sort strategy.
    pub tags_sort_by: String,
    /// Whether page tags are sorted in reverse.
    pub tags_sort_reverse: bool,
    /// Metadata property containing page tags.
    pub tags_name_property: String,
    /// Template variable receiving tag references.
    pub tags_name_variable: String,
    /// Allowed exact tag names.
    pub tags_allowed: Vec<String>,
    /// Whether listing directives are rendered.
    pub listings: bool,
    /// Named listing configurations.
    pub listings_map: BTreeMap<String, TagsListingConfig>,
    /// Built-in listing item sort strategy.
    pub listings_sort_by: String,
    /// Whether listing items are sorted in reverse.
    pub listings_sort_reverse: bool,
    /// Built-in listing tag sort strategy.
    pub listings_tags_sort_by: String,
    /// Whether listing tags are sorted in reverse.
    pub listings_tags_sort_reverse: bool,
    /// HTML comment directive name.
    pub listings_directive: String,
    /// Default listing fragment layout.
    pub listings_layout: String,
    /// Whether listings populate the page table of contents.
    pub listings_toc: bool,
    /// Whether shadow tags are rendered by default.
    pub shadow: bool,
    /// Whether serve mode enables shadow tags.
    pub shadow_on_serve: bool,
    /// Exact shadow tag names.
    pub shadow_tags: Vec<String>,
    /// Shadow tag prefix.
    pub shadow_tags_prefix: String,
    /// Shadow tag suffix.
    pub shadow_tags_suffix: String,
}

// ----------------------------------------------------------------------------

/// Strict reader for one Python mapping.
struct Reader<'py> {
    /// Python configuration mapping.
    value: &'py Bound<'py, PyDict>,
    /// Qualified path used in configuration errors.
    path: String,
}

// ----------------------------------------------------------------------------

/// Callable identity lowered without invoking Python code.
struct Callable {
    /// Qualified or short callable name.
    name: String,
    /// Declarative keyword arguments.
    keywords: BTreeMap<String, Dynamic>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl TagsPluginConfig {
    /// Normalizes and validates one raw plugin mapping.
    fn from_python(value: &Bound<'_, PyAny>, path: String) -> PyResult<Self> {
        let value = value.cast::<PyDict>().map_err(|_| {
            configuration_error(&path, "expected a configuration mapping")
        })?;
        let reader = Reader { value, path };
        reject_deprecated(&reader)?;
        reader.reject_unknown(OPTIONS)?;

        // Accepted export switches are consumed here. Native tags deliberately
        // never emit the legacy JSON artifact, so they must not enter typed
        // configuration, hashing, or downstream module dependencies.
        reader.optional_bool("export")?;
        reader.optional_string("export_file")?;
        if reader.optional_bool("export_only")?.unwrap_or(false) {
            return Err(reader.error(
                "export_only",
                "is not supported because native tags do not export JSON",
            ));
        }

        let config = Self::read(&reader)?;
        validate(&config, &reader)?;
        Ok(config)
    }

    /// Reads all supported values after the surface has been validated.
    fn read(reader: &Reader<'_>) -> PyResult<Self> {
        let mut config = Self::default();
        config.enabled = reader.bool("enabled", config.enabled)?;
        config.filters = filters(reader)?;
        config.tags = reader.bool("tags", config.tags)?;
        config.tags_slugify = reader.strategy(
            "tags_slugify",
            &config.tags_slugify,
            Strategy::Slug,
        )?;
        config.tags_slugify_separator = reader
            .string("tags_slugify_separator", &config.tags_slugify_separator)?;
        config.tags_slugify_format = reader
            .string("tags_slugify_format", &config.tags_slugify_format)?;
        config.tags_hierarchy =
            reader.bool("tags_hierarchy", config.tags_hierarchy)?;
        config.tags_hierarchy_separator = reader.string(
            "tags_hierarchy_separator",
            &config.tags_hierarchy_separator,
        )?;
        config.tags_sort_by = reader.strategy(
            "tags_sort_by",
            &config.tags_sort_by,
            Strategy::Tag,
        )?;
        config.tags_sort_reverse =
            reader.bool("tags_sort_reverse", config.tags_sort_reverse)?;
        config.tags_name_property =
            reader.string("tags_name_property", &config.tags_name_property)?;
        config.tags_name_variable =
            reader.string("tags_name_variable", &config.tags_name_variable)?;
        config.tags_allowed = reader.scalar_list("tags_allowed")?;
        config.listings = reader.bool("listings", config.listings)?;
        config.listings_map = listings(reader)?;
        config.listings_sort_by = reader.strategy(
            "listings_sort_by",
            &config.listings_sort_by,
            Strategy::Item,
        )?;
        config.listings_sort_reverse = reader
            .bool("listings_sort_reverse", config.listings_sort_reverse)?;
        config.listings_tags_sort_by = reader.strategy(
            "listings_tags_sort_by",
            &config.listings_tags_sort_by,
            Strategy::Tag,
        )?;
        config.listings_tags_sort_reverse = reader.bool(
            "listings_tags_sort_reverse",
            config.listings_tags_sort_reverse,
        )?;
        config.listings_directive =
            reader.string("listings_directive", &config.listings_directive)?;
        config.listings_layout =
            reader.string("listings_layout", &config.listings_layout)?;
        config.listings_toc =
            reader.bool("listings_toc", config.listings_toc)?;
        config.shadow = reader.bool("shadow", config.shadow)?;
        config.shadow_on_serve =
            reader.bool("shadow_on_serve", config.shadow_on_serve)?;
        config.shadow_tags = reader.scalar_list("shadow_tags")?;
        config.shadow_tags_prefix =
            reader.string("shadow_tags_prefix", &config.shadow_tags_prefix)?;
        config.shadow_tags_suffix =
            reader.string("shadow_tags_suffix", &config.shadow_tags_suffix)?;

        Ok(config)
    }
}

// ----------------------------------------------------------------------------

impl<'py> Reader<'py> {
    /// Rejects misspelled and unsupported keys before defaults hide them.
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

    /// Returns a present non-null option.
    fn get(&self, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        Ok(self.value.get_item(name)?.filter(|value| !value.is_none()))
    }

    /// Reads a Boolean with a native default.
    fn bool(&self, name: &str, default: bool) -> PyResult<bool> {
        self.optional_bool(name)
            .map(|value| value.unwrap_or(default))
    }

    /// Reads an optional Boolean.
    fn optional_bool(&self, name: &str) -> PyResult<Option<bool>> {
        self.get(name)?
            .map(|value| {
                value
                    .extract::<bool>()
                    .map_err(|_| self.error(name, "must be a Boolean"))
            })
            .transpose()
    }

    /// Reads a string with a native default.
    fn string(&self, name: &str, default: &str) -> PyResult<String> {
        self.optional_string(name)
            .map(|value| value.unwrap_or_else(|| default.into()))
    }

    /// Reads an optional string.
    fn optional_string(&self, name: &str) -> PyResult<Option<String>> {
        self.get(name)?
            .map(|value| {
                value
                    .extract::<String>()
                    .map_err(|_| self.error(name, "must be a string"))
            })
            .transpose()
    }

    /// Reads and Python-coerces one list of public tag names.
    fn scalar_list(&self, name: &str) -> PyResult<Vec<String>> {
        let Some(value) = self.get(name)? else {
            return Ok(Vec::new());
        };
        let values = value
            .cast::<PyList>()
            .map_err(|_| self.error(name, "must be a list"))?;
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let value = value.extract::<Dynamic>().map_err(|_| {
                    self.error(name, &format!("item {index} must be scalar"))
                })?;
                python_scalar(&value).ok_or_else(|| {
                    self.error(name, &format!("item {index} must be scalar"))
                })
            })
            .collect()
    }

    /// Lowers one supported callable or textual alias to a native strategy.
    fn strategy(
        &self, name: &str, default: &str, strategy: Strategy,
    ) -> PyResult<String> {
        let Some(value) = self.get(name)? else {
            return Ok(default.into());
        };
        let callable =
            callable(&value).map_err(|reason| self.error(name, &reason))?;
        strategy
            .lower(callable)
            .map_err(|reason| self.error(name, &reason))
    }

    /// Creates a path-qualified configuration error.
    fn error(&self, name: &str, reason: &str) -> PyErr {
        configuration_error(&format!("{}.{}", self.path, name), reason)
    }
}

// ----------------------------------------------------------------------------

impl Strategy {
    /// Maps compatibility callable identities to finite native behavior.
    fn lower(self, callable: Callable) -> Result<String, String> {
        match self {
            Self::Slug => lower_slug(callable),
            Self::Tag => lower_simple(
                callable,
                &[
                    ("tag_name", "tag_name"),
                    ("material.plugins.tags.tag_name", "tag_name"),
                    ("tag_name_casefold", "tag_name_casefold"),
                    (
                        "material.plugins.tags.tag_name_casefold",
                        "tag_name_casefold",
                    ),
                ],
            ),
            Self::Item => lower_simple(
                callable,
                &[
                    ("item_title", "item_title"),
                    ("material.plugins.tags.item_title", "item_title"),
                    ("item_url", "item_url"),
                    ("material.plugins.tags.item_url", "item_url"),
                ],
            ),
        }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Default for TagsPluginConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            filters: TagsFilterConfig {
                include: Vec::new(),
                exclude: Vec::new(),
            },
            tags: true,
            tags_slugify: "pymdownx:lower".into(),
            tags_slugify_separator: "-".into(),
            tags_slugify_format: "tag:{slug}".into(),
            tags_hierarchy: false,
            tags_hierarchy_separator: "/".into(),
            tags_sort_by: "tag_name".into(),
            tags_sort_reverse: false,
            tags_name_property: "tags".into(),
            tags_name_variable: "tags".into(),
            tags_allowed: Vec::new(),
            listings: true,
            listings_map: BTreeMap::new(),
            listings_sort_by: "item_title".into(),
            listings_sort_reverse: false,
            listings_tags_sort_by: "tag_name".into(),
            listings_tags_sort_reverse: false,
            listings_directive: "material/tags".into(),
            listings_layout: "default".into(),
            listings_toc: true,
            shadow: false,
            shadow_on_serve: true,
            shadow_tags: Vec::new(),
            shadow_tags_prefix: String::new(),
            shadow_tags_suffix: String::new(),
        }
    }
}

// ----------------------------------------------------------------------------

impl<'a, 'py> FromPyObject<'a, 'py> for TagsPlugin {
    type Error = PyErr;

    fn extract(obj: Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        let root = obj.cast::<PyDict>().map_err(|_| {
            configuration_error("plugins.tags", "expected a mapping")
        })?;
        let entries = root.get_item("config")?.ok_or_else(|| {
            configuration_error("plugins.tags", "missing configuration")
        })?;
        let entries = entries.cast::<PyList>().map_err(|_| {
            configuration_error("plugins.tags", "expected an instance list")
        })?;
        let mut config = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let entry = entry.cast::<PyDict>().map_err(|_| {
                configuration_error(
                    &format!("plugins.tags[{index}]"),
                    "expected an instance mapping",
                )
            })?;
            let name = entry
                .get_item("name")?
                .ok_or_else(|| {
                    configuration_error(
                        &format!("plugins.tags[{index}]"),
                        "missing instance name",
                    )
                })?
                .extract::<String>()?;
            let raw = entry.get_item("config")?.ok_or_else(|| {
                configuration_error(
                    &format!("plugins.tags[{index}]"),
                    "missing instance configuration",
                )
            })?;
            let path = format!("plugins.{name}");
            config.push(TagsPluginInstance {
                name,
                config: TagsPluginConfig::from_python(&raw, path)?,
            });
        }
        Ok(Self { config })
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Reads strict source filters.
fn filters(reader: &Reader<'_>) -> PyResult<TagsFilterConfig> {
    let Some(value) = reader.get("filters")? else {
        return Ok(TagsFilterConfig {
            include: Vec::new(),
            exclude: Vec::new(),
        });
    };
    let value = value
        .cast::<PyDict>()
        .map_err(|_| reader.error("filters", "must be a mapping"))?;
    let nested = Reader {
        value,
        path: format!("{}.filters", reader.path),
    };
    nested.reject_unknown(&["include", "exclude"])?;
    Ok(TagsFilterConfig {
        include: nested.scalar_list("include")?,
        exclude: nested.scalar_list("exclude")?,
    })
}

/// Reads strict named listing configurations.
fn listings(
    reader: &Reader<'_>,
) -> PyResult<BTreeMap<String, TagsListingConfig>> {
    let Some(value) = reader.get("listings_map")? else {
        return Ok(BTreeMap::new());
    };
    let value = value
        .cast::<PyDict>()
        .map_err(|_| reader.error("listings_map", "must be a mapping"))?;
    let mut listings = BTreeMap::new();
    for (name, value) in value.iter() {
        let name = name.extract::<String>().map_err(|_| {
            reader.error("listings_map", "listing names must be strings")
        })?;
        let value = value.cast::<PyDict>().map_err(|_| {
            configuration_error(
                &format!("{}.listings_map.{name}", reader.path),
                "must be a mapping",
            )
        })?;
        let nested = Reader {
            value,
            path: format!("{}.listings_map.{name}", reader.path),
        };
        nested.reject_unknown(&[
            "scope", "shadow", "layout", "toc", "include", "exclude",
        ])?;
        listings.insert(
            name,
            TagsListingConfig {
                scope: nested.optional_bool("scope")?,
                shadow: nested.optional_bool("shadow")?,
                layout: nested.optional_string("layout")?,
                toc: nested.optional_bool("toc")?,
                include: nested
                    .get("include")?
                    .map(|_| nested.scalar_list("include"))
                    .transpose()?,
                exclude: nested
                    .get("exclude")?
                    .map(|_| nested.scalar_list("exclude"))
                    .transpose()?,
            },
        );
    }
    Ok(listings)
}

/// Rejects configuration removed from the supported Material surface.
fn reject_deprecated(reader: &Reader<'_>) -> PyResult<()> {
    for (name, replacement) in [
        ("tags_compare", Some("tags_sort_by")),
        ("tags_compare_reverse", Some("tags_sort_reverse")),
        ("tags_pages_compare", Some("listings_sort_by")),
        ("tags_pages_compare_reverse", Some("listings_sort_reverse")),
        ("tags_file", None),
        ("tags_extra_files", None),
    ] {
        if reader.value.contains(name)? {
            let reason = replacement.map_or_else(
                || {
                    "is deprecated; use a material/tags listing directive"
                        .into()
                },
                |replacement| {
                    format!("is deprecated; use '{replacement}' instead")
                },
            );
            return Err(reader.error(name, &reason));
        }
    }
    Ok(())
}

/// Validates configuration whose correctness is independent of page content.
fn validate(config: &TagsPluginConfig, reader: &Reader<'_>) -> PyResult<()> {
    if !config.tags_slugify_format.contains("{slug}") {
        return Err(reader.error(
            "tags_slugify_format",
            "must contain the '{slug}' placeholder",
        ));
    }
    if config.tags_hierarchy && config.tags_hierarchy_separator.is_empty() {
        return Err(reader.error(
            "tags_hierarchy_separator",
            "must not be empty when hierarchy is enabled",
        ));
    }
    if config.listings_directive.trim().is_empty() {
        return Err(reader.error("listings_directive", "must not be empty"));
    }
    Ok(())
}

/// Extracts a string, declarative object descriptor, or Python callable name.
fn callable(value: &Bound<'_, PyAny>) -> Result<Callable, String> {
    if let Ok(name) = value.extract::<String>() {
        return Ok(Callable {
            name,
            keywords: BTreeMap::new(),
        });
    }
    if let Ok(value) = value.cast::<PyDict>() {
        let name = value
            .get_item("object")
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "callable descriptor requires 'object'".to_string())?
            .extract::<String>()
            .map_err(|_| {
                "callable descriptor 'object' must be a string".to_string()
            })?;
        let keywords = value
            .get_item("kwds")
            .map_err(|error| error.to_string())?
            .filter(|value| !value.is_none())
            .map(|value| value.extract::<BTreeMap<String, Dynamic>>())
            .transpose()
            .map_err(|_| {
                "callable descriptor 'kwds' must be a mapping".to_string()
            })?
            .unwrap_or_default();
        return Ok(Callable { name, keywords });
    }

    let (target, keywords) =
        if value.hasattr("func").map_err(|error| error.to_string())? {
            let keywords = value
                .getattr("keywords")
                .ok()
                .filter(|value| !value.is_none())
                .map(|value| value.extract::<BTreeMap<String, Dynamic>>())
                .transpose()
                .map_err(|_| "partial keywords must be a mapping".to_string())?
                .unwrap_or_default();
            (
                value.getattr("func").map_err(|error| error.to_string())?,
                keywords,
            )
        } else {
            (value.clone(), BTreeMap::new())
        };
    let module = target
        .getattr("__module__")
        .and_then(|value| value.extract::<String>())
        .map_err(|_| "unsupported callable without a module".to_string())?;
    let name = target
        .getattr("__name__")
        .and_then(|value| value.extract::<String>())
        .map_err(|_| "unsupported callable without a name".to_string())?;
    Ok(Callable {
        name: format!("{module}.{name}"),
        keywords,
    })
}

/// Lowers the supported native slug functions.
fn lower_slug(callable: Callable) -> Result<String, String> {
    match callable.name.as_str() {
        "pymdownx:lower" | "pymdownx.slugs.uslugify" => {
            require_no_keywords(&callable)?;
            Ok("pymdownx:lower".into())
        }
        "pymdownx:fold" => {
            require_no_keywords(&callable)?;
            Ok("pymdownx:fold".into())
        }
        "markdown:slugify" | "markdown.extensions.toc.slugify" => {
            require_no_keywords(&callable)?;
            Ok("markdown:slugify".into())
        }
        "pymdownx.slugs.slugify" | "pymdownx.slugs._uslugify" => {
            let case = keyword_string(&callable, "case", "lower")?;
            let normalize = keyword_string(&callable, "normalize", "NFC")?;
            let percent = keyword_bool(&callable, "percent_encode", false)?;
            let supported = ["case", "normalize", "percent_encode"];
            if callable
                .keywords
                .keys()
                .any(|name| !supported.contains(&name.as_str()))
            {
                return Err("slug callable has unsupported keywords".into());
            }
            if normalize != "NFC" || percent {
                return Err(
                    "only NFC, non-percent-encoded pymdownx slugs are supported"
                        .into(),
                );
            }
            match case.as_str() {
                "lower" | "fold" => Ok(format!("pymdownx:{case}")),
                _ => Err("pymdownx slug case must be 'lower' or 'fold'".into()),
            }
        }
        _ => Err(format!("unsupported slug callable '{}'", callable.name)),
    }
}

/// Lowers a keyword-free sorting callable.
fn lower_simple(
    callable: Callable, aliases: &[(&str, &str)],
) -> Result<String, String> {
    require_no_keywords(&callable)?;
    aliases
        .iter()
        .find(|(name, _)| *name == callable.name)
        .map(|(_, strategy)| (*strategy).into())
        .ok_or_else(|| format!("unsupported callable '{}'", callable.name))
}

/// Rejects arguments for compatibility functions that accept none.
fn require_no_keywords(callable: &Callable) -> Result<(), String> {
    if callable.keywords.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "callable '{}' does not accept keywords",
            callable.name
        ))
    }
}

/// Reads a string keyword with a default.
fn keyword_string(
    callable: &Callable, name: &str, default: &str,
) -> Result<String, String> {
    match callable.keywords.get(name) {
        None => Ok(default.into()),
        Some(Dynamic::String(value)) => Ok(value.clone()),
        Some(_) => Err(format!("slug keyword '{name}' must be a string")),
    }
}

/// Reads a Boolean keyword with a default.
fn keyword_bool(
    callable: &Callable, name: &str, default: bool,
) -> Result<bool, String> {
    match callable.keywords.get(name) {
        None => Ok(default),
        Some(Dynamic::Bool(value)) => Ok(*value),
        Some(_) => Err(format!("slug keyword '{name}' must be a Boolean")),
    }
}

/// Converts Material's accepted scalar domain using Python `str` semantics.
pub fn python_scalar(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(value) => Some(value.clone()),
        Dynamic::Bool(value) => Some(python_bool(*value).into()),
        Dynamic::Integer(value) => Some(value.to_string()),
        Dynamic::Float(value) => Some(python_float(value.get())),
        Dynamic::Null | Dynamic::List(_) | Dynamic::Map(_) => None,
    }
}

/// Formats a Boolean using Python's public scalar spelling.
pub fn python_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

/// Formats a floating-point tag closely following Python `str` semantics.
pub fn python_float(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    let mut output = value.to_string();
    if value.is_finite() && !output.contains(['.', 'e', 'E']) {
        output.push_str(".0");
    }
    output
}

/// Creates one consistent configuration diagnostic.
fn configuration_error(path: &str, reason: &str) -> PyErr {
    PyValueError::new_err(format!(
        "invalid tags configuration at '{path}': {reason}"
    ))
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::structure::dynamic::Dynamic;

    use super::python_scalar;

    #[test]
    fn scalar_names_match_python_spelling() {
        assert_eq!(
            python_scalar(&Dynamic::Bool(true)).as_deref(),
            Some("True")
        );
        assert_eq!(
            python_scalar(&Dynamic::from_float(1.0)).as_deref(),
            Some("1.0")
        );
        assert_eq!(
            python_scalar(&Dynamic::from_float(1.5)).as_deref(),
            Some("1.5")
        );
    }
}
