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

//! Navigation.

use ahash::HashMap;
use pyo3::types::{PyAny, PyAnyMethods};
use pyo3::{Bound, FromPyObject, PyResult};
use serde::Serialize;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use zrx::scheduler::Value;

use crate::path::SourcePath;

use super::page::Page;

mod item;
mod iter;
mod view;

pub use item::NavigationItem;
use iter::Iter;
pub use view::NavigationView;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Navigation.
///
/// Besides the list of navigation items, this also provides methods to create
/// a navigation from a list of pages, and to set the active item based on the
/// current page, as well as to retrieve ancestors, previous and next pages.
/// This mirrors MkDocs' behavior, which is important for compatibility.
#[derive(Clone, Debug, PartialEq, Eq, FromPyObject, Serialize)]
pub struct Navigation {
    /// Navigation items.
    #[pyo3(from_py_with = extract_shared_items)]
    pub items: Arc<Vec<NavigationItem>>,
    /// Homepage, if defined.
    pub homepage: Option<NavigationItem>,
    /// Precomputed navigation-structure hash.
    pub hash: u64,
    /// Site snapshot generation this navigation was created from.
    #[serde(skip)]
    pub generation: u64,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Navigation {
    /// Creates a navigation from the given items.
    pub fn new(mut items: Vec<NavigationItem>, pages: Vec<Page>) -> Self {
        if items.is_empty() {
            return Self::from(pages);
        }

        // Create a map of pages for easy lookup, so we can resolve titles and
        // icons from the file location of the respective page.
        let pages = pages
            .into_iter()
            .map(|page| (page.source().to_string(), page))
            .collect::<HashMap<_, _>>();

        // Since a navigation structure is given, we just need to add titles and
        // icons where necessary and defined in page metadata
        let mut stack = vec![&mut items];
        while let Some(children) = stack.pop() {
            for item in children.iter_mut() {
                // Here, we differ from MkDocs, in that navigation items can or
                // cannot have URLs, since we model sections and pages with the
                // same data type. This is definitely not the final design that
                // we want, and we'll switch to a much more flexible approach
                // once we work on modular navigation. The component system
                // will also make things much easier here.
                if let Some(url) = &item.url {
                    // Try to obtain a page for the given url. Users might also
                    // refer to non-existing pages, which we just ignore for now
                    if let Some(page) = pages.get(url) {
                        // Set URLs from page - we currently resolve the final
                        // URL during rendering, so we just need to set it here.
                        // Once we start working on the component and module
                        // system, all of this is going to change anyway
                        item.url = Some(page.url.clone());
                        item.canonical_url = page.canonical_url.clone();

                        // Set item title from page if not set
                        if item.title.is_none() {
                            item.title = Some(page.title.clone());
                        }

                        // Extract page metadata for selected keys
                        item.meta = Some(page.meta.clone());
                    }
                }

                // Push children onto the stack for further processing
                if !item.children.is_empty() {
                    stack.push(&mut item.children);
                }
            }
        }

        // Determine homepage - here, we mirror MkDocs behavior, which only
        // considers index pages at the root level as potential homepages
        let mut homepage = items.iter().find(|item| item.is_index).cloned();
        if homepage.is_none() {
            // However, if we couldn't find anything, but there's still an index
            // page, we check if it's out of navigation, and if so, use it
            if let Some(page) = pages.get("index.md") {
                if !Iter::new(&items)
                    .any(|item| item.url.as_deref() == Some(&page.url))
                {
                    homepage = Some(NavigationItem {
                        title: Some(page.title.clone()),
                        url: Some(page.url.clone()),
                        canonical_url: page.canonical_url.clone(),
                        meta: Some(page.meta.clone()),
                        children: Vec::new(),
                        is_index: true,
                        active: false,
                    });
                }
            }
        }

        // Precompute hash
        let hash = navigation_hash(&items);

        // Return navigation
        Self {
            items: Arc::new(items),
            homepage,
            hash,
            generation: 0,
        }
    }

    /// Returns ancestors of the page with the given URL.
    ///
    /// Note that only the ancestors, not the page itself is returned, which
    /// again, mirrors MkDocs' behavior, and is necessary for breadcrumbs.
    pub fn ancestors(&self, page: &Page) -> Vec<NavigationItem> {
        self.ancestors_for_url(&page.url)
    }

    /// Returns ancestors of a page URL without requiring the complete page.
    pub fn ancestors_for_url(&self, url: &str) -> Vec<NavigationItem> {
        // Recursively find ancestors of the page with the given URL.
        fn recurse<'a>(
            items: &'a [NavigationItem], url: &str,
            ancestors: &mut Vec<&'a NavigationItem>,
        ) -> bool {
            for item in items {
                // If this item's URL matches, we've found the page.
                if item.url.as_deref() == Some(url) {
                    return true;
                }

                // Recurse into children, then treat this item as a potential
                // ancestor, and push it before recursing and pop if the branch
                // does not contain the page.
                if !item.children.is_empty() {
                    ancestors.push(item);
                    if recurse(&item.children, url, ancestors) {
                        return true;
                    }
                    ancestors.pop();
                }
            }
            false
        }

        // Clone the ancestors into owned items and reverse them, so we start
        // at the ancestor closest to the page, not the root itself
        let mut items: Vec<&NavigationItem> = Vec::new();
        let _ = recurse(&self.items, url, &mut items);
        items.into_iter().rev().cloned().collect()
    }

    /// Returns an iterator over all navigation items in pre-order.
    pub fn iter(&self) -> Iter<'_> {
        Iter::new(&self.items)
    }

    /// Return the next page for the given page in pre-order, if any.
    pub fn next_page(&self, page: &Page) -> Option<NavigationItem> {
        let mut found = false;
        for item in self {
            if found {
                if item.url.is_some() {
                    return Some(item.clone());
                }
                continue;
            }
            if item.url.as_deref() == Some(&page.url) {
                found = true;
            }
        }
        None
    }

    /// Return the previous page for the given page in pre-order, if any.
    pub fn previous_page(&self, page: &Page) -> Option<NavigationItem> {
        let mut prev: Option<NavigationItem> = None;
        for item in self {
            if item.url.as_deref() == Some(&page.url) {
                return prev;
            }
            if item.url.is_some() {
                prev = Some(item.clone());
            }
        }
        None
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Navigation {}

// ----------------------------------------------------------------------------

impl From<Vec<Page>> for Navigation {
    /// Creates a navigation from pages.
    ///
    /// This mirrors the functionality of auto-populated navigation that MkDocs
    /// provides. In the future, we intend to refactor this into a more flexible
    /// system that allows for custom and modular navigation structures, but for
    /// now, compatibility is key.
    fn from(pages: Vec<Page>) -> Self {
        let mut items: Vec<NavigationItem> = Vec::new();

        // Sort pages by the exact same method that MkDocs uses.
        let mut pages = pages;
        pages.sort_by_key(|page| source_sort_key(page.source()));

        // There can only be pages, no URLs, since we're auto-populating the
        // navigation from the files in the docs directory
        for page in pages {
            let source = page.source();
            let file = source.file_name();

            // Now, first obtain the subsection in which we need to insert the
            // page. If there are no parents, we insert it at the top level.
            let mut section = &mut items;
            for component in
                source.parent().iter().flat_map(SourcePath::components)
            {
                let title = to_title(component);

                // Next, we try to find an existing section with the same title.
                // If we find one, we descend into it, otherwise, we create.
                let mut iter = section.iter();
                if let Some(index) =
                    iter.position(|item| item.title.as_ref() == Some(&title))
                {
                    section = &mut section[index].children;
                } else {
                    section.push(NavigationItem {
                        title: Some(title),
                        url: None,
                        canonical_url: None,
                        meta: None,
                        children: Vec::new(),
                        is_index: false,
                        active: false,
                    });

                    // We just inserted an item, so it's safe to unwrap
                    let item = section.last_mut().expect("invariant");
                    section = &mut item.children;
                }
            }

            // Insert page into the section
            section.push(NavigationItem {
                title: Some(page.title.clone()),
                url: Some(page.url.clone()),
                canonical_url: page.canonical_url.clone(),
                meta: Some(page.meta.clone()),
                children: Vec::new(),
                is_index: is_index(file),
                active: false,
            });
        }

        // Precompute hash
        let hash = navigation_hash(&items);

        // Determine homepage and return navigation
        Self {
            homepage: items.iter().find(|item| item.is_index).cloned(),
            items: Arc::new(items),
            hash,
            generation: 0,
        }
    }
}

// ----------------------------------------------------------------------------

impl<'a> IntoIterator for &'a Navigation {
    type Item = &'a NavigationItem;
    type IntoIter = Iter<'a>;

    /// Returns an iterator over all navigation items in pre-order.
    fn into_iter(self) -> Self::IntoIter {
        Iter::new(&self.items)
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Returns the MkDocs navigation sort key for one validated source path.
pub fn source_sort_key(source: &SourcePath) -> (Vec<String>, bool, String) {
    let file = source.file_name().to_owned();
    let components = source
        .parent()
        .iter()
        .flat_map(SourcePath::components)
        .map(ToOwned::to_owned)
        .collect();
    (components, !is_index(&file), file)
}

/// Returns whether the given file name is an index file.
fn is_index(component: &str) -> bool {
    component == "index.md" || component == "README.md"
}

/// Hash the navigation structure that can affect page templates.
fn navigation_hash(items: &[NavigationItem]) -> u64 {
    let mut hasher = DefaultHasher::default();
    items.hash(&mut hasher);
    hasher.finish()
}

/// Computes a page title from a file name, replicating MkDocs' behavior.
pub fn to_title(component: &str) -> String {
    let title = component.trim_end_matches(".md").replace(['-', '_'], " ");
    let first = title.chars().next().unwrap_or_default();

    // Only uppercase first character if it's an ASCII character, and keep
    // other languages like Chinese as-is
    if title.to_lowercase() == title && first.is_ascii_alphabetic() {
        first.to_uppercase().to_string() + &title[1..]
    } else {
        title
    }
}

fn extract_shared_items(
    value: &Bound<'_, PyAny>,
) -> PyResult<Arc<Vec<NavigationItem>>> {
    value.extract::<Vec<NavigationItem>>().map(Arc::new)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::path::SourcePath;

    use super::{navigation_hash, source_sort_key, to_title, Navigation};

    #[test]
    fn test_clone_shares_immutable_data() {
        let nav = Navigation {
            items: Arc::new(Vec::new()),
            homepage: None,
            hash: 0,
            generation: 0,
        };

        let clone = nav.clone();

        assert!(Arc::ptr_eq(&nav.items, &clone.items));
    }

    #[test]
    fn serialization_omits_internal_generation() {
        let nav = Navigation {
            items: Arc::new(Vec::new()),
            homepage: None,
            hash: navigation_hash(&[]),
            generation: 0,
        };

        let value = serde_json::to_value(nav).expect("invariant");
        assert!(value.get("generation").is_none());
        assert!(value.get("hash").is_some());
    }

    /// https://github.com/zensical/zensical/issues/66
    #[test]
    fn test_to_title() {
        assert_eq!(to_title("hello-world"), "Hello world");
        assert_eq!(to_title("编译器笔记"), "编译器笔记");
    }

    #[test]
    fn source_sorting_places_index_before_siblings() {
        let mut sources = [
            "guide/zebra.md".parse::<SourcePath>().unwrap(),
            "guide/index.md".parse().unwrap(),
            "guide/café.md".parse().unwrap(),
        ];
        sources.sort_by_key(source_sort_key);

        assert_eq!(sources[0].as_str(), "guide/index.md");
        assert_eq!(sources[1].as_str(), "guide/café.md");
        assert_eq!(sources[2].as_str(), "guide/zebra.md");
    }
}
