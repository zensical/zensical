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

//! Backlink placeholder replacement over the shared HTML tokenizer.

use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;
use std::ops::Range;

use crate::compat::mkdocs::html::{Editor, Visitor};

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Placeholder attribute currently receiving a decoded value.
#[derive(Default)]
enum Attribute {
    Handler,
    Identifier,
    #[default]
    Other,
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// One standalone backlink placeholder emitted by a handler template.
struct Placeholder {
    /// Byte range of the complete start tag in the original HTML.
    range: Range<usize>,
    /// Handler that renders the backlinks.
    handler: String,
    /// Object whose backlinks are requested.
    identifier: String,
}

/// Collects backlink placeholders from shared tokenizer events.
#[derive(Default)]
pub struct Parser {
    /// Completed placeholders in document order.
    placeholders: Vec<Placeholder>,
    /// Start tag currently being assembled.
    pending: Option<Placeholder>,
    /// Attribute currently receiving a value.
    attribute: Attribute,
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Visitor for Parser {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        _editor: &mut Editor<'_>,
    ) {
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.attribute = Attribute::Other;
                self.pending = (*name == b"backlinks").then(|| Placeholder {
                    range: span.start..span.end,
                    handler: String::new(),
                    identifier: String::new(),
                });
            }
            CallbackEvent::AttributeName { name } => {
                self.attribute = match *name {
                    b"handler" => Attribute::Handler,
                    b"identifier" => Attribute::Identifier,
                    _ => Attribute::Other,
                };
            }
            CallbackEvent::AttributeValue { value } => {
                if let Some(placeholder) = &mut self.pending {
                    let target = match self.attribute {
                        Attribute::Handler => &mut placeholder.handler,
                        Attribute::Identifier => &mut placeholder.identifier,
                        Attribute::Other => return,
                    };
                    *target = String::from_utf8_lossy(value).into_owned();
                }
            }
            CallbackEvent::CloseStartTag { .. } => {
                if let Some(mut placeholder) = self.pending.take()
                    && !placeholder.handler.is_empty()
                    && !placeholder.identifier.is_empty()
                {
                    placeholder.range.end = span.end;
                    self.placeholders.push(placeholder);
                }
            }
            _ => {}
        }
    }
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Parser {
    /// Renders collected placeholders and adds their edits to the shared pass.
    ///
    /// The renderer receives decoded `(handler, identifier)` pairs in document
    /// order and must return one HTML fragment for each pair. Collecting all
    /// pairs first lets the caller cache the result for the complete page.
    pub fn render<F>(
        self, editor: &mut Editor<'_>, render: F,
    ) -> anyhow::Result<()>
    where
        F: FnOnce(&[(&str, &str)]) -> anyhow::Result<Vec<String>>,
    {
        if self.placeholders.is_empty() {
            return Ok(());
        }

        let descriptors = self
            .placeholders
            .iter()
            .map(|placeholder| {
                (
                    placeholder.handler.as_str(),
                    placeholder.identifier.as_str(),
                )
            })
            .collect::<Vec<_>>();
        let rendered = render(&descriptors)?;

        for (placeholder, rendered) in
            self.placeholders.into_iter().zip(rendered)
        {
            editor.replace(placeholder.range, rendered);
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::compat::mkdocs::html::Editor;
    use crate::compat::mkdocs::plugin::autorefs;

    use super::Parser;

    fn replace<F>(input: String, render: F) -> anyhow::Result<String>
    where
        F: FnOnce(&[(&str, &str)]) -> anyhow::Result<Vec<String>>,
    {
        let mut parser = Parser::default();
        let mut editor = Editor::scan(&input, &mut [&mut parser]);
        parser.render(&mut editor, render)?;
        Ok(editor.finish().unwrap_or(input))
    }

    #[test]
    fn shares_source_spans_with_autoref_edits() {
        // Both visitors receive the same HTML, although the autoref edit
        // changes the output length before the backlink placeholder.
        let input = concat!(
            "<autoref identifier='target'>Template reference</autoref>",
            " — <backlinks handler=python identifier=target>",
        );
        let mut references = autorefs::Parser::default();
        let mut backlinks = Parser::default();

        let mut editor =
            Editor::scan(input, &mut [&mut references, &mut backlinks]);
        backlinks
            .render(&mut editor, |descriptors| {
                assert_eq!(descriptors, &[("python", "target")]);
                Ok(vec!["<aside>Backlinks</aside>".into()])
            })
            .unwrap();
        let output = editor.finish().unwrap();

        // The shared edit pass keeps the autoref slot for later resolution
        // and replaces the backlink tag at its original source position.
        assert_eq!(
            output,
            "<!-- zensical:autoref:0 --> — <aside>Backlinks</aside>"
        );
        let (references, _) = references.finish();
        assert_eq!(references.get(0).unwrap().title(), "Template reference");
    }

    #[test]
    fn replaces_placeholders_with_varied_attribute_syntax() {
        // Templates can reorder attributes, change case and quoting, and add
        // unrelated attributes. Both supported start-tag forms are markers.
        let input = concat!(
            "<section class='original'>Éléments\n",
            r#"<backlinks identifier="package.First" handler="python" />"#,
            "\n<BACKLINKS HANDLER=python IDENTIFIER=package.Second>",
            "\n<backlinks\nhandler = 'python' data-extra='>' ",
            "identifier = 'package.Third'/>\n</section>",
        );

        let output = replace(input.into(), |descriptors| {
            assert_eq!(
                descriptors,
                &[
                    ("python", "package.First"),
                    ("python", "package.Second"),
                    ("python", "package.Third"),
                ],
            );
            Ok(vec![
                "<aside>First</aside>".into(),
                String::new(),
                "<aside>Third</aside>".into(),
            ])
        })
        .unwrap();

        // Empty backlink results remove their tag while all surrounding bytes
        // and the order of the other rendered results stay intact.
        assert_eq!(
            output,
            "<section class='original'>Éléments\n<aside>First</aside>\n\n<aside>Third</aside>\n</section>",
        );
    }

    #[test]
    fn decodes_placeholder_attributes_before_rendering() {
        let input = r#"<backlinks identifier="package.&lt;T&gt;&amp;&#xE9;" handler="pyth&#111;n"/>"#;

        let output = replace(input.into(), |descriptors| {
            assert_eq!(descriptors, &[("python", "package.<T>&é")]);
            Ok(vec!["<aside>Decoded</aside>".into()])
        })
        .unwrap();

        assert_eq!(output, "<aside>Decoded</aside>");
    }

    #[test]
    fn leaves_tag_text_in_comments_attributes_and_raw_text_untouched() {
        let input = concat!(
            r#"<!-- <backlinks identifier="comment" handler="python" /> -->"#,
            r#"<div title='<backlinks identifier="attribute" handler="python" />'>Text</div>"#,
            r#"<script>const tag = '<backlinks identifier="script" handler="python" />';</script>"#,
            r#"<style>p::after { content: '<backlinks identifier="style" handler="python" />'; }</style>"#,
            r#"<textarea><backlinks identifier="textarea" handler="python" /></textarea>"#,
            r#"<title><backlinks identifier="title" handler="python" /></title>"#,
            r#"&lt;backlinks identifier="escaped" handler="python" /&gt;"#,
        );

        let output = replace(input.into(), |_| {
            panic!("Text that resembles a tag must not invoke the renderer")
        })
        .unwrap();

        assert_eq!(output, input);
    }

    #[test]
    fn leaves_incomplete_or_unrelated_tags_untouched() {
        // Both required attributes must have values, and the start tag must
        // close before it can become a renderable placeholder.
        let input = concat!(
            "<backlinks handler=python>",
            "<backlinks identifier=missing_handler>",
            "<backlinks handler identifier=boolean_handler>",
            "<backlinks handler=python identifier=''>",
            "<backlinks-example handler=python identifier=other>",
            "<backlinks handler=python identifier='unfinished'",
        );

        let output = replace(input.into(), |_| {
            panic!("Invalid placeholders must not invoke the renderer")
        })
        .unwrap();

        assert_eq!(output, input);
    }

    #[test]
    fn propagates_backlink_rendering_errors() {
        let input = "<backlinks handler=python identifier=target>";

        let error = replace(input.into(), |_| {
            anyhow::bail!("handler failed to render backlinks")
        })
        .unwrap_err();

        assert_eq!(error.to_string(), "handler failed to render backlinks");
    }
}
