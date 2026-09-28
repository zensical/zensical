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

//! Native file exclusion for mkdocs-exclude.

use anyhow::{Context, Result};
use pyo3::prelude::*;
use std::sync::Arc;

use zrx::id::Id;
use zrx::stream::{Stream, Value};

use crate::compat::mkdocs::resource::Resource;
use crate::config::Config;
use crate::path::SourcePath;

mod pattern;

use pattern::Pattern;

/// File exclusion shared by documentation and resource streams.
#[derive(Clone, Debug)]
pub struct Exclude {
    settings: Arc<Settings>,
}

#[derive(Debug)]
struct Settings {
    enabled: bool,
    docs: String,
    globs: Vec<Pattern>,
    regexes: Vec<Py<PyAny>>,
}

impl Exclude {
    /// Compiles configured patterns once for the workflow.
    pub fn new(config: &Config) -> Result<Self> {
        let plugin = &config.project.plugins.exclude.config;
        let mut settings = Settings {
            enabled: plugin.enabled,
            docs: config.project.docs_dir.clone(),
            globs: Vec::new(),
            regexes: Vec::new(),
        };
        if plugin.enabled {
            settings.globs = plugin
                .glob
                .iter()
                .map(|pattern| Pattern::compile(pattern))
                .collect::<Result<_>>()?;
            if !plugin.regex.is_empty() {
                // Preserve Python lookarounds, backreferences and flags.
                settings.regexes = Python::attach(|py| {
                    let re = py.import("re")?;
                    plugin
                        .regex
                        .iter()
                        .map(|pattern| {
                            re.call_method1("compile", (pattern,))
                                .map(Bound::unbind)
                                .with_context(|| {
                                    format!("invalid exclude regex {pattern:?}")
                                })
                        })
                        .collect::<Result<_>>()
                })?;
            }
        }
        Ok(Self { settings: Arc::new(settings) })
    }

    /// Removes matching documentation sources before reading their contents.
    pub fn sources<T: Value>(&self, sources: &Stream<Id, T>) -> Stream<Id, T> {
        if !self.settings.enabled {
            return sources.clone();
        }
        let settings = self.settings.clone();
        sources.filter_map(move |id: &Id, value: &T| {
            if id.context() == settings.docs {
                let path = id.location().parse::<SourcePath>()?;
                // Hidden files can configure other plugins, but MkDocs does
                // not include them in the Files collection being filtered.
                if !path.is_hidden() && !settings.includes(path.as_str())? {
                    return Ok(None);
                }
            }
            Ok::<_, anyhow::Error>(Some(value.clone()))
        })
    }

    /// Filters effective assets, including files supplied by the theme.
    pub fn resources(
        &self, resources: &Stream<Id, Resource>,
    ) -> Stream<Id, Resource> {
        if !self.settings.enabled {
            return resources.clone();
        }
        let settings = self.settings.clone();
        resources.filter_map(move |resource: &Resource| {
            Ok::<_, anyhow::Error>(
                settings
                    .includes(resource.source_path.as_str())?
                    .then(|| resource.clone()),
            )
        })
    }
}

impl Settings {
    /// Returns whether no configured pattern excludes the source path.
    fn includes(&self, path: &str) -> Result<bool> {
        if self.globs.iter().any(|pattern| pattern.matches(path)) {
            return Ok(false);
        }
        if self.regexes.is_empty() {
            return Ok(true);
        }
        Python::attach(|py| {
            for pattern in &self.regexes {
                if !pattern.bind(py).call_method1("match", (path,))?.is_none() {
                    return Ok(false);
                }
            }
            Ok(true)
        })
    }
}
