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

//! Autorefs (mkdocstrings).

use ahash::HashMap;
use pyo3::FromPyObject;
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::string::ToString;
use std::sync::LazyLock;
use zrx::path::PathExt;

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Autoref regex.
static AUTOREF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<autoref (?P<attrs>.*?)>(?P<title>.*?)</autoref>").unwrap()
});

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
#[derive(
    Clone, Debug, Default, FromPyObject, Serialize, Deserialize, PartialEq, Eq,
)]
#[pyo3(from_item_all)]
pub struct Autorefs {
    // Primary URLs.
    pub primary: HashMap<String, Vec<String>>,
    // Secondary URLs.
    pub secondary: HashMap<String, Vec<String>>,
    // Inventory URLs.
    pub inventory: HashMap<String, String>,
    // Titles.
    pub titles: HashMap<String, String>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Autorefs {
    /// Creates a new, empty autorefs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses HTML attributes string into a HashMap.
    ///
    /// @todo Document that this is not the most resilient HTML parser
    /// but since we control the autorefs elements, it's fine for now
    fn parse_attributes(attrs_str: &str) -> HashMap<String, String> {
        let mut attrs = HashMap::default();
        let mut chars = attrs_str.chars().peekable();

        while let Some(ch) = chars.peek() {
            // Skip whitespace
            if ch.is_whitespace() {
                chars.next();
                continue;
            }

            // Parse attribute name
            let mut name = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() || ch == '=' {
                    break;
                }
                name.push(ch);
                chars.next();
            }

            if name.is_empty() {
                break;
            }

            // Skip whitespace
            while let Some(&ch) = chars.peek() {
                if !ch.is_whitespace() {
                    break;
                }
                chars.next();
            }

            // Check for '='
            let has_value = chars.peek() == Some(&'=');
            if has_value {
                chars.next(); // consume '='

                // Skip whitespace after '='
                while let Some(&ch) = chars.peek() {
                    if !ch.is_whitespace() {
                        break;
                    }
                    chars.next();
                }

                // Parse value
                let value = if let Some(&quote) = chars.peek() {
                    if quote == '"' || quote == '\'' {
                        chars.next(); // consume opening quote
                        let mut val = String::new();
                        for ch in chars.by_ref() {
                            if ch == quote {
                                break; // consume closing quote
                            }
                            val.push(ch);
                        }
                        val
                    } else {
                        // Unquoted value
                        let mut val = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch.is_whitespace() {
                                break;
                            }
                            val.push(ch);
                            chars.next();
                        }
                        val
                    }
                } else {
                    String::new()
                };

                attrs.insert(name, value);
            } else {
                // Boolean attribute
                attrs.insert(name, String::new());
            }
        }

        attrs
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

    /// Extends autorefs with another instance.
    pub fn extend(&mut self, other: Autorefs) {
        for (key, values) in other.primary {
            self.primary.entry(key).or_default().extend(values);
        }
        for (key, values) in other.secondary {
            self.secondary.entry(key).or_default().extend(values);
        }
        for (key, value) in other.inventory {
            self.inventory.insert(key, value);
        }
        for (key, value) in other.titles {
            self.titles.insert(key, value);
        }
    }

    /// Replaces autorefs in the given content.
    pub fn replace_in(&self, content: String, from_url: &str) -> String {
        let output = AUTOREF_RE.replace_all(&content, |captures: &Captures| {
            let attrs_str =
                captures.name("attrs").map_or("", |m| m.as_str());
            let title =
                captures.name("title").map_or("", |m| m.as_str());

            // Parse the HTML attributes
            let attrs = Self::parse_attributes(attrs_str);
            let identifier =
                attrs.get("identifier").cloned().unwrap_or_default();
            let slug = attrs.get("slug").cloned().unwrap_or_default();
            let optional = attrs.contains_key("optional");

            let identifiers = if slug.is_empty() {
                vec![identifier.clone()]
            } else {
                vec![identifier.clone(), slug.clone()]
            };

            match self.get_url_and_title_from_ids(&identifiers, from_url) {
                Ok((url, original_title)) => {
                    // Check if URL is external (not relative)
                    let external = !is_relative_url(&url);

                    // Build CSS classes
                    let mut classes = vec![
                        "autorefs".to_string(),
                        if external {
                            "autorefs-external".to_string()
                        } else {
                            "autorefs-internal".to_string()
                        },
                    ];

                    // Add existing classes from attrs
                    if let Some(class_str) = attrs.get("class") {
                        classes.extend(
                            class_str
                                .split_whitespace()
                                .map(ToString::to_string),
                        );
                    }
                    let class_attr = classes.join(" ");

                    // Build remaining attributes (those not in the handled set)
                    let remaining_attrs: Vec<String> = attrs
                        .iter()
                        .filter(|(k, _)| !HANDLED_ATTRS.contains(&k.as_str()))
                        .map(|(k, v)| {
                            if v.is_empty() {
                                // Boolean attribute (no value)
                                k.clone()
                            } else {
                                // Attribute with value
                                format!("{k}=\"{v}\"")
                            }
                        })
                        .collect();

                    let remaining = if remaining_attrs.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", remaining_attrs.join(" "))
                    };

                    // Build title attribute (link_titles is always true, strip_title_tags is always false)
                    let tooltip = if optional {
                        // For optional, we use identifier as fallback if no original_title
                        original_title.as_deref().unwrap_or(&identifier).to_string()
                    } else {
                        // For non-optional, use original_title or empty
                        original_title.as_deref().unwrap_or("").to_string()
                    };

                    let title_attr = if !tooltip.is_empty() && !format!("<code>{title}</code>").contains(&tooltip) {
                        format!(" title=\"{}\"", html_escape(&tooltip))
                    } else {
                        String::new()
                    };

                    let escaped_url = html_escape(&url);
                    format!(
                        "<a class=\"{class_attr}\"{title_attr} href=\"{escaped_url}\"{remaining}>{title}</a>"
                    )
                }
                Err(_) => {
                    if optional {
                        format!("<span title=\"{identifier}\">{title}</span>")
                    } else {
                        // @todo: unmapped.append((identifier, attrs.context))
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
        });

        output.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
