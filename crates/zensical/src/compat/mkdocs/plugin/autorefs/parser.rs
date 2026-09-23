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

//! Streaming extraction of MkDocs-compatible autorefs facts.

use ahash::{HashMap, HashSet};
use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;
use serde::{Deserialize, Serialize};
use std::ops::Range;
use std::{mem, rc::Rc, sync::Arc};

use crate::compat::mkdocs::html::{Editor, Visitor};

use super::Facts;

/// Internal marker for references previously visible to the tree processor.
const BACKLINK_MARKER: &str = "data-zensical-autoref";

/// Prefix of an internal nested-Markdown context marker.
const CONTEXT_START: &[u8] = b"zensical:autoref-context:start";

/// Complete internal marker that closes a nested-Markdown context.
const CONTEXT_END: &[u8] = b"zensical:autoref-context:end";

/// Prefix of an internal page-local autoref slot.
pub const SLOT_PREFIX: &str = "<!-- zensical:autoref:";

/// Suffix of an internal page-local autoref slot.
pub const SLOT_SUFFIX: &str = " -->";

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// HTML tags relevant to autorefs processing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    Anchor,
    Autoref,
    Heading(u8),
    Paragraph,
    Void,
    Other(usize),
}

/// Attribute currently receiving a value.
#[derive(Clone, Copy, Default)]
enum StartAttribute {
    Id,
    Href,
    #[default]
    Other,
}

/// Anchor-scanner behavior of one open element.
enum AnchorElement {
    /// Descendants of headings and anchors are not visited by the old scanner.
    Ignored,
    /// An anchor that may interrupt or extend an alias chain.
    Anchor {
        scope: usize,
        has_text: bool,
        has_child: bool,
        flush: bool,
    },
    /// A heading that consumes the pending anchors in its scope as aliases.
    Heading { scope: usize, aliases: Vec<String> },
    /// A paragraph reuses its parent's pending-anchor collection.
    Paragraph {
        scope: usize,
        previous_heading: Option<Rc<str>>,
    },
    /// Other elements are scanned in a separate pending-anchor context.
    Other { scope: usize },
}

/// Deferred inspection of text following a direct child.
#[derive(Clone, Copy)]
enum Tail {
    Anchor,
    Paragraph,
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Autoref placeholders extracted from one rendered Markdown page.
#[derive(
    Clone, Debug, Default, Deserialize, Hash, PartialEq, Eq, Serialize,
)]
pub struct References {
    /// References in document order; their positions are stable slot IDs.
    references: Vec<Reference>,
}

/// One unresolved autoref placeholder.
#[derive(Clone, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
pub struct Reference {
    /// Attributes in their source order.
    attributes: Vec<Attribute>,
    /// Raw inner HTML used as link content.
    title: Box<str>,
}

/// One parsed HTML attribute.
#[derive(Clone, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
struct Attribute {
    /// Decoded attribute name.
    name: Arc<str>,
    /// Decoded attribute value, or an empty string for boolean attributes.
    value: Box<str>,
}

/// Page-local autorefs visitor.
#[derive(Default)]
pub struct Parser {
    /// URL of the page currently being scanned.
    page_url: String,
    /// Whether references should receive inferred backlink metadata.
    record_backlinks: bool,
    /// Whether to collect Markdown headings and anchor aliases.
    collect_registrations: bool,
    /// Start tag currently being assembled.
    start: Option<StartTag>,
    /// Open elements used by the former tree-processor semantics.
    context: Vec<Element>,
    /// Autoref element currently being extracted.
    pending: Option<PendingReference>,
    /// Completed page-local references.
    references: Vec<Reference>,
    /// Source ranges of template reference titles that can contain other edits.
    title_ranges: Vec<Range<usize>>,
    /// Heading and Markdown-anchor registrations.
    facts: Facts,
    /// Anchor scanner contexts; the first entry represents the document root.
    anchor_scopes: Vec<AnchorScope>,
    /// Number of open headings whose descendants the heading scanner skips.
    heading_skip: usize,
    /// Number of open heading/autoref elements skipped by backlink inference.
    backlink_skip: usize,
    /// Most recent heading ID in document order.
    last_heading_id: Option<String>,
    /// Heading state saved at nested Markdown conversion boundaries.
    backlink_contexts: Vec<Option<String>>,
    /// Interned names for tags without autorefs-specific behavior.
    other_tags: HashMap<Box<[u8]>, usize>,
    /// Shared names retained by autoref attributes.
    attribute_names: HashSet<Arc<str>>,
}

/// Start tag assembled from html5gum's callback events.
struct StartTag {
    /// Parsed tag.
    tag: Tag,
    /// Start byte of the complete element.
    start: usize,
    /// Attributes retained for an autoref element.
    attributes: Vec<Attribute>,
    /// Index of the autoref attribute currently receiving a value.
    reference_attribute: Option<usize>,
    /// Attribute relevant to heading and anchor scanning.
    attribute: StartAttribute,
    /// Decoded element ID.
    id: Option<String>,
    /// Whether an anchor has a non-empty target.
    href_nonempty: bool,
    /// Whether a heading opts out of local inventory registration.
    skip_inventory: bool,
    /// Whether this autoref was visible to the former backlink treeprocessor.
    backlink_marker: bool,
}

/// Open element state shared by the former tree processors.
struct Element {
    /// Parsed tag, retained to reject malformed mismatched end tags.
    tag: Tag,
    /// Anchor-scanner state for this element.
    anchor: AnchorElement,
    /// Heading registration and direct-text state.
    heading: Option<Heading>,
    /// Whether this element increments `heading_skip`.
    heading_boundary: bool,
    /// Whether this element increments `backlink_skip`.
    backlink_boundary: bool,
}

/// Heading whose immediate text and ID are being collected.
struct Heading {
    /// Decoded heading ID.
    id: Option<String>,
    /// Immediate ElementTree-style text, excluding child element content.
    title: String,
    /// Whether the heading has received any immediate text.
    has_text: bool,
    /// Whether a child element has started.
    has_child: bool,
    /// Whether this heading opts out of local inventory registration.
    skip_inventory: bool,
}

/// Autoref element currently being extracted.
struct PendingReference {
    /// Start of the complete element.
    start: usize,
    /// Start of its raw inner HTML after the start tag closes.
    content: usize,
    /// Parsed start-tag attributes.
    attributes: Vec<Attribute>,
    /// Nested autoref elements, which are retained inside the outer title.
    nested: usize,
}

/// One recursive context of the former Markdown-anchor scanner.
#[derive(Default)]
struct AnchorScope {
    /// Anchors waiting to become aliases of a following heading.
    pending: Vec<String>,
    /// Immediate text of the most recent heading in this scope.
    last_heading: Option<Rc<str>>,
    /// Child whose following text still needs to be inspected.
    tail: Option<Tail>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl References {
    /// Returns the reference at a page-local slot index.
    pub fn get(&self, index: usize) -> Option<&Reference> {
        self.references.get(index)
    }

    /// Returns whether no page-local autorefs were extracted.
    pub fn is_empty(&self) -> bool {
        self.references.is_empty()
    }

    /// Iterates over page-local auto-references in document order.
    pub fn iter(&self) -> std::slice::Iter<'_, Reference> {
        self.references.iter()
    }
}

// ----------------------------------------------------------------------------

impl Reference {
    /// Returns the last value for an attribute, matching the old map parser.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .rev()
            .find(|attribute| attribute.name.as_ref() == name)
            .map(|attribute| attribute.value.as_ref())
    }

    /// Returns whether an attribute is present.
    pub fn contains(&self, name: &str) -> bool {
        self.attributes
            .iter()
            .any(|attribute| attribute.name.as_ref() == name)
    }

    /// Iterates over parsed attributes in source order.
    pub fn attributes(&self) -> impl Iterator<Item = (&str, &str)> {
        self.attributes.iter().map(|attribute| {
            (attribute.name.as_ref(), attribute.value.as_ref())
        })
    }

    /// Returns raw inner HTML.
    pub fn title(&self) -> &str {
        &self.title
    }
}

// ----------------------------------------------------------------------------

impl Parser {
    /// Creates a page-local visitor seeded with Markdown registrations.
    pub fn with_facts(
        page_url: impl Into<String>, record_backlinks: bool, facts: Facts,
    ) -> Self {
        Self {
            page_url: page_url.into(),
            record_backlinks,
            collect_registrations: true,
            facts,
            anchor_scopes: vec![AnchorScope::default()],
            ..Self::default()
        }
    }

    /// Converts the visitor into cached page-local references and registrations.
    pub fn finish(mut self) -> (References, Facts) {
        if self.collect_registrations {
            self.flush_scope(0, None, false);
        }
        (References { references: self.references }, self.facts)
    }

    /// Retains nested replacements inside template reference titles.
    pub fn apply_title_edits(&mut self, editor: &mut Editor<'_>) {
        for (reference, range) in
            self.references.iter_mut().zip(&self.title_ranges)
        {
            if let Some(title) = editor.render_range(range.clone()) {
                reference.title = title.into_boxed_str();
            }
        }
    }

    /// Classifies a tag, interning an unknown name only on first occurrence.
    fn tag(&mut self, name: &[u8]) -> Tag {
        if let Some(tag) = Tag::from_bytes(name) {
            return tag;
        }
        if let Some(identifier) = self.other_tags.get(name) {
            return Tag::Other(*identifier);
        }
        let identifier = self.other_tags.len();
        self.other_tags.insert(name.into(), identifier);
        Tag::Other(identifier)
    }

    /// Handles one tokenizer event.
    fn handle(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        editor: &mut Editor<'_>,
    ) {
        match event {
            CallbackEvent::OpenStartTag { name } => {
                if self.collect_registrations {
                    self.open_start_tag(name, span.start);
                } else {
                    self.start = (*name == b"autoref")
                        .then(|| StartTag::new(Tag::Autoref, span.start));
                }
            }
            CallbackEvent::AttributeName { name } => {
                if let Some(start) = &mut self.start {
                    start.attribute_name(name, &mut self.attribute_names);
                }
            }
            CallbackEvent::AttributeValue { value } => {
                if let Some(start) = &mut self.start {
                    start.attribute_value(value);
                }
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                if let Some(start) = self.start.take() {
                    if self.collect_registrations {
                        self.close_start_tag(start, span.end, *self_closing);
                    } else {
                        self.open_reference(
                            start.start,
                            span.end,
                            start.attributes,
                            *self_closing,
                            false,
                        );
                    }
                }
            }
            CallbackEvent::EndTag { name } => {
                if !self.collect_registrations {
                    if *name == b"autoref" {
                        self.close_reference(span, editor);
                    }
                    return;
                }
                let tag = self.tag(name);
                if self.context.last().is_none_or(|element| element.tag != tag)
                {
                    return;
                }
                if tag == Tag::Autoref {
                    self.close_reference(span, editor);
                }
                self.close_element();
            }
            CallbackEvent::String { value } if self.collect_registrations => {
                self.text(value);
            }
            CallbackEvent::Comment { value } if self.record_backlinks => {
                self.context_marker(value, span, editor);
            }
            _ => {}
        }
    }

    /// Starts assembling one HTML start tag.
    fn open_start_tag(&mut self, name: &[u8], start: usize) {
        let tag = self.tag(name);
        self.clear_tail();
        if let Some(parent) = self.context.last_mut() {
            if let Some(heading) = &mut parent.heading {
                heading.has_child = true;
            }
            if let AnchorElement::Anchor { has_child, .. } = &mut parent.anchor
            {
                *has_child = true;
            }
        }
        self.start = Some(StartTag::new(tag, start));
    }

    /// Applies a fully assembled start tag.
    fn close_start_tag(
        &mut self, mut start: StartTag, content: usize, self_closing: bool,
    ) {
        let is_heading = matches!(start.tag, Tag::Heading(_));
        let heading_boundary = is_heading;
        let collect_heading = is_heading && self.heading_skip == 0;
        if heading_boundary {
            self.heading_skip += 1;
        }

        let backlink_boundary = if self.record_backlinks
            && self.backlink_skip == 0
            && matches!(start.tag, Tag::Heading(_) | Tag::Autoref)
        {
            if is_heading {
                self.last_heading_id.clone_from(&start.id);
            }
            self.backlink_skip += 1;
            true
        } else {
            false
        };

        if start.tag == Tag::Autoref {
            self.open_reference(
                start.start,
                content,
                start.attributes,
                self_closing,
                self.record_backlinks
                    && backlink_boundary
                    && start.backlink_marker,
            );
        }

        let anchor = self.open_anchor_element(
            &start.tag,
            start.id.as_deref(),
            start.href_nonempty,
        );
        let heading = collect_heading.then(|| Heading {
            id: start.id.take(),
            title: String::new(),
            has_text: false,
            has_child: false,
            skip_inventory: start.skip_inventory,
        });
        let is_void = start.tag.is_void();
        self.context.push(Element {
            tag: start.tag,
            anchor,
            heading,
            heading_boundary,
            backlink_boundary,
        });

        if self_closing || is_void {
            self.close_element();
        }
    }

    /// Consumes one internal nested-Markdown context marker.
    fn context_marker(
        &mut self, value: &[u8], span: Span<usize>, editor: &mut Editor<'_>,
    ) {
        let (start, anchor) = if value == CONTEXT_START {
            (true, None)
        } else if let Some(value) = value
            .strip_prefix(CONTEXT_START)
            .and_then(|value| value.strip_prefix(b":"))
        {
            let Some(anchor) = decode_hex(value) else {
                return;
            };
            (true, Some(anchor))
        } else if value == CONTEXT_END {
            (false, None)
        } else {
            return;
        };

        self.clear_tail();
        if start {
            self.backlink_contexts.push(self.last_heading_id.clone());
            self.last_heading_id = anchor;
            let scope = self.anchor_scopes.len() - 1;
            self.flush_scope(scope, None, false);
            self.anchor_scopes.push(AnchorScope::default());
            editor.replace(span.start..span.end, Box::default());
        } else {
            if self.anchor_scopes.len() > 1 {
                let scope = self.anchor_scopes.len() - 1;
                self.flush_scope(scope, None, false);
                self.anchor_scopes.pop();
            }
            self.last_heading_id = self.backlink_contexts.pop().flatten();

            // Markdown joins root children with a newline. The ending comment
            // is an internal child, so consume its otherwise observable
            // separator when removing it.
            let mut start = span.start;
            let prefix = editor.text(0..start).as_bytes();
            while start > 0
                && matches!(
                    prefix[start - 1],
                    b'\t' | b'\n' | 0x0c | b'\r' | b' '
                )
            {
                start -= 1;
            }
            editor.replace(start..span.end, Box::default());
        }
    }

    /// Starts extracting an autoref element.
    fn open_reference(
        &mut self, start: usize, content: usize, attributes: Vec<Attribute>,
        self_closing: bool, enhance: bool,
    ) {
        if let Some(pending) = &mut self.pending {
            if !self_closing {
                pending.nested += 1;
            }
            return;
        }
        if self_closing {
            return;
        }

        let mut attributes = attributes;
        if enhance {
            if !contains_attribute(&attributes, "backlink-type") {
                attributes.push(Attribute {
                    name: intern_name(
                        &mut self.attribute_names,
                        b"backlink-type",
                    ),
                    value: "referenced-by".into(),
                });
            }
            if !contains_attribute(&attributes, "backlink-anchor")
                && let Some(anchor) = &self.last_heading_id
            {
                attributes.push(Attribute {
                    name: intern_name(
                        &mut self.attribute_names,
                        b"backlink-anchor",
                    ),
                    value: anchor.as_str().into(),
                });
            }
        }

        self.pending = Some(PendingReference {
            start,
            content,
            attributes,
            nested: 0,
        });
    }

    /// Completes an extracted autoref element.
    fn close_reference(&mut self, span: Span<usize>, editor: &mut Editor<'_>) {
        let Some(mut pending) = self.pending.take() else {
            return;
        };
        if pending.nested > 0 {
            pending.nested -= 1;
            self.pending = Some(pending);
            return;
        }

        let index = self.references.len();
        self.references.push(Reference {
            attributes: pending.attributes,
            title: editor.text(pending.content..span.start).into(),
        });
        if !self.collect_registrations {
            self.title_ranges.push(pending.content..span.start);
        }
        editor.replace(pending.start..span.end, slot(index));
    }

    /// Opens the anchor-scanner state for one element.
    fn open_anchor_element(
        &mut self, tag: &Tag, id: Option<&str>, href_nonempty: bool,
    ) -> AnchorElement {
        if self
            .context
            .last()
            .is_some_and(|element| element.anchor.ignores_descendants())
        {
            return AnchorElement::Ignored;
        }

        let scope = self.anchor_scopes.len() - 1;
        match tag {
            Tag::Anchor => {
                if let Some(id) = id.filter(|id| !id.is_empty()) {
                    self.anchor_scopes[scope].pending.push(id.to_string());
                }
                AnchorElement::Anchor {
                    scope,
                    has_text: false,
                    has_child: false,
                    flush: href_nonempty,
                }
            }
            Tag::Heading(_) => AnchorElement::Heading {
                scope,
                aliases: mem::take(&mut self.anchor_scopes[scope].pending),
            },
            Tag::Paragraph => AnchorElement::Paragraph {
                scope,
                previous_heading: self.anchor_scopes[scope]
                    .last_heading
                    .clone(),
            },
            Tag::Autoref | Tag::Void | Tag::Other(_) => {
                self.flush_scope(scope, None, true);
                self.anchor_scopes.push(AnchorScope::default());
                AnchorElement::Other {
                    scope: self.anchor_scopes.len() - 1,
                }
            }
        }
    }

    /// Closes the most recent element and applies deferred registrations.
    fn close_element(&mut self) {
        self.clear_tail();
        let Some(element) = self.context.pop() else {
            return;
        };

        let title = element.heading.as_ref().and_then(Heading::text);
        if let Some(heading) = &element.heading
            && !heading.skip_inventory
            && let Some(id) = heading.id.as_deref().filter(|id| !id.is_empty())
        {
            self.facts
                .register_anchor(&self.page_url, id, None, title, true);
        }

        match element.anchor {
            AnchorElement::Ignored => {}
            AnchorElement::Anchor { scope, has_text, flush, .. } => {
                if has_text || flush {
                    self.flush_scope(scope, None, true);
                }
                self.anchor_scopes[scope].tail = Some(Tail::Anchor);
            }
            AnchorElement::Heading { scope, aliases } => {
                let target = element
                    .heading
                    .as_ref()
                    .and_then(|heading| heading.id.as_deref())
                    .filter(|id| !id.is_empty());
                for alias in aliases {
                    self.facts.register_anchor(
                        &self.page_url,
                        &alias,
                        target,
                        title,
                        true,
                    );
                }
                self.anchor_scopes[scope].last_heading = title.map(Rc::from);
            }
            AnchorElement::Paragraph { scope, previous_heading } => {
                self.anchor_scopes[scope].last_heading = previous_heading;
                self.anchor_scopes[scope].tail = Some(Tail::Paragraph);
            }
            AnchorElement::Other { scope } => {
                debug_assert_eq!(scope, self.anchor_scopes.len() - 1);
                self.flush_scope(scope, None, false);
                self.anchor_scopes.pop();
            }
        }

        if element.heading_boundary {
            self.heading_skip -= 1;
        }
        if element.backlink_boundary {
            self.backlink_skip -= 1;
        }
    }

    /// Observes decoded text for ElementTree-style `.text` and `.tail` rules.
    fn text(&mut self, value: &[u8]) {
        let value = String::from_utf8_lossy(value);
        self.observe_tail(&value);

        let Some(element) = self.context.last_mut() else {
            return;
        };
        if let Some(heading) = &mut element.heading
            && !heading.has_child
        {
            heading.title.push_str(&value);
            heading.has_text |= !value.is_empty();
        }
        if let AnchorElement::Anchor { has_text, has_child, .. } =
            &mut element.anchor
            && !*has_child
        {
            *has_text |= !value.is_empty();
        }
    }

    /// Applies a non-whitespace tail to the corresponding scanner rule.
    fn observe_tail(&mut self, value: &str) {
        let scope = self.anchor_scopes.len() - 1;
        let Some(tail) = self.anchor_scopes[scope].tail else {
            return;
        };
        if value.trim().is_empty() {
            return;
        }
        match tail {
            Tail::Anchor => self.flush_scope(scope, None, true),
            Tail::Paragraph => self.flush_scope(scope, None, false),
        }
        self.anchor_scopes[scope].tail = None;
    }

    /// Stops observing the tail of the previous direct child.
    fn clear_tail(&mut self) {
        if let Some(scope) = self.anchor_scopes.last_mut() {
            scope.tail = None;
        }
    }

    /// Registers and clears pending anchors in one scanner scope.
    fn flush_scope(
        &mut self, scope: usize, alias_to: Option<&str>, use_title: bool,
    ) {
        let pending = mem::take(&mut self.anchor_scopes[scope].pending);
        if pending.is_empty() {
            return;
        }
        let title = use_title
            .then(|| self.anchor_scopes[scope].last_heading.as_deref())
            .flatten();
        for anchor in pending {
            self.facts.register_anchor(
                &self.page_url,
                &anchor,
                alias_to,
                title,
                true,
            );
        }
    }
}

impl StartTag {
    /// Creates a pending start tag.
    fn new(tag: Tag, start: usize) -> Self {
        Self {
            tag,
            start,
            attributes: Vec::new(),
            reference_attribute: None,
            attribute: StartAttribute::Other,
            id: None,
            href_nonempty: false,
            skip_inventory: false,
            backlink_marker: false,
        }
    }

    /// Observes one decoded attribute name.
    fn attribute_name(&mut self, name: &[u8], names: &mut HashSet<Arc<str>>) {
        self.attribute = match (self.tag, name) {
            (Tag::Anchor | Tag::Heading(_), b"id") => StartAttribute::Id,
            (Tag::Anchor, b"href") => StartAttribute::Href,
            _ => StartAttribute::Other,
        };
        self.skip_inventory |= matches!(self.tag, Tag::Heading(_))
            && name == b"data-skip-inventory";

        if self.tag == Tag::Autoref {
            self.reference_attribute = None;
            if name == BACKLINK_MARKER.as_bytes() {
                self.backlink_marker = true;
                return;
            }
            self.attributes.push(Attribute {
                name: intern_name(names, name),
                value: Box::default(),
            });
            self.reference_attribute = Some(self.attributes.len() - 1);
        }
    }

    /// Observes one decoded attribute value.
    fn attribute_value(&mut self, value: &[u8]) {
        if let Some(index) = self.reference_attribute {
            self.attributes[index].value =
                String::from_utf8_lossy(value).into_owned().into_boxed_str();
            return;
        }
        match self.attribute {
            StartAttribute::Id => {
                self.id = Some(String::from_utf8_lossy(value).into_owned());
            }
            StartAttribute::Href => {
                self.href_nonempty = !value.is_empty();
            }
            StartAttribute::Other => {}
        }
    }
}

impl Heading {
    /// Returns the heading's immediate text when ElementTree would have one.
    fn text(&self) -> Option<&str> {
        self.has_text.then_some(self.title.as_str())
    }
}

impl AnchorElement {
    /// Returns whether the old recursive scanner skipped this subtree.
    fn ignores_descendants(&self) -> bool {
        matches!(
            self,
            Self::Ignored | Self::Anchor { .. } | Self::Heading { .. }
        )
    }
}

impl Tag {
    /// Parses a tokenizer tag name with known autorefs behavior.
    fn from_bytes(name: &[u8]) -> Option<Self> {
        match name {
            b"a" => Some(Self::Anchor),
            b"autoref" => Some(Self::Autoref),
            b"h1" => Some(Self::Heading(1)),
            b"h2" => Some(Self::Heading(2)),
            b"h3" => Some(Self::Heading(3)),
            b"h4" => Some(Self::Heading(4)),
            b"h5" => Some(Self::Heading(5)),
            b"h6" => Some(Self::Heading(6)),
            b"p" => Some(Self::Paragraph),
            b"area" | b"base" | b"br" | b"col" | b"embed" | b"hr" | b"img"
            | b"input" | b"link" | b"meta" | b"param" | b"source"
            | b"track" | b"wbr" => Some(Self::Void),
            _ => None,
        }
    }

    /// Returns whether the element closes with its start tag in HTML.
    fn is_void(&self) -> bool {
        matches!(self, Self::Void)
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

/// Returns whether an attribute is present.
fn contains_attribute(attributes: &[Attribute], name: &str) -> bool {
    attributes
        .iter()
        .any(|attribute| attribute.name.as_ref() == name)
}

/// Interns an autoref attribute name for the lifetime of its page facts.
fn intern_name(names: &mut HashSet<Arc<str>>, value: &[u8]) -> Arc<str> {
    let value = String::from_utf8_lossy(value);
    if let Some(name) = names.get(value.as_ref()) {
        return Arc::clone(name);
    }
    let name: Arc<str> = Arc::from(value.as_ref());
    names.insert(Arc::clone(&name));
    name
}

/// Decodes a UTF-8 string represented by lowercase hexadecimal bytes.
fn decode_hex(value: &[u8]) -> Option<String> {
    let (pairs, remainder) = value.as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    let bytes = pairs
        .iter()
        .map(|pair| {
            let high = hex_digit(pair[0])?;
            let low = hex_digit(pair[1])?;
            Some((high << 4) | low)
        })
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

/// Decodes one ASCII hexadecimal digit.
fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

/// Creates the stable marker for a page-local autoref slot.
fn slot(index: usize) -> String {
    format!("{SLOT_PREFIX}{index}{SLOT_SUFFIX}")
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use ahash::HashMap;

    use crate::compat::mkdocs::html;

    use super::{Facts, Parser, References, SLOT_PREFIX, SLOT_SUFFIX};

    fn parse(input: &str, backlinks: bool) -> (String, References, Facts) {
        let mut parser =
            Parser::with_facts("page", backlinks, Facts::default());
        let output = html::scan(input, &mut [&mut parser])
            .unwrap_or_else(|| input.to_string());
        let (references, facts) = parser.finish();
        (output, references, facts)
    }

    #[test]
    fn extracts_attributes_and_raw_inner_html() {
        let input = concat!(
            "<p>Before ",
            "<autoref\n identifier='Foo &amp; Bar' optional>",
            "<code>Foo &amp; Bar</code>",
            "</autoref> after</p>",
        );
        let (output, references, _) = parse(input, false);
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
    fn template_pass_only_extracts_references() {
        // Template headings and anchors are outside Markdown registration.
        let input = concat!(
            "<h2 id=\"navigation\">Navigation</h2>",
            "<a id=\"footer\"></a>",
            "<autoref identifier=\"target\">Target</autoref>",
        );
        let mut parser = Parser::default();

        let output = html::scan(input, &mut [&mut parser]).unwrap();
        let (references, facts) = parser.finish();

        assert_eq!(
            references.get(0).unwrap().get("identifier"),
            Some("target")
        );
        assert_eq!(facts, Facts::default());
        assert!(output.starts_with("<h2 id=\"navigation\">Navigation</h2>"));
        assert!(output.ends_with(&format!("{SLOT_PREFIX}0{SLOT_SUFFIX}")));
    }

    #[test]
    fn leaves_unclosed_and_self_closing_elements_untouched() {
        for input in ["<autoref identifier=x>Title", "<autoref identifier=x/>"]
        {
            let (output, references, _) = parse(input, false);
            assert_eq!(output, input);
            assert!(references.is_empty());
        }
    }

    #[test]
    fn registers_headings_and_markdown_anchor_aliases() {
        let input = concat!(
            "<p><a href=\"\" id=\"foo\"></a></p>\n",
            "<h2 id=\"heading-foo\">Heading foo</h2>\n",
            "<p><a href=\"\" id=\"bar\"></a>\nParagraph 2.</p>\n",
            "<p><a href=\"\" id=\"alias1\"></a>\n",
            "<a href=\"\" id=\"alias2\"></a></p>\n",
            "<h2 id=\"heading-bar\">Heading bar</h2>\n",
            "<p><a href=\"\" id=\"alias3\"></a>Text.</p>\n",
            "<p><a href=\"\" id=\"alias4\"></a></p>\n",
            "<h2 id=\"heading-baz\">Heading baz</h2>\n",
            "<p><a href=\"\" id=\"alias5\"></a>\n",
            "<a href=\"\" id=\"alias6\"></a>\nDecoy.</p>\n",
            "<h2 id=\"heading-more1\">Heading more1</h2>\n",
            "<p><a href=\"\" id=\"alias7\"></a>\n",
            "<a href=\"\" id=\"alias8\">decoy</a>\n",
            "<a href=\"\" id=\"alias9\"></a></p>\n",
            "<h2 id=\"heading-custom2\">Heading more2</h2>\n",
            "<p><a href=\"\" id=\"aliasSame\"></a></p>\n",
            "<h2 id=\"same-heading-1\">Same heading 1</h2>\n",
            "<p><a href=\"\" id=\"aliasSame\"></a></p>\n",
            "<h2 id=\"same-heading-2\">Same heading 2</h2>\n",
            "<p><a href=\"\" id=\"alias10\"></a></p>",
        );
        let (_, _, facts) = parse(input, false);

        let expected = HashMap::from_iter([
            ("foo".into(), vec!["page#heading-foo".into()]),
            ("heading-foo".into(), vec!["page#heading-foo".into()]),
            ("bar".into(), vec!["page#bar".into()]),
            ("alias1".into(), vec!["page#heading-bar".into()]),
            ("alias2".into(), vec!["page#heading-bar".into()]),
            ("heading-bar".into(), vec!["page#heading-bar".into()]),
            ("alias3".into(), vec!["page#alias3".into()]),
            ("alias4".into(), vec!["page#heading-baz".into()]),
            ("heading-baz".into(), vec!["page#heading-baz".into()]),
            ("alias5".into(), vec!["page#alias5".into()]),
            ("alias6".into(), vec!["page#alias6".into()]),
            ("heading-more1".into(), vec!["page#heading-more1".into()]),
            ("alias7".into(), vec!["page#alias7".into()]),
            ("alias8".into(), vec!["page#alias8".into()]),
            ("alias9".into(), vec!["page#heading-custom2".into()]),
            (
                "heading-custom2".into(),
                vec!["page#heading-custom2".into()],
            ),
            (
                "aliasSame".into(),
                vec![
                    "page#same-heading-1".into(),
                    "page#same-heading-2".into(),
                ],
            ),
            ("same-heading-1".into(), vec!["page#same-heading-1".into()]),
            ("same-heading-2".into(), vec!["page#same-heading-2".into()]),
            ("alias10".into(), vec!["page#alias10".into()]),
        ]);
        assert_eq!(facts.primary, expected);
        assert_eq!(facts.titles["page#heading-foo"], "Heading foo");
        assert_eq!(facts.titles["page#heading-bar"], "Heading bar");
        assert_eq!(facts.titles["page#heading-baz"], "Heading baz");
        assert!(!facts.titles.contains_key("page#alias10"));
    }

    #[test]
    fn extends_seeded_facts_without_overwriting_precedence() {
        let facts = Facts {
            primary: HashMap::from_iter([(
                "heading".into(),
                vec!["page#heading".into()],
            )]),
            titles: HashMap::from_iter([(
                "page#heading".into(),
                "Richer title".into(),
            )]),
            ..Facts::default()
        };
        let mut parser = Parser::with_facts("page", false, facts);
        let _ = html::scan(
            "<h2 id=\"heading\">Plain title</h2>",
            &mut [&mut parser],
        );
        let (_, facts) = parser.finish();

        assert_eq!(facts.primary["heading"], ["page#heading"]);
        assert_eq!(facts.titles["page#heading"], "Richer title");
    }

    #[test]
    fn nested_containers_have_independent_anchor_aliases() {
        let input = concat!(
            "<p><a href=\"\" id=\"alias1\"></a></p>\n",
            "<div class=\"admonition\">",
            "<h2 id=\"heading-foo\">Heading foo</h2>",
            "<p><a href=\"\" id=\"alias2\"></a></p>",
            "<h2 id=\"heading-bar\">Heading bar</h2>",
            "<p><a href=\"\" id=\"alias3\"></a></p>",
            "</div>",
            "<h2 id=\"heading-baz\">Heading baz</h2>",
        );
        let (_, _, facts) = parse(input, false);

        assert_eq!(facts.primary["alias1"], ["page#alias1"]);
        assert_eq!(facts.primary["alias2"], ["page#heading-bar"]);
        assert_eq!(facts.primary["alias3"], ["page#alias3"]);
        assert!(!facts.titles.contains_key("page#alias3"));
    }

    #[test]
    fn infers_backlinks_from_the_final_page_heading_order() {
        let input = concat!(
            "<h2 id=\"object-id\">Object</h2>",
            "<h3 id=\"parameter-id\">Parameter</h3>",
            "<!--zensical:autoref-context:start:6f626a6563742d6964-->",
            "<p><autoref identifier=\"Foo\" data-zensical-autoref>",
            "Foo</autoref></p>",
            "<!--zensical:autoref-context:end-->",
            "<autoref identifier=\"Bar\" data-zensical-autoref>",
            "Bar</autoref>",
        );
        let (output, references, _) = parse(input, true);
        let reference = references.get(0).expect("reference");
        let following = references.get(1).expect("following reference");

        assert_eq!(reference.get("backlink-type"), Some("referenced-by"));
        assert_eq!(reference.get("backlink-anchor"), Some("object-id"));
        assert_eq!(following.get("backlink-anchor"), Some("parameter-id"));
        assert!(!reference.contains("data-zensical-autoref"));
        assert!(!output.contains("autoref-context"));
    }

    #[test]
    fn disabled_backlinks_do_not_add_metadata() {
        let input = concat!(
            "<h2 id=\"heading\">Heading</h2>",
            "<autoref identifier=\"Foo\">Foo</autoref>",
        );
        let (_, references, _) = parse(input, false);
        let reference = references.get(0).expect("reference");

        assert!(!reference.contains("backlink-type"));
        assert!(!reference.contains("backlink-anchor"));
    }

    #[test]
    fn explicit_backlink_metadata_is_preserved() {
        let input = concat!(
            "<h2 id=\"heading\">Heading</h2>",
            "<autoref identifier=\"Foo\" backlink-type=\"used-by\" ",
            "backlink-anchor=\"object-id\">Foo</autoref>",
        );
        let (_, references, _) = parse(input, true);
        let reference = references.get(0).expect("reference");

        assert_eq!(reference.get("backlink-type"), Some("used-by"));
        assert_eq!(reference.get("backlink-anchor"), Some("object-id"));
    }

    #[test]
    fn headings_without_ids_clear_the_backlink_anchor() {
        let input = concat!(
            "<h2 id=\"heading\">Heading</h2>",
            "<h3>No ID</h3>",
            "<autoref identifier=\"Foo\" data-zensical-autoref>Foo</autoref>",
        );
        let (_, references, _) = parse(input, true);
        let reference = references.get(0).expect("reference");

        assert_eq!(reference.get("backlink-type"), Some("referenced-by"));
        assert!(!reference.contains("backlink-anchor"));
    }

    #[test]
    fn inventory_opt_out_headings_are_not_registered() {
        let input = "<h2 id=\"local\" data-skip-inventory=\"true\">Local</h2>";
        let (_, _, facts) = parse(input, false);

        assert!(facts.primary.is_empty());
    }
}
