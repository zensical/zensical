// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! External asset processing for the MkDocs-compatible minify plugin.

use anyhow::{anyhow, Context as _};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use sha2::{Digest, Sha384};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::{fs, io};
use zrx::id::Id;
use zrx::scheduler::action::{Action, Concurrency, Context};
use zrx::stream::function::Collection;
use zrx::stream::operator::Operator;
use zrx::stream::{Change, Key, Signal, Stream, Value};

use crate::config::plugins::MinifyPluginConfig;
use crate::config::{Config, Project};

use super::{script, style};

// -----------------------------------------------------------------------------
// Structs
// -----------------------------------------------------------------------------

/// One source that may become a site asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Resource {
    /// Logical output path relative to the site directory.
    pub(crate) path: String,
    /// Physical source path.
    pub(crate) source: PathBuf,
    /// Override priority, with lower values taking precedence.
    pub(crate) priority: usize,
}

impl Value for Resource {}

/// Final asset bytes or a source file that can be copied without reading it.
#[derive(Clone, Debug)]
enum Contents {
    Copy(PathBuf),
    Bytes(Arc<[u8]>),
}

/// One fully resolved output asset.
#[derive(Clone, Debug)]
struct Emission {
    source_path: String,
    output_path: String,
    contents: Contents,
    claimed: bool,
}

impl Value for Emission {}

/// Path rewrite produced by one claimed asset.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Mapping {
    source_path: String,
    output_path: String,
}

impl Value for Mapping {}

/// Asset-derived project view consumed by template rendering.
#[derive(Clone, Debug)]
pub(crate) struct Manifest {
    /// Project with configured asset paths rewritten to their emitted names.
    pub(crate) project: Arc<Project>,
    /// Stable hash of the current path mapping.
    pub(crate) hash: u64,
}

impl Value for Manifest {}

/// Compiled settings for external asset selection and transformation.
#[derive(Clone, Debug)]
struct Settings {
    enabled: bool,
    javascript: Selectors,
    stylesheet: Selectors,
    minify: BTreeSet<AssetKind>,
    cache_safe: bool,
}

/// Exact and glob selectors for one kind of asset.
#[derive(Clone, Debug)]
struct Selectors {
    exact: BTreeSet<String>,
    globs: GlobSet,
    error: Option<String>,
}

/// Writes insertions and removes retracted output paths.
#[derive(Clone)]
struct Writer {
    root_dir: PathBuf,
    site_dir: String,
}

// -----------------------------------------------------------------------------
// Implementations
// -----------------------------------------------------------------------------

impl Settings {
    fn new(config: &MinifyPluginConfig) -> Self {
        Self {
            enabled: config.enabled,
            javascript: Selectors::new(&config.js_files),
            stylesheet: Selectors::new(&config.css_files),
            minify: [
                config.minify_js.then_some(AssetKind::JavaScript),
                config.minify_css.then_some(AssetKind::Stylesheet),
            ]
            .into_iter()
            .flatten()
            .collect(),
            cache_safe: config.cache_safe,
        }
    }

    fn claim(&self, path: &str) -> anyhow::Result<Option<AssetKind>> {
        if !self.active() {
            return Ok(None);
        }
        let javascript = self.active_kind(AssetKind::JavaScript)
            && self.javascript.matches(path)?;
        let stylesheet = self.active_kind(AssetKind::Stylesheet)
            && self.stylesheet.matches(path)?;
        match (javascript, stylesheet) {
            (true, true) => Err(anyhow!(
                "asset is selected as both JavaScript and CSS: {path}"
            )),
            (true, false) => {
                ensure_extension(path, "js")?;
                Ok(Some(AssetKind::JavaScript))
            }
            (false, true) => {
                ensure_extension(path, "css")?;
                Ok(Some(AssetKind::Stylesheet))
            }
            (false, false) => Ok(None),
        }
    }

    fn transform(&self, resource: &Resource) -> anyhow::Result<Emission> {
        let Some(kind) = self.claim(&resource.path)? else {
            return Ok(Emission {
                source_path: resource.path.clone(),
                output_path: resource.path.clone(),
                contents: Contents::Copy(resource.source.clone()),
                claimed: false,
            });
        };

        let source =
            fs::read_to_string(&resource.source).with_context(|| {
                format!("failed to read asset {}", resource.source.display())
            })?;
        let minify = self.minify.contains(&kind);
        let transformed = if minify {
            match kind {
                AssetKind::JavaScript => script::minify(&source, false),
                AssetKind::Stylesheet => style::minify(&source),
            }
            .filter(|output| output.len() < source.len())
            .unwrap_or(source)
        } else {
            source
        };
        let bytes = Arc::<[u8]>::from(transformed.into_bytes());
        let output_path = output_path(
            &resource.path,
            minify,
            self.cache_safe.then(|| digest(&bytes)),
        )?;
        Ok(Emission {
            source_path: resource.path.clone(),
            output_path,
            contents: Contents::Bytes(bytes),
            claimed: true,
        })
    }

    fn validate<'a>(
        &self, mappings: impl Iterator<Item = &'a Mapping>,
    ) -> anyhow::Result<()> {
        if !self.active() {
            return Ok(());
        }
        for (kind, selectors) in [
            (AssetKind::JavaScript, &self.javascript),
            (AssetKind::Stylesheet, &self.stylesheet),
        ] {
            if !self.active_kind(kind) {
                continue;
            }
            if let Some(error) = &selectors.error {
                return Err(anyhow!("invalid minify asset selector: {error}"));
            }
        }
        let found = mappings
            .map(|mapping| mapping.source_path.as_str())
            .collect::<BTreeSet<_>>();
        let missing = [
            (AssetKind::JavaScript, &self.javascript),
            (AssetKind::Stylesheet, &self.stylesheet),
        ]
        .into_iter()
        .filter(|(kind, _)| self.active_kind(*kind))
        .flat_map(|(_, selectors)| &selectors.exact)
        .filter(|path| !found.contains(path.as_str()))
        .collect::<Vec<_>>();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(anyhow!(
                "selected asset does not exist: {}",
                missing
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    }

    fn active(&self) -> bool {
        self.active_kind(AssetKind::JavaScript)
            || self.active_kind(AssetKind::Stylesheet)
    }

    fn active_kind(&self, kind: AssetKind) -> bool {
        if !self.enabled || (!self.minify.contains(&kind) && !self.cache_safe) {
            return false;
        }
        match kind {
            AssetKind::JavaScript => !self.javascript.is_empty(),
            AssetKind::Stylesheet => !self.stylesheet.is_empty(),
        }
    }
}

impl Selectors {
    fn new(patterns: &[String]) -> Self {
        let mut exact = BTreeSet::new();
        let mut builder = GlobSetBuilder::new();
        let mut error = None;
        for pattern in patterns {
            let pattern = normalize(pattern);
            if let Err(reason) = validate_relative(&pattern) {
                error.get_or_insert_with(|| reason.to_string());
                continue;
            }
            if contains_glob(&pattern) {
                match GlobBuilder::new(&pattern)
                    .literal_separator(true)
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
            } else {
                exact.insert(pattern);
            }
        }
        let globs = builder.build().unwrap_or_else(|reason| {
            error.get_or_insert_with(|| reason.to_string());
            GlobSetBuilder::new().build().expect("empty glob set")
        });
        Self { exact, globs, error }
    }

    fn matches(&self, path: &str) -> anyhow::Result<bool> {
        if let Some(error) = &self.error {
            return Err(anyhow!("invalid minify asset selector: {error}"));
        }
        let path = normalize(path);
        Ok(self.exact.contains(&path) || self.globs.is_match(path))
    }

    fn is_empty(&self) -> bool {
        self.error.is_none() && self.exact.is_empty() && self.globs.is_empty()
    }
}

impl Manifest {
    pub(crate) fn base(project: Arc<Project>) -> Self {
        Self { project, hash: 0 }
    }

    fn new<'a>(
        project: Arc<Project>, mappings: impl Iterator<Item = &'a Mapping>,
    ) -> Self {
        let paths = mappings
            .map(|mapping| {
                (mapping.source_path.clone(), mapping.output_path.clone())
            })
            .collect::<BTreeMap<_, _>>();
        if paths.is_empty() {
            return Self { project, hash: 0 };
        }

        let mut projected = (*project).clone();
        projected.extra_css = projected
            .extra_css
            .iter()
            .map(|path| rewrite(path, &paths))
            .collect();
        projected.extra_javascript = projected
            .extra_javascript
            .iter()
            .cloned()
            .map(|mut script| {
                script.path = rewrite(&script.path, &paths);
                script
            })
            .collect();
        let changed = projected.extra_css != project.extra_css
            || projected
                .extra_javascript
                .iter()
                .map(|script| &script.path)
                .ne(project.extra_javascript.iter().map(|script| &script.path));
        if !changed {
            return Self { project, hash: 0 };
        }
        let mut hasher = DefaultHasher::new();
        projected.extra_css.hash(&mut hasher);
        projected.extra_javascript.hash(&mut hasher);
        Self {
            project: Arc::new(projected),
            hash: hasher.finish(),
        }
    }
}

impl Writer {
    fn output_path(&self, key: &Key<Id>) -> anyhow::Result<PathBuf> {
        let id = key.try_as_id()?;
        if id.context() != self.site_dir {
            return Err(anyhow!("asset output escaped the site directory"));
        }
        let location = id.location();
        validate_relative(&location)?;
        Ok(self.root_dir.join(id.to_path()))
    }

    fn insert(&self, key: &Key<Id>, emission: &Emission) -> anyhow::Result<()> {
        let path = self.output_path(key)?;
        fs::create_dir_all(path.parent().expect("site asset has parent"))?;
        match &emission.contents {
            Contents::Copy(source) => copy_file(source, path)?,
            Contents::Bytes(bytes) => fs::write(path, bytes)?,
        }
        Ok(())
    }

    fn remove(&self, key: &Key<Id>) -> anyhow::Result<()> {
        let path = self.output_path(key)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

impl Action<Key<Id>> for Writer {
    type Inputs = (Emission,);
    type Output = ();

    fn concurrency(&self) -> Concurrency<Self> {
        Concurrency::adaptive()
    }

    fn execute(&mut self, context: Context<'_, Key<Id>, Self>) {
        let Context { inputs: input, output, .. } = context;
        input.for_each(output, |change, emit| {
            match change {
                Change::Insert(key, emission) => {
                    self.insert(&key, emission.as_ref())?;
                    emit.insert(key, ());
                }
                Change::Remove(key) => {
                    self.remove(&key)?;
                    emit.remove(key);
                }
            }
            Ok(())
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum AssetKind {
    JavaScript,
    Stylesheet,
}

// -----------------------------------------------------------------------------
// Functions
// -----------------------------------------------------------------------------

/// Transforms selected resources, writes every effective asset, and publishes
/// the project view whose configured asset paths name the emitted files.
pub(crate) fn attach(
    config: &Config, resources: &Stream<Id, Resource>,
) -> Signal<Id, Manifest> {
    let settings = Settings::new(&config.project.plugins.minify.config);
    let settings_for_transform = settings.clone();
    let emissions = resources.map(move |resource: &Resource| {
        settings_for_transform.transform(resource)
    });

    let site_dir = config.project.site_dir.clone();
    let site_dir_for_key = site_dir.clone();
    let outputs = emissions.unique_by_key(move |emission: &Emission| {
        output_key(&site_dir_for_key, &emission.output_path)
    });
    let _ = outputs.subscribe(Writer {
        root_dir: config.get_root_dir(),
        site_dir,
    });

    let project = config.project.clone();
    let settings_for_manifest = settings;
    outputs
        .filter(|emission: &Emission| emission.claimed)
        .map(|emission: &Emission| Mapping {
            source_path: emission.source_path.clone(),
            output_path: emission.output_path.clone(),
        })
        .reduce(move |mappings: &dyn Collection<Key<Id>, Mapping>| {
            settings_for_manifest
                .validate(mappings.iter().map(|(_, mapping)| mapping))?;
            Ok::<_, anyhow::Error>(Some(Manifest::new(
                project.clone(),
                mappings.iter().map(|(_, mapping)| mapping),
            )))
        })
}

/// Returns whether configuration claims any external asset resources.
pub(crate) fn is_enabled(config: &Config) -> bool {
    Settings::new(&config.project.plugins.minify.config).active()
}

/// Creates the group key used to resolve theme and project overrides before
/// any asset is transformed or written.
pub(crate) fn resource_key(path: &str) -> anyhow::Result<Key<Id>> {
    validate_relative(path)?;
    let id = Id::builder()
        .provider("asset")
        .context(".")
        .location(path)
        .build()?;
    Ok(Key::from(id))
}

fn output_key(site_dir: &str, path: &str) -> anyhow::Result<Key<Id>> {
    validate_relative(path)?;
    let id = Id::builder()
        .provider("file")
        .context(site_dir)
        .location(path)
        .build()?;
    Ok(Key::from(id))
}

fn output_path(
    path: &str, minify: bool, digest: Option<String>,
) -> anyhow::Result<String> {
    let path = Path::new(path);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            anyhow!("selected asset has no extension: {}", path.display())
        })?;
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            anyhow!("selected asset has no file name: {}", path.display())
        })?;
    let mut name = stem.to_owned();
    if let Some(digest) = digest {
        name.push('.');
        name.push_str(&digest[..6]);
    }
    if minify {
        name.push_str(".min");
    }
    name.push('.');
    name.push_str(extension);
    Ok(path
        .with_file_name(name)
        .to_string_lossy()
        .replace('\\', "/"))
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha384::digest(bytes))
}

fn rewrite(path: &str, mappings: &BTreeMap<String, String>) -> String {
    let normalized = normalize(path);
    mappings
        .get(&normalized)
        .cloned()
        .unwrap_or_else(|| path.to_owned())
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_owned()
}

fn contains_glob(pattern: &str) -> bool {
    pattern
        .bytes()
        .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']'))
}

fn validate_relative(path: &str) -> anyhow::Result<()> {
    if path.is_empty()
        || Path::new(path).is_absolute()
        || Path::new(path)
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(anyhow!("asset path must be relative to the site: {path}"));
    }
    Ok(())
}

fn ensure_extension(path: &str, expected: &str) -> anyhow::Result<()> {
    if Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
    {
        Ok(())
    } else {
        Err(anyhow!(
            "selected asset must have a .{expected} extension: {path}"
        ))
    }
}

fn copy_file(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    let mut from = fs::File::open(from)?;
    let mut to = fs::File::create(to)?;
    io::copy(&mut from, &mut to).map(|_| ())
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_support_exact_paths_and_recursive_globs() {
        let selectors =
            Selectors::new(&["scripts/app.js".into(), "vendor/**/*.js".into()]);
        assert!(selectors.matches("./scripts/app.js").unwrap());
        assert!(selectors.matches("vendor/lib/tool.js").unwrap());
        assert!(!selectors.matches("scripts/other.js").unwrap());
    }

    #[test]
    fn output_names_follow_minify_and_cache_safe_modes() {
        assert_eq!(
            output_path("assets/app.js", true, None).unwrap(),
            "assets/app.min.js"
        );
        assert_eq!(
            output_path("assets/app.js", false, Some("abcdef12".into()))
                .unwrap(),
            "assets/app.abcdef.js"
        );
        assert_eq!(
            output_path("assets/app.js", true, Some("abcdef12".into()))
                .unwrap(),
            "assets/app.abcdef.min.js"
        );
    }

    #[test]
    fn digest_uses_sha384_and_emitted_bytes() {
        assert_eq!(&digest(b"const value=1;")[..6], "11c998");
    }

    #[test]
    fn rewrite_preserves_unmatched_and_structured_paths() {
        let paths = BTreeMap::from([(
            "assets/app.js".into(),
            "assets/app.abcdef.min.js".into(),
        )]);
        assert_eq!(
            rewrite("./assets/app.js", &paths),
            "assets/app.abcdef.min.js"
        );
        assert_eq!(
            rewrite("https://example.com/app.js", &paths),
            "https://example.com/app.js"
        );
    }
}
