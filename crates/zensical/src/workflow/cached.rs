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

//! Workflow cache.

use serde::{Deserialize, Serialize};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use zrx::scheduler::action::report::IntoReport;
use zrx::scheduler::Value;

use crate::config::Config;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Workflow cache.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Cached<T> {
    /// Cached data.
    pub data: T,
    /// Computed hash.
    pub hash: u64,
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Caches the result of an expensive computation based on an identifier and
/// input arguments. Note that this is only a preliminary implementation, and
/// will be replaced with a more generic caching mechanism integrated into
/// the runtime.
pub fn cached<I, T, F, R, U>(
    config: &Config, id: I, args: T, mut f: F,
) -> impl IntoReport<U>
where
    I: Hash,
    T: Hash,
    F: FnMut(T) -> R,
    R: IntoReport<U>,
    U: Value + Serialize + for<'de> Deserialize<'de>,
{
    // Compute hash of identifier
    let id_hash = {
        let mut hasher = DefaultHasher::default();
        id.hash(&mut hasher);
        hasher.finish()
    };

    // Compute hash of content
    let hash = {
        let mut hasher = DefaultHasher::default();
        args.hash(&mut hasher);
        hasher.finish()
    };

    // Compute path to cache file from cache directory and identifier hash, and
    // check if we already have a cached version of the artifact. If so, compare
    // the content hash and return cached version if it matches. Otherwise, we
    // continue and compute the artifact.
    let path = config.get_cache_dir().join(id_hash.to_string());
    if let Ok(data) = fs::read(&path) {
        let cached: Cached<U> =
            serde_json::from_slice(&data).expect("invariant");

        // In case content hashes match, return cached data
        if cached.hash == hash {
            return cached.data.into_report();
        }
    }

    // Compute artifact and convert into report - note that we need to properly
    // handle encoding and file I/O errors here as well
    f(args).into_report().inspect(|report| {
        serde_json::to_string_pretty(&Cached { data: &report.data, hash })
            .map(|content| fs::write(path, content).expect("invariant"))
            .expect("invariant");
    })
}
