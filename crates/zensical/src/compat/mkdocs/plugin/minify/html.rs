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

//! HTML minification over the shared HTML tokenizer.

use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};
use html5gum::{Span, Tokenizer};
use std::convert::Infallible;

use crate::config::plugins::HtmlMinOptions;

mod inline;
mod serializer;
mod syntax;

use inline::InlineEditor;
use serializer::Serializer;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Minifies a complete rendered HTML document.
pub fn minify(
    input: &str, options: &HtmlMinOptions, inline_script: bool,
    inline_style: bool,
) -> String {
    let mut serializer =
        Serializer::new(input, options, inline_script, inline_style);
    {
        let mut emitter = CallbackEmitter::new(
            |event: CallbackEvent<'_>, span: Span<usize>| {
                serializer.event(event, span);
                None::<Infallible>
            },
        );
        emitter.naively_switch_states(true);
        Tokenizer::new_with_emitter(input, emitter)
            .finish()
            .expect("string input is infallible");
    }
    serializer.finish()
}

/// Minifies only inline language bodies, retaining all surrounding HTML.
pub fn minify_inline(
    input: String, inline_script: bool, inline_style: bool,
) -> String {
    let edits = {
        let mut editor = InlineEditor::new(&input, inline_script, inline_style);
        {
            let mut emitter = CallbackEmitter::new(
                |event: CallbackEvent<'_>, span: Span<usize>| {
                    editor.event(event, span);
                    None::<Infallible>
                },
            );
            emitter.naively_switch_states(true);
            Tokenizer::new_with_emitter(input.as_str(), emitter)
                .finish()
                .expect("string input is infallible");
        }
        editor.finish()
    };
    if edits.is_empty() {
        return input;
    }

    let removed = edits.iter().map(|(range, _)| range.len()).sum::<usize>();
    let inserted = edits.iter().map(|(_, value)| value.len()).sum::<usize>();
    let mut output = String::with_capacity(input.len() - removed + inserted);
    let mut cursor = 0;
    for (range, value) in edits {
        output.push_str(&input[cursor..range.start]);
        output.push_str(&value);
        cursor = range.end;
    }
    output.push_str(&input[cursor..]);
    output
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::config::plugins::HtmlMinOptions;

    use super::{minify, minify_inline};

    fn options() -> HtmlMinOptions {
        HtmlMinOptions {
            remove_comments: false,
            remove_empty_space: false,
            remove_all_empty_space: false,
            reduce_empty_attributes: true,
            reduce_boolean_attributes: false,
            remove_optional_attribute_quotes: true,
            convert_charrefs: true,
            keep_pre: false,
            pre_tags: vec!["pre".into(), "textarea".into()],
            pre_attr: "pre".into(),
        }
    }

    #[test]
    fn matches_default_htmlmin_behavior() {
        let input = r#"<!doctype html>
            <html><head><!-- keep --><title>  A   title  </title></head>
            <body><input disabled="disabled" class="">
            <p> A   <strong> B </strong> </p>
            <pre>  exact
 value </pre><section pre>  exact  </section></body></html>"#;
        let output = minify(input, &options(), false, false);
        assert_eq!(
            output,
            "<!doctype html><html><head><!-- keep --><title>A title</title></head> \
<body><input disabled=disabled class> <p> A <strong> B </strong> </p> \
<pre>  exact\n value </pre><section>  exact  </section></body></html>"
        );
    }

    #[test]
    fn supports_comment_and_whitespace_modes() {
        let mut options = options();
        options.remove_comments = true;
        options.remove_empty_space = true;
        let output = minify(
            "<body>\n<div>A</div> \t <div>B</div><!-- x --><!--! y --></body>",
            &options,
            false,
            false,
        );
        assert_eq!(output, "<body><div>A</div> <div>B</div><!-- y --></body>");

        options.remove_all_empty_space = true;
        let output =
            minify("<body> <i>A</i> <i>B</i> </body>", &options, false, false);
        assert_eq!(output, "<body><i>A</i><i>B</i></body>");
    }

    #[test]
    fn supports_attribute_options() {
        let mut options = options();
        options.reduce_boolean_attributes = true;
        options.remove_optional_attribute_quotes = false;
        options.convert_charrefs = false;
        let output = minify(
            r#"<input disabled="disabled" title="a&amp;b" alt="">"#,
            &options,
            false,
            false,
        );
        assert_eq!(output, r#"<input disabled title="a&amp;b" alt>"#);
    }

    #[test]
    fn supports_custom_preservation_markers() {
        let mut options = options();
        options.pre_attr = "custom".into();
        options.pre_tags.push("code".into());
        let output = minify(
            "<div custom custom-title='a&amp;b'>  exact  </div><code>  code  </code>",
            &options,
            false,
            false,
        );
        assert_eq!(
            output,
            "<div title=a&amp;b>  exact  </div><code>  code  </code>"
        );
    }

    #[test]
    fn supports_preservation_configuration() {
        let mut options = options();
        options.keep_pre = true;
        assert_eq!(
            minify(
                r#"<strong pre style="">  exact  </strong>"#,
                &options,
                false,
                false,
            ),
            "<strong pre style>  exact  </strong>"
        );

        options.pre_tags.clear();
        assert_eq!(
            minify("<pre>  compact  </pre>", &options, false, false),
            "<pre> compact </pre>"
        );
    }

    #[test]
    fn controls_character_reference_conversion() {
        let input =
            r#"<input value="&#34;&#39;&#39;&#39;&lt;&#46;&pi;&gt; &#34;">"#;
        let converted = minify(input, &options(), false, false);
        assert!(converted.contains(".π>"));
        assert!(!converted.contains("&#46;"));

        let mut options = options();
        options.convert_charrefs = false;
        assert_eq!(minify(input, &options, false, false), input);
    }

    #[test]
    fn preserves_scripts_styles_and_conditional_comments_by_default() {
        let mut options = options();
        options.remove_comments = true;
        let input = "<script>  const x = 1;  </script><style>  .a { color: red }  </style><!--[if IE]>x<![endif]-->";
        assert_eq!(minify(input, &options, false, false), input);
    }

    #[test]
    fn preserves_foreign_element_and_attribute_case() {
        let input = r#"<svg viewBox="0 0 10 10"><linearGradient gradientUnits="userSpaceOnUse"></linearGradient></svg>"#;
        assert_eq!(
            minify(input, &options(), false, false),
            r#"<svg viewBox="0 0 10 10"><linearGradient gradientUnits=userSpaceOnUse></linearGradient></svg>"#
        );
    }

    #[test]
    fn minifies_supported_inline_languages() {
        let input = r#"<script> const value = 1 + 2; </script>
<script type="application/ld+json"> { "value": 3 } </script>
<style> @media screen and (min-width: 45em) { .a { color: red; } } </style>"#;
        let output = minify(input, &options(), true, true);
        assert!(output.contains("<script>const value=1+2;</script>"));
        assert!(output.contains(
            "<script type=application/ld+json> { \"value\": 3 } </script>"
        ));
        assert!(output.contains("screen and (min-width:45em)"));
    }

    #[test]
    fn minifies_module_scripts_as_modules() {
        let input = "<script type=module> await Promise.resolve(1); </script>";
        assert_eq!(
            minify(input, &options(), true, false),
            "<script type=module>await Promise.resolve(1);</script>"
        );
    }

    #[test]
    fn inline_only_retains_surrounding_html() {
        let input =
            "<div  class=\"x\">  A  </div><script> const x = 1; </script>";
        let output = minify_inline(input.into(), true, false);
        assert!(output.starts_with("<div  class=\"x\">  A  </div>"));
        assert!(output.ends_with("<script>const x=1;</script>"));
    }

    #[test]
    fn inline_minification_never_expands_a_body() {
        let input = "<script>x()</script><style>a{color:red}</style>";
        let owned = input.to_string();
        let pointer = owned.as_ptr();
        let output = minify_inline(owned, true, true);
        assert_eq!(output, input);
        assert_eq!(output.as_ptr(), pointer);
        assert_eq!(minify(input, &options(), true, true), input);
    }

    #[test]
    fn invalid_inline_languages_are_preserved() {
        let input = "<script> const = ; </script><style>.a { color: ;</style>";
        assert_eq!(minify_inline(input.into(), true, true), input);
    }
}
