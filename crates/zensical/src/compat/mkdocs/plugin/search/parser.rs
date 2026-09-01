// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

//! MkDocs-compatible search extraction from rendered HTML.

use html5gum::emitters::callback::CallbackEvent;
use html5gum::Span;

use crate::compat::mkdocs::html::{Editor, Visitor};

use super::SearchSection;

// ----------------------------------------------------------------------------
// Parser
// ----------------------------------------------------------------------------

/// Streaming search parser.
#[derive(Default)]
pub(crate) struct Parser {
    /// Whether extraction is disabled for a page excluded through metadata.
    discard: bool,
    /// Open HTML elements.
    context: Vec<Element>,
    /// Section currently receiving text.
    current: Option<usize>,
    /// Sections extracted from the input.
    sections: Vec<SectionState>,
    /// Number of excluded elements currently open.
    skip: usize,
    /// Start tag currently emitted by the tokenizer.
    start: Option<StartTag>,
    /// Current attribute of the pending start tag.
    attribute: Attribute,
}

impl Parser {
    /// Creates a parser that only applies search-related HTML cleanup.
    pub(crate) fn discarding() -> Self {
        Self {
            discard: true,
            ..Self::default()
        }
    }

    /// Handles a tokenizer event.
    fn handle(
        &mut self, event: &CallbackEvent<'_>, span: Span<usize>,
        editor: &mut Editor<'_>,
    ) {
        if let CallbackEvent::AttributeName { name } = event
            && *name == b"data-search-exclude"
        {
            editor.remove_attribute(name, span);
        }
        if self.discard {
            return;
        }

        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.start = Some(StartTag::new(Tag::from_bytes(name)));
                self.attribute = Attribute::Other;
            }
            CallbackEvent::AttributeName { name } => {
                self.attribute = Attribute::from_bytes(name);
                if let Some(start) = &mut self.start {
                    start.observe_attribute(self.attribute, None);
                }
            }
            CallbackEvent::AttributeValue { value } => {
                if let Some(start) = &mut self.start {
                    start.observe_attribute(self.attribute, Some(value));
                }
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                if let Some(start) = self.start.take() {
                    let tag = start.tag.clone();
                    self.start(start);
                    if *self_closing {
                        self.end(&tag);
                    }
                }
            }
            CallbackEvent::EndTag { name } => {
                self.end(&Tag::from_bytes(name));
            }
            CallbackEvent::String { value } => {
                self.text(String::from_utf8_lossy(value).as_ref());
            }
            CallbackEvent::Comment { .. }
            | CallbackEvent::Doctype { .. }
            | CallbackEvent::Error(_) => {}
        }
    }

    /// Handles a complete start tag.
    fn start(&mut self, start: StartTag) {
        if start.tag.is_void() {
            return;
        }

        let heading = start.tag.heading_level();
        let skipped = start.tag.is_skipped()
            || start.excluded
            || start.class.as_deref() == Some(b"linenodiv");
        let headerlink = start.tag == Tag::A
            && start.class.as_deref() == Some(b"headerlink");
        let tag = start.tag.clone();

        self.context.push(Element {
            tag: start.tag,
            skipped,
            headerlink,
            kept: None,
        });

        if let (Some(level), true) = (heading, start.id_present) {
            let depth = self.context.len();

            if level != 1 && self.sections.is_empty() {
                self.push_preface();
            }

            let location = if self.sections.is_empty() {
                None
            } else {
                start.id
            };
            let section = SectionState::new(
                Some(level),
                level.into(),
                depth,
                location,
                start.excluded,
            );
            self.sections.push(section);
            self.current = Some(self.sections.len() - 1);
        }

        self.ensure_section();

        if skipped {
            self.skip += 1;
            return;
        }

        if self.skip == 0 && tag.is_kept() {
            let (section, title) = self.output_target();
            let data = self.sections[section].output_mut(title);
            let start = data.value.len();
            let previous_whitespace = data.last_whitespace;
            data.value.push('<');
            data.value.push_str(tag.name());
            data.value.push('>');
            data.last_whitespace = false;

            self.context.last_mut().expect("context").kept =
                Some(KeptElement {
                    section,
                    title,
                    start,
                    previous_whitespace,
                });
        }
    }

    /// Handles an end tag.
    fn end(&mut self, tag: &Tag) {
        if self.context.last().is_none_or(|el| el.tag != *tag) {
            return;
        }

        let depth = self.context.len();
        if let Some(index) = self.current
            && self.sections[index].exited_or_deeper_than(depth)
            && let Some(parent) = self
                .sections
                .iter()
                .rposition(|section| !section.exited && section.depth <= depth)
        {
            self.sections[index].exited = true;
            self.current = Some(parent);
        }

        let element = self.context.pop().expect("context");
        if element.skipped {
            self.skip -= 1;
            return;
        }

        if self.skip == 0 && tag.is_kept() {
            let (section, title) = self.output_target();
            let data = self.sections[section].output_mut(title);
            let opening = format!("<{}>", tag.name());

            if let Some(start) = data.value.find(&opening) {
                let following = &data.value[start + opening.len()..];
                if following.chars().any(|char| !char.is_whitespace()) {
                    data.value.push_str("</");
                    data.value.push_str(tag.name());
                    data.value.push('>');
                    data.last_whitespace = false;
                } else {
                    data.value.truncate(start);
                    data.last_whitespace = element.kept.map_or_else(
                        || {
                            data.value
                                .chars()
                                .last()
                                .is_some_and(char::is_whitespace)
                        },
                        |kept| {
                            debug_assert_eq!(kept.section, section);
                            debug_assert_eq!(kept.title, title);
                            debug_assert_eq!(kept.start, start);
                            kept.previous_whitespace
                        },
                    );
                }
            }
        }
    }

    /// Handles text content.
    fn text(&mut self, value: &str) {
        if self.skip > 0 {
            return;
        }

        let preformatted = self.context.iter().any(|el| el.tag == Tag::Pre);
        let whitespace = value.chars().all(char::is_whitespace);
        let text;
        let value = if preformatted {
            value
        } else if whitespace {
            " "
        } else if value.contains('\n') {
            text = value.replace('\n', " ");
            &text
        } else {
            value
        };

        self.ensure_section();
        let (section, title) = self.output_target();

        if title {
            if self.context.iter().any(|el| el.headerlink) {
                return;
            }
            escape(value, &mut self.sections[section].title.value);
            self.sections[section].title.last_whitespace = whitespace;
        } else {
            let data = &mut self.sections[section].text;
            if !whitespace || preformatted || !data.last_whitespace {
                escape(value, &mut data.value);
                data.last_whitespace = whitespace;
            }
        }
    }

    /// Returns the current section and whether its title receives output.
    fn output_target(&self) -> (usize, bool) {
        let section = self.current.expect("section");
        let heading = self.sections[section].heading;
        let title = heading.is_some_and(|level| {
            self.context
                .iter()
                .any(|el| el.tag.heading_level() == Some(level))
        });
        (section, title)
    }

    /// Ensures a section exists for preface content.
    fn ensure_section(&mut self) {
        if self.current.is_none() {
            self.push_preface();
        }
    }

    /// Adds the implicit top-level preface section.
    fn push_preface(&mut self) {
        self.sections
            .push(SectionState::new(None, 1, 0, None, false));
        self.current = Some(self.sections.len() - 1);
    }

    /// Converts parser state into page-local search sections.
    pub(crate) fn finish(self) -> Vec<SearchSection> {
        self.sections
            .into_iter()
            .filter(|section| !section.excluded)
            .map(|section| SearchSection {
                location: section.location,
                level: section.level,
                title: trim(section.title.value),
                text: trim(section.text.value),
            })
            .collect()
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
// State
// ----------------------------------------------------------------------------

/// Section being assembled.
struct SectionState {
    heading: Option<u8>,
    level: u32,
    depth: usize,
    exited: bool,
    excluded: bool,
    location: Option<String>,
    title: Output,
    text: Output,
}

impl SectionState {
    fn new(
        heading: Option<u8>, level: u32, depth: usize,
        location: Option<String>, excluded: bool,
    ) -> Self {
        Self {
            heading,
            level,
            depth,
            exited: false,
            excluded,
            location,
            title: Output::default(),
            text: Output::default(),
        }
    }

    fn exited_or_deeper_than(&self, depth: usize) -> bool {
        self.exited || self.depth > depth
    }

    fn output_mut(&mut self, title: bool) -> &mut Output {
        if title {
            &mut self.title
        } else {
            &mut self.text
        }
    }
}

/// Output buffer and whitespace state.
#[derive(Default)]
struct Output {
    value: String,
    last_whitespace: bool,
}

/// Open HTML element.
struct Element {
    tag: Tag,
    skipped: bool,
    headerlink: bool,
    kept: Option<KeptElement>,
}

/// Location at which a retained element was opened.
#[derive(Clone, Copy)]
struct KeptElement {
    section: usize,
    title: bool,
    start: usize,
    previous_whitespace: bool,
}

/// Start tag under construction.
struct StartTag {
    tag: Tag,
    id_present: bool,
    id: Option<String>,
    excluded: bool,
    class: Option<Vec<u8>>,
}

impl StartTag {
    fn new(tag: Tag) -> Self {
        Self {
            tag,
            id_present: false,
            id: None,
            excluded: false,
            class: None,
        }
    }

    fn observe_attribute(
        &mut self, attribute: Attribute, value: Option<&[u8]>,
    ) {
        match attribute {
            Attribute::Id => {
                self.id_present = true;
                self.id = value
                    .map(|value| String::from_utf8_lossy(value).into_owned());
            }
            Attribute::Class => {
                self.class = value.map(<[u8]>::to_vec);
            }
            Attribute::SearchExclude => self.excluded = true,
            Attribute::Other => {}
        }
    }
}

/// Attributes relevant to extraction.
#[derive(Clone, Copy, Default)]
enum Attribute {
    Id,
    Class,
    SearchExclude,
    #[default]
    Other,
}

impl Attribute {
    fn from_bytes(value: &[u8]) -> Self {
        match value {
            b"id" => Self::Id,
            b"class" => Self::Class,
            b"data-search-exclude" => Self::SearchExclude,
            _ => Self::Other,
        }
    }
}

/// HTML tags relevant to extraction.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Tag {
    A,
    Area,
    Base,
    Br,
    Code,
    Col,
    Embed,
    H(u8),
    Hr,
    Img,
    Input,
    Li,
    Link,
    Meta,
    Object,
    Ol,
    P,
    Param,
    Pre,
    Script,
    Small,
    Source,
    Style,
    Sub,
    Sup,
    Track,
    Ul,
    Wbr,
    Other(Box<str>),
}

impl Tag {
    fn from_bytes(value: &[u8]) -> Self {
        match value {
            b"a" => Self::A,
            b"area" => Self::Area,
            b"base" => Self::Base,
            b"br" => Self::Br,
            b"code" => Self::Code,
            b"col" => Self::Col,
            b"embed" => Self::Embed,
            b"h1" => Self::H(1),
            b"h2" => Self::H(2),
            b"h3" => Self::H(3),
            b"h4" => Self::H(4),
            b"h5" => Self::H(5),
            b"h6" => Self::H(6),
            b"hr" => Self::Hr,
            b"img" => Self::Img,
            b"input" => Self::Input,
            b"li" => Self::Li,
            b"link" => Self::Link,
            b"meta" => Self::Meta,
            b"object" => Self::Object,
            b"ol" => Self::Ol,
            b"p" => Self::P,
            b"param" => Self::Param,
            b"pre" => Self::Pre,
            b"script" => Self::Script,
            b"small" => Self::Small,
            b"source" => Self::Source,
            b"style" => Self::Style,
            b"sub" => Self::Sub,
            b"sup" => Self::Sup,
            b"track" => Self::Track,
            b"ul" => Self::Ul,
            b"wbr" => Self::Wbr,
            _ => {
                Self::Other(String::from_utf8_lossy(value).into_owned().into())
            }
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::A => "a",
            Self::Area => "area",
            Self::Base => "base",
            Self::Br => "br",
            Self::Code => "code",
            Self::Col => "col",
            Self::Embed => "embed",
            Self::H(1) => "h1",
            Self::H(2) => "h2",
            Self::H(3) => "h3",
            Self::H(4) => "h4",
            Self::H(5) => "h5",
            Self::H(6) => "h6",
            Self::H(_) => unreachable!("heading level"),
            Self::Hr => "hr",
            Self::Img => "img",
            Self::Input => "input",
            Self::Li => "li",
            Self::Link => "link",
            Self::Meta => "meta",
            Self::Object => "object",
            Self::Ol => "ol",
            Self::P => "p",
            Self::Param => "param",
            Self::Pre => "pre",
            Self::Script => "script",
            Self::Small => "small",
            Self::Source => "source",
            Self::Style => "style",
            Self::Sub => "sub",
            Self::Sup => "sup",
            Self::Track => "track",
            Self::Ul => "ul",
            Self::Wbr => "wbr",
            Self::Other(name) => name,
        }
    }

    fn heading_level(&self) -> Option<u8> {
        if let Self::H(level) = self {
            Some(*level)
        } else {
            None
        }
    }

    fn is_kept(&self) -> bool {
        matches!(
            self,
            Self::P
                | Self::Code
                | Self::Pre
                | Self::Li
                | Self::Ol
                | Self::Ul
                | Self::Small
                | Self::Sub
                | Self::Sup
        )
    }

    fn is_skipped(&self) -> bool {
        matches!(self, Self::Object | Self::Script | Self::Style)
    }

    fn is_void(&self) -> bool {
        matches!(
            self,
            Self::Area
                | Self::Base
                | Self::Br
                | Self::Col
                | Self::Embed
                | Self::Hr
                | Self::Img
                | Self::Input
                | Self::Link
                | Self::Meta
                | Self::Param
                | Self::Source
                | Self::Track
                | Self::Wbr
        )
    }
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

/// Escapes text like `html.escape(..., quote=False)`.
fn escape(value: &str, output: &mut String) {
    for char in value.chars() {
        match char {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(char),
        }
    }
}

/// Trims Unicode whitespace without retaining the original allocation twice.
fn trim(mut value: String) -> String {
    let end = value.trim_end().len();
    value.truncate(end);
    let start = value.len() - value.trim_start().len();
    value.drain(..start);
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::mkdocs::html::scan;

    fn extract(html: &str) -> Vec<SearchSection> {
        let mut parser = Parser::default();
        let _ = scan(html, &mut [&mut parser]);
        parser.finish()
    }

    fn item(
        location: Option<&str>, level: u32, title: &str, text: &str,
    ) -> SearchSection {
        SearchSection {
            location: location.map(ToString::to_string),
            level,
            title: title.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn extracts_preface() {
        let html = "<p>Before <em>heading</em>.</p>";
        assert_eq!(
            extract(html),
            vec![item(None, 1, "", "<p>Before heading.</p>")]
        );
    }

    #[test]
    fn divides_content_into_sections() {
        let html = concat!(
            r##"<h1 id="top">Top <code>code</code>"##,
            r##"<a class="headerlink" href="#top">¶</a></h1>"##,
            r#"<p>First &amp; second.</p>"#,
            r#"<h2 id="child">Child</h2><p>Body</p>"#,
        );
        assert_eq!(
            extract(html),
            vec![
                item(
                    None,
                    1,
                    "Top <code>code</code>",
                    "<p>First &amp; second.</p>"
                ),
                item(Some("child"), 2, "Child", "<p>Body</p>"),
            ]
        );
    }

    #[test]
    fn preserves_missing_heading_id_behavior() {
        let html = "<h2>No ID</h2><p>Body</p>";
        assert_eq!(extract(html), vec![item(None, 1, "", "No ID<p>Body</p>")]);
    }

    #[test]
    fn excludes_configured_content() {
        let html = concat!(
            r#"<h1 id="top">Top</h1><p>Keep</p>"#,
            r#"<div data-search-exclude><p>Drop</p></div><p>After</p>"#,
            r#"<div class="linenodiv"><pre>1</pre></div>"#,
            r#"<script>ignored <b>script</b></script>"#,
        );
        assert_eq!(
            extract(html),
            vec![item(None, 1, "Top", "<p>Keep</p><p>After</p>")]
        );
    }

    #[test]
    fn removes_exclusion_attributes_from_rendered_html() {
        let html = concat!(
            r#"<h1 id="top">Top</h1><p>Keep</p>"#,
            r#"<div data-search-exclude="true"><p>Drop</p></div>"#,
        );
        let mut parser = Parser::default();
        let output = scan(html, &mut [&mut parser]).expect("search edit");

        assert_eq!(
            output,
            concat!(
                r#"<h1 id="top">Top</h1><p>Keep</p>"#,
                r#"<div><p>Drop</p></div>"#,
            )
        );
        assert_eq!(parser.finish(), vec![item(None, 1, "Top", "<p>Keep</p>")]);
    }

    #[test]
    fn excluded_pages_only_apply_html_cleanup() {
        let html = r"<p data-search-exclude>Drop</p>";
        let mut parser = Parser::discarding();
        let output = scan(html, &mut [&mut parser]).expect("search edit");

        assert_eq!(output, "<p>Drop</p>");
        assert!(parser.finish().is_empty());
    }

    #[test]
    fn preserves_selected_markup_and_empty_elements() {
        let html = concat!(
            r#"<h1 id="top">Top</h1><p>Text <small>small</small></p>"#,
            r#"<p>   </p><ul><li>One</li><li><code>x</code></li></ul>"#,
        );
        assert_eq!(
            extract(html),
            vec![item(
                None,
                1,
                "Top",
                "<p>Text <small>small</small></p><p> </p><ul><li>One</li><li><code>x</code></li></ul>",
            )]
        );
    }

    #[test]
    fn preserves_whitespace_and_preformatted_content() {
        let html = concat!(
            "<h1 id=\"top\">Top</h1><p>one\n  two</p>",
            "<pre><code>a &lt; b\n  c</code></pre>",
        );
        assert_eq!(
            extract(html),
            vec![item(
                None,
                1,
                "Top",
                "<p>one   two</p><pre><code>a &lt; b\n  c</code></pre>",
            )]
        );
    }

    #[test]
    fn decodes_and_escapes_entities_like_python() {
        let html = concat!(
            r#"<h1 id="top">A &amp; B &#169;</h1>"#,
            r#"<p>&lt;tag&gt; &quot;x&quot; &apos;y&apos; &nbsp;</p>"#,
        );
        assert_eq!(
            extract(html),
            vec![item(
                None,
                1,
                "A &amp; B ©",
                "<p>&lt;tag&gt; \"x\" 'y' \u{a0}</p>"
            )]
        );
    }

    #[test]
    fn restores_parent_section_after_nested_heading() {
        let html = concat!(
            r#"<div><h2 id="nested">Nested</h2><p>Inside</p></div>"#,
            r#"<p>Outside</p>"#,
        );
        assert_eq!(
            extract(html),
            vec![
                item(None, 1, "", "<p>Outside</p>"),
                item(Some("nested"), 2, "Nested", "<p>Inside</p>"),
            ]
        );
    }

    #[test]
    fn preserves_malformed_html_behavior() {
        let html = concat!(
            r#"<h1 id="top">Top</h1><p>Before <code>open</p>"#,
            r#"<h2 id="next">Next</h2><p>After"#,
        );
        assert_eq!(
            extract(html),
            vec![
                item(None, 1, "Top", "<p>Before <code>open"),
                item(Some("next"), 2, "Next", "<p>After"),
            ]
        );
    }

    #[test]
    fn ignores_void_elements() {
        let html = concat!(
            r#"<h1 id="top">Top</h1>"#,
            r#"<p>A<br>B<img src="x">C<hr>D</p>"#,
        );
        assert_eq!(extract(html), vec![item(None, 1, "Top", "<p>ABCD</p>")]);
    }
}
