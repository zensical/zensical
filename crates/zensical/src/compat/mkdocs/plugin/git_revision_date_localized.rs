// Copyright (c) 2025-2026 Zensical and contributors
// SPDX-License-Identifier: MIT

//! Native mkdocs-git-revision-date-localized-plugin compatibility.
//!
//! Author timestamps are read by the Git executable from each physical source's
//! repository. Rename following is enabled by default. Shallow history is used
//! with a warning; repositories are never fetched or otherwise modified.
//!
//! Git state is refreshed on content rebuilds, without additional Git watches.
//! Raw history results are cached persistently under `.cache/plugin/` and are
//! invalidated by HEAD, shallow boundaries, history replacements, configuration,
//! and ignored revision changes. Build timestamps are never cached as history.
//!
//! Dates are localized with ICU and converted with Jiff's time zone database.
//! Browser-relative dates are localized with `Intl.RelativeTimeFormat` and are
//! accompanied by an ISO date for printing. Small differences from Babel and
//! timeago.js output are expected.

use anyhow::{bail, Context, Result};
use regex::{Captures, Regex};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use zrx::id::Id;
use zrx::stream::function::Collection;
use zrx::stream::{concurrent, Key, Stream, Value};

use crate::config::{plugins::GitRevisionDateLocalizedPluginConfig, Config};
use crate::structure::dynamic::Dynamic;
use crate::structure::page::{PageDescriptor, PageOrigin};
use crate::watcher::Source;
use crate::workflow::{output::Artifact, Configuration};

use super::exclude::Pattern;

mod date;
mod git;

use date::Formatter;
use git::{Commit, Repository};

/// History and presentation settings shared by all pages.
#[derive(Clone, Debug)]
pub struct GitRevisionDates {
    config: Config,
    settings: GitRevisionDateLocalizedPluginConfig,
    formatter: Option<Formatter>,
    excluded: Arc<Vec<Pattern>>,
    placeholders: Regex,
    cache: PathBuf,
    strict: bool,
}

/// Rebuild-local inputs, separate from persistent history facts.
#[derive(Clone, Debug)]
struct Revision {
    timestamp: i64,
    ignored: Arc<Vec<String>>,
    state: Arc<Mutex<State>>,
    site: Arc<OnceLock<Result<Commit, String>>>,
}

impl Value for Revision {}

#[derive(Debug, Default)]
struct State {
    directories: BTreeMap<PathBuf, Result<PathBuf, String>>,
    repositories: BTreeMap<PathBuf, Arc<Repository>>,
}

impl GitRevisionDates {
    /// Settings are resolved without executing the original Python plugin.
    pub fn new(config: &Config, strict: bool) -> Result<Self> {
        let settings = config
            .project
            .plugins
            .git_revision_date_localized
            .config
            .clone();
        let formatter = settings
            .enabled
            .then(|| {
                Formatter::new(&settings.timezone, &settings.custom_format)
            })
            .transpose()?;
        let excluded = if settings.enabled {
            settings
                .exclude
                .iter()
                .map(|pattern| Pattern::compile(pattern))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        if let Some(formatter) = &formatter {
            formatter.formats(
                0,
                settings
                    .locale
                    .as_deref()
                    .unwrap_or(&config.project.theme.language),
            )?;
        }
        Ok(Self {
            config: config.clone(),
            settings,
            formatter,
            excluded: Arc::new(excluded),
            placeholders: Regex::new(
                r"(?i)\{\{\s*(git_(?:revision|creation|site_revision)_date_localized)\s*\}\}",
            )?,
            cache: config
                .get_cache_dir()
                .join("plugin/git-revision-date-localized"),
            strict,
        })
    }

    /// Git state is refreshed only when another source change starts a rebuild.
    pub fn setup(
        &self, pages: &Stream<Id, PageDescriptor>, sources: &Stream<Id, Source>,
    ) -> Stream<Id, PageDescriptor> {
        if !self.settings.enabled {
            return pages.clone();
        }
        let pipeline = self.clone();
        // This signal also invalidates retained pages after Git changes when
        // their Markdown files were not changed by the triggering rebuild.
        let revision =
            sources.reduce(move |_: &dyn Collection<Key<Id>, Source>| {
                let ignored = pipeline
                    .settings
                    .ignored_commits_file
                    .as_ref()
                    .map(|path| {
                        let path = pipeline
                            .config
                            .path
                            .parent()
                            .expect("configuration parent")
                            .join(path);
                        let data =
                            fs::read_to_string(&path).with_context(|| {
                                format!(
                                    "unable to read ignored_commits_file {}",
                                    path.display()
                                )
                            })?;
                        Ok::<_, anyhow::Error>(
                            data.lines()
                                .filter_map(|line| {
                                    let value = line
                                        .split('#')
                                        .next()
                                        .unwrap_or_default()
                                        .trim();
                                    (!value.is_empty())
                                        .then(|| value.to_owned())
                                })
                                .collect(),
                        )
                    })
                    .transpose()?
                    .unwrap_or_default();
                Ok::<_, anyhow::Error>(Some(Revision {
                    timestamp: jiff::Timestamp::now().as_second(),
                    ignored: Arc::new(ignored),
                    state: Arc::new(Mutex::new(State::default())),
                    site: Arc::new(OnceLock::new()),
                }))
            });
        let pipeline = self.clone();
        let concurrency = if self.settings.enable_parallel_processing {
            std::thread::available_parallelism()
                .map_or(1, std::num::NonZeroUsize::get)
                .min(10)
        } else {
            1
        };
        pages.product(&revision).map(concurrent(
            concurrency,
            move |page: &PageDescriptor, revision: &Revision| {
                pipeline.apply(page, revision)
            },
        ))
    }

    /// Timeago assets are reconciled by the ordinary output ownership stage.
    pub fn assets(
        &self, configuration: &Stream<Id, Configuration>,
    ) -> Stream<Id, Artifact> {
        let enabled =
            self.settings.enabled && self.settings.r#type == "timeago";
        configuration.flat_map(move |_: &Configuration| {
            let mut artifacts = Vec::new();
            if enabled {
                for (path, contents) in [
                    (
                        "js/git-revision-date-localized.js",
                        include_str!("git_revision_date_localized/timeago.js"),
                    ),
                    (
                        "css/git-revision-date-localized.css",
                        include_str!("git_revision_date_localized/timeago.css"),
                    ),
                ] {
                    let key = Id::builder()
                        .provider("git-revision-date-localized")
                        .context(".")
                        .location(path)
                        .build()?;
                    let owner = PageOrigin::Generated {
                        identity: format!("git-revision-date-localized:{path}"),
                        provenance: None,
                    };
                    artifacts.push((
                        key.into(),
                        Artifact::page(
                            owner,
                            path.parse()?,
                            contents.as_bytes().to_vec(),
                        ),
                    ));
                }
            }
            Ok::<Vec<(Key<Id>, Artifact)>, anyhow::Error>(artifacts)
        })
    }

    fn apply(
        &self, page: &PageDescriptor, revision: &Revision,
    ) -> Result<PageDescriptor> {
        let source = &page.document.source;
        if self
            .excluded
            .iter()
            .any(|pattern| pattern.matches(source.as_str()))
        {
            return Ok(page.clone());
        }
        let mut page = page.clone();
        let locale = page
            .document
            .meta
            .get("locale")
            .map(ToString::to_string)
            .filter(|locale| !locale.is_empty())
            .or_else(|| self.settings.locale.clone())
            .unwrap_or_else(|| self.config.project.theme.language.clone());
        let site = revision
            .site
            .get_or_init(|| {
                self.commit(self.config.docs_root().as_path(), false, revision)
                    .map(|(commit, _)| commit)
                    .map_err(|error| format!("{error:#}"))
            })
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        self.insert(
            &mut page,
            "git_site_revision_date_localized",
            site,
            self.tag_for(self.config.docs_root().as_path(), site, revision)?,
            &locale,
        )?;

        let generated = matches!(page.origin, PageOrigin::Generated { .. })
            || self.config.api.files.contains_key(&page.document.source);
        let path = self
            .config
            .docs_root()
            .as_path()
            .join(page.document.source.as_str());
        let (updated, tag) = if generated {
            (fallback(revision), String::new())
        } else {
            self.commit(&path, false, revision)?
        };
        self.insert(
            &mut page,
            "git_revision_date_localized",
            &updated,
            tag,
            &locale,
        )?;
        if self.settings.enable_creation_date {
            let (mut created, mut tag) = if generated {
                (fallback(revision), String::new())
            } else {
                self.commit(&path, true, revision)?
            };
            if created.timestamp > updated.timestamp {
                self.diagnostic(&format!("creation date is later than revision date for {}; the revision date is used", page.document.source))?;
                tag = self.tag_for(&path, &updated, revision)?;
                created = updated;
            }
            self.insert(
                &mut page,
                "git_creation_date_localized",
                &created,
                tag,
                &locale,
            )?;
        }
        page.document.body = self
            .placeholders
            .replace_all(&page.document.body, |captures: &Captures<'_>| {
                page.document
                    .meta
                    .get(&captures[1].to_ascii_lowercase())
                    .map_or_else(|| captures[0].to_owned(), ToString::to_string)
            })
            .into_owned();
        Ok(page)
    }

    fn insert(
        &self, page: &mut PageDescriptor, name: &str, commit: &Commit,
        tag: String, locale: &str,
    ) -> Result<()> {
        let formats = self
            .formatter
            .as_ref()
            .expect("enabled formatter")
            .formats(commit.timestamp, locale)?;
        let meta = &mut page.document.meta;
        meta.insert(
            name.into(),
            Dynamic::String(date::display(&formats, &self.settings.r#type)),
        );
        meta.insert(
            format!("{name}_hash"),
            Dynamic::String(commit.hash.clone()),
        );
        meta.insert(format!("{name}_tag"), Dynamic::String(tag));
        for (kind, value) in formats {
            meta.insert(format!("{name}_raw_{kind}"), Dynamic::String(value));
        }
        Ok(())
    }

    fn repository(
        &self, path: &Path, revision: &Revision,
    ) -> Result<Arc<Repository>> {
        let directory = if path.is_dir() {
            path
        } else {
            path.parent().context("source parent")?
        };
        let mut state = revision.state.lock().expect("Git state lock");
        let root = state
            .directories
            .entry(directory.to_owned())
            .or_insert_with(|| {
                Repository::discover(directory)
                    .map_err(|error| format!("{error:#}"))
            })
            .clone()
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        if let Some(repository) = state.repositories.get(&root) {
            return Ok(repository.clone());
        }
        let repository = Arc::new(Repository::new(root.clone())?);
        if repository.shallow {
            self.diagnostic(&format!("shallow repository {}; available history is used and dates may be incomplete", root.display()))?;
        }
        state.repositories.insert(root, repository.clone());
        Ok(repository)
    }

    fn commit(
        &self, path: &Path, creation: bool, revision: &Revision,
    ) -> Result<(Commit, String)> {
        let result = (|| {
            let path = path.canonicalize()?;
            let repository = self.repository(&path, revision)?;
            let commit = repository.query(
                &path,
                creation,
                self.settings.enable_git_follow,
                &revision.ignored,
                &self.cache,
            )?;
            Ok::<_, anyhow::Error>(commit.map(|commit| {
                let tag = repository.tag(&commit);
                (commit, tag)
            }))
        })();
        match result {
            Ok(Some(commit)) => Ok(commit),
            Ok(None) => {
                self.diagnostic(&format!("{} has no eligible Git history; the build date is used", path.display()))?;
                Ok((fallback(revision), String::new()))
            }
            Err(error) if self.settings.fallback_to_build_date => {
                self.diagnostic(&format!("{}: {error:#}; the build date is used", path.display()))?;
                Ok((fallback(revision), String::new()))
            }
            Err(error) => Err(error.context(format!("git-revision-date-localized: {}; fallback_to_build_date can be enabled to use the build date", path.display()))),
        }
    }

    fn tag_for(
        &self, path: &Path, commit: &Commit, revision: &Revision,
    ) -> Result<String> {
        if commit.hash.is_empty() {
            return Ok(String::new());
        }
        Ok(self
            .repository(&path.canonicalize()?, revision)?
            .tag(commit))
    }

    fn diagnostic(&self, message: &str) -> Result<()> {
        let level = if self.settings.strict {
            "WARNING"
        } else {
            "INFO"
        };
        eprintln!("{level} -  git-revision-date-localized: {message}");
        if self.settings.strict && self.strict {
            bail!("git-revision-date-localized: {message}");
        }
        Ok(())
    }
}

fn fallback(revision: &Revision) -> Commit {
    Commit {
        hash: String::new(),
        timestamp: revision.timestamp,
    }
}
