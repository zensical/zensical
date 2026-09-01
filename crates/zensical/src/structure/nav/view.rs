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

//! Lazy MiniJinja navigation views.

use minijinja::value::{Enumerator, Object, Value};
use std::sync::Arc;

use super::{Navigation, NavigationItem};

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Navigation fields visible to templates.
const NAVIGATION_FIELDS: &[&str] = &["items", "homepage", "hash"];

/// Navigation item fields visible to templates.
const ITEM_FIELDS: &[&str] = &[
    "title",
    "url",
    "canonical_url",
    "meta",
    "children",
    "is_index",
    "active",
];

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Shared immutable navigation plus one page-local active path.
#[derive(Debug)]
struct Overlay {
    /// Navigation tree.
    navigation: Navigation,
    /// Child indices from the root to the active page.
    active: Vec<usize>,
}

// ----------------------------------------------------------------------------

/// Lazy navigation object exposed to MiniJinja.
#[derive(Clone, Debug)]
pub struct NavigationView {
    /// Shared overlay.
    overlay: Arc<Overlay>,
}

// ----------------------------------------------------------------------------

/// Lazy navigation item object.
#[derive(Debug)]
struct ItemView {
    /// Shared overlay.
    overlay: Arc<Overlay>,
    /// Path to this item.
    path: Vec<usize>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Overlay {
    /// Returns the item at the given path.
    fn item(&self, path: &[usize]) -> &NavigationItem {
        let (first, tail) =
            path.split_first().expect("item paths are non-empty");
        let mut item = &self.navigation.items[*first];
        for index in tail {
            item = &item.children[*index];
        }
        item
    }
}

// ----------------------------------------------------------------------------

impl NavigationView {
    /// Creates a navigation view for an optional active page.
    pub fn new(navigation: Navigation, active: Option<&str>) -> Self {
        fn find(
            items: &[NavigationItem], url: &str, path: &mut Vec<usize>,
        ) -> bool {
            for (index, item) in items.iter().enumerate() {
                path.push(index);
                if item.url.as_deref() == Some(url)
                    || find(&item.children, url, path)
                {
                    return true;
                }
                path.pop();
            }
            false
        }

        let mut path = Vec::new();
        if let Some(url) = active {
            let _ = find(&navigation.items, url, &mut path);
        }
        Self {
            overlay: Arc::new(Overlay { navigation, active: path }),
        }
    }

    /// Creates the flattened page sequence used by static templates.
    pub fn pages(&self) -> Value {
        fn collect(
            items: &[NavigationItem], parent: &mut Vec<usize>,
            overlay: &Arc<Overlay>, values: &mut Vec<Value>,
        ) {
            for (index, item) in items.iter().enumerate() {
                parent.push(index);
                values.push(Value::from_object(ItemView {
                    overlay: Arc::clone(overlay),
                    path: parent.clone(),
                }));
                collect(&item.children, parent, overlay, values);
                parent.pop();
            }
        }

        let mut values = Vec::new();
        collect(
            &self.overlay.navigation.items,
            &mut Vec::new(),
            &self.overlay,
            &mut values,
        );
        Value::from_object(values)
    }

    /// Resolves one template-visible field.
    fn field(&self, field: &str) -> Option<Value> {
        match field {
            "items" => Some(items(&self.overlay, &[])),
            "homepage" => {
                Some(Value::from_serialize(&self.overlay.navigation.homepage))
            }
            "hash" => Some(Value::from(self.overlay.navigation.hash)),
            _ => None,
        }
    }
}

// ----------------------------------------------------------------------------

impl ItemView {
    /// Resolves one template-visible field.
    fn field(&self, field: &str) -> Option<Value> {
        let item = self.overlay.item(&self.path);
        match field {
            "title" => Some(Value::from_serialize(&item.title)),
            "url" => Some(Value::from_serialize(&item.url)),
            "canonical_url" => Some(Value::from_serialize(&item.canonical_url)),
            "meta" => Some(Value::from_serialize(&item.meta)),
            "children" => Some(items(&self.overlay, &self.path)),
            "is_index" => Some(Value::from(item.is_index)),
            "active" => Some(Value::from(
                !self.overlay.active.is_empty()
                    && self.overlay.active.starts_with(&self.path),
            )),
            _ => None,
        }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Object for NavigationView {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        self.field(key.as_str()?)
    }

    fn get_value_by_str(self: &Arc<Self>, key: &str) -> Option<Value> {
        self.field(key)
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(NAVIGATION_FIELDS)
    }
}

// ----------------------------------------------------------------------------

impl Object for ItemView {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        self.field(key.as_str()?)
    }

    fn get_value_by_str(self: &Arc<Self>, key: &str) -> Option<Value> {
        self.field(key)
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(ITEM_FIELDS)
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Creates one lazy sequence of navigation item views.
fn items(overlay: &Arc<Overlay>, parent: &[usize]) -> Value {
    let children = if parent.is_empty() {
        &*overlay.navigation.items
    } else {
        &overlay.item(parent).children
    };
    let values = (0..children.len())
        .map(|index| {
            let mut path = parent.to_vec();
            path.push(index);
            Value::from_object(ItemView {
                overlay: Arc::clone(overlay),
                path,
            })
        })
        .collect::<Vec<_>>();
    Value::from_object(values)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use minijinja::{context, Environment, Value};
    use std::sync::Arc;

    use super::{Navigation, NavigationItem, NavigationView};

    /// Creates the same tree in immutable and page-active forms.
    fn navigation(active: bool) -> Navigation {
        let child = NavigationItem {
            title: Some("Child".into()),
            url: Some("child/".into()),
            canonical_url: None,
            meta: None,
            children: Vec::new(),
            is_index: false,
            active,
        };
        let root = NavigationItem {
            title: Some("Root".into()),
            url: None,
            canonical_url: None,
            meta: None,
            children: vec![child],
            is_index: false,
            active,
        };
        let sibling = NavigationItem {
            title: Some("Sibling".into()),
            url: Some("sibling/".into()),
            canonical_url: None,
            meta: None,
            children: Vec::new(),
            is_index: false,
            active: false,
        };
        Navigation {
            items: Arc::new(vec![root, sibling]),
            homepage: None,
            hash: 42,
            generation: 0,
        }
    }

    #[test]
    fn overlay_matches_materialized_navigation() {
        let environment = Environment::new();
        let template = environment
            .template_from_str(concat!(
                "{{ nav.hash }}|",
                "{% for item in nav.items %}",
                "{{ item.title }}={{ item.active }}[",
                "{% for child in item.children %}",
                "{{ child.title }}={{ child.active }}",
                "{% endfor %}]",
                "{% endfor %}",
            ))
            .expect("template is valid");

        let expected = template
            .render(context! { nav => navigation(true) })
            .expect("materialized navigation renders");
        let actual = template
            .render(context! {
                nav => Value::from_object(NavigationView::new(
                    navigation(false),
                    Some("child/"),
                ))
            })
            .expect("navigation view renders");
        assert_eq!(actual, expected);
    }

    #[test]
    fn pages_preserve_preorder() {
        let environment = Environment::new();
        let template = environment
            .template_from_str(
                "{% for item in pages %}{{ item.title }}|{% endfor %}",
            )
            .expect("template is valid");
        let view = NavigationView::new(navigation(false), None);
        let pages = view.pages();
        let rendered = template
            .render(context! { pages => pages })
            .expect("page view renders");
        assert_eq!(rendered, "Root|Child|Sibling|");
    }
}
