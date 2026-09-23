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

//! Native configuration for Material blog compatibility.

use pyo3::exceptions::PyValueError;
use pyo3::types::{
    PyAny, PyAnyMethods, PyDict, PyDictMethods, PyList, PyListMethods,
};
use pyo3::{Borrowed, Bound, FromPyObject, PyErr, PyResult};
use serde::Serialize;
use std::collections::BTreeSet;

use super::tags::{callable, lower_slug};

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Complete Material blog configuration surface.
const OPTIONS: &[&str] = &[
    "enabled",
    "blog_dir",
    "blog_toc",
    "post_dir",
    "post_date_format",
    "post_url_date_format",
    "post_url_format",
    "post_url_max_categories",
    "post_slugify",
    "post_slugify_separator",
    "post_excerpt",
    "post_excerpt_max_authors",
    "post_excerpt_max_categories",
    "post_excerpt_separator",
    "post_readtime",
    "post_readtime_words_per_minute",
    "archive",
    "archive_name",
    "archive_date_format",
    "archive_url_date_format",
    "archive_url_format",
    "archive_pagination",
    "archive_pagination_per_page",
    "archive_toc",
    "categories",
    "categories_name",
    "categories_url_format",
    "categories_slugify",
    "categories_slugify_separator",
    "categories_sort_by",
    "categories_sort_reverse",
    "categories_allowed",
    "categories_pagination",
    "categories_pagination_per_page",
    "categories_toc",
    "authors",
    "authors_file",
    "authors_profiles",
    "authors_profiles_name",
    "authors_profiles_url_format",
    "authors_profiles_pagination",
    "authors_profiles_pagination_per_page",
    "authors_profiles_toc",
    "pagination",
    "pagination_per_page",
    "pagination_url_format",
    "pagination_format",
    "pagination_if_single_page",
    "pagination_keep_content",
    "draft",
    "draft_on_serve",
    "draft_if_future_date",
    "pagination_template",
];

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Native category ordering strategies.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CategorySort {
    /// Sort by display name.
    Name,
    /// Sort by descending or ascending post count.
    PostCount,
}

/// Excerpt separator policy.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExcerptPolicy {
    /// Use the whole post if the separator is absent.
    Optional,
    /// Reject a post whose separator is absent.
    Required,
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Material blog plugins.
#[derive(Clone, Debug, Hash, Serialize)]
pub struct BlogPlugin {
    /// Ordered plugin instances.
    pub config: Vec<BlogPluginInstance>,
}

/// One Material blog plugin instance.
#[derive(Clone, Debug, Hash, Serialize)]
pub struct BlogPluginInstance {
    /// Canonical plugin name.
    pub name: String,
    /// Native instance configuration.
    pub config: BlogPluginConfig,
}

/// One native Material blog configuration.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Hash, Serialize)]
pub struct BlogPluginConfig {
    /// Whether this plugin instance participates in the build.
    pub enabled: bool,
    /// Documentation directory containing the blog entrypoint.
    pub blog_dir: String,
    /// Whether the main blog view integrates post headings into its TOC.
    pub blog_toc: bool,
    /// Documentation directory containing posts.
    pub post_dir: String,
    /// Display format applied to post dates.
    pub post_date_format: String,
    /// Date format substituted into post URLs.
    pub post_url_date_format: String,
    /// Route format used for posts.
    pub post_url_format: String,
    /// Maximum number of category slugs substituted into a post URL.
    pub post_url_max_categories: usize,
    /// Separator used by the configured post slugifier.
    pub post_slugify_separator: String,
    /// Whether posts must contain an excerpt separator.
    pub post_excerpt: ExcerptPolicy,
    /// Maximum number of authors exposed by an excerpt.
    pub post_excerpt_max_authors: usize,
    /// Maximum number of categories exposed by an excerpt.
    pub post_excerpt_max_categories: usize,
    /// Marker separating excerpt content from the rest of a post.
    pub post_excerpt_separator: String,
    /// Whether read time is calculated for posts without an override.
    pub post_readtime: bool,
    /// Word rate used by automatic read-time calculation.
    pub post_readtime_words_per_minute: usize,
    /// Whether archive views are generated.
    pub archive: bool,
    /// Translation key or literal navigation label for archive views.
    pub archive_name: String,
    /// Display format applied to archive dates.
    pub archive_date_format: String,
    /// Date format substituted into archive URLs.
    pub archive_url_date_format: String,
    /// Route format used for archive views.
    pub archive_url_format: String,
    /// Archive-specific pagination override.
    pub archive_pagination: Option<bool>,
    /// Archive-specific page-size override.
    pub archive_pagination_per_page: Option<usize>,
    /// Archive-specific table-of-contents override.
    pub archive_toc: Option<bool>,
    /// Whether category views are generated.
    pub categories: bool,
    /// Translation key or literal navigation label for category views.
    pub categories_name: String,
    /// Route format used for category views.
    pub categories_url_format: String,
    /// Separator used by the configured category slugifier.
    pub categories_slugify_separator: String,
    /// Ordering strategy used for category navigation.
    pub categories_sort_by: CategorySort,
    /// Whether the selected category ordering is reversed.
    pub categories_sort_reverse: bool,
    /// Allowed category names, or an empty list to allow every category.
    pub categories_allowed: Vec<String>,
    /// Category-specific pagination override.
    pub categories_pagination: Option<bool>,
    /// Category-specific page-size override.
    pub categories_pagination_per_page: Option<usize>,
    /// Category-specific table-of-contents override.
    pub categories_toc: Option<bool>,
    /// Whether author metadata is rendered on posts and excerpts.
    pub authors: bool,
    /// Documentation-relative path to the author catalog.
    pub authors_file: String,
    /// Whether author profile views are generated.
    pub authors_profiles: bool,
    /// Translation key or literal navigation label for author profiles.
    pub authors_profiles_name: String,
    /// Route format used for author profile views.
    pub authors_profiles_url_format: String,
    /// Author-profile-specific pagination override.
    pub authors_profiles_pagination: Option<bool>,
    /// Author-profile-specific page-size override.
    pub authors_profiles_pagination_per_page: Option<usize>,
    /// Author-profile-specific table-of-contents override.
    pub authors_profiles_toc: Option<bool>,
    /// Whether the main blog view is paginated.
    pub pagination: bool,
    /// Default maximum number of posts shown on one view page.
    pub pagination_per_page: usize,
    /// Route format used for pagination pages.
    pub pagination_url_format: String,
    /// Material pagination-format expression.
    pub pagination_format: String,
    /// Whether pagination context is exposed for a single page.
    pub pagination_if_single_page: bool,
    /// Whether entrypoint content is retained after the first page.
    pub pagination_keep_content: bool,
    /// Whether draft posts are included in ordinary builds.
    pub draft: bool,
    /// Whether draft posts are included while serving.
    pub draft_on_serve: bool,
    /// Whether future-dated posts are treated as drafts.
    pub draft_if_future_date: bool,
}

/// Typed reader for one Python plugin configuration mapping.
struct Reader<'py> {
    /// Python mapping being validated.
    value: &'py Bound<'py, PyDict>,
    /// Configuration path used in diagnostics.
    path: String,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl BlogPluginConfig {
    fn from_python(value: &Bound<'_, PyAny>, path: String) -> PyResult<Self> {
        let value = value.cast::<PyDict>().map_err(|_| {
            configuration_error(&path, "expected a configuration mapping")
        })?;
        let reader = Reader { value, path };
        reader.reject_unknown()?;
        if reader.get("pagination_template")?.is_some() {
            return Err(reader.error(
                "pagination_template",
                "is deprecated; use 'pagination_format' instead",
            ));
        }

        // Callable options are deliberately only classified here. Their
        // behavior is implemented by native Rust functions.
        reader.slug("post_slugify")?;
        reader.slug("categories_slugify")?;

        let mut config = Self::default();
        config.read_post(&reader)?;
        config.read_views(&reader)?;
        config.read_pagination(&reader)?;
        config.validate(&reader)?;
        Ok(config)
    }

    fn read_post(&mut self, reader: &Reader<'_>) -> PyResult<()> {
        let config = self;
        config.enabled = reader.bool("enabled", config.enabled)?;
        config.blog_dir = reader.string("blog_dir", &config.blog_dir)?;
        config.blog_toc = reader.bool("blog_toc", config.blog_toc)?;
        config.post_dir = reader.string("post_dir", &config.post_dir)?;
        config.post_date_format =
            reader.string("post_date_format", &config.post_date_format)?;
        config.post_url_date_format = reader
            .string("post_url_date_format", &config.post_url_date_format)?;
        config.post_url_format =
            reader.string("post_url_format", &config.post_url_format)?;
        config.post_url_max_categories = reader
            .usize("post_url_max_categories", config.post_url_max_categories)?;
        config.post_slugify_separator = reader
            .string("post_slugify_separator", &config.post_slugify_separator)?;
        config.post_excerpt = reader.excerpt(config.post_excerpt)?;
        config.post_excerpt_max_authors = reader.usize(
            "post_excerpt_max_authors",
            config.post_excerpt_max_authors,
        )?;
        config.post_excerpt_max_categories = reader.usize(
            "post_excerpt_max_categories",
            config.post_excerpt_max_categories,
        )?;
        config.post_excerpt_separator = reader
            .string("post_excerpt_separator", &config.post_excerpt_separator)?;
        config.post_readtime =
            reader.bool("post_readtime", config.post_readtime)?;
        config.post_readtime_words_per_minute = reader.usize(
            "post_readtime_words_per_minute",
            config.post_readtime_words_per_minute,
        )?;
        Ok(())
    }

    fn read_views(&mut self, reader: &Reader<'_>) -> PyResult<()> {
        let config = self;
        config.archive = reader.bool("archive", config.archive)?;
        config.archive_name =
            reader.string("archive_name", &config.archive_name)?;
        config.archive_date_format = reader
            .string("archive_date_format", &config.archive_date_format)?;
        config.archive_url_date_format = reader.string(
            "archive_url_date_format",
            &config.archive_url_date_format,
        )?;
        config.archive_url_format =
            reader.string("archive_url_format", &config.archive_url_format)?;
        config.archive_pagination =
            reader.optional_bool("archive_pagination")?;
        config.archive_pagination_per_page =
            reader.optional_usize("archive_pagination_per_page")?;
        config.archive_toc = reader.optional_bool("archive_toc")?;
        config.categories = reader.bool("categories", config.categories)?;
        config.categories_name =
            reader.string("categories_name", &config.categories_name)?;
        config.categories_url_format = reader
            .string("categories_url_format", &config.categories_url_format)?;
        config.categories_slugify_separator = reader.string(
            "categories_slugify_separator",
            &config.categories_slugify_separator,
        )?;
        config.categories_sort_by = reader.category_sort()?;
        config.categories_sort_reverse = reader
            .bool("categories_sort_reverse", config.categories_sort_reverse)?;
        config.categories_allowed = reader.string_list("categories_allowed")?;
        config.categories_pagination =
            reader.optional_bool("categories_pagination")?;
        config.categories_pagination_per_page =
            reader.optional_usize("categories_pagination_per_page")?;
        config.categories_toc = reader.optional_bool("categories_toc")?;
        config.authors = reader.bool("authors", config.authors)?;
        config.authors_file =
            reader.string("authors_file", &config.authors_file)?;
        config.authors_profiles =
            reader.bool("authors_profiles", config.authors_profiles)?;
        config.authors_profiles_name = reader
            .string("authors_profiles_name", &config.authors_profiles_name)?;
        config.authors_profiles_url_format = reader.string(
            "authors_profiles_url_format",
            &config.authors_profiles_url_format,
        )?;
        config.authors_profiles_pagination =
            reader.optional_bool("authors_profiles_pagination")?;
        config.authors_profiles_pagination_per_page =
            reader.optional_usize("authors_profiles_pagination_per_page")?;
        config.authors_profiles_toc =
            reader.optional_bool("authors_profiles_toc")?;
        Ok(())
    }

    fn read_pagination(&mut self, reader: &Reader<'_>) -> PyResult<()> {
        let config = self;
        config.pagination = reader.bool("pagination", config.pagination)?;
        config.pagination_per_page =
            reader.usize("pagination_per_page", config.pagination_per_page)?;
        config.pagination_url_format = reader
            .string("pagination_url_format", &config.pagination_url_format)?;
        config.pagination_format =
            reader.string("pagination_format", &config.pagination_format)?;
        config.pagination_if_single_page = reader.bool(
            "pagination_if_single_page",
            config.pagination_if_single_page,
        )?;
        config.pagination_keep_content = reader
            .bool("pagination_keep_content", config.pagination_keep_content)?;
        config.draft = reader.bool("draft", config.draft)?;
        config.draft_on_serve =
            reader.bool("draft_on_serve", config.draft_on_serve)?;
        config.draft_if_future_date =
            reader.bool("draft_if_future_date", config.draft_if_future_date)?;
        Ok(())
    }

    fn validate(&self, reader: &Reader<'_>) -> PyResult<()> {
        for (name, value) in [
            ("pagination_per_page", self.pagination_per_page),
            (
                "post_readtime_words_per_minute",
                self.post_readtime_words_per_minute,
            ),
        ] {
            if value == 0 {
                return Err(reader.error(name, "must be greater than zero"));
            }
        }
        for (name, value) in [
            (
                "archive_pagination_per_page",
                self.archive_pagination_per_page,
            ),
            (
                "categories_pagination_per_page",
                self.categories_pagination_per_page,
            ),
            (
                "authors_profiles_pagination_per_page",
                self.authors_profiles_pagination_per_page,
            ),
        ] {
            if value == Some(0) {
                return Err(reader.error(name, "must be greater than zero"));
            }
        }
        Ok(())
    }
}

impl<'py> Reader<'py> {
    fn reject_unknown(&self) -> PyResult<()> {
        let allowed = OPTIONS.iter().copied().collect::<BTreeSet<_>>();
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

    fn get(&self, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        Ok(self.value.get_item(name)?.filter(|value| !value.is_none()))
    }

    fn bool(&self, name: &str, default: bool) -> PyResult<bool> {
        self.optional_bool(name)
            .map(|value| value.unwrap_or(default))
    }

    fn optional_bool(&self, name: &str) -> PyResult<Option<bool>> {
        self.get(name)?
            .map(|value| {
                value
                    .extract::<bool>()
                    .map_err(|_| self.error(name, "must be a Boolean"))
            })
            .transpose()
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

    fn usize(&self, name: &str, default: usize) -> PyResult<usize> {
        self.optional_usize(name)
            .map(|value| value.unwrap_or(default))
    }

    fn optional_usize(&self, name: &str) -> PyResult<Option<usize>> {
        self.get(name)?
            .map(|value| {
                value.extract::<usize>().map_err(|_| {
                    self.error(name, "must be a non-negative integer")
                })
            })
            .transpose()
    }

    fn string_list(&self, name: &str) -> PyResult<Vec<String>> {
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
                value.extract::<String>().map_err(|_| {
                    self.error(name, &format!("item {index} must be a string"))
                })
            })
            .collect()
    }

    fn slug(&self, name: &str) -> PyResult<()> {
        let Some(value) = self.get(name)? else {
            return Ok(());
        };
        let callable =
            callable(&value).map_err(|reason| self.error(name, &reason))?;
        let strategy =
            lower_slug(callable).map_err(|reason| self.error(name, &reason))?;
        if strategy != "pymdownx:lower" {
            return Err(self.error(
                name,
                "only Material's Unicode lowercase slug function is supported",
            ));
        }
        Ok(())
    }

    fn category_sort(&self) -> PyResult<CategorySort> {
        let Some(value) = self.get("categories_sort_by")? else {
            return Ok(CategorySort::Name);
        };
        let callable = callable(&value)
            .map_err(|reason| self.error("categories_sort_by", &reason))?;
        if !callable.keywords.is_empty() {
            return Err(self.error(
                "categories_sort_by",
                "sorting callable does not accept keyword arguments",
            ));
        }
        match callable.name.as_str() {
            "view_name" | "material.plugins.blog.view_name" => {
                Ok(CategorySort::Name)
            }
            "view_post_count" | "material.plugins.blog.view_post_count" => {
                Ok(CategorySort::PostCount)
            }
            _ => Err(self.error(
                "categories_sort_by",
                &format!("unsupported callable '{}'", callable.name),
            )),
        }
    }

    fn excerpt(&self, default: ExcerptPolicy) -> PyResult<ExcerptPolicy> {
        match self.get("post_excerpt")? {
            None => Ok(default),
            Some(value) => match value.extract::<String>()?.as_str() {
                "optional" => Ok(ExcerptPolicy::Optional),
                "required" => Ok(ExcerptPolicy::Required),
                _ => Err(self
                    .error("post_excerpt", "must be 'optional' or 'required'")),
            },
        }
    }

    fn error(&self, name: &str, reason: &str) -> PyErr {
        configuration_error(&format!("{}.{}", self.path, name), reason)
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Default for BlogPluginConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            blog_dir: "blog".into(),
            blog_toc: false,
            post_dir: "{blog}/posts".into(),
            post_date_format: "long".into(),
            post_url_date_format: "yyyy/MM/dd".into(),
            post_url_format: "{date}/{slug}".into(),
            post_url_max_categories: 1,
            post_slugify_separator: "-".into(),
            post_excerpt: ExcerptPolicy::Optional,
            post_excerpt_max_authors: 1,
            post_excerpt_max_categories: 5,
            post_excerpt_separator: "<!-- more -->".into(),
            post_readtime: true,
            post_readtime_words_per_minute: 265,
            archive: true,
            archive_name: "blog.archive".into(),
            archive_date_format: "yyyy".into(),
            archive_url_date_format: "yyyy".into(),
            archive_url_format: "archive/{date}".into(),
            archive_pagination: None,
            archive_pagination_per_page: None,
            archive_toc: None,
            categories: true,
            categories_name: "blog.categories".into(),
            categories_url_format: "category/{slug}".into(),
            categories_slugify_separator: "-".into(),
            categories_sort_by: CategorySort::Name,
            categories_sort_reverse: false,
            categories_allowed: Vec::new(),
            categories_pagination: None,
            categories_pagination_per_page: None,
            categories_toc: None,
            authors: true,
            authors_file: "{blog}/.authors.yml".into(),
            authors_profiles: false,
            authors_profiles_name: "blog.authors".into(),
            authors_profiles_url_format: "author/{slug}".into(),
            authors_profiles_pagination: None,
            authors_profiles_pagination_per_page: None,
            authors_profiles_toc: None,
            pagination: true,
            pagination_per_page: 10,
            pagination_url_format: "page/{page}".into(),
            pagination_format: "~2~".into(),
            pagination_if_single_page: false,
            pagination_keep_content: false,
            draft: false,
            draft_on_serve: true,
            draft_if_future_date: false,
        }
    }
}

impl<'a, 'py> FromPyObject<'a, 'py> for BlogPlugin {
    type Error = PyErr;

    fn extract(obj: Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        let root = obj.cast::<PyDict>().map_err(|_| {
            configuration_error("plugins.blogs", "expected a mapping")
        })?;
        let entries = root.get_item("config")?.ok_or_else(|| {
            configuration_error("plugins.blogs", "missing configuration")
        })?;
        let entries = entries.cast::<PyList>().map_err(|_| {
            configuration_error("plugins.blogs", "expected an instance list")
        })?;
        let mut config = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let entry = entry.cast::<PyDict>().map_err(|_| {
                configuration_error(
                    &format!("plugins.blogs[{index}]"),
                    "expected an instance mapping",
                )
            })?;
            let name = entry
                .get_item("name")?
                .ok_or_else(|| {
                    configuration_error(
                        &format!("plugins.blogs[{index}]"),
                        "missing instance name",
                    )
                })?
                .extract::<String>()?;
            let raw = entry.get_item("config")?.ok_or_else(|| {
                configuration_error(
                    &format!("plugins.blogs[{index}]"),
                    "missing instance configuration",
                )
            })?;
            config.push(BlogPluginInstance {
                name: name.clone(),
                config: BlogPluginConfig::from_python(
                    &raw,
                    format!("plugins.{name}"),
                )?,
            });
        }
        Ok(Self { config })
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

fn configuration_error(path: &str, reason: &str) -> PyErr {
    PyValueError::new_err(format!("invalid {path}: {reason}"))
}
