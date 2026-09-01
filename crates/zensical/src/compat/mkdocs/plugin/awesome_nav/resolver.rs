// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! Filesystem-derived awesome-nav resolution.

use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::structure::dynamic::Dynamic;
use crate::structure::nav::{
    source_sort_key, to_title, Navigation, Plan, PlanItem,
};
use crate::structure::page::Page;

use super::config::{
    self, Config, Direction, Item, Named, PatternOptions, Sections, Sort,
    SortBy, SortKind,
};
use super::pattern::Pattern;
use super::sort::{self, Settings as SortSettings};
use super::{Diagnostic, Level, Settings};

/// Fully inherited directory options.
#[derive(Clone, Debug)]
struct Effective {
    layout: Layout,
    sort: SortSettings,
    ignore: Vec<String>,
    append_unmatched: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Layout {
    flatten: bool,
    preserve_names: bool,
    use_index_title: bool,
}

/// One filesystem page needed during navigation resolution.
#[derive(Clone, Debug)]
struct PageInfo {
    path: String,
    title: String,
    metadata_title: Option<String>,
}

/// Revision-complete documentation catalog.
struct Catalog {
    pages: BTreeMap<String, PageInfo>,
    page_order: Vec<String>,
    directories: BTreeSet<String>,
}

/// Navigation entry with deferred resolution state.
enum Entry {
    Page(Resolved),
    Directory {
        path: String,
        title: Option<String>,
        config: Effective,
        resolved: Option<Vec<Resolved>>,
    },
    Pattern {
        options: PatternOptions,
        config: Effective,
        origin: String,
        resolved: Option<Vec<Resolved>>,
    },
    Link(Resolved),
    Section {
        title: String,
        children: Vec<Entry>,
    },
}

/// One resolved item retaining sort facts until final lowering.
#[derive(Clone, Debug)]
struct Resolved {
    path: String,
    title: String,
    sort_title: String,
    kind: ResolvedKind,
}

#[derive(Clone, Debug)]
enum ResolvedKind {
    Page {
        target: String,
        explicit_title: Option<String>,
    },
    Link {
        target: String,
    },
    Section(Vec<Resolved>),
}

struct Resolver<'a> {
    settings: &'a Settings,
    documents: &'a BTreeMap<String, String>,
    catalog: Catalog,
    pages: &'a [Page],
    seen: HashSet<String>,
    resolving: HashSet<String>,
    parsed: HashMap<String, Config>,
    diagnostics: Vec<Diagnostic>,
}

impl Default for Effective {
    fn default() -> Self {
        Self {
            layout: Layout::default(),
            sort: SortSettings {
                by: SortBy::Path,
                direction: Direction::Ascending,
                kind: SortKind::Natural,
                sections: Sections::Last,
                ignore_case: false,
            },
            ignore: Vec::new(),
            append_unmatched: false,
        }
    }
}

impl Catalog {
    fn new(pages: &[Page]) -> Self {
        let mut ordered = pages.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|page| source_sort_key(page.source()));
        let sources = ordered
            .iter()
            .map(|page| page.source().to_string())
            .collect::<HashSet<_>>();
        let mut result = Self {
            pages: BTreeMap::new(),
            page_order: Vec::new(),
            directories: BTreeSet::from([String::from(".")]),
        };
        for page in ordered {
            let path = page.source().to_string();
            if page.source().file_name() == "README.md"
                && sources.contains(&join(&parent(&path), "index.md"))
            {
                continue;
            }
            let metadata_title = match page.meta.get("title") {
                Some(Dynamic::String(title)) => Some(title.clone()),
                _ => None,
            };
            result.page_order.push(path.clone());
            result.pages.insert(
                path.clone(),
                PageInfo {
                    path: path.clone(),
                    title: page.title.clone(),
                    metadata_title,
                },
            );
            let mut directory = parent(&path);
            loop {
                result.directories.insert(directory.clone());
                if directory == "." {
                    break;
                }
                directory = parent(&directory);
            }
        }
        result
    }

    fn page(&self, path: &str) -> Option<&PageInfo> {
        self.pages.get(path)
    }

    fn is_directory(&self, path: &str) -> bool {
        self.directories.contains(path)
    }

    fn candidates(&self) -> Vec<(String, bool)> {
        let mut candidates = self
            .page_order
            .iter()
            .cloned()
            .map(|path| (path, false))
            .collect::<Vec<_>>();
        let mut directories = self
            .directories
            .iter()
            .filter(|path| path.as_str() != ".")
            .cloned()
            .collect::<Vec<_>>();
        directories.sort();
        candidates.extend(directories.into_iter().map(|path| (path, true)));
        candidates
    }
}

impl<'a> Resolver<'a> {
    fn new(
        settings: &'a Settings, documents: &'a BTreeMap<String, String>,
        pages: &'a [Page],
    ) -> Self {
        Self {
            settings,
            documents,
            catalog: Catalog::new(pages),
            pages,
            seen: HashSet::new(),
            resolving: HashSet::new(),
            parsed: HashMap::new(),
            diagnostics: Vec::new(),
        }
    }

    fn resolve(mut self) -> Result<(Navigation, Vec<Diagnostic>)> {
        let config = self.directory_config(".")?.clone();
        if !self.settings.configured.is_empty() {
            self.diagnostic(
                self.settings.logs.nav_override,
                "'nav' config from mkdocs.yml is being replaced with one generated by awesome-nav",
                None,
            );
        }
        let config_path = self.config_path(".");
        if config.title.is_some() {
            self.diagnostic(
                self.settings.logs.root_title,
                "'title' option has no effect at the top level",
                self.documents
                    .contains_key(&config_path)
                    .then_some(config_path.as_str()),
            );
        }
        if config.hide {
            self.diagnostic(
                self.settings.logs.root_hide,
                "'hide' option has no effect at the top level",
                self.documents
                    .contains_key(&config_path)
                    .then_some(config_path.as_str()),
            );
        }
        let effective = Self::inherit(".", &Effective::default(), &config);
        let items = Self::items(&config, &effective);
        let mut entries = self.parse_entries(".", items, &effective)?;
        self.resolve_entries(&mut entries)?;
        let resolved = flatten(entries);
        let plan =
            Plan::new(resolved.into_iter().map(Resolved::plan).collect());
        Ok((plan.compile(self.pages.to_vec()), self.diagnostics))
    }

    fn resolve_directory(
        &mut self, path: &str, parent_config: &Effective,
        title: Option<String>, from_pattern: bool,
    ) -> Result<Vec<Resolved>> {
        if !self.resolving.insert(path.into()) {
            bail!("recursive awesome-nav directory reference: {path}")
        }
        self.seen.insert(path.into());
        let local = self.directory_config(path)?.clone();
        if from_pattern && local.hide {
            self.resolving.remove(path);
            return Ok(Vec::new());
        }
        let effective = Self::inherit(path, parent_config, &local);
        let items = Self::items(&local, &effective);
        let mut entries = self.parse_entries(path, items, &effective)?;
        self.resolve_entries(&mut entries)?;
        let children = flatten(entries);
        self.resolving.remove(path);
        if children.is_empty() {
            return Ok(Vec::new());
        }
        if effective.layout.flatten
            && children.len() == 1
            && !matches!(children[0].kind, ResolvedKind::Link { .. })
        {
            return Ok(children);
        }
        let title = title
            .or(local.title)
            .or_else(|| {
                (!effective.layout.preserve_names
                    && effective.layout.use_index_title)
                    .then(|| self.index_title(path))
                    .flatten()
            })
            .unwrap_or_else(|| {
                let name = file_name(path);
                if effective.layout.preserve_names {
                    name.into()
                } else {
                    to_title(name)
                }
            });
        Ok(vec![Resolved {
            path: path.into(),
            sort_title: title.clone(),
            title,
            kind: ResolvedKind::Section(children),
        }])
    }

    fn resolve_entries(&mut self, entries: &mut [Entry]) -> Result<()> {
        self.reserve_pages(entries);
        loop {
            let depth = deepest_directory(entries);
            let Some(depth) = depth else { break };
            self.resolve_directories(entries, depth)?;
        }
        self.resolve_patterns(entries)
    }

    fn reserve_pages(&mut self, entries: &mut [Entry]) {
        for entry in entries {
            match entry {
                Entry::Page(item) => {
                    self.seen.insert(item.path.clone());
                }
                Entry::Section { children, .. } => self.reserve_pages(children),
                _ => {}
            }
        }
    }

    fn resolve_directories(
        &mut self, entries: &mut [Entry], depth: usize,
    ) -> Result<()> {
        for entry in entries {
            match entry {
                Entry::Directory { path, title, config, resolved }
                    if resolved.is_none() && components(path) == depth =>
                {
                    *resolved = Some(self.resolve_directory(
                        path,
                        config,
                        title.clone(),
                        false,
                    )?);
                }
                Entry::Section { children, .. } => {
                    self.resolve_directories(children, depth)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn resolve_patterns(&mut self, entries: &mut [Entry]) -> Result<()> {
        for entry in entries {
            match entry {
                Entry::Pattern {
                    options,
                    config,
                    origin,
                    resolved,
                } => {
                    *resolved =
                        Some(self.resolve_pattern(options, config, origin)?);
                }
                Entry::Section { children, .. } => {
                    self.resolve_patterns(children)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn resolve_pattern(
        &mut self, options: &PatternOptions, effective: &Effective,
        origin: &str,
    ) -> Result<Vec<Resolved>> {
        let pattern = Pattern::compile(&options.glob)?;
        let ignores = effective
            .ignore
            .iter()
            .map(|value| Pattern::compile(value))
            .collect::<Result<Vec<_>>>()?;
        let mut candidates = self
            .catalog
            .candidates()
            .into_iter()
            .filter(|(path, directory)| {
                if self.seen.contains(path) {
                    return false;
                }
                let candidate = if *directory {
                    format!("{path}/")
                } else {
                    path.clone()
                };
                pattern.matches(&candidate)
                    && !ignores.iter().any(|ignore| ignore.matches(&candidate))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|(left, left_dir), (right, right_dir)| {
            left_dir.cmp(right_dir).then_with(|| {
                if *left_dir {
                    components(right).cmp(&components(left))
                } else {
                    std::cmp::Ordering::Equal
                }
            })
        });
        let mut matches = Vec::new();
        let mut had_match = false;
        for (path, directory) in candidates {
            self.seen.insert(path.clone());
            if directory {
                if self.directory_config(&path)?.hide {
                    continue;
                }
                had_match = true;
                matches.extend(
                    self.resolve_directory(&path, effective, None, true)?,
                );
            } else if let Some(page) = self.catalog.page(&path) {
                had_match = true;
                matches.push(Resolved::page(page, None));
            }
        }
        if !had_match && !options.ignore_no_matches {
            self.diagnostic(
                self.settings.logs.no_matches,
                &format!(
                    "The nav item '{}' doesn't match any files or directories",
                    pattern.source()
                ),
                Some(origin),
            );
        }
        sort::apply(&mut matches, effective.sort);
        Ok(matches)
    }

    fn parse_entries(
        &mut self, root: &str, items: Vec<Item>, effective: &Effective,
    ) -> Result<Vec<Entry>> {
        items
            .into_iter()
            .map(|item| self.parse_entry(root, item, effective))
            .collect()
    }

    fn parse_entry(
        &mut self, root: &str, item: Item, effective: &Effective,
    ) -> Result<Entry> {
        match item {
            Item::Target(target) => {
                Ok(self.target(root, target, None, effective))
            }
            Item::Named {
                title,
                value: Named::Target(target),
            } => {
                let path = join(root, &target);
                if let Some(page) = self.catalog.page(&path) {
                    Ok(Entry::Page(Resolved::page(page, Some(title))))
                } else if self.catalog.is_directory(&path) {
                    Ok(Entry::Directory {
                        path,
                        title: Some(title),
                        config: effective.clone(),
                        resolved: None,
                    })
                } else {
                    Ok(Entry::Link(Resolved {
                        path: target.clone(),
                        sort_title: title.clone(),
                        title,
                        kind: ResolvedKind::Link { target },
                    }))
                }
            }
            Item::Named {
                title,
                value: Named::Children(children),
            } => Ok(Entry::Section {
                title,
                children: self.parse_entries(root, children, effective)?,
            }),
            Item::Pattern(options) => {
                let config = Self::pattern_config(root, effective, &options);
                let options = PatternOptions {
                    glob: absolute_pattern(root, &options.glob),
                    ..options
                };
                Ok(Entry::Pattern {
                    options,
                    config,
                    origin: self.config_path(root),
                    resolved: None,
                })
            }
        }
    }

    fn target(
        &mut self, root: &str, target: String, title: Option<String>,
        effective: &Effective,
    ) -> Entry {
        let path = join(root, target.trim_end_matches('/'));
        if let Some(page) = self.catalog.page(&path) {
            return Entry::Page(Resolved::page(page, title));
        }
        if self.catalog.is_directory(&path) {
            return Entry::Directory {
                path,
                title,
                config: effective.clone(),
                resolved: None,
            };
        }
        let options = PatternOptions {
            glob: absolute_pattern(root, &target),
            flatten_single_child_sections: None,
            preserve_directory_names: None,
            sort: Sort::default(),
            ignore: None,
            append_unmatched: None,
            ignore_no_matches: false,
        };
        Entry::Pattern {
            options,
            config: effective.clone(),
            origin: self.config_path(root),
            resolved: None,
        }
    }

    fn pattern_config(
        root: &str, parent: &Effective, options: &PatternOptions,
    ) -> Effective {
        let mut effective = parent.clone();
        effective.layout.flatten = options
            .flatten_single_child_sections
            .unwrap_or(effective.layout.flatten);
        effective.layout.preserve_names = options
            .preserve_directory_names
            .unwrap_or(effective.layout.preserve_names);
        effective.sort = merge_sort(effective.sort, &options.sort);
        effective.append_unmatched = options
            .append_unmatched
            .unwrap_or(effective.append_unmatched);
        if let Some(ignore) = &options.ignore {
            effective.ignore = resolve_ignores(root, &parent.ignore, ignore);
        }
        effective
    }

    fn inherit(root: &str, parent: &Effective, config: &Config) -> Effective {
        let mut effective = parent.clone();
        effective.layout.flatten = config
            .flatten_single_child_sections
            .unwrap_or(effective.layout.flatten);
        effective.layout.preserve_names = config
            .preserve_directory_names
            .unwrap_or(effective.layout.preserve_names);
        effective.layout.use_index_title = config
            .use_index_title
            .unwrap_or(effective.layout.use_index_title);
        effective.sort = merge_sort(effective.sort, &config.sort);
        effective.append_unmatched = config
            .append_unmatched
            .unwrap_or(effective.append_unmatched);
        if let Some(ignore) = &config.ignore {
            effective.ignore = resolve_ignores(root, &parent.ignore, ignore);
        }
        effective
    }

    fn items(config: &Config, effective: &Effective) -> Vec<Item> {
        let mut items = config.nav.clone().unwrap_or_else(|| {
            vec![
                Item::Pattern(default_pattern("index.md")),
                Item::Pattern(default_pattern("README.md")),
                Item::Pattern(default_pattern("*")),
            ]
        });
        if effective.append_unmatched {
            items.push(Item::Pattern(default_pattern("*")));
        }
        items
    }

    fn directory_config(&mut self, root: &str) -> Result<&Config> {
        let path = self.config_path(root);
        if !self.parsed.contains_key(&path) {
            let parsed = match self.documents.get(&path) {
                Some(source) => config::parse(&path, source)?,
                None => Config::default(),
            };
            self.parsed.insert(path.clone(), parsed);
        }
        Ok(self.parsed.get(&path).expect("inserted above"))
    }

    fn config_path(&self, root: &str) -> String {
        join(root, &self.settings.filename)
    }

    fn index_title(&self, root: &str) -> Option<String> {
        self.catalog
            .page(&join(root, "index.md"))
            .and_then(|page| page.metadata_title.clone())
    }

    fn diagnostic(&mut self, level: Level, message: &str, path: Option<&str>) {
        self.diagnostics.push(Diagnostic {
            level,
            message: path.map_or_else(
                || format!("awesome-nav: {message}"),
                |path| format!("awesome-nav: {message} [{path}]"),
            ),
        });
    }
}

impl Resolved {
    fn page(page: &PageInfo, explicit_title: Option<String>) -> Self {
        Self {
            path: page.path.clone(),
            sort_title: page
                .metadata_title
                .clone()
                .unwrap_or_else(|| file_name(&page.path).into()),
            title: explicit_title.clone().unwrap_or_else(|| page.title.clone()),
            kind: ResolvedKind::Page {
                target: page.path.clone(),
                explicit_title,
            },
        }
    }

    fn plan(self) -> PlanItem {
        match self.kind {
            ResolvedKind::Page { target, explicit_title } => {
                PlanItem::reference(explicit_title, target)
            }
            ResolvedKind::Link { target } => {
                PlanItem::reference(Some(self.title), target)
            }
            ResolvedKind::Section(children) => PlanItem::section(
                self.title,
                children.into_iter().map(Self::plan).collect(),
            ),
        }
    }
}

impl sort::Item for Resolved {
    fn path(&self) -> &str {
        &self.path
    }

    fn sort_title(&self) -> &str {
        &self.sort_title
    }

    fn is_section(&self) -> bool {
        matches!(self.kind, ResolvedKind::Section(_))
    }
}

fn default_pattern(glob: &str) -> PatternOptions {
    PatternOptions {
        glob: glob.into(),
        flatten_single_child_sections: None,
        preserve_directory_names: None,
        sort: Sort::default(),
        ignore: None,
        append_unmatched: None,
        ignore_no_matches: true,
    }
}

fn merge_sort(parent: SortSettings, local: &Sort) -> SortSettings {
    SortSettings {
        by: local.by.unwrap_or(parent.by),
        direction: local.direction.unwrap_or(parent.direction),
        kind: local.kind.unwrap_or(parent.kind),
        sections: local.sections.unwrap_or(parent.sections),
        ignore_case: local.ignore_case.unwrap_or(parent.ignore_case),
    }
}

fn resolve_ignores(
    root: &str, parent: &[String], values: &[String],
) -> Vec<String> {
    let mut result = Vec::new();
    for value in values {
        if value == "$inherit" {
            result.extend_from_slice(parent);
        } else if let Some(value) = value.strip_prefix('/') {
            result.push(absolute_pattern(root, value));
        } else {
            result.push(absolute_pattern(root, &format!("**/{value}")));
        }
    }
    result
}

fn flatten(entries: Vec<Entry>) -> Vec<Resolved> {
    let mut result = Vec::new();
    for entry in entries {
        match entry {
            Entry::Page(item) | Entry::Link(item) => result.push(item),
            Entry::Directory { resolved, .. }
            | Entry::Pattern { resolved, .. } => {
                result.extend(resolved.unwrap_or_default());
            }
            Entry::Section { title, children } => {
                let children = flatten(children);
                if !children.is_empty() {
                    result.push(Resolved {
                        path: String::new(),
                        sort_title: title.clone(),
                        title,
                        kind: ResolvedKind::Section(children),
                    });
                }
            }
        }
    }
    result
}

fn deepest_directory(entries: &[Entry]) -> Option<usize> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            Entry::Directory { path, resolved: None, .. } => {
                Some(components(path))
            }
            Entry::Section { children, .. } => deepest_directory(children),
            _ => None,
        })
        .max()
}

fn absolute_pattern(root: &str, pattern: &str) -> String {
    let trailing = pattern.ends_with('/');
    let value = join(root, pattern.trim_end_matches('/'));
    if trailing {
        format!("{value}/")
    } else {
        value
    }
}

fn join(root: &str, target: &str) -> String {
    let mut parts = Vec::new();
    let source = if target.starts_with('/') || root == "." {
        target.trim_start_matches('/').into()
    } else {
        format!("{root}/{target}")
    };
    for part in source.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

fn parent(path: &str) -> String {
    path.rsplit_once('/')
        .map_or_else(|| ".".into(), |(parent, _)| parent.into())
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn components(path: &str) -> usize {
    path.split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .count()
}

/// Resolves native awesome-nav configuration and returns diagnostics.
pub fn resolve(
    settings: &Settings, documents: &BTreeMap<String, String>, pages: &[Page],
) -> Result<(Navigation, Vec<Diagnostic>)> {
    Resolver::new(settings, documents, pages)
        .resolve()
        .context("failed to resolve awesome navigation")
}

#[cfg(test)]
mod tests {
    use super::resolve_ignores;

    #[test]
    fn resolves_relative_absolute_and_inherited_ignores() {
        assert_eq!(
            resolve_ignores(
                "guide",
                &["**/draft.md".into()],
                &["$inherit".into(), "*.hidden.md".into(), "/local.md".into()],
            ),
            ["**/draft.md", "guide/**/*.hidden.md", "guide/local.md"]
        );
    }
}
