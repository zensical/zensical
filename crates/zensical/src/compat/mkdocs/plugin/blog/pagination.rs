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

//! Native parsing of Material's pagination format language.

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// One directional link in a pagination format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// Link to the first page.
    First,
    /// Link to the last page.
    Last,
    /// Link to the previous page.
    Previous,
    /// Link to the next page.
    Next,
}

/// One ordered component of a rendered pagination control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaginationItem {
    /// Numbered page link or current-page marker.
    Page {
        /// One-based target page number.
        page: usize,
        /// Whether this item denotes the current page.
        current: bool,
    },
    /// Collapsed range between numbered pages.
    Ellipsis,
    /// Directional link to another page.
    Link {
        /// Direction represented by the link.
        kind: LinkKind,
        /// One-based target page number.
        page: usize,
    },
    /// Literal text from the configured pagination format.
    Text(
        /// Preserved literal or scalar substitution.
        String,
    ),
}

/// Parsed replacement for one pagination-format placeholder.
enum Substitution {
    /// Optional structured pagination item.
    Item(
        /// Generated item, or `None` when a directional link is unreachable.
        Option<PaginationItem>,
    ),
    /// Scalar value rendered as literal text.
    Text(
        /// Rendered scalar value.
        String,
    ),
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Values substituted into scalar pagination placeholders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaginationMetrics {
    /// One-based current page number.
    pub page: usize,
    /// Total number of reachable pages.
    pub pages: usize,
    /// Configured maximum number of items on a page.
    pub items_per_page: usize,
    /// Total number of items across all pages.
    pub item_count: usize,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl LinkKind {
    /// Returns the stable template-facing item type.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::First => "first_page",
            Self::Last => "last_page",
            Self::Previous => "previous_page",
            Self::Next => "next_page",
        }
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Parses a pagination format into bounded, presentation-independent items.
///
/// This mirrors the `paginate` package's format substitutions while leaving
/// link markup and directional symbols to the active UI template.
pub fn items(format: &str, metrics: PaginationMetrics) -> Vec<PaginationItem> {
    debug_assert!(metrics.page >= 1 && metrics.page <= metrics.pages);
    let radius = first_radius(format).unwrap_or(2);
    let bytes = format.as_bytes();
    let mut result = Vec::new();
    let mut text = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if let Some((end, _)) = range(bytes, index) {
            push_text(&mut result, &mut text);
            result.extend(page_range(metrics, radius));
            index = end;
        } else if bytes[index] == b'$' {
            let (end, placeholder) = placeholder(format, index);
            match placeholder.and_then(|name| substitution(name, metrics)) {
                Some(Substitution::Item(item)) => {
                    if let Some(item) = item {
                        push_text(&mut result, &mut text);
                        result.push(item);
                    }
                }
                Some(Substitution::Text(value)) => text.push_str(&value),
                None if format[index..end].starts_with("$$") => {
                    text.push('$');
                }
                None => text.push_str(&format[index..end]),
            }
            index = end;
        } else {
            let character = format[index..]
                .chars()
                .next()
                .expect("index is within the format");
            text.push(character);
            index += character.len_utf8();
        }
    }
    push_text(&mut result, &mut text);
    result
}

fn substitution(
    name: &str, metrics: PaginationMetrics,
) -> Option<Substitution> {
    let scalar = match name {
        "first_page" => Some(1),
        "last_page" | "page_count" => Some(metrics.pages),
        "page" => Some(metrics.page),
        "items_per_page" => Some(metrics.items_per_page),
        "first_item" => Some(
            (metrics.page - 1)
                .saturating_mul(metrics.items_per_page)
                .saturating_add(1)
                .min(metrics.item_count),
        ),
        "last_item" => Some(
            metrics
                .page
                .saturating_mul(metrics.items_per_page)
                .min(metrics.item_count),
        ),
        "item_count" => Some(metrics.item_count),
        _ => None,
    };
    if let Some(value) = scalar {
        return Some(Substitution::Text(value.to_string()));
    }

    let link = match name {
        "link_first" => Some((LinkKind::First, 1, metrics.page > 1)),
        "link_last" => {
            Some((LinkKind::Last, metrics.pages, metrics.page < metrics.pages))
        }
        "link_previous" => Some((
            LinkKind::Previous,
            metrics.page.saturating_sub(1),
            metrics.page > 1,
        )),
        "link_next" => Some((
            LinkKind::Next,
            metrics.page.saturating_add(1),
            metrics.page < metrics.pages,
        )),
        _ => None,
    };
    link.map(|(kind, page, visible)| {
        Substitution::Item(
            visible.then_some(PaginationItem::Link { kind, page }),
        )
    })
}

fn page_range(
    metrics: PaginationMetrics, radius: usize,
) -> Vec<PaginationItem> {
    let left = metrics.page.saturating_sub(radius).max(1);
    let right = metrics.page.saturating_add(radius).min(metrics.pages);
    let mut result = Vec::new();
    if metrics.page != 1 && 1 < left {
        result.push(PaginationItem::Page { page: 1, current: false });
    }
    if left.saturating_sub(1) > 1 {
        result.push(PaginationItem::Ellipsis);
    }
    result.extend((left..=right).map(|page| PaginationItem::Page {
        page,
        current: page == metrics.page,
    }));
    if metrics.pages.saturating_sub(right) > 1 {
        result.push(PaginationItem::Ellipsis);
    }
    if metrics.page != metrics.pages && right < metrics.pages {
        result.push(PaginationItem::Page {
            page: metrics.pages,
            current: false,
        });
    }
    result
}

fn first_radius(format: &str) -> Option<usize> {
    let bytes = format.as_bytes();
    (0..bytes.len()).find_map(|index| range(bytes, index).map(|(_, n)| n))
}

fn range(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    if bytes.get(start) != Some(&b'~') {
        return None;
    }
    let mut index = start + 1;
    let first = index;
    let mut value = 0usize;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        value = value
            .saturating_mul(10)
            .saturating_add(usize::from(bytes[index] - b'0'));
        index += 1;
    }
    (index > first && bytes.get(index) == Some(&b'~'))
        .then_some((index + 1, value))
}

fn placeholder(format: &str, start: usize) -> (usize, Option<&str>) {
    let bytes = format.as_bytes();
    let Some(next) = bytes.get(start + 1).copied() else {
        return (start + 1, None);
    };
    if next == b'$' {
        return (start + 2, None);
    }
    if next == b'{' {
        let name_start = start + 2;
        let mut end = name_start;
        while bytes.get(end).is_some_and(|byte| *byte != b'}') {
            end += 1;
        }
        if bytes.get(end) == Some(&b'}') {
            let name = &format[name_start..end];
            return (end + 1, identifier(name).then_some(name));
        }
        return (start + 1, None);
    }
    let name_start = start + 1;
    if !identifier_start(next) {
        return (start + 1, None);
    }
    let mut end = name_start + 1;
    while bytes.get(end).is_some_and(|byte| identifier_byte(*byte)) {
        end += 1;
    }
    (end, Some(&format[name_start..end]))
}

fn identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes.next().is_some_and(identifier_start) && bytes.all(identifier_byte)
}

fn identifier_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn identifier_byte(byte: u8) -> bool {
    identifier_start(byte) || byte.is_ascii_digit()
}

fn push_text(result: &mut Vec<PaginationItem>, text: &mut String) {
    if !text.is_empty() {
        result.push(PaginationItem::Text(std::mem::take(text)));
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(page: usize) -> PaginationMetrics {
        PaginationMetrics {
            page,
            pages: 10,
            items_per_page: 3,
            item_count: 29,
        }
    }

    #[test]
    fn expands_the_default_page_range() {
        assert_eq!(
            items("~2~", metrics(5)),
            vec![
                PaginationItem::Page { page: 1, current: false },
                PaginationItem::Ellipsis,
                PaginationItem::Page { page: 3, current: false },
                PaginationItem::Page { page: 4, current: false },
                PaginationItem::Page { page: 5, current: true },
                PaginationItem::Page { page: 6, current: false },
                PaginationItem::Page { page: 7, current: false },
                PaginationItem::Ellipsis,
                PaginationItem::Page { page: 10, current: false },
            ]
        );
    }

    #[test]
    fn preserves_directional_and_scalar_placeholder_order() {
        assert_eq!(
            items(
                "$link_first $link_previous $page/$page_count \
                 $link_next $link_last",
                metrics(5),
            ),
            vec![
                PaginationItem::Link { kind: LinkKind::First, page: 1 },
                PaginationItem::Text(" ".into()),
                PaginationItem::Link {
                    kind: LinkKind::Previous,
                    page: 4,
                },
                PaginationItem::Text(" 5/10 ".into()),
                PaginationItem::Link { kind: LinkKind::Next, page: 6 },
                PaginationItem::Text(" ".into()),
                PaginationItem::Link { kind: LinkKind::Last, page: 10 },
            ]
        );
    }

    #[test]
    fn omits_unreachable_links_but_preserves_literal_separators() {
        assert_eq!(
            items("$link_previous $page $link_next", metrics(1)),
            vec![
                PaginationItem::Text(" 1 ".into()),
                PaginationItem::Link { kind: LinkKind::Next, page: 2 },
            ]
        );
    }

    #[test]
    fn substitutes_item_boundaries_and_keeps_unknown_tokens() {
        assert_eq!(
            items(
                "${first_item}-${last_item}/$item_count $$ $unknown",
                metrics(10),
            ),
            vec![PaginationItem::Text("28-29/29 $ $unknown".into())]
        );
    }

    #[test]
    fn uses_the_first_radius_for_every_range_placeholder() {
        let result = items("~0~ + ~3~", metrics(5));
        assert_eq!(
            result,
            vec![
                PaginationItem::Page { page: 1, current: false },
                PaginationItem::Ellipsis,
                PaginationItem::Page { page: 5, current: true },
                PaginationItem::Ellipsis,
                PaginationItem::Page { page: 10, current: false },
                PaginationItem::Text(" + ".into()),
                PaginationItem::Page { page: 1, current: false },
                PaginationItem::Ellipsis,
                PaginationItem::Page { page: 5, current: true },
                PaginationItem::Ellipsis,
                PaginationItem::Page { page: 10, current: false },
            ]
        );
    }
}
