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

//! Material-compatible image optimization for local and downloaded assets.

use anyhow::{anyhow, Context as _, Result};
use pyo3::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::config::plugins::OptimizePluginConfig;
use crate::config::Config;

mod image;

/// Enabled instances and savings shared by all asset producers in one build.
#[derive(Clone, Debug, Default)]
pub struct Optimize {
    instances: Arc<[Instance]>,
    gains: Arc<Mutex<BTreeMap<(usize, String), usize>>>,
}

#[derive(Clone, Debug)]
struct Instance {
    name: String,
    config: OptimizePluginConfig,
    cache: PathBuf,
    limit: Arc<Limit>,
}

/// Encoding concurrency is bounded across local and downloaded asset streams.
#[derive(Debug)]
struct Limit {
    maximum: usize,
    active: Mutex<usize>,
    available: Condvar,
}

struct Permit<'a>(&'a Limit);

impl Limit {
    fn acquire(&self) -> Result<Permit<'_>> {
        let active = self.active.lock().map_err(|_| {
            anyhow!("optimization concurrency limiter was poisoned")
        })?;
        let mut active = self
            .available
            .wait_while(active, |active| *active >= self.maximum)
            .map_err(|_| {
                anyhow!("optimization concurrency limiter was poisoned")
            })?;
        *active += 1;
        Ok(Permit(self))
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.0.active.lock() {
            *active -= 1;
            self.0.available.notify_one();
        }
    }
}

impl Optimize {
    /// Enabled optimization instances are resolved outside serve mode.
    pub fn new(config: &Config, serve: bool) -> Self {
        let mut instances = Vec::new();
        if !serve {
            for entry in &config.project.plugins.optimize.config {
                if entry.config.enabled && entry.config.optimize {
                    instances.push(Instance {
                        name: entry.name.clone(),
                        cache: config
                            .project
                            .root_dir
                            .join(&entry.config.cache_dir),
                        config: entry.config.clone(),
                        limit: Arc::new(Limit {
                            maximum: entry.config.concurrency,
                            active: Mutex::new(0),
                            available: Condvar::new(),
                        }),
                    });
                }
            }
        }
        Self {
            instances: instances.into(),
            ..Self::default()
        }
    }

    /// Whether an optimization instance participates in this build.
    pub fn active(&self) -> bool {
        !self.instances.is_empty()
    }

    /// The configured upper bound is used for concurrent asset processing.
    pub fn concurrency(&self) -> usize {
        self.instances
            .iter()
            .map(|instance| instance.config.concurrency)
            .max()
            .unwrap_or(1)
    }

    /// Unsupported and excluded files are rejected before their bytes are read.
    pub fn selected(&self, path: &str) -> Result<bool> {
        for instance in self.instances.iter() {
            if instance.selected(path)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Selected images are optimized by the first matching instance.
    pub fn image(
        &self, path: &str, mut bytes: Arc<Vec<u8>>,
    ) -> Result<Arc<Vec<u8>>> {
        for (index, instance) in self.instances.iter().enumerate() {
            if !instance.selected(path)? {
                continue;
            }
            let optimized =
                instance.encode(path, &bytes).with_context(|| {
                    format!("[{}] failed to optimize {path}", instance.name)
                })?;
            let gain = bytes.len().saturating_sub(optimized.len());
            if gain > 0 {
                bytes = Arc::new(optimized);
            }
            let previous = self
                .gains
                .lock()
                .map_err(|_| anyhow!("optimization statistics were poisoned"))?
                .insert((index, path.to_owned()), gain);
            if previous.is_none() && instance.config.print_gain {
                eprintln!(
                    "INFO -  [{}] {path}: saved {gain} bytes",
                    instance.name
                );
            }
            // Files are claimed by the first matching instance in Material's
            // on_env hook, so the same ownership order must be preserved here.
            break;
        }
        Ok(bytes)
    }

    /// Total savings are reported once after a successful build.
    pub fn report(&self) -> Result<()> {
        let gains = self
            .gains
            .lock()
            .map_err(|_| anyhow!("optimization statistics were poisoned"))?;
        for (index, instance) in self.instances.iter().enumerate() {
            if instance.config.print_gain_summary {
                let gain: usize = gains
                    .iter()
                    .filter(|((id, _), _)| *id == index)
                    .map(|(_, gain)| gain)
                    .sum();
                eprintln!(
                    "INFO -  [{}] Total savings: {gain} bytes",
                    instance.name
                );
            }
        }
        Ok(())
    }
}

impl Instance {
    fn selected(&self, path: &str) -> Result<bool> {
        let extension = extension(path);
        if !match extension.as_str() {
            "png" => self.config.optimize_png,
            "jpg" | "jpeg" => self.config.optimize_jpg,
            _ => false,
        } {
            return Ok(false);
        }
        // Material's Python fnmatch behavior is preserved, including literal
        // braces and platform-specific matching rules.
        Python::attach(|py| {
            let matches =
                py.import("zensical.compat.privacy")?.getattr("matches")?;
            if self.config.optimize_include.is_empty() {
                Ok(!matches
                    .call1((path, self.config.optimize_exclude.clone()))?
                    .extract::<bool>()?)
            } else {
                Ok(matches
                    .call1((path, self.config.optimize_include.clone()))?
                    .extract()?)
            }
        })
    }

    fn encode(&self, path: &str, bytes: &[u8]) -> Result<Vec<u8>> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        // Encoder settings and format are included so cached bytes can be
        // reused across paths, instances, and unrelated configuration changes.
        let mut digest = Sha256::new();
        // Cache entries produced by the former Pillow encoder are invalidated.
        digest.update(b"zensical-optimize-v2\0");
        digest.update(serde_json::to_vec(&(
            self.config.optimize_png_speed,
            self.config.optimize_png_strip,
            self.config.optimize_jpg_quality,
            self.config.optimize_jpg_progressive,
        ))?);
        digest.update(extension(path).as_bytes());
        digest.update(bytes);
        let key = format!("{:x}", digest.finalize());
        let cached = self.cache.join(key);
        if self.config.cache
            && let Ok(bytes) = std::fs::read(&cached)
        {
            return Ok(bytes);
        }
        let _permit = self.limit.acquire()?;
        let optimized = match extension(path).as_str() {
            "png" => image::png(
                bytes,
                self.config.optimize_png_speed,
                self.config.optimize_png_strip,
            ),
            _ => image::jpeg(
                bytes,
                self.config.optimize_jpg_quality,
                self.config.optimize_jpg_progressive,
            ),
        }?;
        let optimized = if optimized.len() < bytes.len() {
            optimized
        } else {
            bytes.to_vec()
        };
        if self.config.cache {
            std::fs::create_dir_all(&self.cache)?;
            // Concurrent builds are prevented from exposing partial cache entries.
            let temporary = cached.with_extension(format!(
                "{}.{}.tmp",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let result = (|| -> std::io::Result<()> {
                std::fs::write(&temporary, &optimized)?;
                std::fs::rename(&temporary, &cached)
            })();
            let _ = std::fs::remove_file(temporary);
            result?;
        }
        Ok(optimized)
    }
}

fn extension(path: &str) -> String {
    path.rsplit_once('.')
        .map_or_else(String::new, |(_, extension)| {
            extension.to_ascii_lowercase()
        })
}

#[cfg(test)]
mod tests {
    use super::Limit;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Condvar, Mutex};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn encoding_limit_is_shared_across_workers() {
        for maximum in [1, 2] {
            let limit = Limit {
                maximum,
                active: Mutex::new(0),
                available: Condvar::new(),
            };
            let active = AtomicUsize::new(0);
            let peak = AtomicUsize::new(0);

            // The same limit is shared by independent asset streams.
            thread::scope(|scope| {
                for _ in 0..8 {
                    scope.spawn(|| {
                        let _permit = limit.acquire().unwrap();
                        let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(count, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(1));
                        active.fetch_sub(1, Ordering::SeqCst);
                    });
                }
            });

            assert!((1..=maximum).contains(&peak.load(Ordering::SeqCst)));
            assert_eq!(active.load(Ordering::SeqCst), 0);
        }
    }
}
