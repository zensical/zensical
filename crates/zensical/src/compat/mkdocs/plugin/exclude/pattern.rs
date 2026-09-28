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

//! Glob matching for source file exclusion.

use anyhow::{Context, Result};
use globset::{GlobBuilder, GlobMatcher};

/// One compiled, case-sensitive source path pattern.
#[derive(Debug)]
pub struct Pattern {
    matcher: GlobMatcher,
}

impl Pattern {
    /// Compiles a glob whose wildcards can cross directory separators.
    pub fn compile(source: &str) -> Result<Self> {
        let matcher = GlobBuilder::new(source)
            .literal_separator(false)
            .backslash_escape(true)
            .build()
            .with_context(|| format!("invalid exclude glob {source:?}"))?
            .compile_matcher();
        Ok(Self { matcher })
    }

    /// Matches the complete source path relative to its documentation root.
    pub fn matches(&self, path: &str) -> bool {
        self.matcher.is_match(path)
    }
}

#[cfg(test)]
mod tests {
    use super::Pattern;

    #[test]
    fn wildcards_cross_directories_and_preserve_case() {
        let pattern = Pattern::compile("*.tmp").unwrap();

        assert!(pattern.matches("file.tmp"));
        assert!(pattern.matches("guide/assets/file.tmp"));
        assert!(!pattern.matches("guide/file.TMP"));
        assert!(!pattern.matches("guide/file.tmp.md"));

        // Patterns remain rooted even though wildcards can span separators.
        let pattern = Pattern::compile("drafts/*").unwrap();

        assert!(pattern.matches("drafts/page.md"));
        assert!(pattern.matches("drafts/nested/page.md"));
        assert!(!pattern.matches("guide/drafts/page.md"));
    }

    #[test]
    fn supports_character_classes_and_single_character_wildcards() {
        let pattern = Pattern::compile("page[!0-3]?.md").unwrap();

        assert!(pattern.matches("page4a.md"));
        assert!(!pattern.matches("page2a.md"));
        assert!(!pattern.matches("page4ab.md"));
    }

    #[test]
    fn uses_globset_recursive_patterns_and_alternatives() {
        let pattern = Pattern::compile("drafts/**/{old,new}.md").unwrap();

        assert!(pattern.matches("drafts/old.md"));
        assert!(pattern.matches("drafts/nested/new.md"));
        assert!(!pattern.matches("drafts/current.md"));
    }

    #[test]
    fn backslashes_escape_glob_characters() {
        let pattern = Pattern::compile(r"file\[draft\].md").unwrap();

        assert!(pattern.matches("file[draft].md"));
        assert!(!pattern.matches("filed.md"));
    }

    #[test]
    fn invalid_patterns_identify_the_option_and_expression() {
        let error = Pattern::compile("[bad").unwrap_err().to_string();

        assert!(error.contains("invalid exclude glob"));
        assert!(error.contains("[bad"));
    }
}
