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

use ahash::{HashMap, HashSet};
use pyo3::types::PyAnyMethods;
use pyo3::Python;
use regex::Regex;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::{fs, io};
use zrx::id::matcher::Matcher;
use zrx::id::{id, Id};
use zrx::module::{self, Context, Module};
use zrx::scheduler::Key;
use zrx::stream::{Barrier, Stream, Workflow};

use super::config::Config;
use super::python::{Anchors, Issues, References};
use super::structure::markdown::{AutorefResolutions, Markdown};
use super::structure::nav::Navigation;
use super::structure::page::Page;
use super::structure::search::SearchIndex;
use super::template::Template;
use super::watcher::Source;

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
pub struct Main {
    /// Configuration.
    config: Config,
    /// Strict mode.
    strict: bool,
}

/// Page-content generation used to align validation inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PageGeneration(u64);

impl zrx::scheduler::Value for PageGeneration {}

/// Render generation used to align validation with rendered autoreferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ValidationGeneration(u64, u64);

impl zrx::scheduler::Value for ValidationGeneration {}

/// Navigation generation used to construct validation sites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ValidationNavigation(u64);

impl zrx::scheduler::Value for ValidationNavigation {}

/// Site-wide validation pages shared across workflow products.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SharedValidationPages(Arc<ValidationPages>);

impl zrx::scheduler::Value for SharedValidationPages {}

/// Site-wide validation inputs shared across rendered pages.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ValidationSite {
    /// Monotonically increasing site snapshot sequence.
    sequence: u64,
    /// Navigation hash used when pages were rendered.
    nav_hash: u64,
    /// References, anchors, and generations for all pages.
    pages: Arc<ValidationPages>,
}

impl zrx::scheduler::Value for ValidationSite {}

/// Validation state collected while pages are rendered.
#[derive(Debug, Default)]
struct ValidationState {
    /// Current site snapshot sequence.
    sequence: u64,
    /// Whether issues for the current site snapshot were printed.
    reported: bool,
    /// Pages expected in the current site snapshot.
    expected: HashSet<(Id, ValidationGeneration)>,
    /// Pages that have not rendered for the current site snapshot.
    missing: HashSet<(Id, ValidationGeneration)>,
    /// Current site snapshot, if validation inputs are ready.
    site: Option<ValidationSite>,
    /// Autoref resolutions indexed by source page and generation.
    resolutions: HashMap<(Id, ValidationGeneration), AutorefResolutions>,
}

type ValidationPages = Vec<(Key<Id>, (References, (Anchors, PageGeneration)))>;

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Module for Main {
    /// Initializes the module.
    fn setup(&self, ctx: &mut Context) -> module::Result {
        let files = ctx.add::<Source>();

        // Set up workflow to process static assets, as well as Markdown files,
        // and create a barrier to wait for the completion of all Markdown files
        process_theme_assets(&self.config, &files);
        process_assets(&self.config, &files);
        let markdown = process_markdown(&self.config, &files);

        // Generate pages, and use the barrier to ensure that all pages have been
        // processed, in order to create the navigation and search index
        let page = generate_page(&self.config, &markdown);
        let pages = page.select([wait_for_markdown(&self.config)]);

        // Generate navigation and search index
        let nav = generate_nav(&self.config, &pages);
        generate_search_index(&self.config, &nav, &pages);

        // Generate object inventory
        generate_object_inventory(&self.config, &pages);

        // Render static and extra templates, as well as pages.
        render_templates(&self.config, &files, &nav);
        let autorefs = render_pages(&self.config, &page, &nav);

        // Validate and print issues after all page autorefs have resolved.
        if self.config.project.validation.is_enabled() {
            let references = collect_references(&self.config, &files);
            let anchors = collect_anchors(&page);
            validate(
                &self.config,
                self.strict,
                references,
                anchors,
                autorefs,
                &nav,
            );
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

// Return condition waiting for all Markdown files
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
    config: &Config, files: &Stream<Id, Source>,
) -> Stream<Id, References> {
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
        .map(|Source { path }| fs::read_to_string(&*path)?.parse())
}

/// Create a stream to collect anchors and generations from pages.
pub fn collect_anchors(
    pages: &Stream<Id, Page>,
) -> Stream<Id, (Anchors, PageGeneration)> {
    pages.map(move |page: Page| {
        let generation = page_generation(&page);
        Ok::<_, anyhow::Error>((page.content.parse()?, generation))
    })
}

/// Create a stream to validate references against anchors.
pub fn validate(
    config: &Config, strict: bool, refs: Stream<Id, References>,
    anchors: Stream<Id, (Anchors, PageGeneration)>,
    autorefs: Stream<Id, (Id, ValidationGeneration, AutorefResolutions)>,
    nav: &Stream<Id, Navigation>,
) {
    let validation = config.project.validation.clone();
    let pages = refs
        .join(&anchors)
        .select([wait_for_markdown(config)])
        .map(|pages: ValidationPages| SharedValidationPages(Arc::new(pages)));
    let nav = nav.map(|nav: Navigation| ValidationNavigation(nav.hash));
    let sequence = Arc::new(AtomicU64::new(0));
    let pages = pages.join(&nav).map(
        move |pages: SharedValidationPages, nav: ValidationNavigation| {
            ValidationSite {
                sequence: sequence.fetch_add(1, Ordering::Relaxed) + 1,
                nav_hash: nav.0,
                pages: pages.0,
            }
        },
    );
    let state = Arc::new(Mutex::new(ValidationState::default()));

    let site_state = Arc::clone(&state);
    let site_validation = validation.clone();
    pages.map(move |site: ValidationSite| {
        let issues = {
            let mut state = site_state.lock().expect("invariant");
            update_validation_site(&mut state, site);
            validation_issues(&mut state)
        };
        if let Some(issues) = issues {
            issues.print(&site_validation, strict)?;
        }
        Ok::<_, anyhow::Error>(())
    });

    autorefs.map(
        move |source: Id,
              generation: ValidationGeneration,
              autorefs: AutorefResolutions| {
            let issues = {
                let mut state = state.lock().expect("invariant");
                let current = (source, generation);
                state.resolutions.insert(current.clone(), autorefs);
                if state.expected.contains(&current) {
                    state.missing.remove(&current);
                }
                validation_issues(&mut state)
            };
            if let Some(issues) = issues {
                issues.print(&validation, strict)?;
            }
            Ok::<_, anyhow::Error>(())
        },
    );
}

/// Update validation state with a new site snapshot.
fn update_validation_site(state: &mut ValidationState, site: ValidationSite) {
    if site.sequence <= state.sequence {
        return;
    }

    let expected = site
        .pages
        .iter()
        .map(|(key, (_, (_, page_generation)))| {
            let id = key.try_as_id().expect("invariant");
            let generation =
                ValidationGeneration(page_generation.0, site.nav_hash);
            (id.clone(), generation)
        })
        .collect::<HashSet<_>>();
    state.resolutions.retain(|key, _| expected.contains(key));
    state.missing = expected
        .iter()
        .filter(|key| !state.resolutions.contains_key(*key))
        .cloned()
        .collect();
    state.sequence = site.sequence;
    state.reported = false;
    state.expected = expected;
    state.site = Some(site);
}

/// Create issues once every page in the current snapshot has rendered.
fn validation_issues(state: &mut ValidationState) -> Option<Issues> {
    if state.reported || !state.missing.is_empty() {
        return None;
    }

    let site = state.site.as_ref()?;
    let iter = site.pages.iter().cloned().map(
        |(key, (references, (anchors, page_generation)))| {
            let id = key.try_as_id().expect("invariant");
            let generation =
                ValidationGeneration(page_generation.0, site.nav_hash);
            let autorefs = state
                .resolutions
                .get(&(id.clone(), generation))
                .cloned()
                .expect("invariant");
            (key, (references, anchors, autorefs))
        },
    );
    let issues = Issues::new(iter);
    state.reported = true;
    Some(issues)
}

/// Compute a generation from page content relevant to validation.
fn page_generation(page: &Page) -> PageGeneration {
    let mut hasher = DefaultHasher::new();
    page.content.hash(&mut hasher);
    page.meta.hash(&mut hasher);
    PageGeneration(hasher.finish())
}

/// Create a stream to process static assets.
pub fn process_assets(config: &Config, files: &Stream<Id, Source>) {
    let extra_templates = config.project.extra_templates.clone();
    let docs_dir = config.project.docs_dir.clone();
    let matcher = Arc::new(
        Matcher::from_str(&format!("zrs::::{docs_dir}::")).expect("invariant"),
    );

    // Create pipeline to copy static assets
    let site_dir = config.project.site_dir.clone();
    let root_dir = config.get_root_dir();
    files.map(move |id: &Id, from: Source| {
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
        let builder = id.to_builder().context(&site_dir);
        let id = builder.build().expect("invariant");

        // Compute parent path, create intermediate directories and copy files
        let to = root_dir.join(id.to_path());
        fs::create_dir_all(to.parent().expect("invariant"))?;
        copy_file(&*from, to)?;
        Ok::<(), anyhow::Error>(())
    });
}

/// Create a stream to process static assets in theme.
pub fn process_theme_assets(config: &Config, files: &Stream<Id, Source>) {
    let matcher =
        Arc::new(Matcher::from_str("zrs::::templates/*::").expect("invariant"));

    // Create pipeline to copy static assets
    let site_dir = config.project.site_dir.clone();
    let root_dir = config.get_root_dir();
    files.map(move |id: &Id, from: Source| {
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
        copy_file(&*from, to)?;
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

/// Create a stream to process Markdown files.
pub fn process_markdown(
    config: &Config, files: &Stream<Id, Source>,
) -> Stream<Id, Markdown> {
    let matcher = Arc::new(
        Matcher::from_str(&format!(
            "zrs::::{}:**/*.md:",
            config.project.docs_dir
        ))
        .expect("invariant"),
    );

    // Create pipeline to render Markdown files
    let config = config.clone();
    files
        .filter(move |id: &Id| matcher.is_match(id).expect("invariant"))
        // Render Markdown if we don't have a recent cached version at our own
        // disposal. Otherwise, just return that if the content did not change.
        // Note that we need to limit concurrency here, or we'll overwhelm the
        // Python interpreter with all tasks competing for the GIL.
        .map(move |id: &Id, path: Source| {
            let data = fs::read_to_string(&*path)?;

            // Remove Byte-Order-Mark (BOM)
            let data = data.strip_prefix('\u{FEFF}').unwrap_or(&data);
            let data = data.to_owned();

            // Compute URL using same logic as Page::new()
            let site_dir = config.project.site_dir.clone();
            let use_directory_urls = config.project.use_directory_urls;

            let builder = id.to_builder().context(&site_dir);
            let url_id = builder.clone().build().expect("invariant");

            let mut url_path: PathBuf = url_id.location().to_string().into();
            let is_index = url_path.ends_with("index.md")
                || url_path.ends_with("README.md");

            if url_path.ends_with("README.md") {
                url_path.pop();
                url_path = url_path.join("index.md");
            }

            if !use_directory_urls || is_index {
                url_path.set_extension("html");
            } else {
                url_path.set_extension("");
                url_path.push("index.html");
            }

            let url_path = url_path.to_string_lossy().into_owned();
            let url_id = builder
                .location(url_path.replace('\\', "/"))
                .build()
                .expect("invariant");

            let url = url_id.as_uri().to_string();
            let url = if use_directory_urls {
                url.trim_end_matches("index.html").to_string()
            } else {
                url
            };

            // Don't cache page if it inserts (pymdownx) snippets.
            // This is a hack while waiting for CommonMark (AST) and components,
            // as well as topic-based authoring functionality.
            if SNIPPET_RE.is_match(&data) {
                Markdown::new(id, url, data)
            } else {
                cached(
                    &config,
                    id.as_str(),
                    (config.hash, data.clone(), url.clone()),
                    |(_, data, url)| Markdown::new(id, url, data),
                )
            }
        })
}

/// Generate pages from Markdown files.
pub fn generate_page(
    config: &Config, markdown: &Stream<Id, Markdown>,
) -> Stream<Id, Page> {
    let config = config.clone();
    markdown.map(move |id: &Id, markdown| Page::new(&config, id, markdown))
}

/// Generate navigation from all pages.
pub fn generate_nav(
    config: &Config, pages: &Stream<Id, Vec<(Key<Id>, Page)>>,
) -> Stream<Id, Navigation> {
    let config = config.clone();
    pages.map(move |pages: Vec<(Key<Id>, Page)>| {
        Navigation::new(
            config.get_cache_dir(),
            config.project.nav.clone(),
            pages,
        )
    })
}

/// Generate object inventory
pub fn generate_object_inventory(
    config: &Config, pages: &Stream<Id, Vec<(Key<Id>, Page)>>,
) {
    // Retrieve inventory from Python interpreter using pyo3
    let config = config.clone();
    pages.map(move |_| {
        let cache_dir = config.get_cache_dir();
        let cache_path = cache_dir.join("objects.inv");

        // Load previously cached inventory, if any
        let cached = fs::read(&cache_path).ok();

        let data = Python::attach(|py| {
            let module = py.import("zensical.compat.mkdocstrings")?;
            module
                .call_method1("get_inventory", (cached,))?
                .extract::<Vec<u8>>()
        });

        // Write object inventory to disk and update cache
        if let Ok(data) = data {
            let site_dir = config.get_site_dir();
            let path = site_dir.join("objects.inv");
            let _ = fs::create_dir_all(path.parent().expect("invariant"));
            let _ = fs::write(path, &data);
            let _ = fs::create_dir_all(&cache_dir);
            let _ = fs::write(&cache_path, &data);
        }
    });
}

/// Generate search index
pub fn generate_search_index(
    config: &Config, nav: &Stream<Id, Navigation>,
    pages: &Stream<Id, Vec<(Key<Id>, Page)>>,
) {
    let config = config.clone();
    pages.product(nav).map(move |pages, nav| {
        let plugin = config.project.plugins.search.config.clone();
        let language = &config.project.theme.language;
        let search = SearchIndex::new(pages, &nav, plugin, language);

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
        Ok::<_, anyhow::Error>(())
    });
}

/// Render static and extra templates.
pub fn render_templates(
    config: &Config, files: &Stream<Id, Source>, nav: &Stream<Id, Navigation>,
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
    let config = config.clone();
    templates.product(nav).map(move |template: Source, nav| {
        let name = Path::new(&*template).file_name().expect("invariant");
        let site_dir = config.get_site_dir();

        // Obtain template
        let template =
            Template::new(name.to_string_lossy(), theme_dirs.clone());

        // Render template and write to disk
        let data = template.render(&config, &nav)?;
        let path = site_dir.join(name);
        fs::create_dir_all(path.parent().expect("invariant"))?;
        fs::write(path, &data)?;
        Ok::<_, anyhow::Error>(())
    })
}

/// Render pages.
pub fn render_pages(
    config: &Config, page: &Stream<Id, Page>, nav: &Stream<Id, Navigation>,
) -> Stream<Id, (Id, ValidationGeneration, AutorefResolutions)> {
    let config = config.clone();
    let pages = page.map(|id: &Id, page: Page| (id.clone(), page));
    pages.product(nav).map(
        move |(source, mut page): (Id, Page), nav: Navigation| {
            let id = page.url.clone();

            // Compute hash of page content
            let page_generation = page_generation(&page);
            let generation = ValidationGeneration(page_generation.0, nav.hash);

            // Render page if we don't have a recent cached version at our own
            // disposal. Otherwise, just return if the content did not change.
            let args = (config.hash, nav.hash, page_generation.0);
            cached(
                &config,
                id,
                args,
                |(_, _, _)| Ok(page.render(&config, nav)?),
            )
            .and_then(|(data, autorefs)| {
                let path = Path::new(&page.path);
                fs::create_dir_all(path.parent().expect("invariant"))?;
                fs::write(path, &*data)?;
                Ok((source, generation, autorefs))
            })
        },
    )
}

/// Creates a workflow for the given config.
pub fn create_workflow(config: &Config, strict: bool) -> Workflow<Id> {
    let mut context = Context::default();
    Main { config: config.clone(), strict }
        .setup(&mut context)
        .expect("invariant");
    context.into()
}
