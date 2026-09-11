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

//! mkdocs-autoapi discovery and source retention.
use super::{glob, module_path, walk, write_kept, Node, Section, Snapshot};
use crate::config::Config;
use crate::structure::dynamic::Dynamic;
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::PathBuf;

impl Snapshot {
    pub(super) fn autoapi(&mut self, config: &Config) -> Result<()> {
        let auto = &config.project.plugins.autoapi.config;
        if auto.enabled {
            let title = match &auto.autoapi_add_nav_entry {
                Dynamic::Bool(true) => Some("API Reference".into()),
                Dynamic::String(title) if !title.is_empty() => {
                    Some(title.clone())
                }
                _ => None,
            };
            let mut tree = Node::default();
            if auto.autoapi_generate_api_docs {
                let root = PathBuf::from(&auto.autoapi_dir);
                self.roots.push(root.clone());
                let patterns = auto
                    .autoapi_file_patterns
                    .iter()
                    .map(|pattern| glob(&format!("**/{pattern}")))
                    .collect::<Result<Vec<_>>>()?;
                let ignores = auto
                    .autoapi_ignore
                    .iter()
                    .map(|pattern| glob(pattern))
                    .collect::<Result<Vec<_>>>()?;
                self.patterns.extend(patterns.iter().cloned());
                let mut files = Vec::new();
                walk(&root, &self.ignored_roots, &mut files)?;
                let mut selected = BTreeMap::new();
                for file in files {
                    let relative = file.strip_prefix(&root)?;
                    self.observe(&file)?;
                    if ignores.iter().any(|pattern| pattern.is_match(relative))
                    {
                        continue;
                    }
                    if let Some(priority) = patterns
                        .iter()
                        .position(|pattern| pattern.is_match(relative))
                    {
                        let key = file.with_extension("");
                        let entry = selected
                            .entry(key)
                            .or_insert((priority, file.clone()));
                        if priority < entry.0 {
                            *entry = (priority, file);
                        }
                    }
                }
                let base = if root.join("__init__.py").is_file() {
                    root.parent().unwrap_or(&root)
                } else {
                    &root
                };
                for (_, (_, file)) in selected {
                    let relative = file.strip_prefix(base)?;
                    let (parts, path) =
                        module_path(relative, &auto.autoapi_root, false)?;
                    if parts.is_empty() {
                        continue;
                    }
                    let identifier = if auto.handler == "vba" {
                        relative.to_string_lossy().replace('\\', "/")
                    } else {
                        parts.join(".")
                    };
                    self.add(
                        &path,
                        format!("::: {identifier}\n"),
                        Some(file),
                        false,
                    )?;
                    tree.insert(&parts, path);
                }
                let mut summary = String::new();
                tree.summary(0, &auto.autoapi_root, &mut summary);
                self.add(
                    &format!("{}/summary.md", auto.autoapi_root),
                    summary,
                    None,
                    true,
                )?;
                if auto.autoapi_keep_files {
                    for (path, document) in &self.files {
                        write_kept(config, path, &document.content)?;
                    }
                }
            }
            self.sections.push(Section {
                root: auto.autoapi_root.clone(),
                title,
                children: tree.items("", false, &[]),
                autonav: false,
                generated: auto.autoapi_generate_api_docs,
            });
        }
        Ok(())
    }
}
