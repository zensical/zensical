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

//! Mkdocstrings compatibility plugin.

use pyo3::types::PyAnyMethods;
use pyo3::Python;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

use zrx::id::Id;
use zrx::stream::Signal;

use crate::compat::mkdocs::html::Editor;
use crate::compat::mkdocs::plugin::autorefs::Registry;
use crate::config::Config;
use crate::path::{OutputRoot, SitePath};
use crate::structure::nav::Navigation;

mod backlinks;

pub use backlinks::Parser;

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Python Markdown extension that enables mkdocstrings compatibility.
const EXTENSION_NAME: &str = "zensical.extensions.mkdocstrings";

/// Version of cached backlink page data.
const BACKLINK_CACHE_VERSION: u8 = 3;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// On-disk cache for backlink data grouped by rendered page.
#[derive(Clone, Debug)]
struct BacklinkCache {
    /// Directory containing independently addressable page data.
    directory: PathBuf,
    /// Hash of all configuration that can affect handler rendering.
    config_hash: u64,
}

// ----------------------------------------------------------------------------

/// One page artifact paired with the hash of its inputs.
#[derive(Debug, Deserialize, Serialize)]
struct Cached<T> {
    /// Cached artifact.
    data: T,
    /// Hash of configuration and computation arguments.
    hash: u64,
}

// ----------------------------------------------------------------------------

/// Mkdocstrings compatibility pipeline.
#[derive(Clone, Debug)]
pub struct Mkdocstrings {
    /// Cache directory shared with the Python compatibility layer.
    cache: PathBuf,
    /// Backlink cache, created only when collection is enabled.
    backlink_cache: Option<BacklinkCache>,
    /// Site output directory.
    output: OutputRoot,
}

// ----------------------------------------------------------------------------

/// Inputs required to generate the object inventory.
pub struct Dependencies<'a> {
    /// Revision-complete site navigation.
    pub navigation: &'a Signal<Id, Navigation>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl BacklinkCache {
    /// Returns cached page data, computing and storing it on a miss.
    fn get_or_compute<T, I, A, F>(
        &self, namespace: &str, id: &I, args: &A, compute: F,
    ) -> anyhow::Result<T>
    where
        T: DeserializeOwned + Serialize,
        I: Hash + ?Sized,
        A: Hash + ?Sized,
        F: FnOnce() -> anyhow::Result<T>,
    {
        let path = self.path(namespace, id);
        let hash = self.hash(id, args);
        if let Ok(data) = fs::read(&path)
            && let Ok(cached) = serde_json::from_slice::<Cached<T>>(&data)
            && cached.hash == hash
        {
            return Ok(cached.data);
        }

        let computed = compute()?;
        if let Ok(data) = serde_json::to_vec(&Cached { data: &computed, hash })
        {
            let _ = fs::create_dir_all(path.parent().expect("invariant"));
            let _ = fs::write(path, data);
        }
        Ok(computed)
    }

    /// Derives a stable entry path from a page identity.
    fn path<K>(&self, namespace: &str, key: &K) -> PathBuf
    where
        K: Hash + ?Sized,
    {
        let mut hasher = DefaultHasher::new();
        BACKLINK_CACHE_VERSION.hash(&mut hasher);
        key.hash(&mut hasher);
        self.directory
            .join(namespace)
            .join(format!("{}.json", hasher.finish()))
    }

    /// Hashes the identity and all inputs that can change an artifact.
    fn hash<I, A>(&self, id: &I, args: &A) -> u64
    where
        I: Hash + ?Sized,
        A: Hash + ?Sized,
    {
        let mut hasher = DefaultHasher::new();
        self.config_hash.hash(&mut hasher);
        id.hash(&mut hasher);
        args.hash(&mut hasher);
        hasher.finish()
    }
}

// ----------------------------------------------------------------------------

impl Mkdocstrings {
    /// Resolves the private settings owned by this pipeline instance.
    pub fn new(config: &Config) -> Self {
        let cache = config.get_cache_dir();
        let backlink_cache = (config.has_markdown_extension(EXTENSION_NAME)
            && config.records_backlinks())
        .then(|| BacklinkCache {
            directory: cache.join("mkdocstrings"),
            config_hash: config.hash,
        });
        Self {
            cache,
            backlink_cache,
            output: config.output_root().clone(),
        }
    }

    /// Installs object inventory generation.
    pub fn setup(&self, dependencies: Dependencies<'_>) {
        let pipeline = self.clone();
        let _ = dependencies.navigation.map(move |_: &Navigation| {
            let cache_path = pipeline.cache.join("objects.inv");
            let cached = fs::read(&cache_path).ok();

            let data = Python::attach(|py| {
                let module = py.import("zensical.compat.mkdocstrings")?;
                module
                    .call_method1("get_inventory", (cached,))?
                    .extract::<Vec<u8>>()
            })?;

            let path = pipeline.output.join(
                &"objects.inv".parse::<SitePath>().expect("static site path"),
            );
            fs::create_dir_all(path.parent().expect("invariant"))?;
            fs::write(path, &data)?;
            fs::create_dir_all(&pipeline.cache)?;
            fs::write(&cache_path, &data)?;
            Ok::<_, anyhow::Error>(())
        });
    }

    /// Creates a visitor when handler backlink rendering is enabled.
    pub fn parser(&self) -> Option<Parser> {
        self.backlink_cache.as_ref().map(|_| Parser::default())
    }

    /// Adds rendered backlinks to the shared HTML edits after collection.
    pub fn apply_backlinks(
        &self, parser: Parser, editor: &mut Editor<'_>, autorefs: &Registry,
        from_url: &str,
    ) -> anyhow::Result<()> {
        let Some(cache) = &self.backlink_cache else {
            return Ok(());
        };
        parser.render(editor, |descriptors| {
            let compute = || {
                let aliases = cache.get_or_compute(
                    "aliases",
                    from_url,
                    descriptors,
                    || {
                        descriptors
                            .iter()
                            .map(|(handler, identifier)| {
                                Self::get_backlink_aliases(handler, identifier)
                            })
                            .collect::<anyhow::Result<Vec<_>>>()
                    },
                )?;
                descriptors
                    .iter()
                    .zip(aliases)
                    .map(|((handler, identifier), aliases)| {
                        Self::render_backlink_placeholder(
                            autorefs, from_url, handler, identifier, aliases,
                        )
                    })
                    .collect::<anyhow::Result<Vec<_>>>()
            };
            if let Some(revision) = autorefs.backlink_revision() {
                cache.get_or_compute(
                    "resolved",
                    from_url,
                    &(revision, descriptors),
                    compute,
                )
            } else {
                compute()
            }
        })
    }

    /// Computes one backlink placeholder from the settled autorefs index.
    fn render_backlink_placeholder(
        autorefs: &Registry, from_url: &str, handler: &str, identifier: &str,
        aliases: Vec<String>,
    ) -> anyhow::Result<String> {
        let mut identifiers = Vec::with_capacity(aliases.len() + 1);
        identifiers.push(identifier.to_string());
        identifiers.extend(aliases);
        identifiers.extend(autorefs.get_aliases(identifier, from_url));
        identifiers.sort();
        identifiers.dedup();
        let backlinks = autorefs.get_backlinks(&identifiers, from_url);
        if backlinks.is_empty() {
            return Ok(String::new());
        }

        Python::attach(|py| {
            py.import("zensical.compat.mkdocstrings")?
                .call_method1("render_backlinks", (handler, backlinks))?
                .extract::<String>()
                .map_err(Into::into)
        })
    }

    /// Gets aliases that only exist in the handler's object model.
    fn get_backlink_aliases(
        handler: &str, identifier: &str,
    ) -> anyhow::Result<Vec<String>> {
        Python::attach(|py| {
            py.import("zensical.compat.mkdocstrings")?
                .call_method1("get_backlink_aliases", (handler, identifier))?
                .extract::<Vec<String>>()
                .map_err(Into::into)
        })
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use tempfile::tempdir;

    use super::BacklinkCache;

    #[test]
    fn backlink_pages_are_reused_without_computing_again() {
        let directory = tempdir().unwrap();
        let cache = BacklinkCache {
            directory: directory.path().to_path_buf(),
            config_hash: 42,
        };
        let descriptors = [("python", "package.Object")];
        let calls = Cell::new(0);

        let first: String = cache
            .get_or_compute("resolved", "reference/", &descriptors, || {
                calls.set(calls.get() + 1);
                Ok("<aside>Guide</aside>".into())
            })
            .unwrap();
        let second: String = cache
            .get_or_compute("resolved", "reference/", &descriptors, || {
                calls.set(calls.get() + 1);
                Ok("rendered twice".into())
            })
            .unwrap();

        assert_eq!(first, "<aside>Guide</aside>");
        assert_eq!(second, first);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn backlink_pages_are_scoped_to_configuration() {
        let directory = tempdir().unwrap();
        let descriptors = [("python", "package.Object")];
        let first = BacklinkCache {
            directory: directory.path().to_path_buf(),
            config_hash: 1,
        };
        let second = BacklinkCache {
            directory: directory.path().to_path_buf(),
            config_hash: 2,
        };

        assert_eq!(
            first.path("resolved", "reference/"),
            second.path("resolved", "reference/"),
        );
        let first: String = first
            .get_or_compute("resolved", "reference/", &descriptors, || {
                Ok("first".into())
            })
            .unwrap();
        let second: String = second
            .get_or_compute("resolved", "reference/", &descriptors, || {
                Ok("second".into())
            })
            .unwrap();
        assert_eq!(first, "first");
        assert_eq!(second, "second");
    }
}
