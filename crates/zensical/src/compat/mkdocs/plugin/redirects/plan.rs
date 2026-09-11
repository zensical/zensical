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

//! Redirect planning.
//!
//! Redirects pass through three representations:
//!
//! 1. [`Plan`] validates configuration and resolves redirect chains once.
//! 2. [`Snapshot`] resolves final internal targets against settled page routes.
//! 3. The output stage writes physical page redirects and `redirect.json`.

use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use crate::config::Config;
use crate::path::{SitePath, SourcePath};
use crate::structure::page::PageRoute;

/// Source suffixes recognized by MkDocs as Markdown.
const MARKDOWN_SUFFIXES: &[&str] =
    &[".markdown", ".mdown", ".mkdn", ".mkd", ".md"];

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Target classification prepared before route settlement.
#[derive(Clone, Debug)]
enum Target {
    /// External target copied into the redirect document unchanged.
    External(String),
    /// Internal Markdown source resolved against the live route relation.
    Internal {
        /// Original configured value used in diagnostics.
        configured: String,
        /// Source path without a fragment.
        source: String,
        /// Fragment including its leading `#`, when present.
        fragment: String,
    },
}

// ----------------------------------------------------------------------------

/// Final redirect target resolved against the current page routes.
enum ResolvedTarget<'a> {
    /// External target that is already ready for output.
    External(&'a str),
    /// Internal target composed from a public page URL and fragment.
    Internal {
        /// Site-relative public page URL.
        url: &'a str,
        /// Fragment including its leading `#`, when present.
        fragment: &'a str,
    },
}

// ----------------------------------------------------------------------------

/// Redirect source classification prepared before route settlement.
#[derive(Clone, Debug)]
enum Source {
    /// Page redirect emitted as a physical HTML artifact.
    Page(SitePath),
    /// Anchor redirect emitted into the manifest and matching redirect page.
    Anchor {
        /// Site-relative manifest key including its fragment.
        manifest_key: String,
        /// Physical redirect output for the source page.
        output: SitePath,
        /// Source fragment including its leading `#`.
        fragment: String,
    },
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Redirect configuration prepared independently of live page routes.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// Whether the redirects plugin is enabled.
    enabled: bool,
    /// Generated paths reserved by configured redirects.
    outputs: BTreeSet<SitePath>,
    /// Site-relative manifest keys reserved by configured redirects.
    manifest_keys: BTreeSet<String>,
    /// Internal source paths needed from the live route relation.
    route_sources: BTreeSet<String>,
    /// Validated redirects in deterministic configured-source order.
    specifications: Vec<Specification>,
    /// Diagnostics that depend only on configuration.
    warnings: Vec<String>,
}

// ----------------------------------------------------------------------------

/// One resolved redirect output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Redirect {
    /// Site-relative output path.
    pub output: SitePath,
    /// Resolved redirect target, or `None` when the target is missing.
    pub target: Option<String>,
    /// Fragment-specific target overrides relative to this output.
    pub overrides: BTreeMap<String, String>,
}

// ----------------------------------------------------------------------------

/// One validated redirect mapping awaiting route resolution.
#[derive(Clone, Debug)]
struct Specification {
    /// Original configured source used to resolve redirect chains.
    configured: String,
    /// Prepared page or anchor source.
    source: Source,
    /// Prepared internal or external target.
    target: Target,
}

// ----------------------------------------------------------------------------

/// Revision-settled redirect outputs and warnings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Redirects ordered by configured source URI.
    pub redirects: Vec<Redirect>,
    /// Anchor redirect manifest, or `None` when the plugin is disabled.
    pub manifest: Option<BTreeMap<String, String>>,
    /// Compatibility warnings emitted for this snapshot.
    pub warnings: Vec<String>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Target {
    /// Resolves an internal target against the current page routes.
    fn resolve<'a>(
        &'a self, routes: &'a BTreeMap<String, String>,
    ) -> Option<ResolvedTarget<'a>> {
        match self {
            Self::External(target) => Some(ResolvedTarget::External(target)),
            Self::Internal { source, fragment, .. } => routes
                .get(source)
                .map(|url| ResolvedTarget::Internal { url, fragment }),
        }
    }

    /// Returns the configured value for a missing internal target.
    fn missing(&self) -> Option<&str> {
        match self {
            Self::External(_) => None,
            Self::Internal { configured, .. } => Some(configured),
        }
    }
}

// ----------------------------------------------------------------------------

impl ResolvedTarget<'_> {
    /// Formats this target for the site-wide anchor manifest.
    fn for_manifest(&self) -> String {
        match self {
            Self::External(target) => (*target).into(),
            Self::Internal { url, fragment } => format!("{url}{fragment}"),
        }
    }

    /// Formats this target relative to one physical redirect page.
    fn for_redirect(
        &self, output: &SitePath, use_directory_urls: bool,
    ) -> String {
        match self {
            Self::External(target) => (*target).into(),
            Self::Internal { url, fragment } => {
                relative_target(output, url, fragment, use_directory_urls)
            }
        }
    }
}

// ----------------------------------------------------------------------------

impl Plan {
    /// Validates route-independent configuration once for the workflow.
    pub fn new(config: &Config) -> Result<Self> {
        let plugin = &config.project.plugins.redirects.config;
        if !plugin.enabled {
            return Ok(Self::default());
        }

        let mut plan = Self {
            enabled: true,
            specifications: Vec::with_capacity(plugin.redirect_maps.len()),
            ..Self::default()
        };
        validate_output(
            config,
            &"redirect.json".parse().expect("static site path"),
        )?;
        for (configured_source, configured_target) in &plugin.redirect_maps {
            let source = prepare_source(config, &mut plan, configured_source)?;
            let target = prepare_target(configured_target);
            plan.specifications.push(Specification {
                configured: configured_source.clone(),
                source,
                target,
            });
        }

        // Chains are configuration-only. Resolve them before collecting the
        // page routes needed by the remaining final internal targets.
        resolve_chains(&mut plan.specifications)?;
        plan.route_sources
            .extend(plan.specifications.iter().filter_map(|specification| {
                match &specification.target {
                    Target::Internal { source, .. } => Some(source.clone()),
                    Target::External(_) => None,
                }
            }));
        Ok(plan)
    }
}

// ----------------------------------------------------------------------------

impl Snapshot {
    /// Resolves one prepared plan against a revision-settled page relation.
    pub fn new<'a>(
        plan: &Plan, page_routes: impl Iterator<Item = &'a PageRoute>,
        use_directory_urls: bool,
    ) -> Result<Self> {
        if !plan.enabled {
            return Ok(Self::default());
        }

        // Retain only routes that can become final targets. Collision checks
        // still inspect every page because redirect outputs are exclusive.
        let mut routes = BTreeMap::new();
        for route in page_routes {
            if plan.route_sources.contains(route.source.as_str()) {
                routes.insert(route.source.to_string(), route.url.clone());
            }
            if plan.outputs.contains(&route.destination) {
                bail!(
                    "redirect output '{}' collides with a page",
                    route.destination
                )
            }
        }

        let mut redirects = Vec::with_capacity(plan.specifications.len());
        let mut manifest = BTreeMap::new();
        let mut overrides =
            BTreeMap::<SitePath, BTreeMap<String, String>>::new();
        let mut warnings = plan.warnings.clone();

        // Build the public manifest and physical redirects together. Physical
        // anchor overrides are collected by output path and attached after
        // this loop, because configuration order is not significant.
        for specification in &plan.specifications {
            let target = specification.target.resolve(&routes);
            if target.is_none()
                && let Some(configured) = specification.target.missing()
            {
                warnings.push(format!(
                    "Redirect target '{configured}' does not exist!"
                ));
            }

            match &specification.source {
                Source::Page(output) => {
                    redirects.push(Redirect {
                        output: output.clone(),
                        target: target.as_ref().map(|target| {
                            target.for_redirect(output, use_directory_urls)
                        }),
                        overrides: BTreeMap::new(),
                    });
                }
                Source::Anchor {
                    manifest_key,
                    output,
                    fragment: source_fragment,
                } => {
                    if let Some(target) = &target {
                        manifest.insert(
                            manifest_key.clone(),
                            target.for_manifest(),
                        );

                        // A physical redirect page loads before the UI, so it
                        // must resolve its own fragment-specific destinations.
                        if plan.outputs.contains(output) {
                            overrides
                                .entry(output.clone())
                                .or_default()
                                .insert(
                                    source_fragment.clone(),
                                    target.for_redirect(
                                        output,
                                        use_directory_urls,
                                    ),
                                );
                        }
                    }
                }
            }
        }
        for redirect in &mut redirects {
            redirect.overrides =
                overrides.remove(&redirect.output).unwrap_or_default();
        }
        Ok(Self {
            redirects,
            manifest: Some(manifest),
            warnings,
        })
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Validates and classifies one configured redirect source.
fn prepare_source(
    config: &Config, plan: &mut Plan, configured: &str,
) -> Result<Source> {
    let (source, fragment) = split_fragment(configured);
    let source = normalize_source(source)?;

    // Match mkdocs-redirects: warn about unusual source suffixes, but continue
    // generating the configured redirect.
    if !MARKDOWN_SUFFIXES
        .iter()
        .any(|suffix| source.as_str().ends_with(suffix))
    {
        plan.warnings.push(format!(
            "redirects plugin: '{source}' is not a valid markdown file!"
        ));
    }

    let route = PageRoute::from_source(config, source)?;
    if fragment.is_empty() {
        // Whole-page redirects own physical HTML outputs and must not collide
        // with another configured redirect or output producer.
        if !plan.outputs.insert(route.destination.clone()) {
            bail!(
                "redirect output '{}' is configured more than once",
                route.destination
            )
        }
        validate_output(config, &route.destination)?;
        Ok(Source::Page(route.destination))
    } else {
        // Anchor redirects share their source page, so only their public URL
        // must be unique. Their matching physical output is retained so page
        // redirects can embed fragment-specific overrides later.
        let manifest_key = format!("{}{fragment}", route.url);
        if !plan.manifest_keys.insert(manifest_key.clone()) {
            bail!("redirect source '{configured}' is configured more than once")
        }
        Ok(Source::Anchor {
            manifest_key,
            output: route.destination,
            fragment: fragment.into(),
        })
    }
}

/// Classifies one configured redirect target.
fn prepare_target(configured: &str) -> Target {
    if is_external(configured) {
        Target::External(configured.into())
    } else {
        let (source, fragment) = split_fragment(configured);
        Target::Internal {
            configured: configured.into(),
            source: source.into(),
            fragment: fragment.into(),
        }
    }
}

// ----------------------------------------------------------------------------

/// Resolves configured redirect chains and rejects cycles.
fn resolve_chains(specifications: &mut [Specification]) -> Result<()> {
    // Redirect targets refer to the exact source keys used in configuration.
    // The index turns every chain step into a logarithmic lookup.
    let sources = specifications
        .iter()
        .enumerate()
        .map(|(index, specification)| (specification.configured.clone(), index))
        .collect::<BTreeMap<_, _>>();

    // Cache terminal targets so shared chain tails are resolved only once.
    let mut resolved = vec![None; specifications.len()];
    for index in 0..specifications.len() {
        resolve_chain(
            index,
            specifications,
            &sources,
            &mut Vec::new(),
            &mut resolved,
        )?;
    }
    for (specification, target) in specifications.iter_mut().zip(resolved) {
        specification.target = target.expect("every redirect was resolved");
    }
    Ok(())
}

/// Resolves one redirect target recursively against configured sources.
fn resolve_chain(
    index: usize, specifications: &[Specification],
    sources: &BTreeMap<String, usize>, visiting: &mut Vec<usize>,
    resolved: &mut [Option<Target>],
) -> Result<Target> {
    if let Some(target) = &resolved[index] {
        return Ok(target.clone());
    }

    // The active recursion stack identifies the complete cycle for a useful
    // configuration error instead of allowing a browser redirect loop.
    if let Some(position) = visiting.iter().position(|other| *other == index) {
        let cycle = visiting[position..]
            .iter()
            .chain(std::iter::once(&index))
            .map(|index| specifications[*index].configured.as_str())
            .collect::<Vec<_>>()
            .join(" -> ");
        bail!("redirect cycle detected: {cycle}")
    }

    visiting.push(index);
    let target = match &specifications[index].target {
        Target::Internal { configured, .. } => {
            if let Some(next) = sources.get(configured) {
                resolve_chain(
                    *next,
                    specifications,
                    sources,
                    visiting,
                    resolved,
                )?
            } else {
                specifications[index].target.clone()
            }
        }
        Target::External(_) => specifications[index].target.clone(),
    };
    visiting.pop();
    resolved[index] = Some(target.clone());
    Ok(target)
}

/// Rejects redirect sources that could escape the site directory.
fn normalize_source(source: &str) -> Result<SourcePath> {
    if source.is_empty() || source.contains('\\') {
        bail!("redirect source '{source}' is not a safe relative path")
    }
    let mut parts = Vec::new();
    for component in Path::new(source).components() {
        match component {
            Component::Normal(part) => {
                parts.push(part.to_str().ok_or_else(|| {
                    anyhow::anyhow!(
                        "redirect source '{source}' is not valid UTF-8"
                    )
                })?);
            }
            Component::CurDir => {}
            Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                bail!("redirect source '{source}' is not a safe relative path")
            }
        }
    }
    if parts.is_empty() {
        bail!("redirect source '{source}' is not a safe relative path")
    }
    Ok(parts.join("/").parse()?)
}

/// Rejects redirect paths already owned by another output producer.
fn validate_output(config: &Config, output: &SitePath) -> Result<()> {
    let extra_templates = &config.project.extra_templates;
    let docs_asset = config.docs_root().as_path().join(output.as_str());
    let metadata_file = config
        .project
        .plugins
        .meta
        .config
        .enabled
        .then_some(config.project.plugins.meta.config.meta_file.as_str());

    // Ordinary documentation assets are copied to the site unchanged.
    // Metadata files and extra templates are consumed by other producers.
    if docs_asset.is_file()
        && metadata_file != Some(output.file_name())
        && !extra_templates
            .iter()
            .any(|template| template == output.as_str())
    {
        bail!("redirect output '{output}' collides with a documentation asset")
    }

    // Non-HTML theme files are copied assets; HTML files are templates and
    // therefore checked with the rendered template outputs below.
    if config.theme_dirs.iter().any(|directory| {
        let path = directory.join(output.as_str());
        path.is_file() && path.extension().is_none_or(|ext| ext != "html")
    }) {
        bail!("redirect output '{output}' collides with a theme asset")
    }

    // Static and extra templates render to the site root under their basename.
    let templates = config
        .project
        .theme
        .static_templates
        .iter()
        .chain(extra_templates);
    let output_name = output.file_name();
    if templates
        .filter_map(|template| Path::new(template).file_name())
        .any(|name| name == output_name && output.depth() == 1)
    {
        bail!("redirect output '{output}' collides with a rendered template")
    }
    Ok(())
}

/// Returns whether a configured target is an external HTTP(S) URL.
fn is_external(target: &str) -> bool {
    let target = target.to_ascii_lowercase();
    target.starts_with("http://") || target.starts_with("https://")
}

/// Splits a configured URI into its path and hash fragment.
fn split_fragment(target: &str) -> (&str, &str) {
    target
        .find('#')
        .map_or((target, ""), |index| (&target[..index], &target[index..]))
}

/// Makes a final page URL relative to one redirect output.
fn relative_target(
    output: &SitePath, target: &str, fragment: &str, use_directory_urls: bool,
) -> String {
    let parent = Path::new(output.as_str())
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let mut relative = relative_path(Path::new(target), parent);
    if use_directory_urls {
        relative.push('/');
    }
    relative.push_str(fragment);
    relative
}

/// Computes a lexical POSIX-style relative path.
fn relative_path(target: &Path, base: &Path) -> String {
    let target = target
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect::<Vec<_>>();
    let base = base
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect::<Vec<_>>();
    let common = target
        .iter()
        .zip(&base)
        .take_while(|(left, right)| left == right)
        .count();

    // Leave the unmatched base suffix, then append the unmatched target
    // suffix.
    let mut parts = vec!["..".into(); base.len() - common];
    parts.extend(target[common..].iter().map(|part| {
        part.to_str()
            .expect("redirect URL originated as UTF-8")
            .to_owned()
    }));
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{normalize_source, relative_target};
    use crate::path::{SitePath, SourcePath};
    use crate::structure::page::PageRoute;

    #[test]
    fn matches_upstream_relative_targets() {
        let directory_cases = [
            ("old/index.html", "", "", "../"),
            ("old/index.html", "new/", "", "../new/"),
            ("foo/old/index.html", "foo/new/", "", "../new/"),
            (
                "foo/fizz/old/index.html",
                "foo/bar/new/",
                "",
                "../../bar/new/",
            ),
            (
                "fizz/old/index.html",
                "foo/bar/new/",
                "",
                "../../foo/bar/new/",
            ),
            ("foo/index.html", "foo/", "", "./"),
            (
                "foo/index.html",
                "fake/destination/",
                "",
                "../fake/destination/",
            ),
            ("old/index.html", "new/", "#hash", "../new/#hash"),
            ("foo/index.html", "foo/", "#hash", "./#hash"),
            ("old/index.html", "100%25/", "", "../100%25/"),
        ];
        for (output, target, fragment, expected) in directory_cases {
            assert_eq!(
                relative_target(
                    &output.parse().unwrap(),
                    target,
                    fragment,
                    true
                ),
                expected
            );
        }

        let file_cases = [
            ("old.html", "index.html", "", "index.html"),
            ("old.html", "new.html", "", "new.html"),
            ("foo/old.html", "foo/new.html", "", "new.html"),
            (
                "foo/fizz/old.html",
                "foo/bar/new.html",
                "",
                "../bar/new.html",
            ),
            (
                "fizz/old.html",
                "foo/bar/new.html",
                "",
                "../foo/bar/new.html",
            ),
            ("foo.html", "foo/index.html", "", "foo/index.html"),
            ("old.html", "new.html", "#hash", "new.html#hash"),
        ];
        for (output, target, fragment, expected) in file_cases {
            assert_eq!(
                relative_target(
                    &output.parse().unwrap(),
                    target,
                    fragment,
                    false
                ),
                expected
            );
        }
    }

    #[test]
    fn matches_upstream_redirect_output_paths() {
        let cases = [
            ("old.md", "old.html", "old/index.html"),
            ("README.md", "index.html", "index.html"),
            ("100%.md", "100%.html", "100%/index.html"),
            (
                "foo/fizz/old.md",
                "foo/fizz/old.html",
                "foo/fizz/old/index.html",
            ),
            (
                "foo/fizz/index.md",
                "foo/fizz/index.html",
                "foo/fizz/index.html",
            ),
        ];
        for (source, file, directory) in cases {
            let source = source.parse::<SourcePath>().unwrap();
            assert_eq!(
                PageRoute::destination(&source, false).unwrap().as_str(),
                file
            );
            assert_eq!(
                PageRoute::destination(&source, true).unwrap().as_str(),
                directory
            );
        }
    }

    #[test]
    fn rejects_unsafe_sources() {
        for source in [
            "",
            "../old.md",
            "nested/../old.md",
            "/old.md",
            "old\\page.md",
        ] {
            assert!(normalize_source(source).is_err(), "{source}");
        }
        assert_eq!(
            normalize_source("./old/page.md").unwrap().as_str(),
            "old/page.md"
        );
        assert_eq!(
            normalize_source("guides/café.md").unwrap().as_str(),
            "guides/café.md"
        );
        assert!("../outside.html".parse::<SitePath>().is_err());
    }
}
