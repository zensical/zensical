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

//! Navigation plans before rendered page facts are attached.

use super::{Navigation, NavigationItem, NavigationResolution};
use crate::structure::page::Page;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// One unresolved navigation-plan item.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum PlanItem {
    /// Page reference or external URL.
    Reference {
        /// Optional explicit display title.
        title: Option<String>,
        /// Source-relative page path or external URL.
        target: String,
    },
    /// Named navigation section.
    Section {
        /// Section display title.
        title: String,
        /// Ordered child items.
        children: Vec<PlanItem>,
    },
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Complete unresolved navigation plan.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Plan {
    /// Ordered root items.
    items: Vec<PlanItem>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Plan {
    /// Creates a plan from ordered items.
    pub fn new(items: Vec<PlanItem>) -> Self {
        Self { items }
    }

    /// Attaches rendered page facts and creates the final navigation.
    pub fn compile(self, pages: Vec<Page>) -> NavigationResolution {
        Navigation::from_plan(
            self.items.into_iter().map(PlanItem::into_item).collect(),
            pages,
        )
    }
}

impl PlanItem {
    /// Creates a page or URL reference.
    pub fn reference(title: Option<String>, target: impl Into<String>) -> Self {
        Self::Reference { title, target: target.into() }
    }

    /// Creates a named section.
    pub fn section(title: impl Into<String>, children: Vec<Self>) -> Self {
        Self::Section { title: title.into(), children }
    }

    /// Lowers one plan item into the existing navigation input shape.
    fn into_item(self) -> NavigationItem {
        match self {
            Self::Reference { title, target } => NavigationItem {
                title,
                is_index: false,
                url: Some(target),
                canonical_url: None,
                meta: None,
                children: Vec::new(),
                active: false,
            },
            Self::Section { title, children } => NavigationItem {
                title: Some(title),
                url: None,
                canonical_url: None,
                meta: None,
                children: children
                    .into_iter()
                    .map(PlanItem::into_item)
                    .collect(),
                is_index: false,
                active: false,
            },
        }
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{Plan, PlanItem};

    #[test]
    fn lowers_references_sections_and_links() {
        let resolution = Plan::new(vec![
            PlanItem::reference(None, "index.md"),
            PlanItem::section(
                "Guide",
                vec![
                    PlanItem::reference(None, "guide/index.md"),
                    PlanItem::reference(Some("Start".into()), "guide.md"),
                ],
            ),
            PlanItem::reference(
                Some("Website".into()),
                "https://example.com/index.md",
            ),
        ])
        .compile(Vec::new());
        let navigation = resolution.navigation;

        assert_eq!(navigation.items[0].url.as_deref(), Some("index.md"));
        assert!(!navigation.items[0].is_index);
        assert_eq!(navigation.items[1].title.as_deref(), Some("Guide"));
        assert!(!navigation.items[1].children[0].is_index);
        assert_eq!(
            navigation.items[1].children[1].title.as_deref(),
            Some("Start")
        );
        assert_eq!(
            navigation.items[2].url.as_deref(),
            Some("https://example.com/index.md")
        );
        assert!(!navigation.items[2].is_index);
    }
}
