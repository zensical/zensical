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
// Structs
// ----------------------------------------------------------------------------

/// Redirect configuration prepared independently of live page routes.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// Generated paths reserved by configured redirects.
    outputs: BTreeSet<SitePath>,
    /// Internal source paths needed from the live route relation.
    targets: BTreeSet<String>,
    /// Validated redirect specifications in configuration order.
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
}

// ----------------------------------------------------------------------------

/// One validated configuration entry awaiting target resolution.
#[derive(Clone, Debug)]
struct Specification {
    /// Site-relative output path.
    output: SitePath,
    /// Prepared internal or external target.
    target: Target,
}

// ----------------------------------------------------------------------------

/// Revision-settled redirect outputs and warnings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Redirects ordered by configured source URI.
    pub redirects: Vec<Redirect>,
    /// Compatibility warnings emitted for this snapshot.
    pub warnings: Vec<String>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Plan {
    /// Validates route-independent configuration once for the workflow.
    pub fn new(config: &Config) -> Result<Self> {
        let plugin = &config.project.plugins.redirects.config;
        if !plugin.enabled || plugin.redirect_maps.is_empty() {
            return Ok(Self::default());
        }

        let mut plan = Self {
            specifications: Vec::with_capacity(plugin.redirect_maps.len()),
            ..Self::default()
        };
        for (source, configured_target) in &plugin.redirect_maps {
            let source = normalize_source(source)?;
            let output = PageRoute::destination(
                &source,
                config.project.use_directory_urls,
            )?;
            if !plan.outputs.insert(output.clone()) {
                bail!("redirect output '{output}' is configured more than once")
            }
            validate_output(config, &output)?;

            if !MARKDOWN_SUFFIXES
                .iter()
                .any(|suffix| source.as_str().ends_with(suffix))
            {
                plan.warnings.push(format!(
                    "redirects plugin: '{source}' is not a valid markdown file!"
                ));
            }

            let target = if is_external(configured_target) {
                Target::External(configured_target.clone())
            } else {
                let (source, fragment) = split_fragment(configured_target);
                plan.targets.insert(source.into());
                Target::Internal {
                    configured: configured_target.clone(),
                    source: source.into(),
                    fragment: fragment.into(),
                }
            };
            plan.specifications.push(Specification { output, target });
        }
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
        if plan.specifications.is_empty() {
            return Ok(Self::default());
        }

        let mut routes = BTreeMap::new();
        for route in page_routes {
            if plan.targets.contains(route.source.as_str()) {
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
        let mut warnings = plan.warnings.clone();
        for specification in &plan.specifications {
            let target = match &specification.target {
                Target::External(target) => Some(target.clone()),
                Target::Internal { configured, source, fragment } => {
                    if let Some(url) = routes.get(source) {
                        Some(relative_target(
                            &specification.output,
                            url,
                            fragment,
                            use_directory_urls,
                        ))
                    } else {
                        warnings.push(format!(
                            "Redirect target '{configured}' does not exist!"
                        ));
                        None
                    }
                }
            };
            redirects.push(Redirect {
                output: specification.output.clone(),
                target,
            });
        }
        Ok(Self { redirects, warnings })
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

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
    if docs_asset.is_file()
        && metadata_file != Some(output.file_name())
        && !extra_templates
            .iter()
            .any(|template| template == output.as_str())
    {
        bail!("redirect output '{output}' collides with a documentation asset")
    }

    if config.theme_dirs.iter().any(|directory| {
        let path = directory.join(output.as_str());
        path.is_file() && path.extension().is_none_or(|ext| ext != "html")
    }) {
        bail!("redirect output '{output}' collides with a theme asset")
    }

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

/// Splits an internal target into source URI and hash fragment.
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
