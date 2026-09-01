// Copyright (c) 2025 Zensical and contributors

// SPDX-License-Identifier: MIT
// Third-party contributions licensed under DCO

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
use std::fs;
use std::path::Path;
use std::string::ToString;
use std::sync::Arc;
use zrx::id::Id;
use zrx::path::PathExt;
use zrx::stream::{Key, Value};

use crate::compat::mkdocs::html;
use crate::config::Config;
use crate::structure::nav::file_sort_key;

mod parser;

pub(crate) use parser::{Parser, References};
use parser::{Reference, SLOT_PREFIX, SLOT_SUFFIX};

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

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
// Helper Functions
// ----------------------------------------------------------------------------

/// Escapes HTML special characters.
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Helper to check if a URL is relative to a base URL.
fn is_relative_to(url: &str, base: &str) -> bool {
    // Remove fragments and query strings for directory comparison
    let url_path = url
        .split('#')
        .next()
        .unwrap_or(url)
        .split('?')
        .next()
        .unwrap_or(url);
    let base_path = base
        .split('#')
        .next()
        .unwrap_or(base)
        .split('?')
        .next()
        .unwrap_or(base);

    // Use Path::starts_with for proper path comparison
    Path::new(url_path).starts_with(Path::new(base_path))
}

/// Gets the parent path of a URL.
fn parent_path(url: &str) -> Option<String> {
    Path::new(url)
        .parent()
        .and_then(|p| p.to_str())
        .map(ToString::to_string)
}

/// Resolves the closest URL from a list relative to from_url.
///
/// We do that when multiple URLs are found for an identifier.
///
/// By closest, we mean a combination of "relative to the current page" and "shortest distance from the current page".
///
/// For example, if you link to identifier `hello` from page `foo/bar/`,
/// and the identifier is found in `foo/`, `foo/baz/` and `foo/bar/baz/qux/` pages,
/// autorefs will resolve to `foo/bar/baz/qux`, which is the only URL relative to `foo/bar/`.
///
/// If multiple URLs are equally close, autorefs will resolve to the first of these equally close URLs.
/// If autorefs cannot find any URL that is close to the current page, it will log a warning and resolve to the first URL found.
///
/// When false and multiple URLs are found for an identifier, autorefs will log a warning and resolve to the first URL.
fn resolve_closest_url(
    from_url: &str, urls: &[String], _qualifier: &str,
) -> String {
    let mut base_url = from_url.to_string();
    let candidates;

    loop {
        let found: Vec<String> = urls
            .iter()
            .filter(|url| is_relative_to(url, &base_url))
            .cloned()
            .collect();

        if !found.is_empty() {
            candidates = found;
            break;
        }

        match parent_path(&base_url) {
            Some(parent) if !parent.is_empty() => {
                base_url = parent;
            }
            _ => {
                // @todo Log warning using qualifier
                return urls[0].clone();
            }
        }
    }

    if candidates.len() == 1 {
        candidates[0].clone()
    } else {
        // Find the URL with the fewest slashes
        candidates
            .into_iter()
            .min_by_key(|url| url.matches('/').count())
            .unwrap()
    }
}

/// Computes a relative URL from from_url to to_url.
fn relative_url(from_url: &str, to_url: &str) -> String {
    let from_path = Path::new(from_url);

    // Split URL and fragment for relative computation
    let (to_path, to_fragment) = to_url
        .split_once('#')
        .map_or((Path::new(to_url), None), |(path, f)| {
            (Path::new(path), Some(f))
        });

    // Make target URL relative to page
    let mut rel_path = to_path
        .relative_to(from_path)
        .to_string_lossy()
        .replace('\\', "/");

    // Add fragment back if present
    if let Some(frag) = to_fragment {
        // If the relative path is "." and we have a fragment,
        // just return the fragment
        if rel_path == "." {
            return format!("#{frag}");
        }
        // If `to_path` was empty (URL was just a fragment),
        // add "/" before the fragment
        if to_path.as_os_str().is_empty() {
            rel_path.push('/');
        }
        rel_path.push('#');
        rel_path.push_str(frag);
    }

    rel_path
}

/// Checks if a URL is relative (no scheme).
fn is_relative_url(url: &str) -> bool {
    !(url.starts_with("http://") || url.starts_with("https://"))
}

// ----------------------------------------------------------------------------
// Structs
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
pub(crate) struct Registry(Option<Arc<Autorefs>>);

// ----------------------------------------------------------------------------

/// Autoref registrations produced while rendering one Markdown page.
#[derive(
    Clone, Debug, Default, FromPyObject, Serialize, Deserialize, PartialEq, Eq,
)]
#[pyo3(from_item_all)]
pub(crate) struct Facts {
    /// Primary page-local URLs.
    primary: HashMap<String, Vec<String>>,
    /// Secondary page-local URLs.
    secondary: HashMap<String, Vec<String>>,
    /// Titles for page-local URLs.
    titles: HashMap<String, String>,
}

// ----------------------------------------------------------------------------

/// Cached global inventory URLs supplied by mkdocstrings handlers.
#[derive(Debug, Default, Serialize, Deserialize)]
struct InventoryCache {
    /// Absolute inventory URLs.
    inventory: HashMap<String, String>,
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
struct Autorefs {
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
                return Ok(resolve_closest_url(from_url, urls, "primary"));
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
                return Ok(resolve_closest_url(from_url, urls, "secondary"));
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
        if is_relative_url(&url) {
            url = relative_url(from_url, &url);
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
                let external = !is_relative_url(&url);
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
// Trait implementations
// ----------------------------------------------------------------------------

impl Registry {
    /// Replace autoref placeholders using this immutable registry.
    pub(crate) fn replace_in<S>(
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

impl Value for Registry {}

// ----------------------------------------------------------------------------

impl Value for UnresolvedAutorefs {}

// ----------------------------------------------------------------------------
// Implementations
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
// Functions
// ----------------------------------------------------------------------------

/// Assemble a complete immutable registry from settled page-local facts.
pub(crate) fn assemble(
    config: &Config, mut facts: Vec<(Key<Id>, Arc<Facts>)>,
) -> Registry {
    if !is_enabled(config) {
        return Registry(None);
    }

    facts.sort_by_key(|(key, _)| file_sort_key(&key[0]));

    let mut registry = Autorefs::new();
    for (_, facts) in facts {
        registry.merge(&facts);
    }
    registry.inventory = inventory(&config.get_cache_dir());
    Registry(Some(Arc::new(registry)))
}

/// Returns whether autorefs is active after configuration shims are applied.
pub(super) fn is_enabled(config: &Config) -> bool {
    config.has_markdown_extension(EXTENSION_NAME)
}

/// Collect and cache global inventory URLs supplied by mkdocstrings.
fn inventory(cache_dir: &Path) -> HashMap<String, String> {
    let path = cache_dir.join("autorefs.json");
    let mut cache = fs::read(&path)
        .ok()
        .and_then(|data| serde_json::from_slice::<InventoryCache>(&data).ok())
        .unwrap_or_default();

    // An absent value means all pages came from the Markdown cache and Python
    // never loaded mkdocstrings handlers. An empty map means rendering ran and
    // no external inventory is configured, so it deliberately clears cache.
    if let Some(inventory) = collect_inventory() {
        cache.inventory = inventory;
    }

    if let Ok(data) = serde_json::to_vec_pretty(&cache) {
        let _ = fs::create_dir_all(cache_dir);
        let _ = fs::write(path, data);
    }
    cache.inventory
}

/// Take registrations produced by the most recently rendered page.
pub(crate) fn take_page(url: &str) -> Arc<Facts> {
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

/// Collect global inventory URLs if Python rendered at least one page.
fn collect_inventory() -> Option<HashMap<String, String>> {
    Python::attach(|py| {
        let module = py.import("zensical.extensions.autorefs")?;
        module
            .call_method0("get_autorefs_inventory_data")?
            .extract::<Option<HashMap<String, String>>>()
    })
    .unwrap_or_default()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn prepare(input: &str) -> (String, References) {
        let mut parser = Parser::default();
        let content = html::scan(input, &mut [&mut parser])
            .unwrap_or_else(|| input.to_string());
        (content, parser.finish())
    }

    #[test]
    fn page_facts_merge_without_overwriting_shared_identifiers() {
        let mut autorefs = Autorefs::new();
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
    fn test_resolve_closest_url() {
        let test_cases = vec![
            ("", vec!["x/#b", "#b"], "#b"),
            ("a/b", vec!["x/#e", "a/c/#e", "a/d/#e"], "a/c/#e"),
            ("a/b/", vec!["x/#e", "a/d/#e", "a/c/#e"], "a/d/#e"),
            ("a/b", vec!["x/#e", "a/c/#e", "a/c/d/#e"], "a/c/#e"),
            ("a/b/", vec!["x/#e", "a/c/d/#e", "a/c/#e"], "a/c/#e"),
            (
                "a/b/c",
                vec!["x/#e", "a/#e", "a/b/#e", "a/b/c/#e", "a/b/c/d/#e"],
                "a/b/c/#e",
            ),
            (
                "a/b/c/",
                vec!["x/#e", "a/#e", "a/b/#e", "a/b/c/d/#e", "a/b/c/#e"],
                "a/b/c/#e",
            ),
            ("a", vec!["b/c/#d", "c/#d"], "b/c/#d"),
            ("a/", vec!["c/#d", "b/c/#d"], "c/#d"),
        ];

        for (base, urls, expected) in test_cases {
            let urls: Vec<String> =
                urls.into_iter().map(String::from).collect();
            let result = resolve_closest_url(base, &urls, "test");
            assert_eq!(result, expected, "Failed for base: {base}");
        }
    }

    #[test]
    fn test_relative_url() {
        let test_cases = vec![
            ("a/", "a#b", "#b"),
            ("a/", "a/b#c", "b#c"),
            ("a/b/", "a/b#c", "#c"),
            ("a/b/", "a/c#d", "../c#d"),
            ("a/b/", "a#c", "..#c"),
            ("a/b/c/", "d#e", "../../../d#e"),
            ("a/b/", "c/d/#e", "../../c/d/#e"),
            ("a/index.html", "a/index.html#b", "#b"),
            ("a/index.html", "a/b.html#c", "b.html#c"),
            ("a/b.html", "a/b.html#c", "#c"),
            ("a/b.html", "a/c.html#d", "c.html#d"),
            ("a/b.html", "a/index.html#c", "index.html#c"),
            ("a/b/c.html", "d.html#e", "../../d.html#e"),
            ("a/b.html", "c/d.html#e", "../c/d.html#e"),
            ("a/b/index.html", "a/b/c/d.html#e", "c/d.html#e"),
            ("", "#x", "#x"),
            ("a/", "#x", "../#x"),
            ("a/b.html", "#x", "../#x"),
            ("", "a/#x", "a/#x"),
            ("", "a/b.html#x", "a/b.html#x"),
        ];

        for (current_url, to_url, expected_href) in test_cases {
            let result = relative_url(current_url, to_url);
            assert_eq!(
                result, expected_href,
                "Failed for relative_url('{current_url}', '{to_url}'), expected '{expected_href}' but got '{result}'"
            );
        }
    }

    #[test]
    fn unresolved_autorefs_are_collected_while_replacing() {
        let mut autorefs = Autorefs::new();
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
        let mut autorefs = Autorefs::new();
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
        let mut autorefs = Autorefs::new();
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
