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

use percent_encoding::percent_decode_str;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
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
    concurrent, Key, Signal, Stream, StreamSetExt, StreamTupleExt, Value,
    Workflow,
};

use crate::compat::mkdocs::plugin::autorefs::UnresolvedAutorefs;
use crate::compat::mkdocs::{
    html,
    plugin::{
        self, autorefs, awesome_nav, blog, literate_nav, meta, minify,
        mkdocstrings, redirects, rss, search, tags,
    },
    resource,
};
use crate::config::Config;
use crate::path::{PathError, SitePath, SourcePath};
use crate::python::{Anchors, Issues, References, SharedReferences};
use crate::structure::document::DocumentHeader;
use crate::structure::dynamic::Dynamic;
use crate::structure::markdown::Markdown;
use crate::structure::nav::{Navigation, NavigationResolution};
use crate::structure::page::{Page, PageDescriptor, PageOrigin, PageRoute};
use crate::template::Template;
use crate::watcher::Source;

mod cached;
pub(crate) mod output;

use cached::cached;

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Regular expression to detect use of snippets
static SNIPPET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*-+8<-+").expect("invariant"));

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

/// Rendered page artifact paired with its validation facts.
#[derive(Clone, Debug)]
struct RenderedSitePage {
    /// Removal-aware, source-owned output artifact.
    artifact: output::Artifact,
    /// Autorefs that could not be resolved in this revision.
    unresolved: UnresolvedAutorefs,
}

impl Value for RenderedSitePage {}

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

/// Cached output of rendering one Markdown source.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RenderedMarkdown {
    /// Stable logical owner and edit provenance.
    origin: PageOrigin,
    /// Route computed once before Markdown rendering.
    route: PageRoute,
    /// Module-owned fields flattened into the template-facing page object.
    properties: BTreeMap<String, Dynamic>,
    /// Module-owned top-level template variables.
    variables: BTreeMap<String, Dynamic>,
    /// Rendered Markdown consumed by page construction.
    markdown: Markdown,
    /// Page title derived from metadata, Markdown, or source name.
    title: String,
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
    /// HTML compatibility facts revision-aligned with the page.
    html: plugin::HtmlFacts,
}

impl Value for RenderedPage {}

/// A source page's ordinary route and its configured published route.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RoutedLink {
    /// Route produced by ordinary Markdown source link resolution.
    source: String,
    /// Decoded spelling used by unescaped Markdown source links.
    decoded: String,
    /// Route where the page is actually published.
    published: String,
}

impl Value for RoutedLink {}

// ----------------------------------------------------------------------------

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Main {
    /// Initializes the module.
    #[allow(clippy::too_many_lines)]
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
        let documents = read_documents(&self.config, &files);
        let plugins = plugin::Settings::new(&self.config, self.serve);
        let blogs = plugins.blog.clone();
        let blog = setup_blog(&blogs, &documents, &sources);
        let view_pages = blog.view_pages;
        let ordered_views = blog.views;
        let posts = blog.posts;
        let markdown = blog.pages;

        // Redirects depend on routes, not rendered Markdown. Settle their
        // compact input independently so they can proceed concurrently with
        // the Python rendering branch.
        let routes = markdown.map(|input: &PageDescriptor| input.route.clone());
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

        let rendered = process_markdown(&self.config, &plugins, &markdown);

        // Navigation needs the final titles derived from Markdown.
        let provisional = generate_page(&self.config, &rendered);
        let provisional_page =
            provisional.map(|rendered: &RenderedPage| rendered.page.clone());
        let navigation_page =
            blogs.navigation_pages(&provisional_page, &view_pages);
        let autorefs = if plugins.autorefs.records_backlinks() {
            None
        } else {
            let autorefs_input = provisional.map(autorefs_page_input);
            Some(
                plugins
                    .autorefs
                    .setup(autorefs::Dependencies { pages: &autorefs_input }),
            )
        };
        let resolution = resolve_navigation(
            &self.config,
            self.strict,
            &blogs,
            &sources,
            &navigation_page,
            &provisional_page,
            &view_pages,
        );
        let blog_patches = blogs.patches(
            &provisional_page,
            &posts,
            &resources,
            &view_pages,
            &ordered_views,
            &resolution,
        );
        let nav = resolution
            .map(|value: &NavigationResolution| value.navigation.clone());
        // MkDocs assigns configured navigation titles when constructing Page
        // objects, before metadata and Markdown fallbacks are evaluated. Our
        // navigation is resolved later, so apply that highest-precedence title
        // once the complete navigation is available.
        let rendered_page = apply_navigation_titles(&provisional, &resolution);
        let rendered_page = apply_blog(&rendered_page, &blog_patches);
        let rendered_page = if blogs.is_empty() {
            rendered_page
        } else {
            apply_routed_links(&self.config, &rendered_page, &markdown)
        };
        let rendered_page = apply_tags(&plugins.tags, &rendered_page);
        let autorefs = if let Some(autorefs) = autorefs {
            autorefs
        } else {
            // Backlink breadcrumbs depend on final navigation titles and the
            // effective ToC, so collect them from one complete snapshot.
            let backlinks = rendered_page.map(autorefs_backlink_input);
            plugins
                .autorefs
                .setup_backlinks(autorefs::BacklinkDependencies {
                    pages: &backlinks,
                    navigation: &nav,
                })
        };
        let page =
            rendered_page.map(|rendered: &RenderedPage| rendered.page.clone());
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
        plugins.search.setup(search::Dependencies {
            documents: &search_document,
            navigation: &nav,
        });
        let mkdocstrings = mkdocstrings::Mkdocstrings::new(&self.config);
        mkdocstrings.setup(mkdocstrings::Dependencies { navigation: &nav });
        // Feed inputs are final pages and their original Markdown bodies.
        let rss_artifacts =
            rss::Rss::new(&self.config).setup(&page, &markdown, &configuration);
        let _ = render_templates(&self.config, &files, &nav, &assets, &minify);
        let unresolved = render_pages(
            &self.config,
            &site_page,
            &nav,
            &autorefs,
            &assets,
            &minify,
            &mkdocstrings,
            &rss_artifacts,
        );
        validate(&self.config, self.strict, &files, &page, &unresolved);
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Applies revision-complete blog view variables to their containing pages.
fn apply_blog(
    pages: &Stream<Id, RenderedPage>, patches: &Stream<Id, blog::Patch>,
) -> Stream<Id, RenderedPage> {
    (pages.clone(), patches.clone()).left_join().map(
        |(rendered, patch): &(RenderedPage, Option<blog::Patch>)| {
            let mut rendered = rendered.clone();
            if let Some(patch) = patch {
                if let Some(url) = &patch.navigation_url {
                    rendered.page.apply_navigation_url(url.clone());
                }
                if let Some((previous, next)) = &patch.siblings {
                    rendered.page.apply_navigation_siblings(
                        previous.clone(),
                        next.clone(),
                    );
                }
                if let Some(template) = &patch.template {
                    rendered.page.apply_template(template.clone());
                }
                if patch.content.is_some() || patch.toc.is_some() {
                    rendered.page.apply_derived(
                        patch.content.clone(),
                        patch.toc.clone(),
                        BTreeMap::new(),
                    );
                }
                rendered.page.merge_properties(&patch.properties);
                rendered
                    .page
                    .merge_template_variables(patch.variables.clone());
            }
            rendered
        },
    )
}

/// Resolve source-file links after all pages have their published routes.
fn apply_routed_links(
    config: &Config, pages: &Stream<Id, RenderedPage>,
    routed: &Stream<Id, PageDescriptor>,
) -> Stream<Id, RenderedPage> {
    let config = config.clone();
    let relocations = routed.filter_map(move |descriptor: &PageDescriptor| {
        if !matches!(&descriptor.origin, PageOrigin::Source(_)) {
            return Ok(None);
        }
        let source = PageRoute::from_source(
            &config,
            descriptor.document.source.clone(),
        )?
        .url;
        let published = descriptor.route.url.clone();
        let decoded =
            percent_decode_str(&source).decode_utf8_lossy().into_owned();
        Ok::<_, PathError>((source != published).then_some(RoutedLink {
            source,
            decoded,
            published,
        }))
    });
    let selected = relocations.select(pages, |rendered| {
        let targets =
            html::local_targets(&rendered.page.content, &rendered.page.url);
        move |route: &RoutedLink| {
            targets.contains(&route.source) || targets.contains(&route.decoded)
        }
    });
    (pages.clone(), selected).join().map(
        |(rendered, routes): &(RenderedPage, Vec<(Key<Id>, RoutedLink)>)| {
            let mut rendered = rendered.clone();
            let mut mappings = HashMap::new();
            for (_, route) in routes {
                mappings
                    .entry(route.decoded.clone())
                    .or_insert_with(|| route.published.clone());
            }
            for (_, route) in routes {
                mappings.insert(route.source.clone(), route.published.clone());
            }
            if let Some(content) = html::rewrite_urls(
                &rendered.page.content,
                &rendered.page.url,
                &mappings,
            ) {
                rendered.page.apply_derived(
                    Some(content),
                    None,
                    BTreeMap::new(),
                );
            }
            rendered
        },
    )
}

fn resolve_navigation(
    config: &Config, strict: bool, blogs: &blog::Blog,
    sources: &Stream<Id, Source>, navigation_pages: &Stream<Id, Page>,
    all_pages: &Stream<Id, Page>, view_pages: &Stream<Id, blog::ViewPageSpec>,
) -> Signal<Id, NavigationResolution> {
    let awesome_nav = awesome_nav::AwesomeNav::new(config, strict)
        .expect("awesome-nav configuration is validated during loading");
    let resolution = if awesome_nav.is_enabled() {
        awesome_nav.setup(awesome_nav::Dependencies {
            sources,
            pages: navigation_pages,
        })
    } else {
        literate_nav::LiterateNav::new(config).setup(
            literate_nav::Dependencies {
                sources,
                pages: navigation_pages,
            },
        )
    };
    blogs.navigation(&resolution, all_pages, view_pages)
}

/// Retains the lightweight autorefs facts needed for ordinary link resolution.
fn autorefs_page_input(rendered: &RenderedPage) -> autorefs::PageInput {
    autorefs::PageInput {
        source: rendered.page.source().clone(),
        facts: rendered.html.autorefs_registrations.clone(),
    }
}

/// Retains resolved page data when backlink collection is enabled.
fn autorefs_backlink_input(rendered: &RenderedPage) -> autorefs::BacklinkInput {
    autorefs::BacklinkInput {
        page: rendered.page.clone(),
        facts: rendered.html.autorefs_registrations.clone(),
        references: rendered.html.autorefs.clone(),
    }
}

// ----------------------------------------------------------------------------

/// Applies explicit navigation titles to their pages.
fn apply_navigation_titles(
    pages: &Stream<Id, RenderedPage>,
    resolution: &Signal<Id, NavigationResolution>,
) -> Stream<Id, RenderedPage> {
    pages.product(resolution).map(
        |rendered: &RenderedPage, resolution: &NavigationResolution| {
            let mut rendered = rendered.clone();
            if let Some(title) = resolution.title(rendered.page.source()) {
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

fn setup_blog(
    blogs: &blog::Blog, documents: &Stream<Id, DocumentHeader>,
    sources: &Stream<Id, Source>,
) -> blog::Output {
    blogs.setup(blog::Dependencies { documents, sources })
}

/// Read Markdown sources and resolve their pre-render document facts.
fn read_documents(
    config: &Config, files: &Stream<Id, Input>,
) -> Stream<Id, DocumentHeader> {
    let matcher = Arc::new(
        Matcher::from_str(&format!(
            "zrs::::{}:**/*.md:",
            config.project.docs_dir
        ))
        .expect("invariant"),
    );
    files.filter_map(move |id: &Id, input: &Input| {
        if !matcher.is_match(id).expect("invariant") {
            return Ok(None);
        }
        let source = id.location().parse::<SourcePath>()?;
        if source.is_hidden() {
            return Ok(None);
        }
        let data = fs::read_to_string(&*input.source)?;
        let (body, page_meta) = meta::front_matter(&source, &data)?;
        let resolved = input.metadata.resolve(&source, page_meta)?;
        Ok::<_, anyhow::Error>(Some(DocumentHeader::new(
            source,
            body,
            resolved.values(),
        )))
    })
}

/// Returns whether Markdown contains a snippet marker.
pub fn has_snippets(data: &str) -> bool {
    SNIPPET_RE.is_match(data.strip_prefix('\u{FEFF}').unwrap_or(data))
}

/// Create a stream to process routed Markdown files.
fn process_markdown(
    config: &Config, plugins: &plugin::Settings,
    routed: &Stream<Id, PageDescriptor>,
) -> Stream<Id, RenderedMarkdown> {
    // Create pipeline to render Markdown files
    let plugins = plugins.clone();
    let config = config.clone();
    routed
        // Render Markdown if we don't have a recent cached version at our own
        // disposal. Otherwise, just return that if the content did not change.
        // Note that we need to limit concurrency here, or we'll overwhelm the
        // Python interpreter with all tasks competing for the GIL.
        .map(concurrent(1, move |routed: &PageDescriptor| {
            let origin = routed.origin.clone();
            let route = routed.route.clone();
            let document = routed.document.clone();
            let properties = routed.properties.clone();
            let variables = routed.variables.clone();
            // Don't cache page if it inserts (pymdownx) snippets.
            // This is a hack while waiting for CommonMark (AST) and components,
            // as well as topic-based authoring functionality.
            if has_snippets(&document.body) {
                render_markdown(
                    &config,
                    origin,
                    route,
                    document,
                    properties,
                    variables,
                    plugins.clone(),
                )
            } else {
                cached(
                    &config,
                    document.source.as_str(),
                    (
                        6_u8,
                        config.hash,
                        origin,
                        document.clone(),
                        route.clone(),
                        properties,
                        variables,
                    ),
                    |(_, _, origin, document, route, properties, variables)| {
                        render_markdown(
                            &config,
                            origin,
                            route,
                            document,
                            properties,
                            variables,
                            plugins.clone(),
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
            let variables = patch
                .variables
                .iter()
                .map(|(name, value)| {
                    Ok((name.clone(), Dynamic::from_serialize(value)?))
                })
                .collect::<Result<BTreeMap<_, _>, serde_json::Error>>()?;
            rendered.page.apply_derived(
                patch.content.clone(),
                patch.toc.clone(),
                variables,
            );
            if let Some(search) = &patch.search {
                rendered.html.search = search.clone();
            }
            Ok::<_, serde_json::Error>(rendered)
        },
    )
}

/// Render Markdown and collect the page-local facts produced alongside it.
fn render_markdown(
    config: &Config, origin: PageOrigin, route: PageRoute,
    document: DocumentHeader, properties: BTreeMap<String, Dynamic>,
    variables: BTreeMap<String, Dynamic>, plugins: plugin::Settings,
) -> anyhow::Result<RenderedMarkdown> {
    let mut properties = properties;
    let (mut markdown, title) = Markdown::new(
        &document.source,
        route.url.clone(),
        document.body,
        document.meta,
    )?;
    let source_route = PageRoute::from_source(config, document.source.clone())?;
    if let Some(content) =
        html::rebase_urls(&markdown.content, &source_route.url, &route.url)
    {
        markdown.replace_content(content);
    }
    let html =
        plugin::prepare(&mut markdown, &route.source, &route.url, &plugins)?;
    plugins.blog.apply_readtime(
        &route.source,
        &markdown.content,
        &mut properties,
    )?;
    Ok(RenderedMarkdown {
        origin,
        route,
        properties,
        variables,
        markdown,
        title,
        html,
    })
}

/// Generate pages from Markdown files.
fn generate_page(
    config: &Config, markdown: &Stream<Id, RenderedMarkdown>,
) -> Stream<Id, RenderedPage> {
    let config = config.clone();
    markdown.map(move |markdown: &RenderedMarkdown| {
        let mut page = match &markdown.origin {
            PageOrigin::Source(_) => Page::new(
                &config,
                markdown.route.clone(),
                markdown.markdown.clone(),
                markdown.title.clone(),
            ),
            PageOrigin::Generated { identity, provenance } => Page::generated(
                &config,
                identity.clone(),
                provenance.clone(),
                markdown.route.clone(),
                markdown.markdown.clone(),
                markdown.title.clone(),
            ),
        };
        page.apply_template_context(
            markdown.properties.clone(),
            markdown.variables.clone(),
        );
        RenderedPage {
            page,
            html: markdown.html.clone(),
        }
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
#[allow(clippy::too_many_arguments)]
fn render_pages(
    config: &Config, pages: &Stream<Id, SitePage>,
    nav: &Signal<Id, Navigation>, autorefs: &Signal<Id, autorefs::Registry>,
    assets: &Signal<Id, minify::Manifest>, minify: &minify::Minify,
    mkdocstrings: &mkdocstrings::Mkdocstrings,
    extra: &Stream<Id, output::Artifact>,
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
    let output = config.output_root().clone();
    let mkdocstrings = mkdocstrings.clone();
    let config = config.clone();
    let rendered = pages.map(move |input: &PageRender| {
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

        // Resolve template references and backlinks in the shared final pass.
        let (data, unresolved) = plugin::finalize(
            rendered.into(),
            references,
            &input.autorefs,
            &mkdocstrings,
            &page.url,
        )?;
        let data = minify.html(data);

        Ok::<_, anyhow::Error>(RenderedSitePage {
            artifact: output::Artifact::page(
                page.origin().clone(),
                page.destination().clone(),
                data.into_bytes(),
            ),
            unresolved,
        })
    });
    let artifacts =
        rendered.map(|rendered: &RenderedSitePage| rendered.artifact.clone());
    output::setup(output, &(artifacts, extra.clone()).coalesce());
    rendered.map(|rendered: &RenderedSitePage| rendered.unresolved.clone())
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
    use zrx::id::Id;

    use super::template_output;

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
