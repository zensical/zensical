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

//! MkDocs navigation weight compatibility.

use anyhow::{bail, Result};
use std::cmp::Ordering;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Signal};

use crate::config::{plugins::NavWeightPluginConfig, Config};
use crate::structure::dynamic::Dynamic;
use crate::structure::nav::{NavigationItem, NavigationResolution};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Navigation is sorted and filtered using page metadata.
#[derive(Clone, Debug)]
pub struct NavWeight {
    settings: NavWeightPluginConfig,
    strict: bool,
}

/// Section settings are supplied by its last direct index page.
#[derive(Default)]
struct Section {
    weight: Number,
    hidden: Option<usize>,
    title: Option<String>,
}

/// Integer precision is retained when numeric weights are compared.
#[derive(Clone, Copy, Default)]
struct Number {
    integer: Option<i64>,
    real: f64,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl NavWeight {
    /// Validated plugin settings are retained for navigation updates.
    pub fn new(config: &Config, strict: bool) -> Self {
        Self {
            settings: config.project.plugins.nav_weight.config.clone(),
            strict,
        }
    }

    /// The resolved navigation is updated whenever its page facts change.
    pub fn setup(
        &self, resolution: &Signal<Id, NavigationResolution>,
    ) -> Signal<Id, NavigationResolution> {
        if !self.settings.enabled {
            return resolution.clone();
        }
        let plugin = self.clone();
        resolution
            .map(move |resolution: &NavigationResolution| {
                plugin.resolve(resolution)
            })
            .reduce(|values: &dyn Collection<Key<Id>, NavigationResolution>| {
                values.values().next().cloned()
            })
    }

    /// Weights, section titles, and visibility are applied to a resolved tree.
    fn resolve(
        &self, resolution: &NavigationResolution,
    ) -> Result<NavigationResolution> {
        let mut resolution = resolution.clone();
        let navigation = &mut resolution.navigation;
        let mut omitted = Vec::new();
        let mut diagnostics = Vec::new();
        let items = navigation.items.as_ref().clone();
        let (items, _, ancestry) =
            self.sort(items, false, &mut omitted, &mut 0, &mut diagnostics);
        omitted.sort_by_key(|(order, _)| *order);
        let omitted = omitted
            .into_iter()
            .map(|(_, item)| item)
            .collect::<Vec<_>>();

        for diagnostic in &diagnostics {
            eprintln!("WARNING -  [mkdocs-nav-weight]: {diagnostic}");
        }
        if self.strict && !diagnostics.is_empty() {
            bail!("Aborted because mkdocs-nav-weight reported warnings");
        }

        let mut hasher = DefaultHasher::default();
        items.hash(&mut hasher);
        omitted.hash(&mut hasher);
        ancestry.hash(&mut hasher);
        self.settings.headless_included.hash(&mut hasher);
        navigation.hash = hasher.finish();
        navigation.items = Arc::new(items);
        navigation.omitted = Arc::new(omitted);
        navigation.ancestry = Arc::new(ancestry);
        navigation.headless_included = self.settings.headless_included;
        navigation.page_only_connections = true;
        Ok(resolution)
    }

    /// Each sibling list is stably sorted before hidden items are removed.
    fn sort(
        &self, items: Vec<NavigationItem>, nested: bool,
        omitted: &mut Vec<(usize, NavigationItem)>, order: &mut usize,
        diagnostics: &mut Vec<String>,
    ) -> (Vec<NavigationItem>, Section, Vec<NavigationItem>) {
        let mut parent = Section::default();
        let mut weighted = Vec::new();
        for mut item in items {
            let mut hidden = None;
            let mut empty = false;
            let mut full_children = Vec::new();
            let weight;
            if item.url.is_none() {
                let (children, section, ancestry) = self.sort(
                    std::mem::take(&mut item.children),
                    true,
                    omitted,
                    order,
                    diagnostics,
                );
                item.children = children;
                full_children = ancestry;
                weight = section.weight;
                hidden = section.hidden;
                if let Some(title) = section.title {
                    item.title = Some(title);
                }
            } else if item.meta.is_some() {
                if item.is_index {
                    weight = number(&self.settings.index_weight)
                        .expect("validated index weight");
                    if nested {
                        parent.weight = self.weight(&item, diagnostics);
                        if self.boolean(&item, "headless", diagnostics)
                            && parent.hidden.is_none()
                        {
                            parent.hidden = Some(*order);
                            *order += 1;
                        }
                        if self.settings.section_renamed
                            || self.boolean(&item, "retitled", diagnostics)
                        {
                            parent.title.clone_from(&item.title);
                        }
                        empty = self.boolean(&item, "empty", diagnostics);
                    }
                } else {
                    if self.boolean(&item, "headless", diagnostics) {
                        hidden = Some(*order);
                        *order += 1;
                    }
                    weight = self.weight(&item, diagnostics);
                }
            } else {
                // External links are assigned the upstream weight of zero.
                weight = Number::default();
            }
            let children = std::mem::take(&mut item.children);
            let mut original = item.clone();
            original.children = full_children;
            item.children = children;
            weighted.push((item, weight, hidden, empty, original));
        }

        weighted.sort_by(|(_, left, _, _, _), (_, right, _, _, _)| {
            let ordering = left.compare(*right);
            if self.settings.reverse {
                ordering.reverse()
            } else {
                ordering
            }
        });

        let mut visible = Vec::new();
        let mut ancestry = Vec::new();
        for (item, _, hidden, empty, original) in weighted {
            ancestry.push(original);
            if let Some(order) = hidden {
                omitted.push((order, item));
            } else if !empty {
                visible.push(item);
            }
        }
        (visible, parent, ancestry)
    }

    /// Invalid or missing numeric metadata is replaced with the default.
    fn weight(
        &self, item: &NavigationItem, diagnostics: &mut Vec<String>,
    ) -> Number {
        let fallback = number(&self.settings.default_page_weight)
            .expect("validated default weight");
        let Some(value) =
            item.meta.as_ref().and_then(|meta| meta.get("weight"))
        else {
            return fallback;
        };
        if let Some(value) = number(value) {
            return value;
        }
        self.warn(
            item,
            "weight",
            &self.settings.default_page_weight,
            diagnostics,
        );
        fallback
    }

    /// Only Boolean values are accepted for visibility and section flags.
    fn boolean(
        &self, item: &NavigationItem, key: &str, diagnostics: &mut Vec<String>,
    ) -> bool {
        match item.meta.as_ref().and_then(|meta| meta.get(key)) {
            Some(Dynamic::Bool(value)) => *value,
            None => false,
            Some(_) => {
                self.warn(item, key, &Dynamic::Bool(false), diagnostics);
                false
            }
        }
    }

    /// Invalid metadata is reported when warnings are enabled.
    fn warn(
        &self, item: &NavigationItem, key: &str, fallback: &Dynamic,
        diagnostics: &mut Vec<String>,
    ) {
        if self.settings.warning {
            diagnostics.push(format!(
                "Invalid value for \"{key}\" in {} ({}), setting to \"{fallback}\"",
                item.title.as_deref().unwrap_or_default(),
                item.url.as_deref().unwrap_or_default(),
            ));
        }
    }
}

/// Python Boolean values are also numbers, as in the upstream plugin.
#[allow(
    clippy::cast_precision_loss,
    reason = "Integer weights are retained separately for exact comparisons."
)]
fn number(value: &Dynamic) -> Option<Number> {
    match value {
        Dynamic::Bool(value) => number(&Dynamic::Integer(i64::from(*value))),
        Dynamic::Integer(value) => Some(Number {
            integer: Some(*value),
            real: *value as f64,
        }),
        Dynamic::Float(value) => Some(Number {
            integer: None,
            real: value.get(),
        }),
        _ => None,
    }
}

impl Number {
    /// Mixed integer and floating-point weights are compared without rounding integers.
    fn compare(self, other: Self) -> Ordering {
        match (self.integer, other.integer) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(left), None) => compare_integer_float(left, other.real),
            (None, Some(right)) => {
                compare_integer_float(right, self.real).reverse()
            }
            (None, None) => self
                .real
                .partial_cmp(&other.real)
                .unwrap_or(Ordering::Equal),
        }
    }
}

/// Floating-point bounds are checked before conversion to a signed integer.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "Bounds are checked before truncation, and float comparisons are only used to break integer ties."
)]
fn compare_integer_float(integer: i64, real: f64) -> Ordering {
    if real.is_nan() {
        return Ordering::Equal;
    }
    if real >= 9_223_372_036_854_775_808.0 {
        return Ordering::Less;
    }
    if real < -9_223_372_036_854_775_808.0 {
        return Ordering::Greater;
    }
    integer.cmp(&(real as i64)).then_with(|| {
        (integer as f64)
            .partial_cmp(&real)
            .unwrap_or(Ordering::Equal)
    })
}
