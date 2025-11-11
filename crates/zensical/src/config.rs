// Copyright (c) 2025 Zensical and contributors

// SPDX-License-Identifier: MIT
// Third-party contributions licensed under DCO

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
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fs, iter};
use zrx::path::PathExt;

mod error;
pub mod extra;
pub mod mdx;
pub mod plugins;
mod project;
pub mod theme;

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
        let path = path.as_ref();
        Python::attach(|py| {
            // Configuration is parsed in Python, since we must support certain
            // YAML tags like `!ENV`, and allow to reference Python functions
            // in configuration. For TOML, this is technically not necessary,
            // but we'll move it through the same pipeline for consistency.
            let module = py.import("zensical.config")?;
            let config = module
                .call_method1("parse_config", (path.to_string_lossy(),))?
                .extract::<Project>()?;

            // Obtain main theme directory from, which is distributed in a
            // subfolder as part of the Python package
            let theme_dir =
                module.call_method0("get_theme_dir")?.extract::<PathBuf>()?;

            // Return configuration and theme directory
            Ok::<_, PyErr>((config, theme_dir))
        })
        .map_err(Into::into)
        .and_then(|(project, theme_dir)| {
            // Merge theme directories, giving precedence to custom directory
            // over the main theme directory to allow for overrides
            let iter = project.theme.custom_dir.clone().into_iter();
            let theme_dirs = iter
                .chain(iter::once(theme_dir))
                .map(|path| path.canonicalize().expect("invariant"))
                .collect();

            // Precompute hash
            let hash = {
                let mut hasher = DefaultHasher::default();
                project.hash(&mut hasher);
                hasher.finish()
            };

            // Return configuration
            Ok(Config {
                path: path.canonicalize()?,
                project: Arc::new(project),
                theme_dirs,
                hash,
            })
        })
    }

    /// Returns the directory the configuration file is located in.
    pub fn get_root_dir(&self) -> PathBuf {
        let mut path = self.path.clone();
        path.pop();
        path
    }

    /// Returns the docs directory, resolved relative to the configuration file.
    pub fn get_docs_dir(&self) -> PathBuf {
        let mut path = self.path.clone();
        path.pop();

        // Ensure directory exists
        let path = path.join(&self.project.docs_dir);
        fs::create_dir_all(&path)
            .and_then(|()| path.canonicalize())
            .expect("invariant")
    }

    /// Returns the site directory, resolved relative to the configuration file.
    pub fn get_site_dir(&self) -> PathBuf {
        let mut path = self.path.clone();
        path.pop();

        // Ensure directory exists
        let path = path.join(&self.project.site_dir);
        fs::create_dir_all(&path)
            .and_then(|()| path.canonicalize())
            .expect("invariant")
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
        PathBuf::from(".")
            .relative_to(path)
            .to_string_lossy()
            .replace('\\', "/")
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
