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

//! Autorefs URLs.

use std::path::Path;
use std::string::ToString;

use zrx::path::PathExt;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Resolves the closest URL from a list relative to a source URL.
pub fn closest(from: &str, urls: &[String], _qualifier: &str) -> String {
    let mut base = from.to_string();
    let candidates;

    loop {
        let found = urls
            .iter()
            .filter(|url| is_relative_to(url, &base))
            .cloned()
            .collect::<Vec<_>>();

        if !found.is_empty() {
            candidates = found;
            break;
        }

        match parent(&base) {
            Some(parent) if !parent.is_empty() => base = parent,
            _ => return urls[0].clone(),
        }
    }

    if candidates.len() == 1 {
        candidates[0].clone()
    } else {
        candidates
            .into_iter()
            .min_by_key(|url| url.matches('/').count())
            .expect("candidate list is nonempty")
    }
}

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

/// Returns whether a URL has no HTTP(S) scheme.
pub fn is_relative(url: &str) -> bool {
    !(url.starts_with("http://") || url.starts_with("https://"))
}

/// Returns whether one URL path begins with another at a component boundary.
fn is_relative_to(url: &str, base: &str) -> bool {
    let url = url
        .split('#')
        .next()
        .unwrap_or(url)
        .split('?')
        .next()
        .unwrap_or(url);
    let base = base
        .split('#')
        .next()
        .unwrap_or(base)
        .split('?')
        .next()
        .unwrap_or(base);
    Path::new(url).starts_with(Path::new(base))
}

/// Returns the parent path of a URL.
fn parent(url: &str) -> Option<String> {
    Path::new(url)
        .parent()
        .and_then(Path::to_str)
        .map(ToString::to_string)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{closest, relative};

    #[test]
    fn resolves_the_closest_url() {
        let cases = [
            ("", vec!["x/#b", "#b"], "#b"),
            ("a/b", vec!["x/#e", "a/c/#e", "a/d/#e"], "a/c/#e"),
            ("a/b/", vec!["x/#e", "a/d/#e", "a/c/#e"], "a/d/#e"),
            ("a/b", vec!["x/#e", "a/c/#e", "a/c/d/#e"], "a/c/#e"),
            ("a/b/", vec!["x/#e", "a/c/d/#e", "a/c/#e"], "a/c/#e"),
            (
                "a/b/c",
                vec!["x/#e", "a/#e", "a/b/#e", "a/b/c/#e", "a/b/c/d/#e"],
                "a/b/c/#e",
            ),
            (
                "a/b/c/",
                vec!["x/#e", "a/#e", "a/b/#e", "a/b/c/d/#e", "a/b/c/#e"],
                "a/b/c/#e",
            ),
            ("a", vec!["b/c/#d", "c/#d"], "b/c/#d"),
            ("a/", vec!["c/#d", "b/c/#d"], "c/#d"),
        ];

        for (base, urls, expected) in cases {
            let urls = urls.into_iter().map(String::from).collect::<Vec<_>>();
            assert_eq!(closest(base, &urls, "test"), expected, "base: {base}");
        }
    }

    #[test]
    fn computes_relative_urls() {
        let cases = [
            ("a/", "a#b", "#b"),
            ("a/", "a/b#c", "b#c"),
            ("a/b/", "a/b#c", "#c"),
            ("a/b/", "a/c#d", "../c#d"),
            ("a/b/", "a#c", "..#c"),
            ("a/b/c/", "d#e", "../../../d#e"),
            ("a/b/", "c/d/#e", "../../c/d/#e"),
            ("a/index.html", "a/index.html#b", "#b"),
            ("a/index.html", "a/b.html#c", "b.html#c"),
            ("a/b.html", "a/b.html#c", "#c"),
            ("a/b.html", "a/c.html#d", "c.html#d"),
            ("a/b.html", "a/index.html#c", "index.html#c"),
            ("a/b/c.html", "d.html#e", "../../d.html#e"),
            ("a/b.html", "c/d.html#e", "../c/d.html#e"),
            ("a/b/index.html", "a/b/c/d.html#e", "c/d.html#e"),
            ("", "#x", "#x"),
            ("a/", "#x", "../#x"),
            ("a/b.html", "#x", "../#x"),
            ("", "a/#x", "a/#x"),
            ("", "a/b.html#x", "a/b.html#x"),
        ];

        for (from, to, expected) in cases {
            assert_eq!(relative(from, to), expected, "from: {from}, to: {to}");
        }
    }
}
