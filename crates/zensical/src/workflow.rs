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

//! Workflow definitions

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::str::FromStr;
use std::{fs, io};
use zrx::id::{Id, Matcher};
use zrx::scheduler::action::report::IntoReport;
use zrx::stream::barrier::Condition;
use zrx::stream::function::{with_id, with_splat};
use zrx::stream::value::{Chunk, Delta};
use zrx::stream::workspace::Workspace;
use zrx::stream::Stream;

use super::config::Config;
use super::structure::markdown::Markdown;
use super::structure::nav::Navigation;
use super::structure::page::Page;
use super::structure::search::SearchIndex;
use super::template::Template;

mod cached;

use cached::cached;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Create a stream to process static assets.
pub fn process_assets(config: &Config, files: &Stream<Id, String>) {
    let extra_templates = config.project.extra_templates.clone();
    let docs_dir = config.project.docs_dir.clone();
    let matcher =
        Matcher::from_str(&format!("zrs::::{docs_dir}::")).expect("invariant");

    // Create pipeline to copy static assets
    let site_dir = config.project.site_dir.clone();
    let root_dir = config.get_root_dir();
    files.map(with_id(move |id: &Id, from: String| {
        if !matcher.is_match(id).expect("invariant") {
            return Ok(());
        }

        // Don't copy Markdown files
        if id.location().ends_with(".md") {
            return Ok(());
        }

        // Don't copy template files that we render later
        if extra_templates.contains(&id.location().into_owned()) {
            return Ok(());
        }

        // Create identifier builder, as we need to change the context in order
        // to copy the file over to the site directory
        let builder = id.to_builder().with_context(&site_dir);
        let id = builder.build().expect("invariant");

        // Compute parent path, create intermediate directories and copy files
        let to = root_dir.join(id.to_path());
        fs::create_dir_all(to.parent().expect("invariant"))?;
        fs::copy(from, to).map(|_| ())
    }));
}

/// Create a stream to process static assets in theme.
pub fn process_theme_assets(config: &Config, files: &Stream<Id, String>) {
    let matcher = Matcher::from_str("zrs::::templates/*::").expect("invariant");

    // Create pipeline to copy static assets
    let site_dir = config.project.site_dir.clone();
    let root_dir = config.get_root_dir();
    files.map(with_id(move |id: &Id, from: String| {
        if !matcher.is_match(id).expect("invariant") {
            return Ok(());
        }

        // Don't copy templates - they will be rendered later
        if id.location().ends_with(".html") {
            return Ok(());
        }

        // Create identifier builder, as we need to change the context in order
        // to copy the file over to the site directory
        let builder = id.to_builder().with_context(&site_dir);
        let id = builder.build().expect("invariant");

        // Compute parent path, create intermediate directories and copy files
        let to = root_dir.join(id.to_path());
        fs::create_dir_all(to.parent().expect("invariant"))?;
        fs::copy(from, to).map(|_| ())
    }));
}

/// Create a stream to process Markdown files.
pub fn process_markdown(
    config: &Config, files: &Stream<Id, String>,
) -> Stream<Id, Markdown> {
    let matcher = Matcher::from_str("zrs:::::**/*.md:").expect("invariant");

    // Create pipeline to render Markdown files
    let config = config.clone();
    files
        .filter(with_id(move |id: &Id, _: &_| {
            matcher.is_match(id).expect("invariant")
        }))
        // Render Markdown if we don't have a recent cached version at our own
        // disposal. Otherwise, just return that if the content did not change.
        // Note that we need to limit concurrency here, or we'll overwhelm the
        // Python interpreter with all tasks competing for the GIL.
        .map_concurrency(
            with_id(move |id: &Id, path: String| {
                let data = fs::read_to_string(path)?;
                cached(&config, id, data, |data| Markdown::new(id, data))
                    .into_report()
            }),
            1,
        )
}

/// Create a stream to wait for all Markdown files to be rendered.
pub fn wait_for_markdown(
    config: &Config, files: &Stream<Id, String>,
) -> Stream<Id, Condition<Id>> {
    let name = config.path.file_name().expect("invariant");
    let matcher =
        Matcher::from_str(&format!("zrs:::::{}:", name.to_string_lossy()))
            .expect("invariant");

    // Set up matcher to filter for the configuration file, and return a new
    // stream that emits a condition in order to implement barriers
    files.filter_map(with_id(move |id: &Id, _: _| {
        matcher.is_match(id).expect("invariant").then(|| {
            let matcher =
                Matcher::from_str("zrs:::::**/*.md:").expect("invariant");

            // Return condition waiting for all Markdown files
            Condition::new(matcher)
        })
    }))
}

/// Generate pages from Markdown files.
pub fn generate_page(
    config: &Config, markdown: &Stream<Id, Markdown>,
) -> Stream<Id, Page> {
    let config = config.clone();
    markdown.map(with_id(move |id: &Id, markdown| {
        Page::new(&config, id, markdown)
    }))
}

/// Generate navigation from all pages.
pub fn generate_nav(
    config: &Config, pages: &Stream<Id, Chunk<Id, Page>>,
) -> Stream<Id, Navigation> {
    let config = config.clone();
    pages.map(move |pages: Chunk<Id, Page>| {
        Navigation::new(config.project.nav.clone(), pages)
    })
}

/// Generte search index
pub fn generate_search_index(
    config: &Config, nav: &Stream<Id, Navigation>,
    pages: &Stream<Id, Chunk<Id, Page>>,
) {
    let config = config.clone();
    pages.product(nav).delta_map(with_splat(move |pages, nav| {
        let plugin = config.project.plugins.search.config.clone();
        let search = SearchIndex::new(pages, &nav, plugin);

        // Serialize search index to json, and obtain site directory
        let data = serde_json::to_string(&search).expect("invariant");
        let site_dir = config.get_site_dir();

        // Write search index to disk
        let path = site_dir.join("search.json");
        fs::create_dir_all(path.parent().expect("invariant"))?;
        fs::write(path, &data)?;

        // If offline plugin is enabled, create search.js as well
        if config.project.plugins.offline.config.enabled {
            let path = site_dir.join("search.js");
            fs::create_dir_all(path.parent().expect("invariant"))?;
            fs::write(path, format!("var __index = {data};").as_str())?;
        }

        // All files were written successfully
        Ok::<_, io::Error>(())
    }));
}

/// Render static and extra templates.
pub fn render_templates(
    config: &Config, files: &Stream<Id, String>, nav: &Stream<Id, Navigation>,
) -> Stream<Id, Delta<Id, ()>> {
    let docs_dir = config.project.docs_dir.clone();

    // Retrieve template names
    let static_templates = &config.project.theme.static_templates.join(",");
    let extra_templates = &config.project.extra_templates.join(",");

    // Build matcher for static and extra templates - we just handle them the
    // same. In MkDocs, extra templates can do even less than static templates,
    // not having access to the `url_filter`, but there's no need for us to
    // differentiate here.
    let mut builder = Matcher::builder();
    builder
        .add(format!("zrs::::templates/*:{{{static_templates}}}:"))
        .expect("invariant");
    builder
        .add(format!("zrs::::{docs_dir}:{{{extra_templates}}}:"))
        .expect("invariant");

    // Create matcher from builder, and filter templates
    let matcher = builder.build().expect("invariant");
    let templates = files.filter(with_id(move |id: &Id, _: &String| {
        matcher.is_match(id).expect("invariant")
    }));

    // Add docs directory to theme templates
    let mut theme_dirs = config.theme_dirs.clone();
    theme_dirs.push(config.get_docs_dir());

    // Create pipeline to render templates
    let config = config.clone();
    templates.product(nav).delta_map(with_splat(
        move |template: String, nav: Navigation| {
            let name = Path::new(&template).file_name().expect("invariant");
            let site_dir = config.get_site_dir();

            // Obtain template
            let template =
                Template::new(name.to_string_lossy(), theme_dirs.clone());

            // Render template and write to disk
            template
                .render(&config, &nav)
                .into_report()
                .and_then(|report| {
                    let path = site_dir.join(name);
                    fs::create_dir_all(path.parent().expect("invariant"))?;
                    fs::write(path, &report.data).map_err(Into::into)
                })
        },
    ))
}

/// Render pages.
pub fn render_pages(
    config: &Config, page: &Stream<Id, Page>, nav: &Stream<Id, Navigation>,
) -> Stream<Id, Delta<Id, ()>> {
    let config = config.clone();
    page.product(nav).delta_map(with_splat(
        move |mut page: Page, nav: Navigation| {
            let id = page.url.clone();

            // Compute hash of page content
            let hash = {
                let mut hasher = DefaultHasher::new();
                page.content.hash(&mut hasher);
                page.meta.hash(&mut hasher);
                hasher.finish()
            };

            // Render page if we don't have a recent cached version at our own
            // disposal. Otherwise, just return if the content did not change.
            let args = (config.hash, nav.hash, hash);
            cached(&config, id, args, |(_, _, _)| page.render(&config, &nav))
                .into_report()
                .and_then(|report| {
                    let path = Path::new(&page.path);
                    fs::create_dir_all(path.parent().expect("invariant"))?;
                    fs::write(path, &report.data)
                        .map_err(Into::into)
                        .inspect(|()| println!("+ /{}", page.url))
                })
        },
    ))
}

/// Creates a new workspace for the given config.
pub fn create_workspace(config: &Config) -> Workspace<Id> {
    let workspace = Workspace::new();
    let config = config.clone();

    // Right now, we use a single workflow for the entirety of the build. Later,
    // when we work on the module system, modules will have their own workflows.
    // Create a source for files, so the file agent can submit file creation,
    // change and delete events to the workflow
    let workflow = workspace.add_workflow();
    let files = workflow.add_source::<String>();

    // Set up workflow to process static assets, as well as Markdown files, and
    // create a barrier to wait for the completion of all Markdown files
    process_theme_assets(&config, &files);
    process_assets(&config, &files);
    let markdown = process_markdown(&config, &files);
    let wait = wait_for_markdown(&config, &files);

    // Generate pages, and use the barrier to ensure that all pages have been
    // processed, in order to create the navigation and search index
    let page = generate_page(&config, &markdown);
    let pages = page.select(&wait).chunks();

    // Generate navigation and search index
    let nav = generate_nav(&config, &pages);
    generate_search_index(&config, &nav, &pages);

    // Render static and extra templates, as well as pages
    render_templates(&config, &files, &nav);
    render_pages(&config, &page, &nav);

    // Return workspace
    workspace
}
