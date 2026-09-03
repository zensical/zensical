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

//! Workflow definitions.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Deref;
use std::str::FromStr;
use std::sync::{Arc, LazyLock, OnceLock};

use zrx::id::matcher::Matcher;
use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::workflow::Builder;
use zrx::stream::{
    concurrent, Key, Signal, Stream, StreamTupleExt, Value, Workflow,
};

use crate::compat::mkdocs::plugin::autorefs::UnresolvedAutorefs;
use crate::compat::mkdocs::{
    plugin::{
        self, autorefs, awesome_nav, literate_nav, meta, minify, mkdocstrings,
        redirects, search, tags,
    },
    resource,
};
use crate::config::Config;
use crate::path::{PathError, SitePath, SourcePath};
use crate::python::{Anchors, Issues, References, SharedReferences};
use crate::structure::markdown::Markdown;
use crate::structure::nav::Navigation;
use crate::structure::page::{Page, PageRoute};
use crate::template::Template;
use crate::watcher::Source;

mod cached;

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
struct Main {
    /// Configuration.
    config: Config,
    /// Strict mode.
    strict: bool,
    /// Whether the retained workflow serves live updates.
    serve: bool,
    /// Metadata pipeline shared with source admission.
    meta: meta::Meta,
}

/// File input enriched with immutable facts for the current revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    /// Source supplied by the file provider.
    source: Source,
    /// Metadata files parsed once and shared by every page in the revision.
    metadata: Arc<meta::Index>,
}

impl Value for Input {}

/// Immutable build configuration supplied through the workflow data plane.
#[derive(Clone, Debug)]
pub struct Configuration {
    /// Fully resolved project configuration.
    config: Arc<Config>,
    /// Whether warnings fail this build.
    strict: bool,
}

impl Value for Configuration {}

impl Configuration {
    /// Creates the configuration fact for one workflow lifetime.
    pub fn new(config: Config, strict: bool) -> Self {
        Self {
            config: Arc::new(config),
            strict,
        }
    }
}

impl Input {
    /// Enriches one provider source with revision-local metadata facts.
    pub fn new(source: Source, metadata: Arc<meta::Index>) -> Self {
        Self { source, metadata }
    }
}

impl Deref for Input {
    type Target = Source;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

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

/// Page-local work paired with revision-settled shared rendering facts.
#[derive(Clone, Debug)]
struct PageRender {
    /// Page and its unresolved autoref slots.
    input: SitePage,
    /// Navigation for the current page relation.
    nav: Navigation,
    /// Autoref registry for the current page relation.
    autorefs: autorefs::Registry,
    /// Asset-projected template configuration.
    project: Arc<crate::config::Project>,
    /// Stable asset mapping hash for the template cache key.
    asset_hash: u64,
}

impl Value for PageRender {}

// ----------------------------------------------------------------------------

/// Effective navigation title indexed by page URL.
#[derive(Clone, Debug)]
struct PageTitles(Arc<HashMap<String, String>>);

impl Value for PageTitles {}

impl PageTitles {
    /// Indexes the first navigation occurrence of each page, like MkDocs.
    fn new(nav: &Navigation) -> Self {
        let mut titles = HashMap::new();
        for item in nav {
            if item.meta.is_some()
                && let (Some(url), Some(title)) = (&item.url, &item.title)
            {
                titles.entry(url.clone()).or_insert_with(|| title.clone());
            }
        }
        Self(Arc::new(titles))
    }

    /// Returns the effective title assigned to a page in navigation.
    fn get(&self, url: &str) -> Option<&str> {
        self.0.get(url).map(String::as_str)
    }
}

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
}

impl Value for RenderedPage {}

// ----------------------------------------------------------------------------

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Main {
    /// Initializes the module.
    fn setup(&self, ctx: &mut Builder<Id>) {
        let files = ctx.input::<Input>();
        let configuration = ctx.input::<Configuration>();
        let minify = minify::Minify::new(&self.config);

        // Set up workflow to process static assets and Markdown files.
        let sources = files.map(|input: &Input| input.source.clone());
        let resources = resource::Resources::new(&self.config, &self.meta)
            .setup(resource::Dependencies { sources: &sources });
        let assets =
            minify.setup(minify::Dependencies { resources: &resources });
        let markdown = route_markdown(&self.config, &files);

        // Redirects depend on routes, not rendered Markdown. Settle their
        // compact input independently so they can proceed concurrently with
        // the Python rendering branch.
        let routes = markdown.map(|input: &RoutedMarkdown| input.route.clone());
        let redirect_settings =
            configuration.map(|configuration: &Configuration| {
                redirects::Settings::new(
                    &configuration.config,
                    configuration.strict,
                )
            });
        redirects::Redirects.setup(redirects::Dependencies {
            settings: &redirect_settings,
            routes: &routes,
        });

        let plugins = plugin::Settings::new(&self.config, self.serve);
        let rendered = process_markdown(&self.config, &plugins, &markdown);

        // Construct pages before resolving navigation, which needs the titles
        // derived from Markdown for entries without an explicit title.
        let rendered_page = generate_page(&self.config, &rendered);
        let page =
            rendered_page.map(|rendered: &RenderedPage| rendered.page.clone());
        let awesome_nav =
            awesome_nav::AwesomeNav::new(&self.config, self.strict).expect(
                "awesome-nav configuration is validated during loading",
            );
        let nav = if awesome_nav.is_enabled() {
            awesome_nav.setup(awesome_nav::Dependencies {
                sources: &sources,
                pages: &page,
            })
        } else {
            literate_nav::LiterateNav::new(&self.config).setup(
                literate_nav::Dependencies {
                    sources: &sources,
                    pages: &page,
                },
            )
        };
        // MkDocs assigns configured navigation titles when constructing Page
        // objects, before metadata and Markdown fallbacks are evaluated. Our
        // navigation is resolved later, so apply that highest-precedence title
        // once the complete navigation is available.
        let rendered_page = apply_navigation_titles(&rendered_page, &nav);
        let rendered_page = apply_tags(&plugins.tags, &rendered_page);
        let site_page = rendered_page.map(|rendered: &RenderedPage| SitePage {
            page: rendered.page.clone(),
            autorefs: rendered.html.autorefs.clone(),
        });
        let search_document =
            rendered_page.filter_map(|rendered: &RenderedPage| {
                (!rendered.html.search.is_empty()).then(|| {
                    search::Document::new(
                        &rendered.page,
                        rendered.html.search.clone(),
                    )
                })
            });
        let autorefs_input =
            rendered_page.map(|rendered: &RenderedPage| autorefs::PageInput {
                source: rendered.page.source().clone(),
                facts: rendered.registrations.clone(),
            });
        let autorefs = plugins
            .autorefs
            .setup(autorefs::Dependencies { pages: &autorefs_input });
        plugins.search.setup(search::Dependencies {
            documents: &search_document,
            navigation: &nav,
        });
        mkdocstrings::Mkdocstrings::new(&self.config)
            .setup(mkdocstrings::Dependencies { navigation: &nav });
        let _ = render_templates(&self.config, &files, &nav, &assets, &minify);
        let unresolved = render_pages(
            &self.config,
            &site_page,
            &nav,
            &autorefs,
            &assets,
            &minify,
        );
        validate(&self.config, self.strict, &files, &page, &unresolved);
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Applies explicit navigation titles to their pages.
fn apply_navigation_titles(
    pages: &Stream<Id, RenderedPage>, nav: &Signal<Id, Navigation>,
) -> Stream<Id, RenderedPage> {
    let titles = nav.map(PageTitles::new);
    pages.product(&titles).map(
        |rendered: &RenderedPage, titles: &PageTitles| {
            let mut rendered = rendered.clone();
            if let Some(title) = titles.get(&rendered.page.url) {
                rendered.page.apply_navigation_title(title);
            }
            rendered
        },
    )
}

/// Create a stream to collect references from all Markdown files.
fn collect_references(
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
                fs::read_to_string(&*source.source)?.parse()?;
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
    page.hash_derived_template_context(&mut hasher);
    autorefs.hash(&mut hasher);
    hasher.finish()
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
        .map(move |id: &Id, input: &Input| {
            Ok::<_, crate::path::PathError>(RoutedMarkdown {
                input: input.clone(),
                route: PageRoute::new(&config, id)?,
            })
        })
}

/// Create a stream to process routed Markdown files.
fn process_markdown(
    config: &Config, plugins: &plugin::Settings,
    routed: &Stream<Id, RoutedMarkdown>,
) -> Stream<Id, RenderedMarkdown> {
    // Create pipeline to render Markdown files
    let plugins = plugins.clone();
    let config = config.clone();
    routed
        // Render Markdown if we don't have a recent cached version at our own
        // disposal. Otherwise, just return that if the content did not change.
        // Note that we need to limit concurrency here, or we'll overwhelm the
        // Python interpreter with all tasks competing for the GIL.
        .map(concurrent(1, move |id: &Id, routed: &RoutedMarkdown| {
            let data = fs::read_to_string(&*routed.input.source)?;
            let route = routed.route.clone();

            let (data, page_meta) = meta::front_matter(&route.source, &data)?;
            let resolved =
                routed.input.metadata.resolve(&route.source, page_meta)?;
            // Don't cache page if it inserts (pymdownx) snippets.
            // This is a hack while waiting for CommonMark (AST) and components,
            // as well as topic-based authoring functionality.
            if SNIPPET_RE.is_match(&data) {
                render_markdown(id, route, data, plugins.clone(), resolved)
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
                        render_markdown(
                            id,
                            route,
                            data,
                            plugins.clone(),
                            resolved,
                        )
                    },
                )
            }
        }))
}

/// Applies revision-complete tag listings and page-level tag references.
fn apply_tags(
    pipeline: &tags::Tags, pages: &Stream<Id, RenderedPage>,
) -> Stream<Id, RenderedPage> {
    if pipeline.is_empty() {
        return pages.clone();
    }

    let inputs = pages.map(|rendered: &RenderedPage| tags::PageInput {
        page: rendered.page.clone(),
        facts: rendered.html.tags.clone(),
    });
    let patches = pipeline.setup(tags::Dependencies { pages: &inputs });
    (pages.clone(), patches).join().map(
        |(rendered, patch): &(RenderedPage, tags::Patch)| {
            let mut rendered = rendered.clone();
            rendered.page.apply_derived(
                patch.content.clone(),
                patch.toc.clone(),
                patch.variables.clone(),
            );
            if let Some(search) = &patch.search {
                rendered.html.search = search.clone();
            }
            rendered
        },
    )
}

/// Render Markdown and collect the page-local facts produced alongside it.
fn render_markdown(
    id: &Id, route: PageRoute, content: String, plugins: plugin::Settings,
    meta: meta::Resolved,
) -> anyhow::Result<RenderedMarkdown> {
    let mut markdown =
        Markdown::new(id, route.url.clone(), content, meta.values())?;
    let html = plugin::prepare(&mut markdown, &route.source, &plugins)?;
    let registrations = plugins.autorefs.take_page(&route.url);
    Ok(RenderedMarkdown {
        route,
        markdown,
        registrations,
        html,
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
    })
}

/// Render static and extra templates.
fn render_templates(
    config: &Config, files: &Stream<Id, Input>, nav: &Signal<Id, Navigation>,
    assets: &Signal<Id, minify::Manifest>, minify: &minify::Minify,
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
    theme_dirs.push(config.docs_root().as_path().to_owned());

    // Create pipeline to render templates
    let renderer = Template::new(theme_dirs);
    let minify = minify.clone();
    let config = config.clone();
    templates.product(nav).product(assets).map(
        move |id: &Id,
              input: &(Input, Navigation),
              assets: &minify::Manifest| {
            let (_, nav) = input;
            let output = template_output(id)?;
            let name = output.as_str();

            // Render template and write to disk
            let data = renderer.render(name, &config, nav, &assets.project)?;
            let data = minify.template(name, data);
            let path = config.output_root().join(&output);
            fs::create_dir_all(path.parent().expect("invariant"))?;
            fs::write(path, &data)?;
            Ok::<_, anyhow::Error>(())
        },
    )
}

/// Maps a template provider identity to its MkDocs-compatible root output.
fn template_output(id: &Id) -> Result<SitePath, PathError> {
    let source = id.location().parse::<SourcePath>()?;
    source.file_name().parse()
}

/// Render pages.
fn render_pages(
    config: &Config, pages: &Stream<Id, SitePage>,
    nav: &Signal<Id, Navigation>, autorefs: &Signal<Id, autorefs::Registry>,
    assets: &Signal<Id, minify::Manifest>, minify: &minify::Minify,
) -> Stream<Id, UnresolvedAutorefs> {
    let pages = pages.product(nav).product(autorefs).product(assets).map(
        |input: &((SitePage, Navigation), autorefs::Registry),
         assets: &minify::Manifest| {
            let ((page, nav), autorefs) = input;
            PageRender {
                input: page.clone(),
                nav: nav.clone(),
                autorefs: autorefs.clone(),
                project: assets.project.clone(),
                asset_hash: assets.hash,
            }
        },
    );

    let template = OnceLock::new();
    let theme_dirs = config.theme_dirs.clone();
    let minify = minify.clone();
    let config = config.clone();
    pages.map(move |input: &PageRender| {
        let mut page = input.input.page.clone();
        let references = &input.input.autorefs;
        let id = page.url.clone();

        // Cache template rendering independently of autorefs, which are
        // substituted below on every pass. Deriving a cache key for the
        // substitution would require the same resolution scan that the
        // substitution itself performs, so caching it can't pay off.
        let args = (
            config.hash,
            input.nav.hash,
            input.asset_hash,
            page_hash(&page, references),
        );
        let rendered =
            cached(&config, ("template", id), args, |(_, _, _, _)| {
                let template =
                    template.get_or_init(|| Template::new(theme_dirs.clone()));
                Ok(page.render_template(
                    template,
                    &config,
                    input.nav.clone(),
                    &input.project,
                )?)
            })?;

        // Replace autorefs and retain unresolved identifiers
        let (data, unresolved) =
            input.autorefs.replace_in(rendered, references, &page.url);
        let data = minify.html(data);

        let path = config.output_root().join(page.destination());
        fs::create_dir_all(path.parent().expect("invariant"))?;
        fs::write(&path, &data)?;
        Ok::<_, anyhow::Error>(unresolved)
    })
}

/// Creates a workflow for the given config.
pub fn create_workflow(
    config: &Config, strict: bool, serve: bool, meta: meta::Meta,
) -> Workflow<Id> {
    Workflow::build(|workflow| {
        Main {
            config: config.clone(),
            strict,
            serve,
            meta,
        }
        .setup(workflow);
    })
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use zrx::id::Id;

    use crate::structure::nav::{Navigation, NavigationItem};

    use super::{template_output, PageTitles};

    fn item(title: &str, url: &str, page: bool) -> NavigationItem {
        NavigationItem {
            title: Some(title.into()),
            url: Some(url.into()),
            canonical_url: None,
            meta: page.then(BTreeMap::new),
            children: Vec::new(),
            is_index: false,
            active: false,
        }
    }

    #[test]
    fn page_titles_index_first_page_occurrence_and_ignores_links() {
        let navigation = Navigation {
            items: Arc::new(vec![
                item("First", "page/", true),
                item("Second", "page/", true),
                item("Link", "link/", false),
            ]),
            homepage: None,
            hash: 0,
            generation: 0,
        };

        let titles = PageTitles::new(&navigation);

        assert_eq!(titles.get("page/"), Some("First"));
        assert_eq!(titles.get("link/"), None);
    }

    #[test]
    fn template_outputs_use_logical_provider_identity() {
        let id = Id::builder()
            .provider("file")
            .context("templates/0")
            .location("nested/café.html")
            .build()
            .unwrap();

        assert_eq!(template_output(&id).unwrap().as_str(), "café.html");
    }
}
