// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! MkDocs-compatible redirects plugin.

use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};
use zrx::id::Id;
use zrx::stream::{Key, Signal, Value};

use crate::compat::mkdocs::plugin::meta;
use crate::config::Config;
use crate::structure::page::PageRoute;

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Redirect document emitted by mkdocs-redirects 1.2.2.
const HTML_TEMPLATE: &str = r##"
<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <title>Redirecting...</title>
    <link rel="canonical" href="{url}">
    <script>var anchor=window.location.hash.substr(1);location.href="{url}"+(anchor?"#"+anchor:"")</script>
    <meta http-equiv="refresh" content="0; url={url}">
</head>
<body>
You're being redirected to a <a href="{url}">new destination</a>.
</body>
</html>
"##;

/// Source suffixes recognized by MkDocs as Markdown.
const MARKDOWN_SUFFIXES: &[&str] =
    &[".markdown", ".mdown", ".mkdn", ".mkd", ".md"];

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// One resolved redirect output.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Redirect {
    /// Site-relative output path.
    output: String,
    /// Resolved redirect target, or `None` when the target is missing.
    target: Option<String>,
}

/// One validated configuration entry awaiting target resolution.
struct Specification<'a> {
    /// Site-relative output path.
    output: String,
    /// Configured internal or external target.
    target: &'a str,
}

/// Revision-settled redirect outputs and warnings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    /// Redirects ordered by configured source URI.
    redirects: Vec<Redirect>,
    /// Compatibility warnings emitted for this snapshot.
    warnings: Vec<String>,
}

impl Value for Snapshot {}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Snapshot {
    /// Resolves configured redirects against the current page relation.
    pub(crate) fn new<'a>(
        config: &Config,
        page_routes: impl Iterator<Item = (&'a Key<Id>, &'a PageRoute)>,
    ) -> Result<Self> {
        let settings = &config.project.plugins.redirects.config;
        if !settings.enabled || settings.redirect_maps.is_empty() {
            return Ok(Self::default());
        }

        let mut outputs = BTreeSet::new();
        let mut targets = BTreeSet::new();
        let mut specifications =
            Vec::with_capacity(settings.redirect_maps.len());
        let mut warnings = Vec::new();
        for (source, configured_target) in &settings.redirect_maps {
            let source = normalize_source(source)?;
            let output = PageRoute::destination(
                &source,
                config.project.use_directory_urls,
            );
            if !outputs.insert(output.clone()) {
                bail!("redirect output '{output}' is configured more than once")
            }
            validate_output(config, &output)?;

            if !MARKDOWN_SUFFIXES
                .iter()
                .any(|suffix| source.ends_with(suffix))
            {
                warnings.push(format!(
                    "redirects plugin: '{source}' is not a valid markdown file!"
                ));
            }

            if !is_external(configured_target) {
                targets.insert(split_fragment(configured_target).0);
            }
            specifications.push(Specification {
                output,
                target: configured_target,
            });
        }

        let mut routes = BTreeMap::new();
        for (_, route) in page_routes {
            if targets.contains(route.source.as_str()) {
                routes.insert(route.source.clone(), route.url.clone());
            }
            if outputs.contains(&route.destination) {
                bail!(
                    "redirect output '{}' collides with a page",
                    route.destination
                )
            }
        }

        let mut redirects = Vec::with_capacity(specifications.len());
        for specification in specifications {
            let target = if is_external(specification.target) {
                Some(specification.target.into())
            } else {
                let (target_source, fragment) =
                    split_fragment(specification.target);
                if let Some(url) = routes.get(target_source) {
                    Some(relative_target(
                        &specification.output,
                        url,
                        fragment,
                        config.project.use_directory_urls,
                    ))
                } else {
                    warnings.push(format!(
                        "Redirect target '{}' does not exist!",
                        specification.target
                    ));
                    None
                }
            };
            redirects.push(Redirect {
                output: specification.output,
                target,
            });
        }
        Ok(Self { redirects, warnings })
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Attaches redirect artifact generation to the settled site graph.
pub(crate) fn attach(
    config: &Config, strict: bool, snapshot: &Signal<Id, Snapshot>,
) {
    let settings = &config.project.plugins.redirects.config;
    if !settings.enabled || settings.redirect_maps.is_empty() {
        return;
    }
    let site_dir = config.get_site_dir();
    let _ = snapshot
        .map(move |snapshot: &Snapshot| write(&site_dir, snapshot, strict));
}

/// Reconciles one redirect snapshot with the site directory.
fn write(site_dir: &Path, snapshot: &Snapshot, strict: bool) -> Result<()> {
    for redirect in &snapshot.redirects {
        let path = site_dir.join(&redirect.output);
        if let Some(target) = &redirect.target {
            fs::create_dir_all(path.parent().expect("redirect has parent"))?;
            fs::write(path, redirect_html(target))?;
        } else if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    for warning in &snapshot.warnings {
        eprintln!("WARNING -  {warning}");
    }
    if strict && !snapshot.warnings.is_empty() {
        bail!("Aborted because --strict flag is set")
    }
    Ok(())
}

/// Rejects redirect sources that could escape the site directory.
fn normalize_source(source: &str) -> Result<String> {
    if source.is_empty() || source.contains('\\') {
        bail!("redirect source '{source}' is not a safe relative path")
    }
    let mut parts = Vec::new();
    for component in Path::new(source).components() {
        match component {
            Component::Normal(part) => {
                parts.push(part.to_string_lossy().into_owned());
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
    Ok(parts.join("/"))
}

/// Rejects redirect paths already owned by another output producer.
fn validate_output(config: &Config, output: &str) -> Result<()> {
    let extra_templates = &config.project.extra_templates;
    let meta = meta::Settings::new(config);
    let docs_asset = config.get_docs_dir().join(output);
    if docs_asset.is_file()
        && !meta::claims(output, &meta)
        && !extra_templates.iter().any(|template| template == output)
    {
        bail!("redirect output '{output}' collides with a documentation asset")
    }

    if config.theme_dirs.iter().any(|directory| {
        let path = directory.join(output);
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
    if templates
        .filter_map(|template| Path::new(template).file_name())
        .any(|name| {
            Path::new(output).file_name().is_some_and(|out| out == name)
                && Path::new(output)
                    .parent()
                    .is_none_or(|parent| parent.as_os_str().is_empty())
        })
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
    output: &str, target: &str, fragment: &str, use_directory_urls: bool,
) -> String {
    let parent = Path::new(output).parent().unwrap_or_else(|| Path::new(""));
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
    parts.extend(
        target[common..]
            .iter()
            .map(|part| part.to_string_lossy().into_owned()),
    );
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

/// Renders the upstream redirect document.
fn redirect_html(target: &str) -> String {
    HTML_TEMPLATE.replace("{url}", target)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
                relative_target(output, target, fragment, true),
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
                relative_target(output, target, fragment, false),
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
            assert_eq!(PageRoute::destination(source, false), file);
            assert_eq!(PageRoute::destination(source, true), directory);
        }
    }

    #[test]
    fn rejects_unsafe_sources() {
        for source in ["", "../old.md", "/old.md", "old\\page.md"] {
            assert!(normalize_source(source).is_err(), "{source}");
        }
        assert_eq!(normalize_source("./old/page.md").unwrap(), "old/page.md");
    }

    #[test]
    fn renders_upstream_document() {
        let html = redirect_html("../new/");
        assert_eq!(
            html,
            r##"
<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <title>Redirecting...</title>
    <link rel="canonical" href="../new/">
    <script>var anchor=window.location.hash.substr(1);location.href="../new/"+(anchor?"#"+anchor:"")</script>
    <meta http-equiv="refresh" content="0; url=../new/">
</head>
<body>
You're being redirected to a <a href="../new/">new destination</a>.
</body>
</html>
"##
        );
    }

    #[test]
    fn removes_a_stale_redirect_when_its_target_disappears() {
        let directory = tempfile::tempdir().unwrap();
        let output = String::from("old/index.html");
        let valid = Snapshot {
            redirects: vec![Redirect {
                output: output.clone(),
                target: Some("../new/".into()),
            }],
            warnings: Vec::new(),
        };
        write(directory.path(), &valid, false).unwrap();
        assert!(directory.path().join(&output).is_file());

        let missing = Snapshot {
            redirects: vec![Redirect {
                output: output.clone(),
                target: None,
            }],
            warnings: vec!["missing".into()],
        };
        write(directory.path(), &missing, false).unwrap();
        assert!(!directory.path().join(output).exists());
        assert!(write(directory.path(), &missing, true).is_err());
    }
}
