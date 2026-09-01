// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! Native compatibility pipeline for filesystem-backed awesome navigation.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{Key, Signal, Stream, Value};

use crate::config::plugins::AwesomeNavLogs;
use crate::config::Config;
use crate::path::SourcePath;
use crate::structure::nav::Navigation;
use crate::structure::page::Page;
use crate::watcher::Source;

mod config;
mod pattern;
mod resolver;
mod sort;

/// Native awesome-nav pipeline.
#[derive(Clone, Debug)]
pub struct AwesomeNav {
    settings: Arc<Settings>,
}

/// Inputs required to derive revision-complete navigation.
pub struct Dependencies<'a> {
    /// Physical sources, including `.nav.yml` control files.
    pub sources: &'a Stream<Id, Source>,
    /// Rendered documentation pages.
    pub pages: &'a Stream<Id, Page>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub level: Level,
    pub message: String,
}

#[derive(Clone, Copy, Debug)]
pub struct Logs {
    pub nav_override: Level,
    pub root_title: Level,
    pub root_hide: Level,
    pub no_matches: Level,
}

#[derive(Clone, Debug)]
pub struct Settings {
    enabled: bool,
    docs: String,
    filename: String,
    configured: Vec<crate::structure::nav::NavigationItem>,
    strict: bool,
    logs: Logs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Document {
    path: SourcePath,
    content: String,
}

impl Value for Document {}

#[derive(Clone, Debug, Default)]
struct Documents(Arc<BTreeMap<String, String>>);

impl Value for Documents {}

#[derive(Clone, Debug)]
struct Pages(Arc<Vec<Page>>);

impl Value for Pages {}

impl AwesomeNav {
    /// Resolves immutable settings for one workflow lifetime.
    pub fn new(config: &Config, strict: bool) -> Result<Self> {
        let plugin = &config.project.plugins.awesome_nav.config;
        if plugin.filename.is_empty() {
            bail!("awesome-nav filename must not be empty")
        }
        Ok(Self {
            settings: Arc::new(Settings {
                enabled: plugin.enabled,
                docs: config.project.docs_dir.clone(),
                filename: plugin.filename.clone(),
                configured: config.project.nav.clone(),
                strict,
                logs: Logs::new(&plugin.logs)?,
            }),
        })
    }

    /// Returns whether awesome-nav replaces other navigation producers.
    pub fn is_enabled(&self) -> bool {
        self.settings.enabled
    }

    /// Installs control-file discovery, settlement and navigation compilation.
    pub fn setup(
        &self, dependencies: Dependencies<'_>,
    ) -> Signal<Id, Navigation> {
        let settings = self.settings.clone();
        let documents = dependencies.sources.filter_map({
            let settings = settings.clone();
            move |id: &Id, source: &Source| {
                if !settings.enabled || id.context() != settings.docs {
                    return Ok(None);
                }
                let path = id.location().parse::<SourcePath>()?;
                if !is_config_file(&path, &settings.filename) {
                    return Ok(None);
                }
                let content =
                    fs::read_to_string(&**source).with_context(|| {
                        format!("failed to read awesome-nav file {path}")
                    })?;
                Ok::<_, anyhow::Error>(Some(Document { path, content }))
            }
        });
        let documents = documents.reduce(
            |documents: &dyn Collection<Key<Id>, Document>| {
                Some(Documents(Arc::new(
                    documents
                        .values()
                        .map(|document| {
                            (
                                document.path.to_string(),
                                document.content.clone(),
                            )
                        })
                        .collect(),
                )))
            },
        );
        let pages = dependencies.pages.reduce(
            |pages: &dyn Collection<Key<Id>, Page>| {
                Some(Pages(Arc::new(pages.values().cloned().collect())))
            },
        );
        let navigation = pages.product(&documents).map(
            move |pages: &Pages, documents: &Documents| {
                let (navigation, diagnostics) = resolver::resolve(
                    &settings,
                    &documents.0,
                    pages.0.as_ref(),
                )?;
                report(&diagnostics, settings.strict)?;
                Ok::<_, anyhow::Error>(navigation)
            },
        );
        navigation.reduce(|navigation: &dyn Collection<Key<Id>, Navigation>| {
            navigation.values().next().cloned()
        })
    }
}

impl Logs {
    fn new(logs: &AwesomeNavLogs) -> Result<Self> {
        Ok(Self {
            nav_override: level(logs.nav_override.as_deref(), Level::Warning)?,
            root_title: level(logs.root_title.as_deref(), Level::Warning)?,
            root_hide: level(logs.root_hide.as_deref(), Level::Warning)?,
            no_matches: level(logs.no_matches.as_deref(), Level::Warning)?,
        })
    }
}

fn level(value: Option<&str>, default: Level) -> Result<Level> {
    match value {
        None => Ok(default),
        Some("info") => Ok(Level::Info),
        Some("warning") => Ok(Level::Warning),
        Some("error") => Ok(Level::Error),
        Some(value) => bail!("invalid awesome-nav log level: {value}"),
    }
}

fn report(diagnostics: &[Diagnostic], strict: bool) -> Result<()> {
    let mut failed = false;
    for diagnostic in diagnostics {
        let label = match diagnostic.level {
            Level::Info => "INFO",
            Level::Warning => "WARNING",
            Level::Error => "ERROR",
        };
        eprintln!("{label} -  {}", diagnostic.message);
        failed |= diagnostic.level == Level::Error
            || (strict && diagnostic.level == Level::Warning);
    }
    if failed {
        bail!("Aborted because awesome-nav reported errors")
    }
    Ok(())
}

fn is_config_file(path: &SourcePath, filename: &str) -> bool {
    path.as_str() == filename
        || path
            .as_str()
            .strip_suffix(filename)
            .is_some_and(|prefix| prefix.ends_with('/'))
}

#[cfg(test)]
mod tests {
    use super::{is_config_file, level, Level};

    #[test]
    fn discovers_root_and_nested_configuration() {
        assert!(is_config_file(&".nav.yml".parse().unwrap(), ".nav.yml"));
        assert!(is_config_file(
            &"guide/.nav.yml".parse().unwrap(),
            ".nav.yml"
        ));
        assert!(!is_config_file(
            &"guide/not.nav.yml".parse().unwrap(),
            ".nav.yml"
        ));
    }

    #[test]
    fn validates_log_levels() {
        assert_eq!(level(Some("info"), Level::Warning).unwrap(), Level::Info);
        assert!(level(Some("debug"), Level::Warning).is_err());
    }
}
