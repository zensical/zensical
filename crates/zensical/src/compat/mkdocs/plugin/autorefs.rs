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

//! MkDocs-compatible autorefs plugin.

use ahash::HashMap;
use pyo3::types::PyAnyMethods;
use pyo3::{FromPyObject, Python};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::string::ToString;
use std::sync::Arc;

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Signal, Stream, Value};

use crate::compat::mkdocs::html;
use crate::config::Config;
use crate::path::SourcePath;
use crate::structure::nav::source_sort_key;

mod inventory;
mod parser;
mod url;

pub use parser::{Parser, References};
use parser::{Reference, SLOT_PREFIX, SLOT_SUFFIX};
use url::{closest, is_relative, relative};

/// Handled autoref attributes that should not be passed through to the output link.
const HANDLED_ATTRS: &[&str] = &[
    "identifier",
    "optional",
    "hover",
    "class",
    "domain",
    "role",
    "origin",
    "filepath",
    "lineno",
    "slug",
    "backlink-type",
    "backlink-anchor",
];

/// Python Markdown extension that produces autorefs compatibility facts.
const EXTENSION_NAME: &str = "zensical.extensions.autorefs";

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// MkDocs-compatible autorefs pipeline.
#[derive(Clone, Debug)]
pub struct Autorefs {
    /// Whether autorefs extraction and settlement are active.
    enabled: bool,
    /// Cache directory containing external inventory facts.
    cache: PathBuf,
}

// ----------------------------------------------------------------------------

/// Inputs required to derive the revision-complete autorefs registry.
pub struct Dependencies<'a> {
    /// Page-local autorefs registrations.
    pub pages: &'a Stream<Id, PageInput>,
}

// ----------------------------------------------------------------------------

/// One page's registrations keyed by its documentation source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageInput {
    /// Documentation-relative source used for deterministic ordering.
    pub source: SourcePath,
    /// Registrations produced while rendering the page.
    pub facts: Arc<Facts>,
}

// ----------------------------------------------------------------------------

/// Autoref identifiers that could not be resolved in a single page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnresolvedAutorefs {
    /// Identifiers in order of first appearance.
    identifiers: Vec<String>,
}

// ----------------------------------------------------------------------------

/// Shared immutable registry used to resolve page-local autorefs.
#[derive(Clone, Debug)]
pub struct Registry(Option<Arc<Resolver>>);

// ----------------------------------------------------------------------------

/// Autoref registrations produced while rendering one Markdown page.
#[derive(
    Clone, Debug, Default, FromPyObject, Serialize, Deserialize, PartialEq, Eq,
)]
#[pyo3(from_item_all)]
pub struct Facts {
    /// Primary page-local URLs.
    primary: HashMap<String, Vec<String>>,
    /// Secondary page-local URLs.
    secondary: HashMap<String, Vec<String>>,
    /// Titles for page-local URLs.
    titles: HashMap<String, String>,
}

// ----------------------------------------------------------------------------

/// Autorefs (mkdocstrings).
///
/// We use three URL maps, one for "primary" URLs, one for "secondary" URLs,
/// and one for "absolute" URLs.
///
/// - A primary URL is an identifier that links to a specific anchor on a page.
/// - A secondary URL is an alias of an identifier that links to the same anchor as the identifier's primary URL.
///   Primary URLs with these aliases as identifiers may or may not be rendered later.
/// - An absolute URL is an identifier that links to an external resource.
///   These URLs are typically registered by mkdocstrings when loading object inventories.
///
/// mkdocstrings registers a primary URL for each heading rendered in a page.
/// Then, for each alias of this heading's identifier, it registers a secondary URL.
///
/// For example:
///
/// - Object `a.b.c.d` has aliases `a.b.d` and `a.d`
/// - Object `a.b.c.d` is rendered.
/// - We register `a.b.c.d` -> page#a.b.c.d as primary
/// - We register `a.b.d` -> page#a.b.c.d as secondary
/// - We register `a.d` -> page#a.b.c.d as secondary
/// - Later, if `a.b.d` or `a.d` are rendered, we will register primary and secondary URLs the same way
/// - This way we are sure that each of `a.b.c.d`, `a.b.d` or `a.d` will link to their primary URL, if any, or their secondary URL, accordingly
///
/// We need to keep track of whether an identifier is primary or secondary,
/// to give it precedence when resolving cross-references.
/// We wouldn't want to log a warning if there is a single primary URL and one or more secondary URLs,
/// instead we want to use the primary URL without any warning.
///
/// - A single primary URL mapped to an identifer? Use it.
/// - Multiple primary URLs mapped to an identifier? Use the first one, or closest one if configured as such.
/// - No primary URL mapped to an identifier, but a secondary URL mapped? Use it.
/// - Multiple secondary URLs mapped to an identifier? Use the first one, or closest one if configured as such.
/// - No secondary URL mapped to an identifier? Try using absolute URLs
///   (typically registered by loading inventories in mkdocstrings).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Resolver {
    // Primary URLs.
    primary: HashMap<String, Vec<String>>,
    // Secondary URLs.
    secondary: HashMap<String, Vec<String>>,
    // Inventory URLs.
    inventory: HashMap<String, String>,
    // Titles.
    titles: HashMap<String, String>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Autorefs {
    /// Resolves the private settings owned by this pipeline instance.
    pub fn new(config: &Config) -> Self {
        Self {
            enabled: config.has_markdown_extension(EXTENSION_NAME),
            cache: config.get_cache_dir(),
        }
    }

    /// Returns whether autorefs participates in page processing.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Installs revision-complete autorefs registry derivation.
    pub fn setup(
        &self, dependencies: Dependencies<'_>,
    ) -> Signal<Id, Registry> {
        let pipeline = self.clone();
        dependencies.pages.reduce(
            move |pages: &dyn Collection<Key<Id>, PageInput>| {
                if !pipeline.enabled {
                    return Some(Registry(None));
                }
                let mut pages = pages.values().cloned().collect::<Vec<_>>();
                pages.sort_by_key(|page| source_sort_key(&page.source));
                let mut registry = Resolver::new();
                for page in pages {
                    registry.merge(&page.facts);
                }
                registry.inventory = inventory::load(&pipeline.cache);
                Some(Registry(Some(Arc::new(registry))))
            },
        )
    }

    /// Takes registrations produced by the most recently rendered page.
    pub fn take_page(&self, url: &str) -> Arc<Facts> {
        if !self.enabled {
            return Arc::default();
        }
        Arc::new(
            Python::attach(|py| {
                let module = py.import("zensical.extensions.autorefs")?;
                module
                    .call_method1("get_autorefs_page_data", (url,))?
                    .extract::<Facts>()
            })
            .unwrap_or_default(),
        )
    }
}

// ----------------------------------------------------------------------------

impl Resolver {
    /// Creates a new, empty autorefs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Merge one page's registrations into the complete registry.
    fn merge(&mut self, facts: &Facts) {
        merge_url_map(&mut self.primary, &facts.primary);
        merge_url_map(&mut self.secondary, &facts.secondary);
        self.titles.extend(facts.titles.clone());
    }

    /// Resolves the URL for an item identifier (internal implementation).
    fn get_url_from_id(
        &self, identifier: &str, from_url: &str, resolve_closest: bool,
    ) -> Result<String, String> {
        // Try primary URLs first - usually, an object should not have multiple
        // primary URLs, but if it does, resolve closest if requested. Primary
        // URLs are the canonical locations objects are defined. If an object
        // is re-exported, it should have a secondary URL instead.
        if let Some(urls) = self.primary.get(identifier) {
            if urls.len() > 1 && resolve_closest {
                return Ok(closest(from_url, urls, "primary"));
                // @todo Log warning about multiple URLs in production
            }
            return Ok(urls[0].clone());
        }

        // Try secondary URLs
        if let Some(urls) = self.secondary.get(identifier) {
            if urls.len() > 1 {
                // Always resolve closest for secondary
                //
                // Downstream projects rendering aliases of objects
                // imported from upstream ones will render these upstream
                // objects' docstrings. These docstrings can contain
                // cross-references to other upstream objects that are not
                // rendered directly in downstream project's docs.
                //
                // If downstream project renders subclasses of upstream
                // class, with inherited members, only primary URLs will be
                // registered for the aliased/downstream identifiers, and
                // only secondary URLs will be registered for the upstream
                // identifiers.
                //
                // When trying to apply the cross-reference
                // for the upstream docstring, autorefs will find only
                // secondary URLs, and multiple ones. But the end user does
                // not have control over this. It means we shouldn't log
                // warnings when multiple secondary URLs are found, and
                // always resolve to closest.
                return Ok(closest(from_url, urls, "secondary"));
            }
            return Ok(urls[0].clone());
        }

        // Try inventory (absolute URLs)
        if let Some(url) = self.inventory.get(identifier) {
            return Ok(url.clone());
        }

        Err(format!("Identifier '{identifier}' not found"))
    }

    /// Gets the URL for an item identifier.
    fn get_url_and_title_from_id(
        &self, identifier: &str, from_url: &str,
    ) -> Result<(String, Option<String>), String> {
        let mut url = self.get_url_from_id(identifier, from_url, true)?;

        // Get title using URL as key (not identifier)
        let title = self.titles.get(&url).cloned();

        // If from_url is provided and URL is relative, compute relative URL
        if is_relative(&url) {
            url = relative(from_url, &url);
        }

        Ok((url, title))
    }

    /// Resolves the URL for the first matching identifier.
    fn get_url_and_title_from_ids(
        &self, identifiers: &[String], from_url: &str,
    ) -> Result<(String, Option<String>), String> {
        for identifier in identifiers {
            if let Ok(result) =
                self.get_url_and_title_from_id(identifier, from_url)
            {
                return Ok(result);
            }
        }
        Err(format!(
            "None of the identifiers {identifiers:?} were found",
        ))
    }

    /// Renders one parsed autoref against the settled registry.
    #[allow(clippy::single_match_else)]
    fn render(
        &self, reference: &Reference, from_url: &str,
        unresolved: &mut UnresolvedAutorefs,
    ) -> String {
        let title = reference.title();
        let identifier = reference.get("identifier").unwrap_or_default();
        let slug = reference.get("slug").unwrap_or_default();
        let optional = reference.contains("optional");
        let identifiers = if slug.is_empty() {
            vec![identifier.to_string()]
        } else {
            vec![identifier.to_string(), slug.to_string()]
        };

        match self.get_url_and_title_from_ids(&identifiers, from_url) {
            Ok((url, original_title)) => {
                let external = !is_relative(&url);
                let mut classes = vec![
                    "autorefs".to_string(),
                    if external {
                        "autorefs-external".to_string()
                    } else {
                        "autorefs-internal".to_string()
                    },
                ];
                if let Some(class) = reference.get("class") {
                    classes.extend(
                        class.split_whitespace().map(ToString::to_string),
                    );
                }
                let class = classes.join(" ");

                // Pass unknown attributes through in source order. html5gum
                // decodes their values, so escape them when serializing.
                let remaining = reference
                    .attributes()
                    .filter(|(name, _)| !HANDLED_ATTRS.contains(name))
                    .map(|(name, value)| {
                        if value.is_empty() {
                            name.to_string()
                        } else {
                            format!("{name}=\"{}\"", html_escape(value))
                        }
                    })
                    .collect::<Vec<_>>();
                let remaining = if remaining.is_empty() {
                    String::new()
                } else {
                    format!(" {}", remaining.join(" "))
                };

                let tooltip = if optional {
                    original_title.as_deref().unwrap_or(identifier)
                } else {
                    original_title.as_deref().unwrap_or_default()
                };
                let title_attr = if !tooltip.is_empty()
                    && !format!("<code>{title}</code>").contains(tooltip)
                {
                    format!(" title=\"{}\"", html_escape(tooltip))
                } else {
                    String::new()
                };

                format!(
                    "<a class=\"{class}\"{title_attr} href=\"{}\"{remaining}>{title}</a>",
                    html_escape(&url)
                )
            }
            Err(_) => {
                if optional {
                    format!("<span title=\"{identifier}\">{title}</span>")
                } else {
                    unresolved.insert(identifier);
                    if title == identifier {
                        format!("[{identifier}][]")
                    } else if title == format!("<code>{identifier}</code>")
                        && slug.is_empty()
                    {
                        format!("[<code>{identifier}</code>][]")
                    } else {
                        format!("[{title}][{identifier}]")
                    }
                }
            }
        }
    }

    /// Expands page-local slots in one linear pass.
    fn replace_slots(
        &self, content: String, references: &References, from_url: &str,
        unresolved: &mut UnresolvedAutorefs,
    ) -> String {
        if references.is_empty() || !content.contains(SLOT_PREFIX) {
            return content;
        }

        let mut output = String::with_capacity(content.len());
        let mut cursor = 0;
        while let Some(offset) = content[cursor..].find(SLOT_PREFIX) {
            let start = cursor + offset;
            let index_start = start + SLOT_PREFIX.len();
            let Some(offset) = content[index_start..].find(SLOT_SUFFIX) else {
                break;
            };
            let index_end = index_start + offset;
            let end = index_end + SLOT_SUFFIX.len();

            output.push_str(&content[cursor..start]);
            if let Ok(index) = content[index_start..index_end].parse::<usize>()
                && let Some(reference) = references.get(index)
            {
                output.push_str(&self.render(reference, from_url, unresolved));
            } else {
                output.push_str(&content[start..end]);
            }
            cursor = end;
        }
        output.push_str(&content[cursor..]);
        output
    }

    /// Replaces cached slots and raw markers introduced by templates.
    fn replace_in<S>(
        &self, content: S, references: &References, from_url: &str,
    ) -> (String, UnresolvedAutorefs)
    where
        S: Into<String>,
    {
        let mut unresolved = UnresolvedAutorefs::default();
        let mut content = self.replace_slots(
            content.into(),
            references,
            from_url,
            &mut unresolved,
        );

        // Templates may introduce autorefs after the cached Markdown pass.
        // Autorefs therefore participates in the deliberate final HTML pass
        // whenever it is enabled, using the same visitor and slot expansion
        // path as page-produced markers.
        let mut parser = Parser::default();
        if let Some(prepared) = html::scan(&content, &mut [&mut parser]) {
            content = self.replace_slots(
                prepared,
                &parser.finish(),
                from_url,
                &mut unresolved,
            );
        }

        (content, unresolved)
    }
}

// ----------------------------------------------------------------------------
impl Registry {
    /// Replace autoref placeholders using this immutable registry.
    pub fn replace_in<S>(
        &self, content: S, references: &References, from_url: &str,
    ) -> (String, UnresolvedAutorefs)
    where
        S: Into<String>,
    {
        if let Some(autorefs) = &self.0 {
            autorefs.replace_in(content, references, from_url)
        } else {
            (content.into(), UnresolvedAutorefs::default())
        }
    }
}

// ----------------------------------------------------------------------------

impl UnresolvedAutorefs {
    /// Records an identifier that failed to resolve.
    fn insert(&mut self, identifier: &str) {
        if !self.identifiers.iter().any(|id| id == identifier) {
            self.identifiers.push(identifier.to_string());
        }
    }

    /// Returns an iterator over the identifiers.
    pub fn iter(&self) -> std::slice::Iter<'_, String> {
        self.identifiers.iter()
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Registry {}

// ----------------------------------------------------------------------------

impl Value for PageInput {}

// ----------------------------------------------------------------------------

impl Value for UnresolvedAutorefs {}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Merge URL lists while preserving registration order and uniqueness.
fn merge_url_map(
    target: &mut HashMap<String, Vec<String>>,
    source: &HashMap<String, Vec<String>>,
) {
    for (identifier, urls) in source {
        let target = target.entry(identifier.clone()).or_default();
        for url in urls {
            if !target.contains(url) {
                target.push(url.clone());
            }
        }
    }
}

/// Escapes text for use in generated HTML attributes and content.
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use ahash::HashMap;

    use crate::compat::mkdocs::html;

    use super::{Facts, Parser, References, Resolver};

    fn prepare(input: &str) -> (String, References) {
        let mut parser = Parser::default();
        let content = html::scan(input, &mut [&mut parser])
            .unwrap_or_else(|| input.to_string());
        (content, parser.finish())
    }

    #[test]
    fn page_facts_merge_without_overwriting_shared_identifiers() {
        let mut autorefs = Resolver::new();
        autorefs.merge(&Facts {
            primary: HashMap::from_iter([(
                "shared".to_string(),
                vec!["one/#shared".to_string()],
            )]),
            ..Default::default()
        });
        autorefs.merge(&Facts {
            primary: HashMap::from_iter([(
                "shared".to_string(),
                vec!["two/#shared".to_string()],
            )]),
            ..Default::default()
        });

        assert_eq!(autorefs.primary["shared"], ["one/#shared", "two/#shared"]);
    }

    #[test]
    fn unresolved_autorefs_are_collected_while_replacing() {
        let mut autorefs = Resolver::new();
        autorefs
            .primary
            .insert("known".to_string(), vec!["reference/#known".to_string()]);

        let (output, unresolved) = autorefs.replace_in(
            concat!(
                "<autoref identifier=\"known\">Known</autoref>",
                "<autoref identifier=\"missing\">Missing</autoref>",
                "<autoref identifier=\"missing\">Missing</autoref>",
                "<autoref identifier=\"skipped\" optional>Skipped</autoref>",
            ),
            &References::default(),
            "guide/",
        );

        let unresolved = unresolved.iter().collect::<Vec<_>>();
        assert_eq!(unresolved, ["missing"]);
        assert!(output.contains("href=\"../reference/#known\""));
        assert!(output.contains("[Missing][missing]"));
        assert!(output.contains("<span title=\"skipped\">Skipped</span>"));
    }

    #[test]
    fn cached_slots_preserve_autoref_rendering_contract() {
        let mut autorefs = Resolver::new();
        autorefs
            .primary
            .insert("known".to_string(), vec!["reference/#known".to_string()]);
        autorefs.titles.insert(
            "reference/#known".to_string(),
            "Canonical title".to_string(),
        );
        let (content, references) = prepare(concat!(
            "<autoref identifier=\"known\" class=\"custom\" ",
            "data-kind=\"a&amp;b\" download>",
            "<code>Known</code></autoref>",
        ));

        let (output, unresolved) =
            autorefs.replace_in(content, &references, "guide/");

        assert_eq!(
            output,
            concat!(
                "<a class=\"autorefs autorefs-internal custom\" ",
                "title=\"Canonical title\" ",
                "href=\"../reference/#known\" ",
                "data-kind=\"a&amp;b\" download>",
                "<code>Known</code></a>",
            )
        );
        assert!(unresolved.iter().next().is_none());
    }

    #[test]
    fn slug_is_used_as_a_resolution_fallback() {
        let mut autorefs = Resolver::new();
        autorefs.primary.insert(
            "foo-bar".to_string(),
            vec!["reference/#foo-bar".to_string()],
        );
        let (content, references) = prepare(
            "<autoref identifier=\"Foo bar\" slug=\"foo-bar\">Foo bar</autoref>",
        );

        let (output, unresolved) =
            autorefs.replace_in(content, &references, "guide/");

        assert_eq!(
            output,
            concat!(
                "<a class=\"autorefs autorefs-internal\" ",
                "href=\"../reference/#foo-bar\">Foo bar</a>",
            )
        );
        assert!(unresolved.iter().next().is_none());
    }
}
