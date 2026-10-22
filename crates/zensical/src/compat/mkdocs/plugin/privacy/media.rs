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

//! URL extraction from local and downloaded media.

use anyhow::Result;
use pyo3::prelude::*;
use regex::Regex;
use std::sync::LazyLock;

use super::Instance;

/// One URL capture and its UTF-8 byte range.
pub(super) struct Link {
    pub start: usize,
    pub end: usize,
    pub value: String,
}

static CSS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?im)url\(\s*(?:"(?P<double>(?:https?:)?//[^)'"\r\n]+)"|'(?P<single>(?:https?:)?//[^)'"\r\n]+)'|(?P<url>(?:https?:)?//[^)'"\s]+))\s*\)"#,
    )
    .unwrap()
});
static JS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?im)["'](?P<url>(?:https?:)?//[^"']+\.(?:css|js(?:on)?))["']"#,
    )
    .unwrap()
});

impl Instance {
    pub(super) fn has_expression(&self, extension: &str) -> bool {
        matches!(extension, ".css" | ".js")
            || self.config.assets_expr_map.contains_key(extension)
    }

    /// Backreferences, lookarounds, and Unicode spans are retained in overrides.
    pub(super) fn media_links(
        &self, content: &str, extension: &str,
    ) -> Result<Vec<Link>> {
        if let Some(expression) = self.config.assets_expr_map.get(extension) {
            return Python::attach(|py| {
                let matches = py
                    .import("zensical.compat.privacy")?
                    .getattr("media_links")?
                    .call1((content, expression))?
                    .extract::<Vec<(usize, usize, String)>>()?;
                Ok(matches
                    .into_iter()
                    .map(|(start, end, value)| Link { start, end, value })
                    .collect())
            });
        }
        let expression = match extension {
            ".css" => &*CSS,
            ".js" => &*JS,
            _ => return Ok(Vec::new()),
        };
        Ok(expression
            .captures_iter(content)
            .filter_map(|captures| {
                captures
                    .name("url")
                    .or_else(|| captures.name("double"))
                    .or_else(|| captures.name("single"))
            })
            .map(|capture| Link {
                start: capture.start(),
                end: capture.end(),
                value: capture.as_str().to_owned(),
            })
            .collect())
    }
}

/// Braces and malformed brackets are treated as literals by Python fnmatch.
/// Its platform-specific behavior is preserved.
pub(super) fn matches(path: &str, patterns: &[String]) -> Result<bool> {
    if patterns.is_empty() {
        return Ok(false);
    }
    Python::attach(|py| {
        Ok(py
            .import("zensical.compat.privacy")?
            .getattr("matches")?
            .call1((path, patterns.to_vec()))?
            .extract()?)
    })
}
