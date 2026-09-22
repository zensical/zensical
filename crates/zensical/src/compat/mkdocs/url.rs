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

//! Shared MkDocs-compatible URL transformations.

use std::path::Path;

use zrx::path::PathExt;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Computes a relative URL from one page URL to another.
pub fn relative(from: &str, to: &str) -> String {
    let from = Path::new(from);
    let (to, fragment) = to
        .split_once('#')
        .map_or((Path::new(to), None), |(path, fragment)| {
            (Path::new(path), Some(fragment))
        });
    let mut relative =
        to.relative_to(from).to_string_lossy().replace('\\', "/");

    if let Some(fragment) = fragment {
        if relative == "." {
            return format!("#{fragment}");
        }
        if to.as_os_str().is_empty() {
            relative.push('/');
        }
        relative.push('#');
        relative.push_str(fragment);
    }
    relative
}

/// Resolves a local URL against `from`, then makes it relative to `to`.
pub fn rebase(from: &str, to: &str, value: &str) -> Option<String> {
    resolve(from, value).map(|target| relative(to, &target))
}

/// Resolves one local URL against a page route.
pub fn resolve(from: &str, value: &str) -> Option<String> {
    if value.starts_with(['#', '?', '/'])
        || value
            .split('/')
            .next()
            .is_some_and(|prefix| prefix.contains(':'))
    {
        return None;
    }
    let suffix = value.find(['?', '#']).unwrap_or(value.len());
    let (path, suffix) = value.split_at(suffix);
    let base = if from.ends_with('/') {
        Path::new(from)
    } else {
        Path::new(from).parent().unwrap_or_else(|| Path::new(""))
    };
    let target = base
        .join(path)
        .normalize()
        .to_string_lossy()
        .replace('\\', "/");
    Some(format!("{target}{suffix}"))
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{rebase, relative, resolve};

    #[test]
    fn computes_relative_urls() {
        assert_eq!(relative("a/b/", "a/c#d"), "../c#d");
        assert_eq!(relative("a/index.html", "a/b.html#c"), "b.html#c");
        assert_eq!(relative("a/b/", "a/b#c"), "#c");
    }

    #[test]
    fn rebases_local_urls() {
        assert_eq!(
            rebase(
                "blog/2026/09/post/",
                "blog/page/2/",
                "../../../../notes/#detail"
            )
            .as_deref(),
            Some("../../../notes/#detail")
        );
        assert_eq!(rebase("a/", "b/", "https://example.com"), None);
        assert_eq!(rebase("a/", "b/", "#local"), None);
    }

    #[test]
    fn resolves_local_urls_against_page_routes() {
        assert_eq!(
            resolve(
                "blog/2026/09/post/",
                "../../../../blog/posts/assets/image.png?raw#preview"
            )
            .as_deref(),
            Some("blog/posts/assets/image.png?raw#preview")
        );
        assert_eq!(resolve("a/", "https://example.com"), None);
        assert_eq!(resolve("a/", "/root"), None);
    }
}
