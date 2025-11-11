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

//! Navigation.

use std::hash::{DefaultHasher, Hash, Hasher};

use ahash::HashMap;
use pyo3::FromPyObject;
use serde::Serialize;
use zrx::id::Id;
use zrx::scheduler::Value;
use zrx::stream::value::Chunk;

use super::page::Page;

mod item;
mod iter;
mod meta;

pub use item::NavigationItem;
use iter::Iter;

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
    pub items: Vec<NavigationItem>,
    /// Homepage, if defined.
    pub homepage: Option<NavigationItem>,
    /// Precomputed hash.
    pub hash: u64,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Navigation {
    /// Creates a navigation from the given items.
    pub fn new(mut items: Vec<NavigationItem>, pages: Chunk<Id, Page>) -> Self {
        if items.is_empty() {
            return Self::from(pages);
        }

        // Create a map of pages for easy lookup, so we can resolve titles and
        // icons from the file location of the respective page.
        let mut pages = pages
            .into_iter()
            .map(|item| {
                let id = item.id.location().to_string();
                (id, item.data)
            })
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
                    if let Some(page) = pages.remove(url) {
                        // Set URLs from page - we currently resolve the final
                        // URL during rendering, so we just need to set it here.
                        // Once we start working on the component and module
                        // system, all of this is going to change anyway
                        item.url = Some(page.url);
                        item.canonical_url = page.canonical_url;

                        // Set item title from page if not set
                        if item.title.is_none() {
                            item.title = Some(page.title);
                        }

                        // Extract page metadata for selected keys
                        item.meta = Some(page.meta.into());
                    }
                }

                // Push children onto the stack for further processing
                if !item.children.is_empty() {
                    stack.push(&mut item.children);
                }
            }
        }

        // Determine homepage - sometimes, the index page isn't linked, which
        // is why we try to obtain it from the remaining pages
        let mut homepage = items.iter().find(|item| item.is_index).cloned();
        if homepage.is_none() {
            if let Some(page) = pages.remove("index.md") {
                homepage = Some(NavigationItem {
                    title: Some(page.title),
                    url: Some(page.url),
                    canonical_url: page.canonical_url,
                    meta: Some(page.meta.into()),
                    children: Vec::new(),
                    is_index: true,
                    active: false,
                });
            }
        }

        // Precompute hash
        let hash = {
            let mut hasher = DefaultHasher::default();
            items.hash(&mut hasher);
            hasher.finish()
        };

        // Return navigation
        Self { items, homepage, hash }
    }

    /// Returns a copy of the navigation with the active item set based on the
    /// current URL. This mirrors MkDocs' behavior of setting the "active"
    /// state on navigation items, which is then used for styling.
    ///
    /// Note that this does not modify the navigation in place, but returns a
    /// new instance with the active state set. This is important, as we need
    /// to keep the original navigation structure intact for other pages.
    pub fn with_active(&self, page: &Page) -> Self {
        /// Recursively set active state on navigation items.
        fn recurse(items: &mut [NavigationItem], url: &str) -> bool {
            for item in items.iter_mut() {
                if item.url.as_deref() == Some(url) {
                    item.active = true;
                    return true;
                }

                // If we haven't found the item yet, recurse into children
                if recurse(&mut item.children, url) {
                    item.active = true;
                    return true;
                }
            }
            false
        }

        // Set active state starting from the root
        let mut items = self.items.clone();
        recurse(&mut items, &page.url);
        Self {
            items,
            homepage: self.homepage.clone(),
            hash: self.hash,
        }
    }

    /// Returns ancestors of the page with the given URL.
    ///
    /// Note that only the ancestors, not the page itself is returned, which
    /// again, mirrors MkDocs' behavior, and is necessary for breadcrumbs.
    pub fn ancestors(&self, page: &Page) -> Vec<NavigationItem> {
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
        let _ = recurse(&self.items, &page.url, &mut items);
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

impl From<Chunk<Id, Page>> for Navigation {
    /// Creates a navigation from pages.
    ///
    /// This mirrors the functionality of auto-populated navigation that MkDocs
    /// provides. In the future, we intend to refactor this into a more flexible
    /// system that allows for custom and modular navigation structures, but for
    /// now, compatibility is key.
    fn from(pages: Chunk<Id, Page>) -> Self {
        let mut items: Vec<NavigationItem> = Vec::new();

        // Convert chunk into a vector for easier processing, and sort pages by
        // the exact same method that MkDocs uses
        let mut pages = Vec::from_iter(pages);
        pages.sort_by_key(|item| file_sort_key(&item.id));

        // There can only be pages, no URLs, since we're auto-populating the
        // navigation from the files in the docs directory
        for page in pages {
            let location = page.id.location();

            // Split location into components at slashes
            let mut components = location
                .split('/')
                .map(ToString::to_string)
                .collect::<Vec<_>>();

            // Extract file, and check, whether it's an index file
            let file = components.pop().expect("invariant");

            // Now, first obtain the subsection in which we need to insert the
            // page. If there are no parents, we insert it at the top level.
            let mut section = &mut items;
            for component in components {
                let title = to_title(&component);

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
                title: Some(page.data.title),
                url: Some(page.data.url),
                canonical_url: page.data.canonical_url,
                meta: Some(page.data.meta.into()),
                children: Vec::new(),
                is_index: is_index(&file),
                active: false,
            });
        }

        // Precompute hash
        let hash = {
            let mut hasher = DefaultHasher::default();
            items.hash(&mut hasher);
            hasher.finish()
        };

        // Determine homepage and return navigation
        Self {
            homepage: items.iter().find(|item| item.is_index).cloned(),
            items,
            hash,
        }
    }
}

// ----------------------------------------------------------------------------

impl Hash for Navigation {
    /// Hashes the navigation.
    #[inline]
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        state.write_u64(self.hash);
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

// Returns a key that replicates MkDocs' navigation sorting behavior, ordering
// by parents, then putting the index page first, then sorting by name
pub(crate) fn file_sort_key(id: &Id) -> (Vec<String>, bool, String) {
    let location = id.location();

    // Split location into components at slashes
    let mut components = location
        .split('/')
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    // Extract file, and check, whether it's an index file
    let file = components.pop().expect("invariant");
    (components, !is_index(&file), file)
}

/// Returns whether the given file name is an index file.
fn is_index(component: &str) -> bool {
    component == "index.md" || component == "README.md"
}

/// Computes a page title from a file name, replicating MkDocs' behavior.
pub(crate) fn to_title(component: &str) -> String {
    let mut title = component.trim_end_matches(".md").replace(['-', '_'], " ");
    if title.to_lowercase() == title {
        let first = title.chars().next().unwrap_or_default();
        title = first.to_uppercase().to_string() + &title[1..];
    }
    title
}
