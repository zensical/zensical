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

//! Page.

use minijinja::{context, Error, Value as TemplateValue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

use zensical_serve::http::Uri;
use zrx::id::Id;
use zrx::scheduler::Value;

use crate::config::{Config, Project};
use crate::path::{PathError, SitePath, SourcePath};
use crate::template::{Output, Template, GENERATOR};

use super::document::DocumentHeader;
use super::dynamic::Dynamic;
use super::markdown::Markdown;
use super::nav::{Navigation, NavigationItem, NavigationView};
use super::tag::Tag;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Stable route facts derived from one Markdown source.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRoute {
    /// Documentation-relative source URI.
    pub source: SourcePath,
    /// Site-relative destination URI.
    pub destination: SitePath,
    /// Encoded page URL.
    pub url: String,
}

impl Value for PageRoute {}

// ----------------------------------------------------------------------------

/// Stable ownership and edit provenance of a page.
#[derive(
    Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum PageOrigin {
    /// Page backed by a document in the documentation source tree.
    Source(SourcePath),
    /// Page emitted by a native module under a stable logical identity.
    Generated {
        /// Module-owned identity, independent of the page's current route.
        identity: String,
        /// Optional source document from which edit links can be derived.
        provenance: Option<SourcePath>,
    },
}

// ----------------------------------------------------------------------------

/// Pre-render page descriptor consumed by the shared page pipeline.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageDescriptor {
    /// Stable owner and optional edit provenance.
    pub origin: PageOrigin,
    /// Resolved Markdown document facts.
    pub document: DocumentHeader,
    /// Final route, which may differ from the document's default route.
    pub route: PageRoute,
    /// Module-owned fields flattened into the template-facing page object.
    pub properties: BTreeMap<String, Dynamic>,
    /// Module-owned top-level template variables.
    pub variables: BTreeMap<String, Dynamic>,
}

impl Value for PageDescriptor {}

// ----------------------------------------------------------------------------

/// Immutable page data shared between scheduler branches.
///
/// Page values are cloned by the scheduler as they fan out into navigation,
/// search, validation, and rendering branches. Keeping the immutable payload
/// behind an [`Arc`] makes those clones constant-sized.
#[derive(Clone, Debug, Serialize)]
pub struct PageData {
    /// Stable owner and optional physical-source provenance.
    #[serde(skip)]
    origin: PageOrigin,
    /// Validated logical source URI used by navigation and link resolution.
    #[serde(skip)]
    source: SourcePath,
    /// Validated site-relative output used by the writer.
    #[serde(skip)]
    destination: SitePath,
    /// Page target URL.
    pub url: String,
    /// Page canonical URL.
    pub canonical_url: Option<String>,
    /// Page edit URL.
    pub edit_url: Option<String>,
    /// Page file system path.
    pub path: String,
    /// Effective page title, including an explicit navigation title.
    pub title: String,
    /// Module-owned template-facing page fields.
    #[serde(flatten)]
    properties: BTreeMap<String, Dynamic>,
    /// Rendered Markdown shared with the upstream value.
    #[serde(flatten)]
    markdown: Markdown,
}

/// Page.
///
/// This data type contains all data necessary for rendering a page, including
/// its content, metadata, table of contents, and relations to other pages. The
/// immutable render inputs are shared across scheduler branches, while page
/// relations remain local because they are populated during rendering.
#[allow(clippy::struct_field_names)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Page {
    /// Immutable page data.
    #[serde(flatten)]
    data: Arc<PageData>,
    /// Ancestor pages.
    pub ancestors: Vec<NavigationItem>,
    /// Previous page.
    pub previous_page: Option<NavigationItem>,
    /// Next page.
    pub next_page: Option<NavigationItem>,
    /// Dynamic page-level template variables supplied by compatibility modules.
    #[serde(skip)]
    template_variables: Option<BTreeMap<String, Dynamic>>,
    /// Visible navigation item represented by this page, when different.
    #[serde(skip)]
    navigation_url: Option<String>,
    /// Sibling relations for a page hidden from visible navigation.
    #[serde(skip)]
    navigation_siblings:
        Option<(Option<NavigationItem>, Option<NavigationItem>)>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl PageRoute {
    /// Computes route facts for a source identifier.
    pub fn new(config: &Config, id: &Id) -> Result<Self, PathError> {
        Self::from_source(config, id.location().parse()?)
    }

    /// Computes route facts for a documentation-relative source URI.
    pub fn from_source(
        config: &Config, source: SourcePath,
    ) -> Result<Self, PathError> {
        let destination =
            Self::destination(&source, config.project.use_directory_urls)?;
        let url = route_url(&destination, config.project.use_directory_urls);
        Ok(Self { source, destination, url })
    }

    /// Creates route facts for a logical source and explicit destination.
    pub fn from_destination(
        config: &Config, source: SourcePath, destination: SitePath,
    ) -> Self {
        let url = route_url(&destination, config.project.use_directory_urls);
        Self { source, destination, url }
    }

    /// Computes the site-relative destination for a Markdown source.
    pub fn destination(
        source: &SourcePath, use_directory_urls: bool,
    ) -> Result<SitePath, PathError> {
        destination(source, use_directory_urls)
    }
}

// ----------------------------------------------------------------------------

impl PageDescriptor {
    /// Creates an ordinary source-backed page descriptor.
    pub fn source(
        config: &Config, document: DocumentHeader,
    ) -> Result<Self, PathError> {
        let origin = PageOrigin::Source(document.source.clone());
        let route = PageRoute::from_source(config, document.source.clone())?;
        Ok(Self {
            origin,
            document,
            route,
            properties: BTreeMap::new(),
            variables: BTreeMap::new(),
        })
    }

    /// Creates a generated descriptor with an explicit stable identity.
    pub fn generated(
        identity: impl Into<String>, provenance: Option<SourcePath>,
        document: DocumentHeader, route: PageRoute,
    ) -> Self {
        debug_assert_eq!(document.source, route.source);
        Self {
            origin: PageOrigin::Generated {
                identity: identity.into(),
                provenance,
            },
            document,
            route,
            properties: BTreeMap::new(),
            variables: BTreeMap::new(),
        }
    }
}

// ----------------------------------------------------------------------------

impl Page {
    /// Creates a page.
    #[allow(clippy::similar_names)]
    pub fn new(
        config: &Config, route: PageRoute, markdown: Markdown, title: String,
    ) -> Page {
        let origin = PageOrigin::Source(route.source.clone());
        Self::from_origin(config, origin, route, markdown, title)
    }

    /// Creates a generated page without requiring a physical Markdown file.
    pub fn generated(
        config: &Config, identity: String, provenance: Option<SourcePath>,
        route: PageRoute, markdown: Markdown, title: String,
    ) -> Page {
        Self::from_origin(
            config,
            PageOrigin::Generated { identity, provenance },
            route,
            markdown,
            title,
        )
    }

    /// Creates a rendered page from a unified pre-render descriptor.
    pub fn from_descriptor(
        config: &Config, descriptor: &PageDescriptor, markdown: Markdown,
        title: String,
    ) -> Page {
        let mut page = Self::from_origin(
            config,
            descriptor.origin.clone(),
            descriptor.route.clone(),
            markdown,
            title,
        );
        Arc::make_mut(&mut page.data).properties =
            descriptor.properties.clone();
        if !descriptor.variables.is_empty() {
            page.template_variables = Some(descriptor.variables.clone());
        }
        page
    }

    /// Creates a page with explicit logical ownership.
    #[allow(clippy::similar_names)]
    fn from_origin(
        config: &Config, origin: PageOrigin, route: PageRoute,
        markdown: Markdown, title: String,
    ) -> Page {
        let path = config.output_root().join(&route.destination);
        let source = route.source;
        let destination = route.destination;
        // Retrieve site URL
        let site_url = config.project.site_url.clone();

        // Retrieve repository URL and edit URI
        let repo_url = config.project.repo_url.clone();
        let edit_uri = config.project.edit_uri.clone();

        // Compute canonical URL
        let url = route.url;
        let canonical_url = site_url.as_ref().map(|base| {
            let base = base.trim_end_matches('/');
            format!("{base}/{url}")
        });

        // Compute edit URL - edit URIs can be relative or absolute, as both
        // variants are supported by MkDocs, so we mirror behavior for now
        let edit_source = match &origin {
            PageOrigin::Source(source) => Some(source),
            PageOrigin::Generated { provenance, .. } => provenance.as_ref(),
        };
        let edit_url = edit_source.and_then(|source| {
            repo_url.clone().and_then(|repo_url| {
                edit_uri.clone().map(|uri| {
                    if uri.starts_with("https://") {
                        format!("{uri}/{source}")
                    } else {
                        format!("{repo_url}/{uri}/{source}")
                    }
                })
            })
        });

        // Return page - note that ancestors, as well as previous and next
        // pages are populated when the navigation is created. This is also a
        // hint that it's not a good idea to centralize all propeties in a
        // single struct, but to split up the page as necessary later on.
        Page {
            data: Arc::new(PageData {
                origin,
                source,
                destination,
                url,
                canonical_url,
                edit_url,
                path: path
                    .to_str()
                    .expect("configured output path is valid UTF-8")
                    .into(),
                title,
                properties: BTreeMap::new(),
                markdown,
            }),
            ancestors: Vec::new(),
            previous_page: None,
            next_page: None,
            template_variables: None,
            navigation_url: None,
            navigation_siblings: None,
        }
    }

    /// Renders the page template, leaving autorefs unresolved.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip_all, fields(url = %self.url))
    )]
    pub fn render_template(
        &mut self, template: &Template, config: &Config, nav: Navigation,
        project: &Arc<Project>,
    ) -> Result<Output, Error> {
        let name = match self.meta.get("template") {
            Some(Dynamic::String(value)) => value.clone(),
            _ => "main.html".into(),
        };

        // Compute page relations from the immutable navigation.
        let navigation_url = self
            .navigation_url
            .clone()
            .unwrap_or_else(|| self.url.clone());
        self.ancestors = nav.ancestors_for_url(&navigation_url);
        self.previous_page = nav.previous_page_for_url(&navigation_url);
        self.next_page = nav.next_page_for_url(&navigation_url);
        if let Some((previous, next)) = &self.navigation_siblings {
            self.previous_page = previous.clone();
            self.next_page = next.clone();
        }

        // Add the page-local active overlay without cloning the navigation tree.
        let nav = NavigationView::new(nav, Some(&navigation_url));
        let variables = self.template_variables.clone().unwrap_or_else(|| {
            BTreeMap::from([(
                String::from("tags"),
                Dynamic::from_serialize(&self.tags())
                    .expect("tag template data is JSON compatible"),
            )])
        });
        let output = template.render_with_context(
            &name,
            context! {
                generator => GENERATOR,
                nav => TemplateValue::from_object(nav),
                base_url => config.get_base_url(&self.url),
                extra_css => project.extra_css.clone(),
                extra_javascript => project.extra_javascript.clone(),
                config => project.clone(),
                page => self,
                .. TemplateValue::from_serialize(&variables),
            },
        )?;

        Ok(Output::from(output))
    }

    /// Returns the tags of the page.
    pub fn tags(&self) -> Vec<Tag> {
        let mut tags = Vec::new();
        if let Some(Dynamic::List(values)) = self.meta.get("tags") {
            for name in values {
                tags.push(Tag {
                    name: name.to_string(),
                    parent: None,
                    url: None,
                    hidden: false,
                    links: Vec::new(),
                });
            }
        }
        tags
    }

    /// Returns the validated site-relative page output.
    pub fn destination(&self) -> &SitePath {
        &self.destination
    }

    /// Returns the stable logical owner and optional edit provenance.
    pub fn origin(&self) -> &PageOrigin {
        &self.origin
    }

    /// Returns the validated logical source URI.
    pub fn source(&self) -> &SourcePath {
        &self.source
    }

    /// Adds module-derived template context to a page-render cache key.
    pub fn hash_derived_template_context<H: Hasher>(&self, state: &mut H) {
        self.template_variables.hash(state);
        self.properties.hash(state);
        self.navigation_url.hash(state);
        self.navigation_siblings.hash(state);
    }

    /// Replaces page-local content, table of contents, and template variables.
    pub fn apply_derived(
        &mut self, content: Option<String>,
        toc: Option<Vec<crate::structure::toc::Section>>,
        variables: BTreeMap<String, Dynamic>,
    ) {
        if content.is_some() || toc.is_some() {
            Arc::make_mut(&mut self.data)
                .markdown
                .replace_derived(content, toc);
        }
        match &mut self.template_variables {
            Some(existing) => existing.extend(variables),
            None => self.template_variables = Some(variables),
        }
    }

    /// Applies module-owned page properties and top-level template variables.
    pub(crate) fn apply_template_context(
        &mut self, properties: BTreeMap<String, Dynamic>,
        variables: BTreeMap<String, Dynamic>,
    ) {
        Arc::make_mut(&mut self.data).properties = properties;
        if !variables.is_empty() {
            self.template_variables = Some(variables);
        }
    }

    /// Merges module-owned top-level template variables into this page.
    pub(crate) fn merge_template_variables(
        &mut self, variables: BTreeMap<String, Dynamic>,
    ) {
        self.template_variables
            .get_or_insert_with(BTreeMap::new)
            .extend(variables);
    }

    /// Deep-merges module-owned page properties.
    pub(crate) fn merge_properties(
        &mut self, properties: &BTreeMap<String, Dynamic>,
    ) {
        merge_properties(
            &mut Arc::make_mut(&mut self.data).properties,
            properties,
        );
    }

    /// Uses another page's visible navigation position for this page.
    pub(crate) fn apply_navigation_url(&mut self, url: String) {
        self.navigation_url = Some(url);
    }

    /// Applies sibling relations for a page hidden from visible navigation.
    pub(crate) fn apply_navigation_siblings(
        &mut self, previous: Option<NavigationItem>,
        next: Option<NavigationItem>,
    ) {
        self.navigation_siblings = Some((previous, next));
    }

    /// Applies a module-selected page template after view classification.
    pub(crate) fn apply_template(&mut self, template: String) {
        Arc::make_mut(&mut self.data)
            .markdown
            .insert_meta("template".into(), Dynamic::String(template));
    }

    /// Applies the title assigned to this page by navigation.
    pub(crate) fn apply_navigation_title(&mut self, title: &str) {
        if title != self.title {
            title.clone_into(&mut Arc::make_mut(&mut self.data).title);
        }
    }
}

fn merge_properties(
    target: &mut BTreeMap<String, Dynamic>, source: &BTreeMap<String, Dynamic>,
) {
    for (name, value) in source {
        match (target.get_mut(name), value) {
            (Some(Dynamic::Map(target)), Dynamic::Map(source)) => {
                merge_properties(target, source);
            }
            _ => {
                target.insert(name.clone(), value.clone());
            }
        }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Page {}

// ----------------------------------------------------------------------------

impl PartialEq for PageData {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
            && self.origin == other.origin
            && self.source == other.source
            && self.destination == other.destination
            && self.canonical_url == other.canonical_url
            && self.edit_url == other.edit_url
            && self.title == other.title
            && self.properties == other.properties
            && self.meta == other.meta
            && self.path == other.path
            && self.content == other.content
            && self.toc == other.toc
    }
}

impl Eq for PageData {}

// ----------------------------------------------------------------------------

impl Deref for PageData {
    type Target = Markdown;

    /// Dereferences to rendered Markdown data.
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.markdown
    }
}

// ----------------------------------------------------------------------------

impl Deref for Page {
    type Target = PageData;

    /// Dereferences to immutable page data.
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

// ----------------------------------------------------------------------------
// Type aliases
// ----------------------------------------------------------------------------

/// Page metadata.
pub type PageMeta = BTreeMap<String, Dynamic>;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Computes the site-relative destination for a Markdown source.
fn destination(
    source: &SourcePath, use_directory_urls: bool,
) -> Result<SitePath, PathError> {
    let parent = source.parent();
    let name = source.file_name();
    let is_index = matches!(name, "index.md" | "README.md");
    let stem = if name == "README.md" {
        "index"
    } else {
        source.file_stem()
    };
    let output = if use_directory_urls && !is_index {
        format!("{stem}/index.html")
    } else {
        format!("{stem}.html")
    };
    let destination = match parent {
        Some(parent) => format!("{parent}/{output}"),
        None => output,
    };
    destination.parse()
}

/// Computes the encoded URL for a site-relative destination.
fn route_url(destination: &SitePath, use_directory_urls: bool) -> String {
    let url = if use_directory_urls {
        destination.as_str().trim_end_matches("index.html")
    } else {
        destination.as_str()
    };
    Uri::from(url).to_string()
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::{
        destination, route_url, Page, PageData, PageDescriptor, PageOrigin,
        PageRoute,
    };
    use crate::structure::document::DocumentHeader;

    fn page() -> Page {
        let markdown = serde_json::from_value(json!({
            "title": "Home",
            "meta": {},
            "content": "<h1>Home</h1>",
            "toc": [],
            "search": [],
        }))
        .unwrap();
        Page {
            data: Arc::new(PageData {
                origin: PageOrigin::Source("index.md".parse().unwrap()),
                source: "index.md".parse().unwrap(),
                destination: "index.html".parse().unwrap(),
                url: String::from("/"),
                canonical_url: None,
                edit_url: None,
                path: String::from("site/index.html"),
                title: String::from("Home"),
                properties: BTreeMap::new(),
                markdown,
            }),
            ancestors: Vec::new(),
            previous_page: None,
            next_page: None,
            template_variables: None,
            navigation_url: None,
            navigation_siblings: None,
        }
    }

    #[test]
    fn clone_shares_immutable_data() {
        let page = page();
        let clone = page.clone();

        assert!(Arc::ptr_eq(&page.data, &clone.data));
    }

    #[test]
    fn computes_mkdocs_destinations() {
        let cases = [
            ("index.md", true, "index.html"),
            ("README.md", true, "index.html"),
            ("guide/README.md", true, "guide/index.html"),
            ("guide/index.md", true, "guide/index.html"),
            ("guide/page.md", true, "guide/page/index.html"),
            ("myindex.md", true, "myindex/index.html"),
            ("guide/page.md", false, "guide/page.html"),
            ("guide/README.md", false, "guide/index.html"),
            ("café/100%.md", true, "café/100%/index.html"),
        ];
        for (source, directory_urls, expected) in cases {
            let source = source.parse().unwrap();
            assert_eq!(
                destination(&source, directory_urls).unwrap().as_str(),
                expected
            );
        }
    }

    #[test]
    fn computes_encoded_urls() {
        let cases = [
            ("100%/index.html", true, "100%25/"),
            ("100%.html", false, "100%25.html"),
            ("café/index.html", true, "caf%C3%A9/"),
            ("myindex/index.html", true, "myindex/"),
        ];
        for (destination, directory_urls, expected) in cases {
            assert_eq!(
                route_url(&destination.parse().unwrap(), directory_urls),
                expected
            );
        }
    }

    #[test]
    fn route_serialization_contains_only_logical_facts() {
        let route = PageRoute {
            source: "guide/café.md".parse().unwrap(),
            destination: "guide/café/index.html".parse().unwrap(),
            url: "guide/caf%C3%A9/".into(),
        };
        let value = serde_json::to_value(&route).unwrap();

        assert_eq!(value["source"], "guide/café.md");
        assert_eq!(value["destination"], "guide/café/index.html");
        assert_eq!(value["url"], "guide/caf%C3%A9/");
        assert!(value.get("path").is_none());
        assert_eq!(serde_json::from_value::<PageRoute>(value).unwrap(), route);
    }

    #[test]
    fn generated_identity_is_independent_of_route_and_provenance() {
        let document = DocumentHeader::new(
            "blog/page/2/index.md".parse().unwrap(),
            "# Blog".into(),
            BTreeMap::default(),
        );
        let route = PageRoute {
            source: document.source.clone(),
            destination: "blog/page/2/index.html".parse().unwrap(),
            url: "blog/page/2/".into(),
        };
        let descriptor = PageDescriptor::generated(
            "blog:main:2",
            Some("blog/index.md".parse().unwrap()),
            document,
            route,
        );

        assert!(matches!(
            descriptor.origin,
            PageOrigin::Generated {
                ref identity,
                provenance: Some(_),
            } if identity == "blog:main:2"
        ));
        assert_eq!(descriptor.route.url, "blog/page/2/");
    }

    #[test]
    fn serialization_keeps_flat_page_shape() {
        let value = serde_json::to_value(page()).unwrap();

        assert_eq!(value["url"], "/");
        assert_eq!(value["title"], "Home");
        assert!(value.get("source").is_none());
        assert!(value.get("destination").is_none());
        assert!(value.get("data").is_none());
    }
}
