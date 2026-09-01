// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! Replaceable awesome-nav pattern matching boundary.

use anyhow::{bail, Result};
use globset::{GlobBuilder, GlobMatcher};

/// One compiled POSIX navigation pattern.
#[derive(Clone, Debug)]
pub struct Pattern {
    source: String,
    directory_only: bool,
    matcher: GlobMatcher,
}

impl Pattern {
    /// Compiles one pattern with path separators kept significant.
    pub fn compile(source: &str) -> Result<Self> {
        if let Some(operator) = extglob(source) {
            bail!(
                "unsupported awesome-nav extglob operator '{operator}(' in pattern {source:?}"
            )
        }
        let matcher = GlobBuilder::new(source)
            .literal_separator(true)
            .build()?
            .compile_matcher();
        Ok(Self {
            source: source.into(),
            directory_only: source.ends_with('/'),
            matcher,
        })
    }

    /// Returns whether a canonical page or directory candidate matches.
    pub fn matches(&self, candidate: &str) -> bool {
        if self.directory_only && !candidate.ends_with('/') {
            return false;
        }
        self.matcher.is_match(candidate)
            || candidate
                .strip_suffix('/')
                .is_some_and(|candidate| self.matcher.is_match(candidate))
    }

    /// Returns the original expression for diagnostics.
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Finds the first unescaped extglob operator.
fn extglob(source: &str) -> Option<char> {
    let mut escaped = false;
    let mut chars = source.chars().peekable();
    while let Some(character) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if matches!(character, '@' | '?' | '*' | '+' | '!')
            && chars.peek() == Some(&'(')
        {
            return Some(character);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::Pattern;

    #[test]
    fn matches_component_sensitive_globs() {
        let pattern = Pattern::compile("guide/**/*.md").unwrap();
        assert!(pattern.matches("guide/start.md"));
        assert!(pattern.matches("guide/api/type.md"));
        assert!(!pattern.matches("other/guide/start.md"));

        let pattern = Pattern::compile("{index,README}.md").unwrap();
        assert!(pattern.matches("index.md"));
        assert!(pattern.matches("README.md"));

        let pattern = Pattern::compile("*").unwrap();
        assert!(pattern.matches("guide/"));
        assert!(!pattern.matches("guide/start.md"));

        let pattern = Pattern::compile("*/").unwrap();
        assert!(pattern.matches("guide/"));
        assert!(!pattern.matches("guide.md"));
    }

    #[test]
    fn rejects_every_extglob_operator() {
        for operator in ['@', '?', '*', '+', '!'] {
            let error = Pattern::compile(&format!("{operator}(a|b).md"))
                .unwrap_err()
                .to_string();
            assert!(error.contains("unsupported awesome-nav extglob"));
        }
    }
}
