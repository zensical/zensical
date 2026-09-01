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
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
use zensical_serve::http::Uri;
use zrx::id::Id;
use zrx::scheduler::Value;

use crate::config::{Config, Project};
use crate::template::{Output, Template, GENERATOR};

use super::dynamic::Dynamic;
use super::markdown::Markdown;
use super::nav::{Navigation, NavigationItem, NavigationView};
use super::tag::Tag;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Stable route facts derived from one Markdown source.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PageRoute {
    /// Documentation-relative source URI.
    pub source: String,
    /// Site-relative destination URI.
    pub destination: String,
    /// Encoded page URL.
    pub url: String,
    /// Absolute destination path.
    pub path: String,
}

impl Value for PageRoute {}

// ----------------------------------------------------------------------------

/// Immutable page data shared between scheduler branches.
///
/// Page values are cloned by the scheduler as they fan out into navigation,
/// search, validation, and rendering branches. Keeping the immutable payload
/// behind an [`Arc`] makes those clones constant-sized.
#[derive(Debug, Serialize)]
pub struct PageData {
    /// Page target URL.
    pub url: String,
    /// Page canonical URL.
    pub canonical_url: Option<String>,
    /// Page edit URL.
    pub edit_url: Option<String>,
    /// Page file system path.
    pub path: String,
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
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl PageRoute {
    /// Computes route facts for a source identifier.
    pub(crate) fn new(config: &Config, id: &Id) -> Self {
        Self::from_source(config, &id.location())
    }

    /// Computes route facts for a documentation-relative source URI.
    pub(crate) fn from_source(config: &Config, source: &str) -> Self {
        let destination =
            Self::destination(source, config.project.use_directory_urls);
        let url = route_url(&destination, config.project.use_directory_urls);
        let path = config.get_site_dir().join(&destination);
        Self {
            source: source.into(),
            destination,
            url,
            path: path.to_string_lossy().into_owned(),
        }
    }

    /// Computes the site-relative destination for a Markdown source.
    pub(crate) fn destination(
        source: &str, use_directory_urls: bool,
    ) -> String {
        destination(source, use_directory_urls)
    }
}

// ----------------------------------------------------------------------------

impl Page {
    /// Creates a page.
    #[allow(clippy::similar_names)]
    pub(crate) fn new(
        config: &Config, route: PageRoute, markdown: Markdown,
    ) -> Page {
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
        let edit_url = repo_url.clone().and_then(|repo_url| {
            edit_uri.clone().map(|uri| {
                if uri.starts_with("https://") {
                    format!("{uri}/{}", route.source)
                } else {
                    format!("{repo_url}/{uri}/{}", route.source)
                }
            })
        });

        // Return page - note that ancestors, as well as previous and next
        // pages are populated when the navigation is created. This is also a
        // hint that it's not a good idea to centralize all propeties in a
        // single struct, but to split up the page as necessary later on.
        Page {
            data: Arc::new(PageData {
                url,
                canonical_url,
                edit_url,
                path: route.path,
                markdown,
            }),
            ancestors: Vec::new(),
            previous_page: None,
            next_page: None,
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
        self.ancestors = nav.ancestors(self);
        self.previous_page = nav.previous_page(self);
        self.next_page = nav.next_page(self);

        // Add the page-local active overlay without cloning the navigation tree.
        let nav = NavigationView::new(nav, Some(&self.url));
        let output = template.render_with_context(
            &name,
            context! {
                generator => GENERATOR,
                nav => TemplateValue::from_object(nav),
                base_url => config.get_base_url(&self.url),
                extra_css => project.extra_css.clone(),
                extra_javascript => project.extra_javascript.clone(),
                config => project.clone(),
                tags => self.tags(),
                page => self,
            },
        )?;

        Ok(Output::from(output))
    }

    /// Returns the tags of the page.
    pub fn tags(&self) -> Vec<Tag> {
        let mut tags = Vec::new();
        if let Some(Dynamic::List(values)) = self.meta.get("tags") {
            for name in values {
                tags.push(Tag { name: name.to_string() });
            }
        }
        tags
    }
}

// ----------------------------------------------------------------------------

/// Computes the site-relative destination for a Markdown source.
fn destination(source: &str, use_directory_urls: bool) -> String {
    let mut path = PathBuf::from(source);
    let is_index = path.ends_with("index.md") || path.ends_with("README.md");
    if path.ends_with("README.md") {
        path.pop();
        path.push("index.md");
    }
    if !use_directory_urls || is_index {
        path.set_extension("html");
    } else {
        path.set_extension("");
        path.push("index.html");
    }
    path.to_string_lossy().replace('\\', "/")
}

/// Computes the encoded URL for a site-relative destination.
fn route_url(destination: &str, use_directory_urls: bool) -> String {
    let url = if use_directory_urls {
        destination.trim_end_matches("index.html")
    } else {
        destination
    };
    Uri::from(url).to_string()
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Page {}

// ----------------------------------------------------------------------------

impl PartialEq for PageData {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
            && self.canonical_url == other.canonical_url
            && self.edit_url == other.edit_url
            && self.title == other.title
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
// Type alises
// ----------------------------------------------------------------------------

/// Page metadata.
pub type PageMeta = BTreeMap<String, Dynamic>;

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
                url: String::from("/"),
                canonical_url: None,
                edit_url: None,
                path: String::from("site/index.html"),
                markdown,
            }),
            ancestors: Vec::new(),
            previous_page: None,
            next_page: None,
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
        assert_eq!(destination("index.md", true), "index.html");
        assert_eq!(destination("README.md", true), "index.html");
        assert_eq!(destination("guide/README.md", true), "guide/index.html");
        assert_eq!(destination("guide/page.md", true), "guide/page/index.html");
        assert_eq!(destination("guide/page.md", false), "guide/page.html");
    }

    #[test]
    fn computes_encoded_urls() {
        assert_eq!(route_url("100%/index.html", true), "100%25/");
        assert_eq!(route_url("100%.html", false), "100%25.html");
    }

    #[test]
    fn serialization_keeps_flat_page_shape() {
        let value = serde_json::to_value(page()).unwrap();

        assert_eq!(value["url"], "/");
        assert_eq!(value["title"], "Home");
        assert!(value.get("data").is_none());
    }
}
