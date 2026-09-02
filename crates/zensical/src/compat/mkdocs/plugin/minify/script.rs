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

//! JavaScript minification.

use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions, CommentOptions};
use oxc_parser::Parser;
use oxc_span::SourceType;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Minifies JavaScript while retaining the original source on parse errors.
pub fn minify(source: &str, module: bool) -> Option<String> {
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

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::minify;

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
