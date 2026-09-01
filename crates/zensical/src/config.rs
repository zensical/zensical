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

//! Configuration.

use fluent_uri::Uri;
use pyo3::types::PyAnyMethods;
use pyo3::{PyErr, Python};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use zrx::path::PathExt;

use crate::config::plugins::TagsPlugin;
use crate::path::{OutputRoot, SourceRoot};

mod error;
pub mod extra;
pub mod mdx;
pub mod plugins;
mod project;
pub mod theme;
pub mod validation;

pub use error::Result;
pub use project::Project;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Configuration.
///
/// Note that this data model exactly matches Material for MkDocs' data model,
/// as it's where we're coming from, and we need to make sure that migration is
/// seamless. This is also why we scope all settings under the `project` key,
/// so we can move them out one by one once we start refactoring configuration.
#[derive(Clone, Debug)]
pub struct Config {
    /// Path to configuration file.
    pub path: PathBuf,
    /// Project settings.
    pub project: Arc<Project>,
    /// Theme directories.
    pub theme_dirs: Vec<PathBuf>,
    /// Canonical documentation source root.
    docs_root: SourceRoot,
    /// Canonical site output root.
    output_root: OutputRoot,
    /// Resolved Python Markdown extensions after compatibility shims.
    markdown_extensions: Arc<[String]>,
    /// Configuration hash.
    pub hash: u64,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Config {
    /// Creates a configuration by loading and parsing the file at given path.
    ///
    /// This method supports `mkdocs.yml`, as well as `zensical.toml` files.
    /// Right now, parsing is done in Python for compatibility with MkDocs.
    pub fn new<P>(path: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        // Resolve the configuration itself before Python interprets relative
        // paths. This keeps Python configuration loading and Rust filesystem
        // roots anchored to the same directory when the file is a symlink.
        let path = path.as_ref().canonicalize()?;
        let value = path.to_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "configuration path must be valid UTF-8",
            )
        })?;
        let (project, markdown_extensions) = Python::attach(|py| {
            // Reset global data in compatibility modules
            py.import("zensical.extensions.autorefs")?
                .call_method0("reset")?;
            py.import("zensical.compat.mkdocstrings")?
                .call_method0("reset")?;

            // Configuration is parsed in Python, since we must support certain
            // YAML tags like `!ENV`, and allow to reference Python functions
            // in configuration. For TOML, this is technically not necessary,
            // but we'll move it through the same pipeline for consistency.
            let module = py.import("zensical.config")?;
            let config = module.call_method1("parse_config", (value,))?;
            let markdown_extensions = config
                .get_item("markdown_extensions")?
                .extract::<Vec<String>>()?;

            // Validate raw native tags configuration before derived project
            // extraction can replace its precise diagnostic with generic
            // nested-field context from PyO3.
            config
                .get_item("plugins")?
                .get_item("tags")?
                .extract::<TagsPlugin>()?;
            let project = config.extract::<Project>()?;

            // Return configuration and theme directory
            Ok::<_, PyErr>((project, markdown_extensions))
        })?;

        // Merge theme directories, giving precedence to custom directory over
        // the main theme directory to allow for overrides.
        let iter = project.theme_dirs.clone().into_iter();
        let theme_dirs = iter
            .map(|path| path.canonicalize().expect("invariant"))
            .collect();

        // Precompute hash
        let hash = {
            let mut hasher = DefaultHasher::default();
            project.hash(&mut hasher);
            hasher.finish()
        };

        // Resolve physical roots once. Logical source and output paths are
        // joined to these canonical roots at filesystem boundaries.
        let root = path.parent().expect("configuration has parent");
        let docs_dir = root.join(&project.docs_dir);
        fs::create_dir_all(&docs_dir)?;
        let docs_root = SourceRoot::open(docs_dir)?;
        let output_root = OutputRoot::prepare(root.join(&project.site_dir))?;

        // Return configuration
        Ok(Config {
            path,
            project: Arc::new(project),
            theme_dirs,
            docs_root,
            output_root,
            markdown_extensions: markdown_extensions.into(),
            hash,
        })
    }

    /// Returns whether a resolved Python Markdown extension is active.
    pub fn has_markdown_extension(&self, name: &str) -> bool {
        self.markdown_extensions
            .iter()
            .any(|extension| extension == name)
    }

    /// Returns the canonical documentation source root.
    pub fn docs_root(&self) -> &SourceRoot {
        &self.docs_root
    }

    /// Returns the canonical site output root.
    pub fn output_root(&self) -> &OutputRoot {
        &self.output_root
    }

    /// Returns the cache directory, resolved relative to the configuration file.
    pub fn get_cache_dir(&self) -> PathBuf {
        let mut path = self.path.clone();
        path.pop();

        // Ensure directory exists
        let path = path.join(".cache");
        fs::create_dir_all(&path)
            .and_then(|()| path.canonicalize())
            .inspect(|path| {
                let gitignore = path.join(".gitignore");
                if !gitignore.exists() {
                    fs::write(gitignore, "*").expect("invariant");
                }
            })
            .expect("invariant")
    }

    /// Returns the base URL, derived from the site URL if available.
    #[allow(clippy::unused_self)]
    pub fn get_base_url<P>(&self, path: P) -> String
    where
        P: AsRef<Path>,
    {
        relative_base_url(path)
    }

    /// Returns the base path, derived from the site URL if available.
    pub fn get_base_path(&self) -> String {
        let site_url = self.project.site_url.clone();

        // Determine base path from site URL, if available
        let mut base = match Uri::parse(site_url.unwrap_or_default()) {
            Ok(uri) => uri.path().as_str().to_string(),
            Err(_) => String::from("/"),
        };

        // Ensure base path is at least a slash
        if base.is_empty() {
            base = String::from("/");
        }

        // Ensure base path doesn't end with slash, unless it's just a slash
        if base == "/" {
            base
        } else {
            base.trim_end_matches('/').to_string()
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Computes the relative root used by page and fragment template contexts.
pub fn relative_base_url<P>(path: P) -> String
where
    P: AsRef<Path>,
{
    PathBuf::from(".")
        .relative_to(path)
        .to_string_lossy()
        .replace('\\', "/")
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Hash for Config {
    /// Hashes the navigation.
    #[inline]
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        state.write_u64(self.hash);
    }
}
