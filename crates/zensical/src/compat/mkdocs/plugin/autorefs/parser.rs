// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! Streaming extraction of MkDocs-compatible autoref placeholders.

use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;
use serde::{Deserialize, Serialize};

use crate::compat::mkdocs::html::{Editor, Visitor};

// ----------------------------------------------------------------------------
// Constants
// ----------------------------------------------------------------------------

/// Prefix of an internal page-local autoref slot.
pub(super) const SLOT_PREFIX: &str = "<!-- zensical:autoref:";

/// Suffix of an internal page-local autoref slot.
pub(super) const SLOT_SUFFIX: &str = " -->";

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Autoref placeholders extracted from one rendered Markdown page.
#[derive(
    Clone, Debug, Default, Deserialize, Hash, PartialEq, Eq, Serialize,
)]
pub(crate) struct References {
    /// References in document order; their positions are stable slot IDs.
    references: Vec<Reference>,
}

/// One unresolved autoref placeholder.
#[derive(Clone, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
pub(super) struct Reference {
    /// Attributes in their source order.
    attributes: Vec<Attribute>,
    /// Raw inner HTML used as link content.
    title: String,
}

/// One parsed HTML attribute.
#[derive(Clone, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
pub(super) struct Attribute {
    /// Decoded attribute name.
    name: String,
    /// Decoded attribute value, or an empty string for boolean attributes.
    value: String,
}

/// Page-local autoref visitor.
#[derive(Default)]
pub(crate) struct Parser {
    /// Autoref start tag currently being assembled.
    pending: Option<Pending>,
    /// Completed page-local references.
    references: Vec<Reference>,
}

/// Autoref element currently being assembled.
struct Pending {
    /// Start of the complete element.
    start: usize,
    /// Start of its raw inner HTML after the start tag closes.
    content: Option<usize>,
    /// Parsed start-tag attributes.
    attributes: Vec<Attribute>,
    /// Attribute currently receiving a value.
    attribute: Option<usize>,
    /// Nested autoref elements, which are retained inside the outer title.
    nested: usize,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl References {
    /// Returns the reference at a page-local slot index.
    pub(super) fn get(&self, index: usize) -> Option<&Reference> {
        self.references.get(index)
    }

    /// Returns whether no page-local autorefs were extracted.
    pub(crate) fn is_empty(&self) -> bool {
        self.references.is_empty()
    }
}

// ----------------------------------------------------------------------------

impl Reference {
    /// Returns the last value for an attribute, matching the old map parser.
    pub(super) fn get(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .rev()
            .find(|attribute| attribute.name == name)
            .map(|attribute| attribute.value.as_str())
    }

    /// Returns whether an attribute is present.
    pub(super) fn contains(&self, name: &str) -> bool {
        self.attributes
            .iter()
            .any(|attribute| attribute.name == name)
    }

    /// Iterates over parsed attributes in source order.
    pub(super) fn attributes(&self) -> impl Iterator<Item = (&str, &str)> {
        self.attributes.iter().map(|attribute| {
            (attribute.name.as_str(), attribute.value.as_str())
        })
    }

    /// Returns raw inner HTML.
    pub(super) fn title(&self) -> &str {
        &self.title
    }
}

// ----------------------------------------------------------------------------

impl Parser {
    /// Converts the visitor into cached page-local references.
    pub(crate) fn finish(self) -> References {
        References { references: self.references }
    }

    /// Handles one tokenizer event.
    fn handle(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        editor: &mut Editor<'_>,
    ) {
        match event {
            CallbackEvent::OpenStartTag { name } if *name == b"autoref" => {
                if let Some(pending) = &mut self.pending {
                    pending.nested += 1;
                } else {
                    self.pending = Some(Pending {
                        start: span.start,
                        content: None,
                        attributes: Vec::new(),
                        attribute: None,
                        nested: 0,
                    });
                }
            }
            CallbackEvent::AttributeName { name } => {
                if let Some(pending) = &mut self.pending
                    && pending.content.is_none()
                {
                    pending.attributes.push(Attribute {
                        name: String::from_utf8_lossy(name).into_owned(),
                        value: String::new(),
                    });
                    pending.attribute = Some(pending.attributes.len() - 1);
                }
            }
            CallbackEvent::AttributeValue { value } => {
                if let Some(pending) = &mut self.pending
                    && pending.content.is_none()
                    && let Some(index) = pending.attribute
                {
                    pending.attributes[index].value =
                        String::from_utf8_lossy(value).into_owned();
                }
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                if let Some(pending) = &mut self.pending
                    && pending.content.is_none()
                {
                    if *self_closing {
                        self.pending = None;
                    } else {
                        pending.content = Some(span.end);
                    }
                }
            }
            CallbackEvent::EndTag { name } if *name == b"autoref" => {
                let Some(mut pending) = self.pending.take() else {
                    return;
                };
                if pending.nested > 0 {
                    pending.nested -= 1;
                    self.pending = Some(pending);
                    return;
                }

                let Some(content) = pending.content else {
                    return;
                };
                let index = self.references.len();
                self.references.push(Reference {
                    attributes: pending.attributes,
                    title: editor.text(content..span.start).to_string(),
                });
                editor.replace(pending.start..span.end, slot(index));
            }
            _ => {}
        }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Visitor for Parser {
    fn visit(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        editor: &mut Editor<'_>,
    ) {
        self.handle(event, span, editor);
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Creates the stable marker for a page-local autoref slot.
fn slot(index: usize) -> String {
    format!("{SLOT_PREFIX}{index}{SLOT_SUFFIX}")
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::mkdocs::html;

    #[test]
    fn extracts_attributes_and_raw_inner_html() {
        let input = concat!(
            "<p>Before ",
            "<autoref\n identifier='Foo &amp; Bar' optional>",
            "<code>Foo &amp; Bar</code>",
            "</autoref> after</p>",
        );
        let mut parser = Parser::default();
        let output = html::scan(input, &mut [&mut parser]).expect("slot edit");
        let references = parser.finish();
        let reference = references.get(0).expect("reference");

        assert_eq!(
            output,
            format!("<p>Before {SLOT_PREFIX}0{SLOT_SUFFIX} after</p>")
        );
        assert_eq!(reference.get("identifier"), Some("Foo & Bar"));
        assert!(reference.contains("optional"));
        assert_eq!(reference.title(), "<code>Foo &amp; Bar</code>");
    }

    #[test]
    fn leaves_unclosed_and_self_closing_elements_untouched() {
        for input in ["<autoref identifier=x>Title", "<autoref identifier=x/>"]
        {
            let mut parser = Parser::default();
            assert_eq!(html::scan(input, &mut [&mut parser]), None);
            assert!(parser.finish().is_empty());
        }
    }
}
