// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! CSS minification.

use swc_common::input::StringInput;
use swc_common::{BytePos, Globals, GLOBALS};
use swc_css::ast::Stylesheet;
use swc_css::codegen::writer::basic::{BasicCssWriter, BasicCssWriterConfig};
use swc_css::codegen::{CodeGenerator, CodegenConfig, Emit};
use swc_css::minifier::options::MinifyOptions;
use swc_css::parser::parser::ParserConfig;

/// Minifies CSS while retaining the original source on parse errors.
pub(super) fn minify(source: &str) -> Option<String> {
    GLOBALS.set(&Globals::default(), || {
        let legal = legal_comments(source);
        let end = u32::try_from(source.len()).ok()?;
        let input = StringInput::new(source, BytePos(0), BytePos(end));
        let mut errors = Vec::new();
        let mut stylesheet: Stylesheet = swc_css::parser::parse_string_input(
            input,
            None,
            ParserConfig::default(),
            &mut errors,
        )
        .ok()?;
        if !errors.is_empty() {
            return None;
        }

        swc_css::minifier::minify(&mut stylesheet, MinifyOptions::default());

        let mut output = String::new();
        let writer = BasicCssWriter::new(
            &mut output,
            None,
            BasicCssWriterConfig::default(),
        );
        CodeGenerator::new(writer, CodegenConfig { minify: true })
            .emit(&stylesheet)
            .ok()?;
        if !legal.is_empty() {
            let mut preserved = String::with_capacity(
                legal.iter().map(|comment| comment.len()).sum::<usize>()
                    + output.len(),
            );
            for comment in legal {
                if !output.contains(comment) {
                    preserved.push_str(comment);
                }
            }
            preserved.push_str(&output);
            output = preserved;
        }
        Some(output)
    })
}

/// Returns legal comments that minifiers conventionally preserve.
fn legal_comments(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut comments = Vec::new();
    let mut offset = 0;
    let mut quote = None;
    while offset < bytes.len() {
        match (quote, bytes[offset]) {
            (Some(_), b'\\') => offset += 2,
            (Some(current), byte) if byte == current => {
                quote = None;
                offset += 1;
            }
            (None, byte @ (b'\'' | b'"')) => {
                quote = Some(byte);
                offset += 1;
            }
            (None, b'/') if bytes.get(offset + 1) == Some(&b'*') => {
                let start = offset;
                offset += 2;
                while offset + 1 < bytes.len()
                    && bytes[offset..offset + 2] != *b"*/"
                {
                    offset += 1;
                }
                offset = (offset + 2).min(bytes.len());
                if bytes.get(start + 2) == Some(&b'!') {
                    comments.push(&source[start..offset]);
                }
            }
            (Some(_) | None, _) => offset += 1,
        }
    }
    comments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minifies_modern_css() {
        let source = r"
            /*! license */
            @media screen and (min-width: 45em) {
                .card:has(> img) { color: rgb(255, 0, 0); }
            }
        ";
        let output = minify(source).expect("valid CSS");
        assert!(output.contains("/*! license */"));
        assert!(output.contains("screen and (min-width:45em)"));
        assert!(!output.contains("screen and("));
    }

    #[test]
    fn rejects_invalid_css_without_rewriting_it() {
        assert!(minify(".a { color: ;").is_none());
    }

    #[test]
    fn ignores_legal_comment_markers_inside_strings() {
        assert!(legal_comments(r#".a { content: \"/*! not legal */\" }"#)
            .is_empty());
    }
}
