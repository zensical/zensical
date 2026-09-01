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

//! Workflow definitions

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Deref;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, LazyLock, OnceLock};
use std::{fs, io};
use zrx::id::matcher::Matcher;
use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::workflow::Builder;
use zrx::stream::{
    concurrent, Key, Signal, Stream, StreamTupleExt, Value, Workflow,
};

use super::compat::mkdocs::plugin::{
    self, autorefs, meta, mkdocstrings, redirects, search,
};
use super::config::Config;
use super::structure::markdown::Markdown;
use super::structure::nav::Navigation;
use super::structure::page::{Page, PageRoute};
use super::template::Template;
use super::watcher::Source;

use super::compat::mkdocs::plugin::autorefs::UnresolvedAutorefs;
use super::python::{Anchors, Issues, References, SharedReferences};

// TODO: Migrate aggregation after the basic workflow runs on the new runtime.
// mod aggregate;
mod cached;

// use aggregate::aggregate;
use cached::cached;

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Regular expression to detect use of snippets
static SNIPPET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]*-+8<-+").expect("invariant"));

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Main module.
///
/// With the advent of the module system at the beginning of April 2026, we can
/// start our journey to migrate all logic into modules. We now move the entire
/// build process into a single module, and then factor out functionality into
/// smaller, logically self-contained units. This approach ensures that we can
/// ship the module system as fast as possible, allowing us to work on feature
/// parity, while testing the module system in a real-world codebase.
#[derive(Debug)]
pub struct Main {
    /// Configuration.
    config: Config,
    /// Strict mode.
    strict: bool,
}

/// File input enriched with immutable facts for the current revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Input {
    /// Source supplied by the file provider.
    source: Source,
    /// Metadata files parsed once and shared by every page in the revision.
    metadata: Arc<meta::Index>,
}

impl Value for Input {}

impl Input {
    /// Enriches one provider source with revision-local metadata facts.
    pub(crate) fn new(source: Source, metadata: Arc<meta::Index>) -> Self {
        Self { source, metadata }
    }
}

impl Deref for Input {
    type Target = Source;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

/// Revision-settled site batch derived from the current page relation.
#[derive(Clone, Debug)]
struct Site {
    /// Complete pages selected for this batch.
    pages: Arc<Vec<(Key<Id>, SitePage)>>,
    /// Navigation derived from the current pages.
    nav: Navigation,
    /// Autoref registry derived from the same settled page snapshot.
    autorefs: autorefs::Registry,
    /// Search inputs derived from the same settled page snapshot.
    search: search::Snapshot,
}

impl Value for Site {}

// ----------------------------------------------------------------------------

/// Page render input retained after site-wide settlement.
#[derive(Clone, Debug)]
struct SitePage {
    /// Page passed to the template renderer.
    page: Page,
    /// Page-local autorefs replaced with stable slots.
    autorefs: Arc<autorefs::References>,
}

impl Value for SitePage {}

// ----------------------------------------------------------------------------

/// Markdown source paired with route facts available before rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RoutedMarkdown {
    /// Source and revision-local facts supplied by the provider.
    input: Input,
    /// Route derived without parsing or rendering Markdown.
    route: PageRoute,
}

impl Value for RoutedMarkdown {}

// ----------------------------------------------------------------------------

/// Cached output of rendering one Markdown source.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RenderedMarkdown {
    /// Route computed once before Markdown rendering.
    route: PageRoute,
    /// Rendered Markdown consumed by page construction.
    markdown: Markdown,
    /// Page-local registrations consumed during site settlement.
    registrations: Arc<autorefs::Facts>,
    /// Facts extracted by the shared MkDocs-compatible HTML pass.
    html: plugin::HtmlFacts,
    /// Resolved metadata and source mappings for later compatibility modules.
    pub(crate) meta: Arc<meta::Resolved>,
}

impl Value for RenderedMarkdown {}

// ----------------------------------------------------------------------------

/// Page plus compatibility facts derived from the same Markdown render.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RenderedPage {
    /// Page consumed by site-wide and page-local branches.
    page: Page,
    /// Autoref registrations revision-aligned with the page.
    registrations: Arc<autorefs::Facts>,
    /// HTML compatibility facts revision-aligned with the page.
    html: plugin::HtmlFacts,
    /// Resolved metadata and source mappings for later compatibility modules.
    pub(crate) meta: Arc<meta::Resolved>,
}

impl Value for RenderedPage {}

// ----------------------------------------------------------------------------

// Implementations
// ----------------------------------------------------------------------------

impl Main {
    /// Initializes the module.
    fn setup(&self, ctx: &mut Builder<Id>) {
        let files = ctx.input::<Input>();
        let meta = meta::Settings::new(&self.config);

        // Set up workflow to process static assets and Markdown files.
        process_theme_assets(&self.config, &files);
        process_assets(&self.config, &files, &meta);
        let markdown = route_markdown(&self.config, &files);

        // Redirects depend on routes, not rendered Markdown. Settle their
        // compact input independently so they can proceed concurrently with
        // the Python rendering branch.
        let routes = markdown.map(|input: &RoutedMarkdown| input.route.clone());
        let redirects = generate_redirects(&self.config, &routes);
        redirects::attach(&self.config, self.strict, &redirects);

        let rendered = process_markdown(&self.config, &markdown);

        // Cross the one global settlement boundary, derive all site-wide
        // state, then expand the resulting batch into independent page work.
        let rendered_page = generate_page(&self.config, &rendered);
        let page =
            rendered_page.map(|rendered: &RenderedPage| rendered.page.clone());
        let site = generate_site(&self.config, &rendered_page);
        let nav = generate_nav(&site);
        let search = site.map(|site: &Site| site.search.clone());
        search::attach(&self.config, &search);
        mkdocstrings::attach(&self.config, &nav);
        let _ = render_templates(&self.config, &files, &nav);
        let unresolved = render_pages(&self.config, &site);
        validate(&self.config, self.strict, &files, &page, &unresolved);
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

// TODO: Replace the legacy barrier with revision-settled aggregation.
// Return condition waiting for all Markdown files
#[cfg(any())]
pub fn wait_for_markdown(config: &Config) -> (Key<Id>, Barrier<Id>) {
    let docs_dir = config.project.docs_dir.clone();
    let matcher = Matcher::from_str(&format!("zrs::::{docs_dir}:**/*.md:"))
        .expect("invariant");

    // Create barrier that waits for all Markdown files to be processed
    let barrier = Barrier::new(move |id: &Key<Id>| {
        matcher.is_match(&id[0]).expect("invariant")
    });

    // Create key for barrier
    let id =
        Key::from_iter([
            id!(provider = "file", context = ".", location = ".").unwrap()
        ]);

    // Return both
    (id, barrier)
}

/// Create a stream to collect references from all Markdown files.
pub fn collect_references(
    config: &Config, files: &Stream<Id, Input>,
) -> Stream<Id, SharedReferences> {
    let matcher = Arc::new(
        Matcher::from_str(&format!(
            "zrs::::{}:**/*.md:",
            config.project.docs_dir
        ))
        .expect("invariant"),
    );

    // Create pipeline to collect references
    files
        .filter(move |id: &Id| matcher.is_match(id).expect("invariant"))
        .map(|source: &Input| {
            let references: References =
                fs::read_to_string(&*source.path)?.parse()?;
            Ok::<_, anyhow::Error>(SharedReferences::from(references))
        })
}

/// Validate references and autorefs after every current page has rendered.
fn validate(
    config: &Config, strict: bool, files: &Stream<Id, Input>,
    pages: &Stream<Id, Page>, unresolved: &Stream<Id, UnresolvedAutorefs>,
) {
    let validation = config.project.validation.clone();
    if !validation.is_enabled() {
        return;
    }

    let references = collect_references(config, files);
    let anchors = pages.map(|page: &Page| {
        page.content.parse::<Anchors>().map_err(anyhow::Error::from)
    });
    let pages = (references, anchors, unresolved.clone()).join();

    // This reduction is downstream of rendering, so its terminal is reached
    // only after every page in the settled relation produced a result.
    let _ = pages.reduce(
        move |pages: &dyn Collection<
            Key<Id>,
            (SharedReferences, Anchors, UnresolvedAutorefs),
        >| {
            // Keep this as a borrowed iterator. Validation only needs the
            // settled relation for the duration of this invocation, so
            // materializing owned tuples would duplicate all anchor and
            // unresolved-autoref data at peak.
            let issues = Issues::new(pages.iter());
            issues.print(&validation, strict)?;
            Ok::<_, anyhow::Error>(Some(()))
        },
    );
}

/// Compute a hash of the page content relevant to template rendering.
fn page_hash(page: &Page, autorefs: &autorefs::References) -> u64 {
    let mut hasher = DefaultHasher::new();
    page.content.hash(&mut hasher);
    page.meta.hash(&mut hasher);
    autorefs.hash(&mut hasher);
    hasher.finish()
}

/// Create a stream to process static assets.
pub fn process_assets(
    config: &Config, files: &Stream<Id, Input>, meta: &meta::Settings,
) {
    let extra_templates = config.project.extra_templates.clone();
    let docs_dir = config.project.docs_dir.clone();
    let matcher = Arc::new(
        Matcher::from_str(&format!("zrs::::{docs_dir}::")).expect("invariant"),
    );

    // Create pipeline to copy static assets
    let site_dir = config.project.site_dir.clone();
    let root_dir = config.get_root_dir();
    let meta = meta.clone();
    let _ = files.map(move |id: &Id, from: &Input| {
        if !matcher.is_match(id).expect("invariant") {
            return Ok(());
        }

        // Don't copy Markdown files
        if id.location().ends_with(".md") {
            return Ok(());
        }

        // Metadata files are inputs, not site assets.
        if meta::claims(&id.location(), &meta) {
            return Ok(());
        }

        // Don't copy template files that we render later
        if extra_templates.contains(&id.location().into_owned()) {
            return Ok(());
        }

        // Create identifier builder, as we need to change the context in order
        // to copy the file over to the site directory
        let builder = id.to_builder().context(&site_dir);
        let id = builder.build().expect("invariant");

        // Compute parent path, create intermediate directories and copy files
        let to = root_dir.join(id.to_path());
        fs::create_dir_all(to.parent().expect("invariant"))?;
        copy_file(&from.path, to)?;
        Ok::<(), anyhow::Error>(())
    });
}

/// Create a stream to process static assets in theme.
pub fn process_theme_assets(config: &Config, files: &Stream<Id, Input>) {
    let matcher =
        Arc::new(Matcher::from_str("zrs::::templates/*::").expect("invariant"));

    // Create pipeline to copy static assets
    let site_dir = config.project.site_dir.clone();
    let root_dir = config.get_root_dir();
    let _ = files.map(move |id: &Id, from: &Input| {
        if !matcher.is_match(id).expect("invariant") {
            return Ok(());
        }

        // Don't copy templates - they will be rendered later
        if id.location().ends_with(".html") {
            return Ok(());
        }

        // Create identifier builder, as we need to change the context in order
        // to copy the file over to the site directory
        let builder = id.to_builder().context(&site_dir);
        let id = builder.build().expect("invariant");

        // Compute parent path, create intermediate directories and copy files
        let to = root_dir.join(id.to_path());
        fs::create_dir_all(to.parent().expect("invariant"))?;
        copy_file(&from.path, to)?;
        Ok::<_, anyhow::Error>(())
    });
}

/// Copy a file to a new location, without copying its permissions.
fn copy_file(
    from: impl AsRef<Path>, to: impl AsRef<Path>,
) -> Result<(), io::Error> {
    let mut from = fs::File::open(from)?;
    let mut to = fs::File::create(to)?;
    io::copy(&mut from, &mut to).map(|_| ())
}

/// Select Markdown sources and derive their routes before rendering.
fn route_markdown(
    config: &Config, files: &Stream<Id, Input>,
) -> Stream<Id, RoutedMarkdown> {
    let matcher = Arc::new(
        Matcher::from_str(&format!(
            "zrs::::{}:**/*.md:",
            config.project.docs_dir
        ))
        .expect("invariant"),
    );
    let config = config.clone();
    files
        .filter(move |id: &Id| matcher.is_match(id).expect("invariant"))
        .map(move |id: &Id, input: &Input| RoutedMarkdown {
            input: input.clone(),
            route: PageRoute::new(&config, id),
        })
}

/// Create a stream to process routed Markdown files.
fn process_markdown(
    config: &Config, routed: &Stream<Id, RoutedMarkdown>,
) -> Stream<Id, RenderedMarkdown> {
    // Create pipeline to render Markdown files
    let plugins = plugin::Settings::new(config);
    let config = config.clone();
    routed
        // Render Markdown if we don't have a recent cached version at our own
        // disposal. Otherwise, just return that if the content did not change.
        // Note that we need to limit concurrency here, or we'll overwhelm the
        // Python interpreter with all tasks competing for the GIL.
        .map(concurrent(1, move |id: &Id, routed: &RoutedMarkdown| {
            let location = id.location().into_owned();
            let data = fs::read_to_string(&*routed.input.path)?;
            let route = routed.route.clone();

            let (data, page_meta) = meta::front_matter(&location, &data)?;
            let resolved =
                routed.input.metadata.resolve(&location, page_meta)?;
            // Don't cache page if it inserts (pymdownx) snippets.
            // This is a hack while waiting for CommonMark (AST) and components,
            // as well as topic-based authoring functionality.
            if SNIPPET_RE.is_match(&data) {
                render_markdown(id, route, data, plugins, resolved)
            } else {
                cached(
                    &config,
                    id.as_str(),
                    (
                        1_u8,
                        config.hash,
                        data.clone(),
                        route.clone(),
                        resolved.clone(),
                    ),
                    |(_, _, data, route, resolved)| {
                        render_markdown(id, route, data, plugins, resolved)
                    },
                )
            }
        }))
}

/// Render Markdown and collect the page-local facts produced alongside it.
fn render_markdown(
    id: &Id, route: PageRoute, content: String, plugins: plugin::Settings,
    meta: meta::Resolved,
) -> anyhow::Result<RenderedMarkdown> {
    let mut markdown =
        Markdown::new(id, route.url.clone(), content, meta.values())?;
    let html = plugin::prepare(&mut markdown, plugins);
    let registrations = if plugins.autorefs {
        autorefs::take_page(&route.url)
    } else {
        Arc::default()
    };
    let meta = Arc::new(meta.reconcile(markdown.meta.clone()));
    Ok(RenderedMarkdown {
        route,
        markdown,
        registrations,
        html,
        meta,
    })
}

/// Generate pages from Markdown files.
fn generate_page(
    config: &Config, markdown: &Stream<Id, RenderedMarkdown>,
) -> Stream<Id, RenderedPage> {
    let config = config.clone();
    markdown.map(move |markdown: &RenderedMarkdown| RenderedPage {
        page: Page::new(
            &config,
            markdown.route.clone(),
            markdown.markdown.clone(),
        ),
        registrations: markdown.registrations.clone(),
        html: markdown.html.clone(),
        meta: markdown.meta.clone(),
    })
}

/// Derive one complete site batch at the page-relation terminal.
fn generate_site(
    config: &Config, pages: &Stream<Id, RenderedPage>,
) -> Signal<Id, Site> {
    let config = config.clone();
    pages.reduce(move |pages: &dyn Collection<Key<Id>, RenderedPage>| {
        let mut nav_pages = Vec::new();
        let mut site_pages = Vec::new();
        let mut facts = Vec::new();
        let mut documents = Vec::new();
        for (key, rendered) in pages.iter() {
            nav_pages.push((key.clone(), rendered.page.clone()));
            site_pages.push((
                key.clone(),
                SitePage {
                    page: rendered.page.clone(),
                    autorefs: rendered.html.autorefs.clone(),
                },
            ));
            facts.push((key.clone(), rendered.registrations.clone()));
            if !rendered.html.search.is_empty() {
                documents.push((
                    key.clone(),
                    search::Document::new(
                        &rendered.page,
                        rendered.html.search.clone(),
                    ),
                ));
            }
        }

        let nav = Navigation::new(config.project.nav.clone(), nav_pages);
        let autorefs = autorefs::assemble(&config, facts);
        let search = search::Snapshot::new(documents, nav.clone());
        Ok::<_, anyhow::Error>(Some(Site {
            pages: Arc::new(site_pages),
            nav,
            autorefs,
            search,
        }))
    })
}

/// Resolve redirects from the compact route relation.
fn generate_redirects(
    config: &Config, routes: &Stream<Id, PageRoute>,
) -> Signal<Id, redirects::Snapshot> {
    let config = config.clone();
    routes.reduce(move |routes: &dyn Collection<Key<Id>, PageRoute>| {
        redirects::Snapshot::new(&config, routes.iter()).map(Some)
    })
}

/// Project navigation from the current site batch.
fn generate_nav(site: &Signal<Id, Site>) -> Signal<Id, Navigation> {
    site.map(|site: &Site| site.nav.clone())
}

/// Render static and extra templates.
pub fn render_templates(
    config: &Config, files: &Stream<Id, Input>, nav: &Signal<Id, Navigation>,
) -> Stream<Id, ()> {
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
        .add(&format!("zrs::::templates/*:{{{static_templates}}}:"))
        .expect("invariant");
    builder
        .add(&format!("zrs::::{docs_dir}:{{{extra_templates}}}:"))
        .expect("invariant");

    // Create matcher from builder, and filter templates
    let matcher = Arc::new(builder.build().expect("invariant"));
    let templates =
        files.filter(move |id: &Id| matcher.is_match(id).expect("invariant"));

    // Add docs directory to theme templates
    let mut theme_dirs = config.theme_dirs.clone();
    theme_dirs.push(config.get_docs_dir());

    // Create pipeline to render templates
    let renderer = Template::new(theme_dirs);
    let config = config.clone();
    templates
        .product(nav)
        .map(move |template: &Input, nav: &Navigation| {
            let name =
                Path::new(&template.path).file_name().expect("invariant");
            let site_dir = config.get_site_dir();

            // Render template and write to disk
            let data =
                renderer.render(&name.to_string_lossy(), &config, nav)?;
            let path = site_dir.join(name);
            fs::create_dir_all(path.parent().expect("invariant"))?;
            fs::write(path, &data)?;
            Ok::<_, anyhow::Error>(())
        })
}

/// Render pages.
fn render_pages(
    config: &Config, site: &Signal<Id, Site>,
) -> Stream<Id, UnresolvedAutorefs> {
    let pages = site.flat_map(|site: &Site| {
        site.pages
            .iter()
            .map(|(key, page)| {
                (
                    key.clone(),
                    (page.clone(), site.nav.clone(), site.autorefs.clone()),
                )
            })
            .collect::<Vec<_>>()
    });

    let template = OnceLock::new();
    let theme_dirs = config.theme_dirs.clone();
    let config = config.clone();
    pages.map(
        move |input: &SitePage,
              nav: &Navigation,
              autorefs: &autorefs::Registry| {
            let mut page = input.page.clone();
            let references = &input.autorefs;
            let id = page.url.clone();

            // Cache template rendering independently of autorefs, which are
            // substituted below on every pass. Deriving a cache key for the
            // substitution would require the same resolution scan that the
            // substitution itself performs, so caching it can't pay off.
            let args = (config.hash, nav.hash, page_hash(&page, references));
            let rendered =
                cached(&config, ("template", id), args, |(_, _, _)| {
                    let template = template
                        .get_or_init(|| Template::new(theme_dirs.clone()));
                    Ok(page.render_template(template, &config, nav.clone())?)
                })?;

            // Replace autorefs and retain unresolved identifiers
            let (data, unresolved) =
                autorefs.replace_in(rendered, references, &page.url);

            let path = Path::new(&page.path);
            fs::create_dir_all(path.parent().expect("invariant"))?;
            fs::write(path, &data)?;
            Ok::<_, anyhow::Error>(unresolved)
        },
    )
}

/// Creates a workflow for the given config.
pub fn create_workflow(config: &Config, strict: bool) -> Workflow<Id> {
    Workflow::build(|workflow| {
        Main { config: config.clone(), strict }.setup(workflow);
    })
}
