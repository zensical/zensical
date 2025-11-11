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

//! Page.

use minijinja::{context, Error};
use pyo3::FromPyObject;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use zensical_serve::http::Uri;
use zrx::id::Id;
use zrx::scheduler::Value;

use crate::config::Config;
use crate::template::{Template, GENERATOR};

use super::dynamic::Dynamic;
use super::markdown::Markdown;
use super::nav::{Navigation, NavigationItem};
use super::search::SearchItem;
use super::tag::Tag;
use super::toc::Section;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Page.
///
/// This data type contains all data necessary for rendering a page, including
/// its content, metadata, table of contents, and relations to other pages. In
/// the future, we're going to split this up into smaller components, to make
/// rendering more modular, but right now, we just replicate what MkDocs does.
#[allow(clippy::struct_field_names)]
#[derive(Clone, Debug, PartialEq, Eq, FromPyObject, Serialize)]
#[pyo3(from_item_all)]
pub struct Page {
    /// Page target URL.
    pub url: String,
    /// Page canonical URL.
    pub canonical_url: Option<String>,
    /// Page edit URL.
    pub edit_url: Option<String>,
    /// Page title.
    pub title: String,
    /// Page metadata.
    pub meta: PageMeta,
    /// Page file system path.
    pub path: String,
    /// Page content.
    pub content: String,
    /// Table of contents.
    pub toc: Vec<Section>,
    /// Search index.
    pub search: Vec<SearchItem>,
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

impl Page {
    /// Creates a page.
    #[allow(clippy::similar_names)]
    pub fn new(config: &Config, id: &Id, markdown: Markdown) -> Page {
        let root_dir = config.get_root_dir();

        // Retrieve site directory and URL
        let site_dir = config.project.site_dir.clone();
        let site_url = config.project.site_url.clone();

        // Retrieve repository URL and edit URI
        let repo_url = config.project.repo_url.clone();
        let edit_uri = config.project.edit_uri.clone();

        // Determine whether to use directory URLs
        let use_directory_urls = config.project.use_directory_urls;
        let file_uri = id.location().into_owned();

        // Create identifier builder, as we need to change the context in order
        // to copy the file over to the site directory
        let builder = id.to_builder().with_context(&site_dir);
        let id = builder.clone().build().expect("invariant");

        // Next, obtain the path, and check whether it is an index file, which
        // is true for index.md, as well as README.md, as MkDocs handles both
        let mut path: PathBuf = id.location().to_string().into();
        let is_index =
            path.ends_with("index.md") || path.ends_with("README.md");

        // Ensure that README.md files are treated as index files
        if path.ends_with("README.md") {
            path.pop();
            path = path.join("index.md");
        }

        // If directory URLs should not be used, and the page is an index page,
        // we need to adjust the path accordingly
        if !use_directory_urls || is_index {
            path.set_extension("html");
        } else {
            path.set_extension("");
            path.push("index.html");
        }

        // Set computed path in id, and compute final target path - once we add
        // more convenience function to the id crate, we can make this shorter
        let path = path.to_string_lossy().into_owned();
        let id = builder
            .with_location(path.replace('\\', "/"))
            .build()
            .expect("invariant");

        // Compute URL of page, and strip the index.html suffix in case
        // directory URLs should be used. The URL is relative.
        let url = id.as_uri().to_string();
        let url = if use_directory_urls {
            url.trim_end_matches("index.html").to_string()
        } else {
            url
        };

        // Ensure path encoding, and compute canonical URL. Note that we should
        // definitely rethink this interface, it's a little inconvenient
        let url = Uri::from(url.as_ref()).to_string();
        let canonical_url = site_url.as_ref().map(|base| {
            let base = base.trim_end_matches('/');
            format!("{base}/{url}")
        });

        // Compute edit URL - edit URIs can be relative or absolute, as both
        // variants are supported by MkDocs, so we mirror behavior for now
        let edit_url = repo_url.clone().and_then(|repo_url| {
            edit_uri.clone().map(|uri| {
                if uri.starts_with("https://") {
                    format!("{uri}/{file_uri}")
                } else {
                    format!("{repo_url}/{uri}/{file_uri}")
                }
            })
        });

        // Return page - note that ancestors, as well as previous and next
        // pages are populated when the navigation is created. This is also a
        // hint that it's not a good idea to centralize all propeties in a
        // single struct, but to split up the page as necessary later on.
        let path = root_dir.join(id.to_path());
        Page {
            url,
            title: markdown.title,
            meta: markdown.meta,
            canonical_url,
            edit_url,
            content: markdown.content,
            toc: markdown.toc,
            search: markdown.search,
            path: path.to_string_lossy().into_owned(),
            ancestors: Vec::new(),
            previous_page: None,
            next_page: None,
        }
    }

    /// Renders the page.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip_all, fields(url = %self.url))
    )]
    pub fn render(
        &mut self, config: &Config, nav: &Navigation,
    ) -> Result<String, Error> {
        let name = self.meta.get("template").map(ToString::to_string);
        let template = Template::new(
            name.unwrap_or(String::from("main.html")),
            config.theme_dirs.clone(),
        );

        // Set active page in navigation and compute ancestors, as well as next
        // and previous page, all of which we need for rendering navigation
        let nav = nav.with_active(self);
        self.ancestors = nav.ancestors(self);
        self.previous_page = nav.previous_page(self);
        self.next_page = nav.next_page(self);

        // Create context and render template
        template.render_with_context(context! {
            generator => GENERATOR,
            nav => nav,
            base_url => config.get_base_url(&self.url),
            extra_css => config.project.extra_css.clone(),
            extra_javascript => config.project.extra_javascript.clone(),
            config => config.project.clone(),
            tags => self.tags(),
            page => self,
        })
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
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Page {}

// ----------------------------------------------------------------------------
// Type alises
// ----------------------------------------------------------------------------

/// Page metadata.
pub type PageMeta = BTreeMap<String, Dynamic>;
