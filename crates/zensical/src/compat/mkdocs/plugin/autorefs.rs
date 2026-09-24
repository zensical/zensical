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

use ahash::{HashMap, HashSet};
use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};
use html5gum::{Span, Tokenizer};
use pyo3::types::PyAnyMethods;
use pyo3::{FromPyObject, Python};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::string::ToString;
use std::sync::{Arc, OnceLock};

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Signal, Stream, Value};

use crate::compat::mkdocs::url::relative;
use crate::config::plugins::{AutorefsPluginConfig, AutorefsTitleSetting};
use crate::config::Config;
use crate::path::SourcePath;
use crate::structure::nav::{source_sort_key, Navigation, NavigationItem};
use crate::structure::page::Page;
use crate::structure::toc::Section;

mod inventory;
mod parser;
mod url;

pub use parser::{Parser, References};
use parser::{Reference, SLOT_PREFIX, SLOT_SUFFIX};
use url::{closest, is_relative};

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

/// Version of the backlink inputs and collection semantics.
const BACKLINK_CACHE_VERSION: u8 = 1;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// MkDocs-compatible autorefs pipeline.
#[derive(Clone, Debug)]
pub struct Autorefs {
    /// Whether autorefs extraction and settlement are active.
    enabled: bool,
    /// Whether resolved references are collected as backlinks.
    record_backlinks: bool,
    /// Cache directory containing external inventory facts.
    cache: PathBuf,
    /// Hash of configuration that can affect backlink collection.
    config_hash: u64,
    /// Resolved URL selection and title rendering behavior.
    settings: Settings,
}

// ----------------------------------------------------------------------------

/// Effective behavior after resolving automatic title settings.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Settings {
    resolve_closest: bool,
    link_titles: LinkTitles,
    strip_title_tags: bool,
}

/// Links on which a generated title may be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkTitles {
    All,
    External,
    None,
}

impl Settings {
    fn new(config: &AutorefsPluginConfig, features: &[String]) -> Self {
        let has_feature = |name| features.iter().any(|feature| feature == name);
        Self {
            resolve_closest: config.resolve_closest,
            link_titles: match &config.link_titles {
                AutorefsTitleSetting::Enabled(true) => LinkTitles::All,
                AutorefsTitleSetting::Enabled(false) => LinkTitles::None,
                AutorefsTitleSetting::Mode(mode) if mode == "external" => {
                    LinkTitles::External
                }
                AutorefsTitleSetting::Mode(_) => {
                    if has_feature("navigation.instant.preview") {
                        LinkTitles::External
                    } else {
                        LinkTitles::All
                    }
                }
            },
            strip_title_tags: match config.strip_title_tags {
                AutorefsTitleSetting::Enabled(enabled) => enabled,
                AutorefsTitleSetting::Mode(_) => {
                    !has_feature("content.tooltips")
                }
            },
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new(&AutorefsPluginConfig::default(), &[])
    }
}

// ----------------------------------------------------------------------------

/// Inputs required to derive the revision-complete autorefs registry.
pub struct Dependencies<'a> {
    /// Page-local autorefs registrations.
    pub pages: &'a Stream<Id, PageInput>,
}

// ----------------------------------------------------------------------------

/// Inputs required only while backlink collection is enabled.
pub struct BacklinkDependencies<'a> {
    /// Fully resolved pages and their auto-reference data.
    pub pages: &'a Stream<Id, BacklinkInput>,
    /// Revision-complete site navigation used to build breadcrumbs.
    pub navigation: &'a Signal<Id, Navigation>,
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

/// Page data retained only while backlink collection is enabled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BacklinkInput {
    /// Fully resolved page, including its effective title and table of contents.
    pub page: Page,
    /// Registrations produced while rendering the page.
    pub facts: Arc<Facts>,
    /// Auto-references produced while rendering the page.
    pub references: Arc<References>,
}

// ----------------------------------------------------------------------------

/// Complete current page relation at the autorefs settlement boundary.
#[derive(Clone, Debug)]
struct BacklinkPages(Arc<Vec<BacklinkInput>>);

// ----------------------------------------------------------------------------

/// Autoref identifiers that could not be resolved in a single page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnresolvedAutorefs {
    /// Identifiers in order of first appearance.
    identifiers: Vec<String>,
}

// ----------------------------------------------------------------------------

/// Shared immutable registry used to resolve page-local autorefs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registry(Option<Arc<Resolver>>);

// ----------------------------------------------------------------------------

/// One breadcrumb rendered by a mkdocstrings handler.
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct BacklinkCrumb {
    /// Breadcrumb label.
    pub title: String,
    /// Link relative to the page on which backlinks are rendered.
    pub url: String,
}

// ----------------------------------------------------------------------------

/// One node in a shared backlink breadcrumb path.
#[derive(Clone, Debug, PartialEq, Eq)]
struct BreadcrumbNode {
    /// Breadcrumb represented by this node.
    crumb: BacklinkCrumb,
    /// Preceding breadcrumb, shared by every descendant path.
    parent: Option<Arc<BreadcrumbNode>>,
}

// ----------------------------------------------------------------------------

/// Serializable backlink data grouped by reference type.
pub type Backlinks = Vec<(String, Vec<Vec<(String, String)>>)>;

// ----------------------------------------------------------------------------

/// Settled backlink data initialized only when a rendered fragment misses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct BacklinkData {
    /// Source URLs grouped by target identifier and reference type.
    backlinks: HashMap<String, HashMap<String, HashSet<String>>>,
    /// Shared breadcrumb tails keyed by their final source URL.
    breadcrumbs: HashMap<String, Arc<BreadcrumbNode>>,
    /// Internal identifiers grouped by their resolved URL.
    aliases: HashMap<String, BTreeSet<String>>,
}

// ----------------------------------------------------------------------------

/// Lazily collected backlink index for one documentation revision.
#[derive(Clone, Debug)]
struct BacklinkIndex {
    /// Hash of every input that affects collected backlinks.
    revision: u64,
    /// Page-local inputs retained for cache-miss recovery.
    pages: Arc<Vec<BacklinkInput>>,
    /// Navigation used to derive breadcrumb paths on a cache miss.
    navigation: Navigation,
    /// Settled data, initialized only when a page artifact misses cache.
    data: OnceLock<Arc<BacklinkData>>,
}

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
    // URL selection and title behavior.
    settings: Settings,
    // Primary URLs.
    primary: HashMap<String, Vec<String>>,
    // Secondary URLs.
    secondary: HashMap<String, Vec<String>>,
    // Inventory URLs.
    inventory: HashMap<String, String>,
    // Titles.
    titles: HashMap<String, String>,
    // Settled backlink data, loaded only when rendered fragments miss cache.
    backlink_index: Option<BacklinkIndex>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Autorefs {
    /// Resolves the private settings owned by this pipeline instance.
    pub fn new(config: &Config) -> Self {
        let options = config
            .project
            .plugins
            .autorefs
            .as_ref()
            .map(|plugin| plugin.config.clone())
            .unwrap_or_default();
        Self {
            enabled: config.has_markdown_extension(EXTENSION_NAME),
            record_backlinks: config.records_backlinks(),
            cache: config.get_cache_dir(),
            config_hash: config.hash,
            settings: Settings::new(&options, &config.project.theme.features),
        }
    }

    /// Returns whether autorefs participates in page processing.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns whether resolved auto-references are collected as backlinks.
    pub fn records_backlinks(&self) -> bool {
        self.record_backlinks
    }

    /// Installs revision-complete autorefs registry derivation.
    pub fn setup(
        &self, dependencies: Dependencies<'_>,
    ) -> Signal<Id, Registry> {
        let pipeline = self.clone();
        dependencies.pages.reduce(
            move |pages: &dyn Collection<Key<Id>, PageInput>| {
                if pipeline.enabled {
                    let mut pages = pages.values().cloned().collect::<Vec<_>>();
                    pages.sort_by_key(|page| source_sort_key(&page.source));
                    let registry = pipeline.build_registry(
                        pages.iter().map(|page| page.facts.as_ref()),
                    );
                    Some(Registry(Some(Arc::new(registry))))
                } else {
                    Some(Registry(None))
                }
            },
        )
    }

    /// Installs backlink collection against final pages and navigation.
    pub fn setup_backlinks(
        &self, dependencies: BacklinkDependencies<'_>,
    ) -> Signal<Id, Registry> {
        let pages = dependencies.pages.reduce(
            |pages: &dyn Collection<Key<Id>, BacklinkInput>| {
                let mut pages = pages.values().cloned().collect::<Vec<_>>();
                pages.sort_by_key(|page| source_sort_key(page.page.source()));
                Some(BacklinkPages(Arc::new(pages)))
            },
        );
        let pipeline = self.clone();
        let registries = pages.product(dependencies.navigation).map(
            move |pages: &BacklinkPages, navigation: &Navigation| {
                let mut registry = pipeline.build_registry(
                    pages.0.iter().map(|page| page.facts.as_ref()),
                );
                let revision = pipeline.backlink_revision(&pages.0, navigation);
                registry.backlink_index = Some(BacklinkIndex {
                    revision,
                    pages: pages.0.clone(),
                    navigation: navigation.clone(),
                    data: OnceLock::new(),
                });
                Registry(Some(Arc::new(registry)))
            },
        );
        registries.reduce(|registries: &dyn Collection<Key<Id>, Registry>| {
            registries.values().next().cloned()
        })
    }

    /// Builds the shared URL registry from page-local facts.
    fn build_registry<'a>(
        &self, facts: impl Iterator<Item = &'a Facts>,
    ) -> Resolver {
        let mut registry = Resolver::new();
        registry.settings = self.settings.clone();
        for facts in facts {
            registry.merge(facts);
        }
        registry.inventory = inventory::load(&self.cache);
        registry
    }

    /// Hashes every input that affects the settled backlink index.
    fn backlink_revision(
        &self, pages: &[BacklinkInput], navigation: &Navigation,
    ) -> u64 {
        let mut hasher = DefaultHasher::new();
        BACKLINK_CACHE_VERSION.hash(&mut hasher);
        self.config_hash.hash(&mut hasher);
        navigation.hash.hash(&mut hasher);
        for page in pages {
            page.hash_backlinks(&mut hasher);
        }
        hasher.finish()
    }

    /// Takes registrations produced by the most recently rendered page.
    pub fn take_page(&self, url: &str) -> Facts {
        if !self.enabled {
            return Facts::default();
        }
        Python::attach(|py| {
            let module = py.import("zensical.extensions.autorefs")?;
            module
                .call_method1("get_autorefs_page_data", (url,))?
                .extract::<Facts>()
        })
        .unwrap_or_default()
    }
}

// ----------------------------------------------------------------------------

impl BacklinkInput {
    /// Hashes the page-local inputs used to collect backlinks.
    fn hash_backlinks<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.page.source().hash(state);
        self.page.url.hash(state);
        self.page.title.hash(state);
        self.page.toc.hash(state);
        self.facts.hash_backlinks(state);
        self.references.hash(state);
    }
}

// ----------------------------------------------------------------------------

impl Facts {
    /// Registers one page-local anchor while preserving URL insertion order.
    fn register_anchor(
        &mut self, page_url: &str, identifier: &str, anchor: Option<&str>,
        title: Option<&str>, primary: bool,
    ) {
        let url = format!("{page_url}#{}", anchor.unwrap_or(identifier));
        let urls = if primary {
            &mut self.primary
        } else {
            &mut self.secondary
        };
        let candidates = urls.entry(identifier.to_string()).or_default();
        let is_new = !candidates.contains(&url);
        if let Some(title) = title.filter(|title| !title.is_empty()) {
            if is_new {
                candidates.push(url.clone());
            }
            self.titles.entry(url).or_insert_with(|| title.to_string());
        } else if is_new {
            candidates.push(url);
        }
    }

    /// Hashes unordered registration maps in stable key order.
    fn hash_backlinks<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        hash_url_map(&self.primary, state);
        hash_url_map(&self.secondary, state);

        let mut keys = self.titles.keys().collect::<Vec<_>>();
        keys.sort_unstable();
        keys.len().hash(state);
        for key in keys {
            key.hash(state);
            self.titles[key].hash(state);
        }
    }
}

// ----------------------------------------------------------------------------

impl PartialEq for BacklinkIndex {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
    }
}

impl Eq for BacklinkIndex {}

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

    /// Returns backlink data, collecting it on the first page-cache miss.
    fn backlink_data(&self) -> Option<&BacklinkData> {
        let index = self.backlink_index.as_ref()?;
        Some(
            index
                .data
                .get_or_init(|| {
                    Arc::new(self.collect_backlinks(
                        index.pages.as_slice(),
                        &index.navigation,
                    ))
                })
                .as_ref(),
        )
    }

    /// Collects backlink sources and the breadcrumb paths that describe them.
    fn collect_backlinks(
        &self, pages: &[BacklinkInput], navigation: &Navigation,
    ) -> BacklinkData {
        let mut data = BacklinkData::default();
        self.index_aliases(&mut data);
        // A page and its first H1 describe the same breadcrumb. Retain the
        // page URL as its stable identity, but use the H1's rendered title,
        // which can contain richer markup from extensions.
        let page_titles = pages
            .iter()
            .map(|input| {
                let page = &input.page;
                let title = page
                    .toc
                    .first()
                    .filter(|section| section.level == 1)
                    .map_or(page.title.as_str(), |section| {
                        section.title.as_str()
                    });
                (page.url.as_str(), title)
            })
            .collect::<HashMap<_, _>>();
        for input in pages {
            Self::register_breadcrumbs(
                &mut data,
                &input.page,
                navigation,
                &page_titles,
            );
        }

        for input in pages {
            for reference in input.references.iter() {
                let Some(identifier) = reference.get("identifier") else {
                    continue;
                };
                let Some(backlink_type) = reference
                    .get("backlink-type")
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                let Some(anchor) = reference
                    .get("backlink-anchor")
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };

                // External inventory targets do not belong to this site and
                // therefore cannot render backlinks from it.
                if !self.primary.contains_key(identifier)
                    && !self.secondary.contains_key(identifier)
                {
                    continue;
                }
                data.backlinks
                    .entry(identifier.to_string())
                    .or_default()
                    .entry(backlink_type.to_string())
                    .or_default()
                    .insert(format!("{}#{anchor}", input.page.url));
            }
        }
        data
    }

    /// Indexes primary and secondary identifiers by their target URL.
    fn index_aliases(&self, data: &mut BacklinkData) {
        for (identifier, urls) in self.primary.iter().chain(&self.secondary) {
            for url in urls {
                data.aliases
                    .entry(url.clone())
                    .or_default()
                    .insert(identifier.clone());
            }
        }
    }

    /// Registers breadcrumb chains for every heading in one page's ToC.
    fn register_breadcrumbs(
        data: &mut BacklinkData, page: &Page, navigation: &Navigation,
        page_titles: &HashMap<&str, &str>,
    ) {
        let ancestors = navigation
            .ancestors_for_url(&page.url)
            .into_iter()
            .rev()
            .map(|item| navigation_crumb(&item, page_titles))
            .fold(None, |parent, crumb| {
                Some(Arc::new(BreadcrumbNode { crumb, parent }))
            });
        let page_crumb = BacklinkCrumb {
            title: page_titles
                .get(page.url.as_str())
                .copied()
                .unwrap_or(&page.title)
                .to_string(),
            url: page.url.clone(),
        };
        let page_path = if ancestors
            .as_ref()
            .is_some_and(|node| node.crumb.url == page.url)
        {
            ancestors.clone()
        } else {
            Some(Arc::new(BreadcrumbNode {
                crumb: page_crumb,
                parent: ancestors.clone(),
            }))
        };

        for (index, section) in page.toc.iter().enumerate() {
            Self::register_section(
                data,
                &page.url,
                section,
                ancestors.as_ref(),
                page_path.as_ref(),
                None,
                index == 0 && section.level == 1,
            );
        }
    }

    /// Registers one ToC branch, preserving its hierarchy as breadcrumbs.
    fn register_section(
        data: &mut BacklinkData, page_url: &str, section: &Section,
        ancestors: Option<&Arc<BreadcrumbNode>>,
        page_path: Option<&Arc<BreadcrumbNode>>,
        parent: Option<&Arc<BreadcrumbNode>>, is_page_heading: bool,
    ) {
        let parent = if let Some(parent) = parent {
            Some(parent.clone())
        } else if section.level == 1 {
            ancestors.cloned()
        } else {
            page_path.cloned()
        };
        let url = format!("{page_url}#{}", section.id);
        let crumbs = if is_page_heading
            && let Some(page) =
                parent.as_ref().filter(|node| node.crumb.url == page_url)
        {
            // An indexed navigation section already represents this page.
            // Keep its page URL for grouping, but prefer the H1's richer
            // rendered title.
            Arc::new(BreadcrumbNode {
                crumb: BacklinkCrumb {
                    title: section.title.clone(),
                    url: page.crumb.url.clone(),
                },
                parent: page.parent.clone(),
            })
        } else {
            Arc::new(BreadcrumbNode {
                crumb: BacklinkCrumb {
                    title: section.title.clone(),
                    url: url.clone(),
                },
                parent,
            })
        };
        data.breadcrumbs.insert(url, crumbs.clone());

        for child in &section.children {
            Self::register_section(
                data,
                page_url,
                child,
                ancestors,
                page_path,
                Some(&crumbs),
                false,
            );
        }
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
        let mut url = self.get_url_from_id(
            identifier,
            from_url,
            self.settings.resolve_closest,
        )?;

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

    /// Returns handler-ready backlinks relative to the rendering page.
    fn get_backlinks(
        &self, identifiers: &[String], from_url: &str,
    ) -> Backlinks {
        let Some(data) = self.backlink_data() else {
            return Vec::new();
        };
        let mut result =
            BTreeMap::<String, BTreeSet<Vec<BacklinkCrumb>>>::new();
        let identifiers = identifiers
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        for identifier in identifiers {
            let Some(backlinks) = data.backlinks.get(identifier) else {
                continue;
            };
            for (backlink_type, urls) in backlinks {
                let target = result.entry(backlink_type.clone()).or_default();
                for url in urls {
                    let Some(crumbs) = data.breadcrumbs.get(url) else {
                        continue;
                    };
                    target.insert(
                        crumbs
                            .to_vec()
                            .into_iter()
                            .map(|crumb| BacklinkCrumb {
                                title: crumb.title,
                                url: if crumb.url.is_empty() {
                                    String::new()
                                } else {
                                    relative(from_url, &crumb.url)
                                },
                            })
                            .collect(),
                    );
                }
            }
        }

        result
            .into_iter()
            .map(|(backlink_type, backlinks)| {
                (
                    backlink_type,
                    backlinks
                        .into_iter()
                        .map(|crumbs| {
                            crumbs
                                .into_iter()
                                .map(|crumb| (crumb.title, crumb.url))
                                .collect()
                        })
                        .collect(),
                )
            })
            .collect()
    }

    /// Returns identifiers registered for the same resolved target.
    fn get_aliases(&self, identifier: &str, from_url: &str) -> Vec<String> {
        let Ok(target) = self.get_url_from_id(identifier, from_url, true)
        else {
            return Vec::new();
        };
        if !is_relative(&target) {
            return Vec::new();
        }

        let Some(data) = self.backlink_data() else {
            return Vec::new();
        };
        data.aliases.get(&target).map_or_else(Vec::new, |aliases| {
            aliases
                .iter()
                .filter(|alias| alias.as_str() != identifier)
                .cloned()
                .collect()
        })
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

                let title_attr = self.title_attribute(
                    identifier,
                    title,
                    original_title.as_deref(),
                    optional,
                    external,
                );

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

    /// Builds a safely escaped title, preserving HTML only when configured.
    fn title_attribute(
        &self, identifier: &str, title: &str, original: Option<&str>,
        optional: bool, external: bool,
    ) -> String {
        if self.settings.link_titles == LinkTitles::None
            || self.settings.link_titles == LinkTitles::External && !external
        {
            return String::new();
        }

        let tooltip = if optional {
            let identifier_text = html_escape(identifier);
            let code = if self.settings.strip_title_tags {
                identifier_text
            } else {
                format!("<code>{identifier_text}</code>")
            };
            match original.filter(|title| !title.is_empty()) {
                Some(title) if title.contains(identifier) => title.to_string(),
                Some(title) => format!("{title} ({code})"),
                None => code,
            }
        } else {
            original.unwrap_or_default().to_string()
        };

        if tooltip.is_empty()
            || format!("<code>{title}</code>").contains(&tooltip)
        {
            return String::new();
        }
        let tooltip = if self.settings.strip_title_tags {
            strip_html(&tooltip)
        } else {
            tooltip
        };
        format!(" title=\"{}\"", html_escape(&tooltip))
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
}

// ----------------------------------------------------------------------------

impl BreadcrumbNode {
    /// Materializes this shared path from its root to its tail.
    fn to_vec(&self) -> Vec<BacklinkCrumb> {
        let mut crumbs = Vec::new();
        let mut current = Some(self);
        while let Some(node) = current {
            crumbs.push(node.crumb.clone());
            current = node.parent.as_deref();
        }
        crumbs.reverse();
        crumbs
    }
}

// ----------------------------------------------------------------------------

impl Registry {
    /// Returns the hash of the settled backlink inputs, when collected.
    pub fn backlink_revision(&self) -> Option<u64> {
        self.0
            .as_ref()
            .and_then(|resolver| resolver.backlink_index.as_ref())
            .map(|index| index.revision)
    }

    /// Creates a visitor for template references when autorefs is enabled.
    pub fn parser(&self) -> Option<Parser> {
        self.0.as_ref().map(|_| Parser::default())
    }

    /// Expands collected reference slots and records unresolved identifiers.
    pub fn replace_slots(
        &self, content: String, references: &References, from_url: &str,
        unresolved: &mut UnresolvedAutorefs,
    ) -> String {
        if let Some(autorefs) = &self.0 {
            autorefs.replace_slots(content, references, from_url, unresolved)
        } else {
            content
        }
    }

    /// Returns backlinks for identifiers relative to the rendering page.
    pub fn get_backlinks(
        &self, identifiers: &[String], from_url: &str,
    ) -> Backlinks {
        self.0.as_ref().map_or_else(Vec::new, |autorefs| {
            autorefs.get_backlinks(identifiers, from_url)
        })
    }

    /// Returns aliases that resolve to the same internal target.
    pub fn get_aliases(&self, identifier: &str, from_url: &str) -> Vec<String> {
        self.0.as_ref().map_or_else(Vec::new, |autorefs| {
            autorefs.get_aliases(identifier, from_url)
        })
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

impl Value for BacklinkInput {}

// ----------------------------------------------------------------------------

impl Value for BacklinkPages {}

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

/// Hashes an unordered URL map in stable key order.
fn hash_url_map<H>(map: &HashMap<String, Vec<String>>, state: &mut H)
where
    H: Hasher,
{
    let mut keys = map.keys().collect::<Vec<_>>();
    keys.sort_unstable();
    keys.len().hash(state);
    for key in keys {
        key.hash(state);
        map[key].hash(state);
    }
}

/// Creates a breadcrumb for a navigation item.
///
/// Navigation sections do not have URLs of their own. When a section contains
/// an index page, the section represents that page in navigation and should
/// link to it from backlink breadcrumbs as well.
fn navigation_crumb(
    item: &NavigationItem, page_titles: &HashMap<&str, &str>,
) -> BacklinkCrumb {
    let url = item.url.as_deref().or_else(|| {
        item.children
            .iter()
            .find(|child| child.is_index)
            .and_then(|child| child.url.as_deref())
    });
    if let Some(title) = url.and_then(|url| page_titles.get(url)) {
        return BacklinkCrumb {
            title: (*title).to_string(),
            url: url.unwrap_or_default().to_string(),
        };
    }
    BacklinkCrumb {
        title: item.display_title().unwrap_or_default().to_string(),
        url: url.unwrap_or_default().to_string(),
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

/// Strips title markup and decodes entities before HTML attribute escaping.
fn strip_html(input: &str) -> String {
    let mut text = String::new();
    let mut emitter =
        CallbackEmitter::new(|event: CallbackEvent<'_>, _: Span<usize>| {
            if let CallbackEvent::String { value } = event {
                text.push_str(&String::from_utf8_lossy(value));
            }
            None::<Infallible>
        });
    emitter.naively_switch_states(true);
    Tokenizer::new_with_emitter(input, emitter)
        .finish()
        .expect("string input is infallible");
    text
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use ahash::HashMap;
    use std::sync::{Arc, OnceLock};

    use crate::compat::mkdocs::html;
    use crate::config::plugins::{AutorefsPluginConfig, AutorefsTitleSetting};
    use crate::structure::nav::{Navigation, NavigationItem};
    use crate::structure::toc::Section;

    use super::{
        navigation_crumb, BacklinkCrumb, BacklinkData, BacklinkIndex,
        BreadcrumbNode, Facts, Parser, References, Resolver, Settings,
        UnresolvedAutorefs,
    };

    fn breadcrumb_path(
        crumbs: impl IntoIterator<Item = BacklinkCrumb>,
    ) -> Option<Arc<BreadcrumbNode>> {
        crumbs.into_iter().fold(None, |parent, crumb| {
            Some(Arc::new(BreadcrumbNode { crumb, parent }))
        })
    }

    fn attach_backlink_data(resolver: &mut Resolver, data: BacklinkData) {
        resolver.backlink_index = Some(BacklinkIndex {
            revision: 1,
            pages: Arc::default(),
            navigation: Navigation::new(Vec::new(), Vec::new()),
            data: OnceLock::from(Arc::new(data)),
        });
    }

    #[test]
    fn backlink_indexes_collect_lazily() {
        let mut resolver = Resolver::new();
        resolver.backlink_index = Some(BacklinkIndex {
            revision: 42,
            pages: Arc::default(),
            navigation: Navigation::new(Vec::new(), Vec::new()),
            data: OnceLock::new(),
        });

        assert!(resolver
            .backlink_index
            .as_ref()
            .unwrap()
            .data
            .get()
            .is_none());
        assert!(resolver
            .get_backlinks(&["target".into()], "api/")
            .is_empty());
        assert!(resolver
            .backlink_index
            .as_ref()
            .unwrap()
            .data
            .get()
            .is_some());
    }

    #[test]
    fn configured_primary_selection_preserves_secondary_resolution() {
        let mut resolver = Resolver::new();
        let urls = vec!["elsewhere/#item".into(), "guide/near/#item".into()];
        resolver.primary.insert("item".into(), urls.clone());
        resolver.secondary.insert("alias".into(), urls);

        for (resolve_closest, expected) in
            [(false, "../../elsewhere/#item"), (true, "../near/#item")]
        {
            resolver.settings.resolve_closest = resolve_closest;
            assert_eq!(
                resolver
                    .get_url_and_title_from_id("item", "guide/current/")
                    .unwrap()
                    .0,
                expected,
            );
            assert_eq!(
                resolver
                    .get_url_and_title_from_id("alias", "guide/current/")
                    .unwrap()
                    .0,
                "../near/#item",
            );
        }
    }

    #[test]
    fn title_modes_apply_to_reference_slots() {
        let input = concat!(
            "<autoref identifier=\"local\">Local</autoref>",
            "<autoref identifier=\"pkg.remote\" optional><code>remote</code></autoref>",
        );
        for (mode, features, internal_title, external_title) in [
            (AutorefsTitleSetting::Enabled(true), vec![], true, true),
            (
                AutorefsTitleSetting::Enabled(false),
                vec!["navigation.instant.preview".into()],
                false,
                false,
            ),
            (
                AutorefsTitleSetting::Mode("external".into()),
                vec![],
                false,
                true,
            ),
            (
                AutorefsTitleSetting::Mode("auto".into()),
                vec![],
                true,
                true,
            ),
            (
                AutorefsTitleSetting::Mode("auto".into()),
                vec!["navigation.instant.preview".into()],
                false,
                true,
            ),
        ] {
            let mut resolver = Resolver::new();
            resolver.settings = Settings::new(
                &AutorefsPluginConfig {
                    link_titles: mode,
                    ..Default::default()
                },
                &features,
            );
            resolver
                .primary
                .insert("local".into(), vec!["target/#local".into()]);
            resolver
                .titles
                .insert("target/#local".into(), "Canonical local".into());
            resolver.inventory.insert(
                "pkg.remote".into(),
                "//example.com/#pkg.remote".into(),
            );

            let (content, references) = prepare(input);
            let mut unresolved = UnresolvedAutorefs::default();
            let output = resolver.replace_slots(
                content,
                &references,
                "guide/",
                &mut unresolved,
            );

            assert_eq!(
                output.contains("title=\"Canonical local\""),
                internal_title
            );
            assert_eq!(output.contains("title=\"pkg.remote\""), external_title);
            assert!(output.contains("href=\"//example.com/#pkg.remote\""));
            assert!(output.contains("autorefs-external"));
            assert!(unresolved.iter().next().is_none());
        }
    }

    #[test]
    fn title_html_modes_preserve_text_and_escape_attributes() {
        let input = "<autoref identifier=\"item\">Label</autoref>";
        let rich =
            "A <em>rich</em> &amp; &quot;quoted&quot; title<!-- hidden -->";
        let plain_attr = "title=\"A rich &amp; &quot;quoted&quot; title\"";
        let html_attr = "title=\"A &lt;em&gt;rich&lt;/em&gt; &amp;amp; &amp;quot;quoted&amp;quot; title&lt;!-- hidden --&gt;\"";
        for (mode, features, expected) in [
            (
                AutorefsTitleSetting::Enabled(true),
                vec!["content.tooltips".into()],
                plain_attr,
            ),
            (AutorefsTitleSetting::Enabled(false), vec![], html_attr),
            (
                AutorefsTitleSetting::Mode("auto".into()),
                vec![],
                plain_attr,
            ),
            (
                AutorefsTitleSetting::Mode("auto".into()),
                vec!["content.tooltips".into()],
                html_attr,
            ),
        ] {
            let mut resolver = Resolver::new();
            resolver.settings = Settings::new(
                &AutorefsPluginConfig {
                    strip_title_tags: mode,
                    ..Default::default()
                },
                &features,
            );
            resolver
                .primary
                .insert("item".into(), vec!["target/#item".into()]);
            resolver.titles.insert("target/#item".into(), rich.into());

            let (content, references) = prepare(input);
            let mut unresolved = UnresolvedAutorefs::default();
            let output = resolver.replace_slots(
                content,
                &references,
                "guide/",
                &mut unresolved,
            );

            assert!(output.contains(expected), "{output}");
            assert!(output.ends_with(">Label</a>"));
        }
    }

    #[test]
    fn optional_titles_append_identifiers_and_suppress_redundancy() {
        let mut resolver = Resolver::new();
        for (strip, expected) in [
            (true, " title=\"Canonical (pkg.item)\""),
            (
                false,
                " title=\"Canonical (&lt;code&gt;pkg.item&lt;/code&gt;)\"",
            ),
        ] {
            resolver.settings.strip_title_tags = strip;
            assert_eq!(
                resolver.title_attribute(
                    "pkg.item",
                    "item",
                    Some("Canonical"),
                    true,
                    false
                ),
                expected,
            );
            assert_eq!(
                resolver.title_attribute(
                    "pkg.item",
                    "<code>pkg.item</code>",
                    None,
                    true,
                    false
                ),
                "",
            );
            assert_eq!(
                resolver.title_attribute(
                    "pkg.item",
                    "Canonical",
                    Some("Canonical"),
                    false,
                    false
                ),
                "",
            );
        }
    }

    fn prepare(input: &str) -> (String, References) {
        let mut parser = Parser::default();
        let content = html::scan(input, &mut [&mut parser])
            .unwrap_or_else(|| input.to_string());
        let (references, _) = parser.finish();
        (content, references)
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

        let (content, references) = prepare(concat!(
            "<autoref identifier=\"known\">Known</autoref>",
            "<autoref identifier=\"missing\">Missing</autoref>",
            "<autoref identifier=\"missing\">Missing</autoref>",
            "<autoref identifier=\"skipped\" optional>Skipped</autoref>",
        ));
        let mut unresolved = UnresolvedAutorefs::default();

        // Resolve collected slots while retaining each missing identifier once.
        let output = autorefs.replace_slots(
            content,
            &references,
            "guide/",
            &mut unresolved,
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

        let mut unresolved = UnresolvedAutorefs::default();

        let output = autorefs.replace_slots(
            content,
            &references,
            "guide/",
            &mut unresolved,
        );

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

        let mut unresolved = UnresolvedAutorefs::default();

        let output = autorefs.replace_slots(
            content,
            &references,
            "guide/",
            &mut unresolved,
        );

        assert_eq!(
            output,
            concat!(
                "<a class=\"autorefs autorefs-internal\" ",
                "href=\"../reference/#foo-bar\">Foo bar</a>",
            )
        );
        assert!(unresolved.iter().next().is_none());
    }

    #[test]
    fn backlinks_include_navigation_and_nested_toc_breadcrumbs() {
        let mut autorefs = Resolver::new();
        let mut data = BacklinkData::default();
        let nested = Section {
            title: "Details".into(),
            content: "Details".into(),
            id: "details".into(),
            url: "#details".into(),
            children: Vec::new(),
            level: 2,
        };
        let root = Section {
            title: "Guide".into(),
            content: "Guide".into(),
            id: "guide".into(),
            url: "#guide".into(),
            children: vec![nested],
            level: 1,
        };
        let manual = BacklinkCrumb {
            title: "Manual".into(),
            url: String::new(),
        };
        let ancestors = breadcrumb_path([manual.clone()]);
        let page_path = breadcrumb_path([
            manual,
            BacklinkCrumb {
                title: "Guide page".into(),
                url: "guide/".into(),
            },
        ]);
        Resolver::register_section(
            &mut data,
            "guide/",
            &root,
            ancestors.as_ref(),
            page_path.as_ref(),
            None,
            true,
        );
        data.backlinks
            .entry("target".into())
            .or_default()
            .entry("referenced-by".into())
            .or_default()
            .insert("guide/#details".into());
        attach_backlink_data(&mut autorefs, data);

        assert_eq!(
            autorefs.get_backlinks(&["target".into()], "api/"),
            vec![(
                "referenced-by".into(),
                vec![vec![
                    ("Manual".into(), String::new()),
                    ("Guide".into(), "../guide/#guide".into()),
                    ("Details".into(), "../guide/#details".into()),
                ]],
            )],
        );
    }

    #[test]
    fn indexed_navigation_sections_link_to_their_index_pages() {
        let index = NavigationItem {
            title: Some("Getting started".into()),
            url: Some("getting-started/".into()),
            canonical_url: None,
            meta: None,
            children: Vec::new(),
            is_index: true,
            active: false,
        };
        let section = NavigationItem {
            title: Some("Getting started".into()),
            url: None,
            canonical_url: None,
            meta: None,
            children: vec![index],
            is_index: false,
            active: false,
        };
        let page_titles = HashMap::default();

        assert_eq!(
            navigation_crumb(&section, &page_titles),
            BacklinkCrumb {
                title: "Getting started".into(),
                url: "getting-started/".into(),
            }
        );
    }

    #[test]
    fn navigation_sections_without_index_pages_remain_unlinked() {
        let section = NavigationItem {
            title: Some("Concepts".into()),
            url: None,
            canonical_url: None,
            meta: None,
            children: Vec::new(),
            is_index: false,
            active: false,
        };
        let page_titles = HashMap::default();

        assert_eq!(
            navigation_crumb(&section, &page_titles),
            BacklinkCrumb {
                title: "Concepts".into(),
                url: String::new(),
            }
        );
    }

    #[test]
    fn page_h1_merges_with_its_indexed_navigation_crumb() {
        let index = NavigationItem {
            title: Some("Models".into()),
            url: Some("models/".into()),
            canonical_url: None,
            meta: None,
            children: Vec::new(),
            is_index: true,
            active: false,
        };
        let navigation = NavigationItem {
            title: Some("Models".into()),
            url: None,
            canonical_url: None,
            meta: None,
            children: vec![index],
            is_index: false,
            active: false,
        };
        let models = BacklinkCrumb {
            title: "<strong>Models</strong>".into(),
            url: "models/".into(),
        };
        let page_titles =
            HashMap::from_iter([("models/", "<strong>Models</strong>")]);
        let ancestors =
            breadcrumb_path([navigation_crumb(&navigation, &page_titles)]);
        let object = Section {
            title: "<code>class</code> Object".into(),
            content: String::new(),
            id: "griffe.Object".into(),
            url: "#griffe.Object".into(),
            children: Vec::new(),
            level: 2,
        };
        let models_heading = Section {
            title: "<strong>Models</strong>".into(),
            content: String::new(),
            id: "models".into(),
            url: "#models".into(),
            children: vec![object],
            level: 1,
        };
        let alias_heading = Section {
            title: "<code>class</code> Alias".into(),
            content: String::new(),
            id: "griffe.Alias".into(),
            url: "#griffe.Alias".into(),
            children: Vec::new(),
            level: 1,
        };
        let mut data = BacklinkData::default();
        Resolver::register_section(
            &mut data,
            "models/",
            &models_heading,
            ancestors.as_ref(),
            ancestors.as_ref(),
            None,
            true,
        );
        let alias_page_path = Some(Arc::new(BreadcrumbNode {
            crumb: BacklinkCrumb {
                title: "Alias".into(),
                url: "models/alias/".into(),
            },
            parent: ancestors.clone(),
        }));
        Resolver::register_section(
            &mut data,
            "models/alias/",
            &alias_heading,
            ancestors.as_ref(),
            alias_page_path.as_ref(),
            None,
            true,
        );

        assert_eq!(
            data.breadcrumbs["models/#griffe.Object"].to_vec(),
            [
                models.clone(),
                BacklinkCrumb {
                    title: "<code>class</code> Object".into(),
                    url: "models/#griffe.Object".into(),
                },
            ]
        );
        assert_eq!(
            data.breadcrumbs["models/alias/#griffe.Alias"].to_vec(),
            [
                models,
                BacklinkCrumb {
                    title: "<code>class</code> Alias".into(),
                    url: "models/alias/#griffe.Alias".into(),
                },
            ]
        );
    }

    #[test]
    fn backlink_aliases_can_be_recovered_from_registered_urls() {
        let mut autorefs = Resolver::new();
        autorefs
            .primary
            .insert("public.Target".into(), vec!["api/#public.Target".into()]);
        autorefs
            .secondary
            .insert("package.Target".into(), vec!["api/#public.Target".into()]);
        autorefs.secondary.insert(
            "package._core.Target".into(),
            vec!["api/#public.Target".into()],
        );
        let mut data = BacklinkData::default();
        autorefs.index_aliases(&mut data);
        attach_backlink_data(&mut autorefs, data);

        assert_eq!(
            autorefs.get_aliases("public.Target", "api/"),
            ["package.Target", "package._core.Target"],
        );
    }
}
