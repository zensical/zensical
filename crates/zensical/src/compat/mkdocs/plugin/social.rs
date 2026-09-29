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

//! Material social plugin compatibility pipeline.

use anyhow::{bail, Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{concurrent, Key, Stream, Value};

use crate::config::plugins::{SocialPluginConfig, SocialPluginInstance};
use crate::config::{Config, Project};
use crate::path::{OutputRoot, SitePath};
use crate::structure::dynamic::Dynamic;
use crate::structure::page::Page;
use crate::watcher::Source;

mod font;
mod layout;
mod render;
mod writer;

use layout::Layout;
use render::{Renderer, Tag};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Material social compatibility pipeline.
#[derive(Clone, Debug)]
pub struct Social {
    /// Enabled plugin instances in configuration order.
    instances: Arc<[Instance]>,
    /// Maximum card-rendering concurrency across the instances.
    concurrency: usize,
    /// Root for generated site files.
    output: OutputRoot,
}

/// Inputs required to generate social cards and metadata.
pub struct Dependencies<'a> {
    /// Rendered pages after page-local compatibility processing.
    pub pages: &'a Stream<Id, Page>,
    /// Physical sources used to invalidate affected card cache checks.
    pub sources: &'a Stream<Id, Source>,
}

/// Social metadata derived for one page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    /// Rendered social tags to inject into the page head.
    tags: Arc<[Tag]>,
    /// Revision of the rendered tags for page cache invalidation.
    hash: u64,
}

/// One enabled social plugin configuration and its rendering state.
#[derive(Clone, Debug)]
struct Instance {
    /// Configuration-order priority for conflicting card paths.
    id: usize,
    /// Configured plugin name for diagnostics.
    name: String,
    /// Validated options for this instance.
    config: Arc<SocialPluginConfig>,
    /// Shared project settings used by templates and routes.
    project: Arc<Project>,
    /// Directory containing the project configuration.
    root: PathBuf,
    /// Renderer and its shared font and dependency caches.
    renderer: Renderer,
    /// Compiled page include and exclude patterns.
    filter: SourceFilter,
    /// Whether the current workflow serves changes continuously.
    serve: bool,
    /// Whether warnings fail the build.
    strict: bool,
}

/// Compiled source-path filters and any deferred pattern error.
#[derive(Clone, Debug)]
struct SourceFilter {
    /// Patterns that take precedence when configured.
    include: GlobSet,
    /// Patterns used when no include patterns are configured.
    exclude: GlobSet,
    /// Whether inclusion is decided by the include set.
    has_include: bool,
    /// Invalid pattern reported when the filter is used.
    error: Option<String>,
}

/// Generated card and its cached PNG source.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Card {
    /// Plugin instance that produced this card.
    instance: usize,
    /// Site-relative output path.
    path: SitePath,
    /// Cached PNG copied into the output tree.
    source: PathBuf,
}

/// Cards and HTML metadata derived from one page.
#[derive(Clone, Debug)]
struct Bundle {
    /// Generated cards keyed by plugin instance.
    cards: Vec<(Key<Id>, Card)>,
    /// Tags selected for this page.
    metadata: Metadata,
}

/// Instance key, generated card, and page metadata tags.
type Generated = (Key<Id>, Card, Vec<Tag>);

/// Content hash of one physical card dependency.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    /// Absolute path of the watched dependency.
    source: PathBuf,
    /// SHA-256 digest of its current bytes.
    digest: [u8; 32],
}

/// Revisions of watched assets available to one card-rendering pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AssetRevision(
    /// SHA-256 digests indexed by physical asset path.
    Arc<BTreeMap<PathBuf, [u8; 32]>>,
);

/// Card error that upstream treats as recoverable plugin input failure.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct PluginError(
    /// Error text surfaced according to the configured log level.
    String,
);

static TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Social {
    /// Resolves immutable settings for all configured plugin instances.
    pub fn new(config: &Config, serve: bool, strict: bool) -> Self {
        let instances = config
            .project
            .plugins
            .social
            .config
            .iter()
            .enumerate()
            .filter(|(_, plugin)| plugin.config.enabled)
            .map(|(id, plugin)| {
                Instance::new(id, plugin, config, serve, strict)
            })
            .collect::<Vec<_>>();
        let concurrency = instances
            .iter()
            .map(|instance| instance.config.concurrency)
            .max()
            .unwrap_or(1);
        if !instances.is_empty() && config.project.site_url.is_none() {
            eprintln!(
                "WARNING -  The 'site_url' option is not set. Social cards are generated but not linked."
            );
        }
        if instances.iter().any(|instance| instance.config.debug) {
            eprintln!(
                "WARNING -  Debug mode is enabled for the 'social' plugin."
            );
        }
        for instance in &instances {
            if instance.config.has_deprecated_cards_color() {
                eprintln!(
                    "WARNING -  The 'cards_color' option of the 'social' plugin is deprecated; use 'cards_layout_options.background_color' and 'cards_layout_options.color'."
                );
            }
            if instance.config.has_deprecated_cards_font() {
                eprintln!(
                    "WARNING -  The 'cards_font' option of the 'social' plugin is deprecated; use 'cards_layout_options.font_family'."
                );
            }
        }
        Self {
            instances: instances.into(),
            concurrency,
            output: config.output_root().clone(),
        }
    }

    /// Generates cards and returns page-local metadata for HTML injection.
    pub fn setup(
        &self, dependencies: Dependencies<'_>,
    ) -> Stream<Id, Metadata> {
        if self.instances.is_empty() {
            return dependencies.pages.map(|_page: &Page| Metadata::default());
        }
        let ignored = self
            .instances
            .iter()
            .map(|instance| {
                resolve_from(&instance.root, &instance.config.cache_dir)
            })
            .chain(std::iter::once(self.output.as_path().to_owned()))
            .collect::<Arc<[_]>>();
        let assets = dependencies
            .sources
            .filter(move |id: &Id, source: &Source| {
                is_card_dependency(id, source, &ignored)
            })
            .map(|source: &Source| {
                Ok::<_, anyhow::Error>(Fingerprint {
                    source: source.to_path_buf(),
                    digest: Sha256::digest(fs::read(&**source)?).into(),
                })
            })
            .reduce(|sources: &dyn Collection<Key<Id>, Fingerprint>| {
                Some(asset_revision(sources.values()))
            });
        let instances = self.instances.clone();
        let bundles = dependencies.pages.product(&assets).map(concurrent(
            self.concurrency,
            move |page: &Page, assets: &AssetRevision| {
                render_page(&instances, page, assets)
            },
        ));
        let cards = bundles
            .flat_map(|bundle: &Bundle| bundle.cards.clone())
            .reduce_by_key(
                |card: &Card| output_key(&card.path),
                |cards: &dyn Collection<Key<Id>, Card>| {
                    cards.values().max_by_key(|card| card.instance).cloned()
                },
            );
        writer::setup(self.output.clone(), &cards);
        bundles.map(|bundle: &Bundle| bundle.metadata.clone())
    }
}

impl Metadata {
    /// Inserts generated meta tags immediately before the closing head tag.
    pub fn inject(&self, mut html: String) -> String {
        if self.tags.is_empty() {
            return html;
        }
        let Some(offset) = html.find("</head>") else {
            return html;
        };
        let tags = self
            .tags
            .iter()
            .map(|tag| {
                format!(
                    "<meta property=\"{}\" content=\"{}\" />",
                    html_attribute(&tag.property),
                    html_attribute(&tag.content),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        html.insert_str(offset, &format!("{tags}\n"));
        html
    }

    /// Adds metadata to the page-render cache key.
    pub fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.hash, state);
    }
}

impl Instance {
    /// Creates rendering state for one enabled plugin instance.
    fn new(
        id: usize, plugin: &SocialPluginInstance, config: &Config, serve: bool,
        strict: bool,
    ) -> Self {
        let root = config
            .path
            .parent()
            .expect("configuration has parent")
            .to_owned();
        let cache = resolve_from(&root, &plugin.config.cache_dir);
        Self {
            id,
            name: plugin.name.clone(),
            config: Arc::new(plugin.config.clone()),
            project: config.project.clone(),
            root,
            renderer: Renderer::new(
                config.project.clone(),
                config.theme_dirs.clone(),
                cache,
            ),
            filter: SourceFilter::new(
                &plugin.config.cards_include,
                &plugin.config.cards_exclude,
            ),
            serve,
            strict,
        }
    }

    /// Builds one page's card, cache entry, and metadata tags.
    fn render(&self, page: &Page, assets: &AssetRevision) -> Result<Generated> {
        let name = page_string(page, "cards_layout")?
            .unwrap_or_else(|| self.config.cards_layout.clone());
        let name = name
            .strip_suffix(".yml")
            .or_else(|| name.strip_suffix(".yaml"))
            .unwrap_or(&name);
        let layout = self.layout(name)?;
        let options = page_options(page, &self.config.cards_layout_options)?;
        let path = card_path(
            &self.config.cards_dir,
            page.destination(),
            self.project.use_directory_urls,
            matches!(page.source().file_name(), "index.md" | "README.md"),
        )?;
        let prepared = self.renderer.prepare(&layout, page, &options)?;
        let dependencies =
            self.renderer.dependency_revision(&prepared, &assets.0)?;
        let source = self.card(&prepared, &dependencies)?;
        let tags = self
            .project
            .site_url
            .as_ref()
            .map(|site_url| {
                let url = format!(
                    "{}/{}",
                    site_url.trim_end_matches('/'),
                    path.as_str()
                );
                self.renderer.tags(&layout, page, &options, &url)
            })
            .transpose()?
            .unwrap_or_default();
        let key = instance_key(self.id)?;
        Ok((
            key,
            Card {
                instance: self.id,
                path,
                source,
            },
            tags,
        ))
    }

    /// Validates page options even when this instance does not render a card.
    fn validate_page(&self, page: &Page) -> Result<bool> {
        if !self.includes(page)? {
            return Ok(false);
        }
        page_string(page, "cards_layout")?;
        page_options(page, &self.config.cards_layout_options)?;
        Ok(true)
    }

    /// Checks page-level card settings and source-path filters.
    fn includes(&self, page: &Page) -> Result<bool> {
        self.filter.validate(&self.name)?;
        let cards = page_bool(page, "cards")?.unwrap_or(self.config.cards);
        if !cards {
            return Ok(false);
        }
        let source = page.source().as_str();
        if self.filter.has_include {
            Ok(self.filter.include.is_match(source))
        } else {
            Ok(!self.filter.exclude.is_match(source))
        }
    }

    /// Loads a custom layout or one of the bundled Material layouts.
    fn layout(&self, name: &str) -> Result<Layout> {
        validate_layout_name(name).map_err(plugin_error)?;
        let directory = resolve_from(&self.root, &self.config.cards_layout_dir);
        let path = directory.join(format!("{name}.yml"));
        if path.is_file() {
            let source = fs::read_to_string(&path).with_context(|| {
                format!("failed to read social layout '{}'", path.display())
            })?;
            return layout::parse(&path.display().to_string(), &source)
                .map_err(plugin_error);
        }
        let source = builtin_layout(name)
            .with_context(|| format!("social card layout not found: {name}"))
            .map_err(plugin_error)?;
        layout::parse(name, source).map_err(plugin_error)
    }

    /// Returns a cached PNG path, rendering the card when needed.
    fn card(
        &self, layout: &Layout, dependencies: &[u8; 32],
    ) -> Result<PathBuf> {
        let cache =
            resolve_from(&self.root, &self.config.cache_dir).join("cards");
        let digest = card_digest(layout, dependencies, self.debug())?;
        let path = cache.join(format!("{digest}.png"));
        if self.config.cache {
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => return Ok(path),
                Ok(_) => {
                    bail!(
                        "social card cache path is not a file: {}",
                        path.display()
                    )
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let contents = self.renderer.card(layout, self.debug())?;
        fs::create_dir_all(&cache)?;
        let temporary = cache.join(format!(
            ".{digest}.{}.{}.tmp",
            std::process::id(),
            TEMPORARY_ID.fetch_add(1, Ordering::Relaxed),
        ));
        fs::write(&temporary, &contents)?;
        if let Err(error) = replace_file(&temporary, &path) {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        Ok(path)
    }

    /// Returns active debug-overlay settings for this build mode.
    fn debug(&self) -> Option<(&str, bool, usize)> {
        (self.config.debug && (self.serve || self.config.debug_on_build))
            .then_some((
                self.config.debug_color.as_str(),
                self.config.debug_grid,
                self.config.debug_grid_step,
            ))
    }

    /// Reports a recoverable card error according to the configured level.
    fn report(&self, page: &Page, error: &anyhow::Error) -> Result<()> {
        match self.config.log_level.as_str() {
            "warn" => eprintln!(
                "WARNING -  Couldn't render social card for '{}': {error:#}",
                page.source()
            ),
            "info" => eprintln!(
                "INFO -  Couldn't render social card for '{}': {error:#}",
                page.source()
            ),
            "ignore" => return Ok(()),
            _ => unreachable!("social log level is validated during loading"),
        }
        if self.strict && self.config.log_level == "warn" {
            bail!("Aborted because --strict flag is set")
        }
        Ok(())
    }
}

impl SourceFilter {
    /// Compiles source patterns while retaining invalid-pattern diagnostics.
    fn new(include: &[String], exclude: &[String]) -> Self {
        let mut error = None;
        Self {
            include: compile_globs(include, &mut error),
            exclude: compile_globs(exclude, &mut error),
            has_include: !include.is_empty(),
            error,
        }
    }

    /// Reports a deferred invalid pattern for this plugin instance.
    fn validate(&self, name: &str) -> Result<()> {
        if let Some(error) = &self.error {
            bail!("invalid source pattern for plugin '{name}': {error}")
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Value for Metadata {}
impl Value for Card {}
impl Value for Bundle {}
impl Value for Fingerprint {}
impl Value for AssetRevision {}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

fn render_page(
    instances: &[Instance], page: &Page, assets: &AssetRevision,
) -> Result<Bundle> {
    let mut cards = Vec::new();
    let mut tags = Vec::new();
    for instance in instances {
        if !instance.validate_page(page)? {
            continue;
        }
        match instance.render(page, assets) {
            Ok((key, card, instance_tags)) => {
                cards.push((key, card));
                tags.extend(instance_tags);
            }
            Err(error)
                if instance.config.log
                    && error.downcast_ref::<PluginError>().is_some() =>
            {
                instance.report(page, &error)?;
            }
            Err(error) => return Err(error),
        }
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&tags, &mut hasher);
    Ok(Bundle {
        cards,
        metadata: Metadata {
            tags: tags.into(),
            hash: std::hash::Hasher::finish(&hasher),
        },
    })
}

fn card_digest(
    layout: &Layout, dependencies: &[u8; 32],
    debug: Option<(&str, bool, usize)>,
) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(b"zensical-social-card-v1");
    digest.update(serde_json::to_vec(&(layout.size, &layout.layers))?);
    digest.update(serde_json::to_vec(&debug)?);
    digest.update(dependencies);
    Ok(format!("{:x}", digest.finalize()))
}

fn plugin_error(error: anyhow::Error) -> anyhow::Error {
    PluginError(format!("{error:#}")).into()
}

fn asset_revision<'a>(
    sources: impl Iterator<Item = &'a Fingerprint>,
) -> AssetRevision {
    let mut files = BTreeMap::new();
    for source in sources {
        files.insert(source.source.clone(), source.digest);
    }
    AssetRevision(Arc::new(files))
}

fn is_card_dependency(id: &Id, source: &Source, ignored: &[PathBuf]) -> bool {
    if ignored
        .iter()
        .any(|directory| source.starts_with(directory))
    {
        return false;
    }
    let location = id.location();
    Path::new(location.as_ref())
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "yml"
                    | "yaml"
                    | "svg"
                    | "png"
                    | "jpg"
                    | "jpeg"
                    | "gif"
                    | "webp"
            )
        })
}

fn page_social(page: &Page) -> Result<Option<&BTreeMap<String, Dynamic>>> {
    match page.meta.get("social") {
        None | Some(Dynamic::Null) => Ok(None),
        Some(Dynamic::Map(value)) => Ok(Some(value)),
        Some(_) => bail!("page social configuration must be a mapping"),
    }
}

fn page_bool(page: &Page, name: &str) -> Result<Option<bool>> {
    match page_social(page)?.and_then(|config| config.get(name)) {
        None | Some(Dynamic::Null) => Ok(None),
        Some(Dynamic::Bool(value)) => Ok(Some(*value)),
        Some(_) => bail!("page social option '{name}' must be a Boolean"),
    }
}

fn page_string(page: &Page, name: &str) -> Result<Option<String>> {
    match page_social(page)?.and_then(|config| config.get(name)) {
        None | Some(Dynamic::Null) => Ok(None),
        Some(Dynamic::String(value)) => Ok(Some(value.clone())),
        Some(_) => bail!("page social option '{name}' must be a string"),
    }
}

fn page_options(
    page: &Page, defaults: &BTreeMap<String, Dynamic>,
) -> Result<BTreeMap<String, Dynamic>> {
    let mut options = defaults.clone();
    match page_social(page)?
        .and_then(|config| config.get("cards_layout_options"))
    {
        None | Some(Dynamic::Null) => {}
        Some(Dynamic::Map(values)) => options.extend(values.clone()),
        Some(_) => {
            bail!("page social option 'cards_layout_options' must be a mapping")
        }
    }
    Ok(options)
}

fn card_path(
    directory: &str, destination: &SitePath, use_directory_urls: bool,
    is_index: bool,
) -> Result<SitePath> {
    let mut path = destination.as_str().to_owned();
    let suffix = if use_directory_urls && !is_index {
        "/index.html"
    } else {
        ".html"
    };
    let stem = path.strip_suffix(suffix).with_context(|| {
        format!("unexpected page destination: {destination}")
    })?;
    path = format!("{stem}.png");
    Ok(directory.parse::<SitePath>()?.join(&path)?)
}

fn instance_key(instance: usize) -> Result<Key<Id>> {
    Ok(Key::from(
        Id::builder()
            .provider("social")
            .context("instance")
            .location(instance.to_string())
            .build()?,
    ))
}

fn output_key(path: &SitePath) -> Result<Key<Id>> {
    Ok(Key::from(
        Id::builder()
            .provider("file")
            .context(".")
            .location(path.as_str())
            .build()?,
    ))
}

fn compile_globs(patterns: &[String], error: &mut Option<String>) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        match GlobBuilder::new(pattern)
            .literal_separator(false)
            .backslash_escape(false)
            .build()
        {
            Ok(pattern) => {
                builder.add(pattern);
            }
            Err(reason) => {
                error.get_or_insert_with(|| reason.to_string());
            }
        }
    }
    builder.build().unwrap_or_else(|reason| {
        error.get_or_insert_with(|| reason.to_string());
        GlobSetBuilder::new().build().expect("empty glob set")
    })
}

fn validate_layout_name(name: &str) -> Result<()> {
    if name.is_empty()
        || Path::new(name).components().any(|component| {
            matches!(
                component,
                Component::Prefix(_)
                    | Component::RootDir
                    | Component::ParentDir
            )
        })
    {
        bail!("invalid social card layout name: {name}")
    }
    Ok(())
}

fn resolve_from(root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    }
}

fn replace_file(temporary: &Path, target: &Path) -> io::Result<()> {
    match fs::rename(temporary, target) {
        Ok(()) => Ok(()),
        Err(error)
            if target.is_file()
                && matches!(
                    error.kind(),
                    io::ErrorKind::AlreadyExists
                        | io::ErrorKind::PermissionDenied
                ) =>
        {
            fs::remove_file(target)?;
            fs::rename(temporary, target)
        }
        Err(error) => Err(error),
    }
}

fn builtin_layout(name: &str) -> Option<&'static str> {
    match name {
        "default" => Some(include_str!("social/layouts/default.yml")),
        "default/accent" => {
            Some(include_str!("social/layouts/default/accent.yml"))
        }
        "default/invert" => {
            Some(include_str!("social/layouts/default/invert.yml"))
        }
        "default/only/image" => {
            Some(include_str!("social/layouts/default/only/image.yml"))
        }
        "default/variant" => {
            Some(include_str!("social/layouts/default/variant.yml"))
        }
        _ => None,
    }
}

fn html_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        builtin_layout, card_path, html_attribute, layout, validate_layout_name,
    };
    use crate::path::SitePath;

    #[test]
    fn parses_all_bundled_layouts() {
        for name in [
            "default",
            "default/accent",
            "default/invert",
            "default/only/image",
            "default/variant",
        ] {
            layout::parse(name, builtin_layout(name).unwrap()).unwrap();
        }
    }

    #[test]
    fn derives_upstream_card_paths() {
        assert_eq!(
            card_path(
                "assets/images/social",
                &"guide/index.html".parse::<SitePath>().unwrap(),
                true,
                false,
            )
            .unwrap()
            .as_str(),
            "assets/images/social/guide.png"
        );
        assert_eq!(
            card_path(
                "assets/images/social",
                &"index.html".parse::<SitePath>().unwrap(),
                true,
                true,
            )
            .unwrap()
            .as_str(),
            "assets/images/social/index.png"
        );
    }

    #[test]
    fn rejects_layout_traversal_and_escapes_metadata() {
        assert!(validate_layout_name("../secret").is_err());
        assert_eq!(html_attribute("a&\"b"), "a&amp;&quot;b");
    }
}
