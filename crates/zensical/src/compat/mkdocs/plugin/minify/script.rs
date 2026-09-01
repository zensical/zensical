// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! JavaScript minification.

use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions, CommentOptions};
use oxc_parser::Parser;
use oxc_span::SourceType;

/// Minifies JavaScript while retaining the original source on parse errors.
pub(super) fn minify(source: &str, module: bool) -> Option<String> {
    let allocator = Allocator::default();
    let source_type = if module {
        SourceType::mjs()
    } else {
        SourceType::unambiguous()
    };
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return None;
    }

    let output = Codegen::new()
        .with_options(CodegenOptions {
            minify: true,
            comments: CommentOptions {
                normal: false,
                jsdoc: false,
                annotation: true,
                ..CommentOptions::default()
            },
            ..CodegenOptions::default()
        })
        .build(&parsed.program);
    Some(output.code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minifies_modern_javascript() {
        let source = r"
            /*! license */
            const value = { nested: { answer: 42 } };
            console.log(value?.nested?.answer ?? `missing ${value}`);
        ";
        let output = minify(source, false).expect("valid JavaScript");
        assert!(output.contains("/*! license */"));
        assert!(output.contains("value?.nested?.answer??"));
        assert!(!output.contains("            "));
    }

    #[test]
    fn rejects_invalid_javascript_without_rewriting_it() {
        assert!(minify("const = ;", false).is_none());
    }

    #[test]
    fn supports_module_only_syntax() {
        assert_eq!(
            minify("await Promise.resolve(1);", true).as_deref(),
            Some("await Promise.resolve(1);")
        );
    }
}
