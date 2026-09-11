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

//! mkdocs-api-autonav discovery and local mkdocstrings options.
use super::{module_path, regex_matches, walk, Node, Section, Snapshot};
use crate::config::plugins::ApiAutonavConfig;
use crate::config::Config;
use crate::structure::dynamic::Dynamic;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

impl Snapshot {
    pub(super) fn autonav(
        &mut self, config: &Config, strict: bool,
    ) -> Result<()> {
        let auto = &config.project.plugins.api_autonav.config;
        if auto.enabled {
            let mut tree = Node::default();
            for root in &auto.modules {
                let root = PathBuf::from(root);
                self.roots.push(root.clone());
                let mut files = Vec::new();
                walk(&root, &self.ignored_roots, &mut files)?;
                for file in &files {
                    self.observe(file)?;
                }
                let base =
                    root.parent().context("module path has no parent")?;
                let mut skipped = Vec::<PathBuf>::new();
                for file in files {
                    if file
                        .extension()
                        .is_none_or(|extension| extension != "py")
                    {
                        continue;
                    }
                    check_namespace(
                        &file,
                        &root,
                        base,
                        &mut skipped,
                        &auto.on_implicit_namespace_package,
                        strict,
                    )?;
                    if skipped.iter().any(|path| file.starts_with(path)) {
                        continue;
                    }
                    let (parts, path) = module_path(
                        file.strip_prefix(base)?,
                        &auto.api_root_uri,
                        true,
                    )?;
                    if parts.is_empty() {
                        continue;
                    }
                    if auto.exclude_private
                        && parts.iter().any(|part| part.starts_with('_'))
                    {
                        continue;
                    }
                    let identifier = parts.join(".");
                    let mut excluded = false;
                    for pattern in &auto.exclude {
                        excluded |= if let Some(pattern) =
                            pattern.strip_prefix("re:")
                        {
                            regex_matches(pattern, &identifier, false)?
                        } else {
                            identifier.starts_with(pattern)
                        };
                    }
                    if excluded {
                        continue;
                    }
                    let content = module_markdown(auto, &parts)?;
                    self.add(&path, content, None, false)?;
                    tree.insert(&parts, path);
                }
            }
            self.sections.push(Section {
                root: auto.api_root_uri.clone(),
                title: Some(auto.nav_section_title.clone()),
                children: tree.items(
                    &auto.nav_item_prefix,
                    auto.show_full_namespace,
                    &[],
                ),
                autonav: true,
                generated: true,
            });
        }
        Ok(())
    }
}

fn check_namespace(
    file: &Path, root: &Path, base: &Path, skipped: &mut Vec<PathBuf>,
    policy: &str, strict: bool,
) -> Result<()> {
    let mut ancestors = file
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .take_while(|path| *path != base)
        .collect::<Vec<_>>();
    if root.is_file() {
        ancestors.clear();
    }
    ancestors.reverse();
    for directory in ancestors {
        if skipped.iter().any(|path| directory.starts_with(path)) {
            break;
        }
        if !directory.join("__init__.py").is_file()
            && fs::read_dir(directory)?
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "py")
                        && entry.path().is_file()
                })
        {
            let message = format!("api-autonav: implicit namespace package without __init__.py at {}", directory.display());
            match policy {
                "raise" => bail!("{message}"),
                "warn" if strict => bail!("{message}"),
                "warn" => println!("[warning] {message}"),
                _ => {}
            }
            skipped.push(directory.to_owned());
            break;
        }
    }
    Ok(())
}

fn module_markdown(
    auto: &ApiAutonavConfig, parts: &[String],
) -> Result<String> {
    let identifier = parts.join(".");
    let mut options =
        BTreeMap::from([("heading_level".into(), Dynamic::Integer(1))]);
    for (pattern, local) in &auto.module_options {
        if regex_matches(pattern, &identifier, true)? {
            options.extend(local.clone());
        }
    }
    let level = options
        .get("heading_level")
        .map(ToString::to_string)
        .unwrap_or_default()
        .parse::<i64>()
        .context("api-autonav heading_level must be an integer")?;
    let heading = if level > 1 {
        format!("# {identifier}\n")
    } else {
        options
            .entry("show_root_heading".into())
            .or_insert(Dynamic::Bool(true));
        String::new()
    };
    let title = if auto.show_full_namespace {
        &identifier
    } else {
        parts.last().unwrap()
    };
    let content = format!(
        "---\ntitle: {}\n---\n{heading}\n::: {identifier}\n    options: {}\n",
        serde_json::to_string(title)?,
        serde_json::to_string(&options)?
    );
    Ok(content)
}
