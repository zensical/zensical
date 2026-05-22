# Copyright (c) 2025-2026 Zensical and contributors

# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO

# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to
# deal in the Software without restriction, including without limitation the
# rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
# sell copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:

# The above copyright notice and this permission notice shall be included in
# all copies or substantial portions of the Software.

# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
# FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
# IN THE SOFTWARE.

from __future__ import annotations

from typing import TYPE_CHECKING

from markdown.util import BLOCK_LEVEL_ELEMENTS

from zensical.collectors.references.types import (
    FootnoteDefinition,
    FootnoteReference,
    Link,
    LinkDefinition,
    LinkReference,
    Reference,
)
from zensical.utilities.span import Span

if TYPE_CHECKING:
    from collections.abc import Iterator

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

# fmt: off

# Whitespace
_NL         = ord(b"\n")
_CR         = ord(b"\r")
_SPACE      = ord(b" ")
_TAB        = ord(b"\t")

# Brackets and delimiters
_LBRACKET   = ord(b"[")
_RBRACKET   = ord(b"]")
_LPAREN     = ord(b"(")
_RPAREN     = ord(b")")
_LANGLE     = ord(b"<")
_RANGLE     = ord(b">")
_LCURLY     = ord(b"{")
_RCURLY     = ord(b"}")

# Punctuation
_BACKSLASH  = ord(b"\\")
_BACKTICK   = ord(b"`")
_TILDE      = ord(b"~")
_BANG       = ord(b"!")
_HASH       = ord(b"#")
_STAR       = ord(b"*")
_DASH       = ord(b"-")
_PLUS       = ord(b"+")
_DOLLAR     = ord(b"$")
_PERCENT    = ord(b"%")
_CARET      = ord(b"^")
_COLON      = ord(b":")
_EQUALS     = ord(b"=")
_SLASH      = ord(b"/")
_QUOTE      = ord(b'"')
_SQUOTE     = ord(b"'")
_UNDERSCORE = ord(b"_")
_PIPE       = ord(b"|")
_DOT        = ord(b".")

# Sets
_WHITESPACE   = frozenset({_SPACE, _TAB})
_QUOTES       = frozenset({_QUOTE, _SQUOTE})
_MARKERS      = frozenset({_DASH, _STAR, _PLUS})
_ESCAPABLE    = frozenset({
    _LBRACKET, _RBRACKET, _LPAREN, _RPAREN, _LANGLE, _RANGLE,
    _LCURLY,   _RCURLY,   _BANG,   _STAR,   _HASH,   _TILDE,
    _DOLLAR,   _BACKSLASH,_BACKTICK,_DASH,  _PLUS,   _CARET,
    _COLON,    _QUOTE,    _SQUOTE, _PIPE,   _UNDERSCORE, _DOT, _PERCENT,
})
_BLOCK_TAGS: frozenset[bytes] = frozenset(
    tag.encode() for tag in BLOCK_LEVEL_ELEMENTS
)

# fmt: on

# ---------------------------------------------------------------------------
# Classes
# ---------------------------------------------------------------------------


class Cursor:
    """Cursor over a `bytes` buffer.

    The cursor tracks:

    * `pos` - current byte offset (absolute, i.e. includes `shift`).
    * `col` - column within the current line (0-based, reset on newline).
    * `data` - the underlying byte buffer.
    * `shift` - offset added to all emitted spans.
    """

    __slots__ = ("col", "data", "end", "pos", "shift")

    def __init__(self, data: bytes, shift: int = 0) -> None:
        self.data = data
        self.end = len(data)
        self.pos = 0
        self.col = 0
        self.shift = shift

    def at_line_start(self) -> bool:
        """Return whether the cursor is at the start of a line."""
        return self.col == 0 or self._is_blank_prefix()

    def peek(self, offset: int = 0) -> int:
        """Return the byte at the current position plus the given offset."""
        i = self.pos + offset
        if i < self.end:
            return self.data[i]
        return -1

    def advance(self, n: int = 1) -> None:
        """Advance the cursor by `n` bytes."""
        for _ in range(n):
            if self.pos < self.end:
                if self.data[self.pos] in (_CR, _NL):
                    self.col = 0
                else:
                    self.col += 1
                self.pos += 1

    def span(self, start: int, end: int) -> Span:
        """Create a `Span` in the outer coordinate system."""
        return Span(self.shift + start, self.shift + end)

    def _is_blank_prefix(self) -> bool:
        """Return whether there's only whitespace before the cursor."""
        i = self.pos - 1
        while i >= 0 and self.data[i] not in (_CR, _NL):
            if self.data[i] not in _WHITESPACE:
                return False
            i -= 1
        return True


# ---------------------------------------------------------------------------
# Functions
# ---------------------------------------------------------------------------


def _scan(cursor: Cursor) -> Iterator[Reference]:
    """Yield references."""
    while cursor.pos < cursor.end:
        char = cursor.data[cursor.pos]

        # Escaped character or math
        if char == _BACKSLASH:
            next = cursor.peek(1)

            # Math: \[...\]
            if next == _LBRACKET:
                end = _scan_math_block_brackets(cursor)
                if end is not None:
                    cursor.advance(end - cursor.pos)
                    continue

            # Math: \(...\)
            if next == _LPAREN:
                end = _scan_math_inline_parens(cursor)
                if end is not None:
                    cursor.advance(end - cursor.pos)
                    continue

            # Escapable character
            if next in _ESCAPABLE:
                cursor.advance(2)
                continue

            # Literal
            cursor.advance(1)
            continue

        # Code block: ```...``` or `...`
        if char == _BACKTICK:
            # Consume the full run so a failed match does not leave trailing
            # backticks to be rescanned as new delimiters.
            count = 1
            while cursor.peek(count) == _BACKTICK:
                count += 1
            if (
                cursor.at_line_start()
                and count >= 3  # noqa: PLR2004
                and cursor.peek(count) in (_SPACE, _TAB, _CR, _NL, -1)
            ):
                end = _scan_fenced_code(cursor, _BACKTICK)
                if end is not None:
                    cursor.advance(end - cursor.pos)
                    continue

                # Literal
                cursor.advance(count)
                continue

            # Inline code: `...`
            end = _scan_inline_code(cursor)
            if end is not None:
                cursor.advance(end - cursor.pos)
                continue

            # Literal
            cursor.advance(count)
            continue

        # Code block: ~~~...~~~
        if char == _TILDE and cursor.at_line_start():
            end = _scan_fenced_code(cursor, _TILDE)
            if end is not None:
                cursor.advance(end - cursor.pos)
                continue

            # Literal
            cursor.advance(1)
            continue

        # Math: $$...$$ or $...$
        if char == _DOLLAR:
            if cursor.peek(1) == _DOLLAR:
                end = _scan_math_block(cursor)
                if end is not None:
                    cursor.advance(end - cursor.pos)
                    continue

            # Math: $...$
            end = _scan_math_inline(cursor)
            if end is not None:
                cursor.advance(end - cursor.pos)
                continue

            # Literal
            cursor.advance(1)
            continue

        # HTML or autolink
        if char == _LANGLE:
            # HTML comment: <!-- ... -->
            end = _scan_html_comment(cursor)
            if end is not None:
                cursor.advance(end - cursor.pos)
                continue

            # HTML block: <tag>...</tag>
            if cursor.at_line_start():
                start = cursor.pos
                end = _scan_html_block(cursor)
                if end is not None:
                    yield from _scan_html_links(
                        Cursor(cursor.data[start:end], cursor.shift + start)
                    )
                    cursor.advance(end - cursor.pos)
                    continue

            # HTML link: <a href="..."> or <img src="...">
            result = _scan_html_tag(cursor)
            if result is not None:
                end, refs = result
                yield from refs
                cursor.advance(end - cursor.pos)
                continue

            # Autolink: <http://example.com>
            ref = _scan_autolink(cursor)
            if ref is not None:
                yield ref
                continue

            # Literal
            cursor.advance(1)
            continue

        # Jinja expression: {{...}} or {%...%} or {#...#}
        if char == _LCURLY:
            end = _scan_jinja(cursor)
            if end is not None:
                cursor.advance(end - cursor.pos)
                continue

            # Literal
            cursor.advance(1)
            continue

        # Exclamation mark - images (inline and reference)
        if char == _BANG and cursor.peek(1) == _LBRACKET:
            ref = _scan_image_or_image_ref(cursor)
            if ref is not None:
                yield ref
                continue

            # Literal
            cursor.advance(1)
            continue

        # Link or footnote
        if char == _LBRACKET:
            # Wikilink: [[target]]
            if cursor.peek(1) == _LBRACKET:
                ref = _scan_wikilink(cursor)
                if ref is not None:
                    yield ref
                    continue

            # Footnote reference or definition: [^id] or [^id]: body
            if cursor.peek(1) == _CARET:
                result = _scan_footnote_ref_or_def(cursor)
                if result is not None:
                    if isinstance(result, Reference):
                        yield result
                    else:
                        yield from result
                    continue

            # Link definition: [id]: href
            if cursor.at_line_start():
                ref = _scan_link_def(cursor)
                if ref is not None:
                    yield ref
                    continue

            # Link or link reference: [text](href) or [text][id] or [text]
            ref = _scan_link_or_link_ref(cursor)
            if ref is not None:
                yield ref

                # References inside link text: [![alt](img)](href)
                yield from _scan_refs(cursor, ref)
                continue

            # Literal
            cursor.advance(1)
            continue

        # Abbreviation definition: *[abbr]: body
        if (
            char == _STAR
            and cursor.at_line_start()
            and cursor.peek(1) == _LBRACKET
        ):
            end = _scan_abbreviation(cursor)
            if end is not None:
                cursor.advance(end - cursor.pos)
                continue

            # Literal
            cursor.advance(1)
            continue

        # List marker at line start - tasklist checkbox
        if char in _MARKERS and cursor.at_line_start():
            end = _scan_tasklist_checkbox(cursor)
            if end is not None:
                cursor.advance(end - cursor.pos)
                continue

            # Literal
            cursor.advance(1)
            continue

        # Literal
        cursor.advance(1)


# ---------------------------------------------------------------------------


def _scan_link_or_link_ref(cursor: Cursor) -> Link | LinkReference | None:
    """Scan for link or link reference.

    This function attempts to parse a link or link reference, since both start
    with `[` and have the same link text syntax. If successful, it returns a
    `Link` if an href is found in parentheses, or a `LinkReference` if an
    optional id is found in brackets, otherwise it returns `None`.
    """
    start = cursor.pos
    if cursor.data[start] != _LBRACKET:
        return None

    # Consume link text
    text = _scan_link_text(cursor, start + 1)
    if text is None:
        return None

    # Consume link href in parenthesis, if present
    end = text.end - cursor.shift + 1
    if end < cursor.end and cursor.data[end] == _LPAREN:
        result = _scan_link_href(cursor, end)
        if result is not None:
            href, end = result

            # Advance cursor and return link
            cursor.advance(end - start)
            return Link(
                cursor.shift + start,
                cursor.shift + end,
                "link",
                text,
                href,
            )

    # Consume link id
    after_text = end
    id, end = _scan_link_id(cursor, end)

    # Ignore empty shortcut references like `[]` or `[][]`
    if id is None and text.start == text.end:
        return None

    # Ignore Python Markdown's table-of-contents marker
    if id is None and _is_toc_marker(cursor, text, after_text):
        return None

    # Ignore GitHub callout markers inside blockquotes
    if id is None and _is_callout_marker(cursor, text, after_text):
        return None

    # Advance cursor and return link reference
    cursor.advance(end - start)
    return LinkReference(
        cursor.shift + start,
        cursor.shift + end,
        "link",
        text,
        id or text,
    )


def _scan_link_def(cursor: Cursor) -> LinkDefinition | None:
    """Scan for link definition."""
    start = cursor.pos

    # Skip if there're more than four spaces of indentation
    if cursor.col > 3:  # noqa: PLR2004
        return None

    # Consume link id
    id, end = _scan_link_id(cursor, start)
    if id is None:
        return None

    # Skip if at end of line or not followed by colon
    if end >= cursor.end or cursor.data[end] != _COLON:
        return None

    # Skip whitespace and consume angle-bracket href, if present
    end = _skip_whitespace(cursor, end + 1)
    if end < cursor.end and cursor.data[end] == _LANGLE:
        result = _scan_link_href_in_angle_brackets(cursor, end)
        if result is not None:
            href, end = result
            while end < cursor.end and cursor.data[end] not in (_CR, _NL):
                end += 1

            # Advance cursor and return link definition
            cursor.advance(end - start)
            return LinkDefinition(
                cursor.shift + start,
                cursor.shift + end,
                id,
                href,
            )

    # Consume link href until whitespace or quote
    begin = end
    while end < cursor.end:
        if cursor.data[end] in (_SPACE, _TAB, _CR, _NL, _QUOTE):
            break

        # Literal
        end += 1

    # Skip if we didn't find an href
    href = cursor.span(begin, end)
    if href.start == href.end:
        return None

    # Advance cursor and return link definition
    end = _skip_whitespace_newline(cursor, end)
    cursor.advance(end - start)
    return LinkDefinition(
        cursor.shift + start,
        cursor.shift + end,
        id,
        href,
    )


# ---------------------------------------------------------------------------


def _scan_link_text(cursor: Cursor, start: int) -> Span | None:
    """Scan for link text, handling nested brackets and escapes.

    This function assumes the opening `[` has already been consumed and starts
    scanning from the first character of the link text. It returns the span of
    the link text (i.e. the content between the outermost brackets) or `None`
    if no matching closing `]` is found.
    """
    depth = 1

    # Start after the opening [
    end = start
    while end < cursor.end:
        char = cursor.data[end]

        # Skip escaped character
        if char == _BACKSLASH and end + 1 < cursor.end:
            end += 2
            continue

        # Increment depth
        if char == _LBRACKET:
            depth += 1

        # Decrement depth, terminate if we reach 0
        elif char == _RBRACKET:
            depth -= 1

            # Terminate if brackets are balanced
            if depth == 0:
                return cursor.span(start, end)

        # Literal
        end += 1

    # Unmatched
    return None


def _scan_link_href(cursor: Cursor, start: int) -> tuple[Span, int] | None:
    """Scan for link href, handling nested parenthesis and escapes."""
    if cursor.data[start] != _LPAREN:
        return None

    # Skip whitespace and consume angle-bracket href, if present
    end = _skip_whitespace(cursor, start + 1)
    if end < cursor.end and cursor.data[end] == _LANGLE:
        result = _scan_link_href_in_angle_brackets(cursor, end)
        if result is not None:
            href, pos = result

            # Skip whitespace and return link if we're at the title or end
            pos = _skip_whitespace_newline(cursor, pos)
            if pos < cursor.end and cursor.data[pos] == _QUOTE:
                while pos < cursor.end and cursor.data[pos] != _RPAREN:
                    if cursor.data[pos] in (_CR, _NL):
                        return None

                    # Literal
                    pos += 1

            # Terminate if we found a closing )
            if pos < cursor.end and cursor.data[pos] == _RPAREN:
                return href, pos + 1

    # Start after the opening (
    depth = 0
    while end < cursor.end:
        char = cursor.data[end]

        # Skip escaped character
        if char == _BACKSLASH and end + 1 < cursor.end:
            end += 2
            continue

        # Increment depth
        if char == _LPAREN:
            depth += 1

        # Decrement depth, terminate if we reach 0
        elif char == _RPAREN:
            if depth == 0:
                break
            depth -= 1

        # Terminate if we reach quote and trim excess whitespace
        elif char in _QUOTES:
            while cursor.data[end - 1] in _WHITESPACE:
                end -= 1
            break

        # Literal
        end += 1

    # Skip optional title and whitespace
    href = cursor.span(start + 1, end)
    while end < cursor.end and cursor.data[end] != _RPAREN:
        if cursor.data[end] in (_CR, _NL):
            return None

        # Literal
        end += 1

    # Terminate if we found a closing )
    if end < cursor.end:
        return href, end + 1

    # Unmatched
    return None


def _scan_link_href_in_angle_brackets(
    cursor: Cursor, end: int
) -> tuple[Span, int] | None:
    """Scan for link href in angle brackets."""
    end += 1

    # Scan until > or end of line
    start = end
    while (
        end < cursor.end
        and cursor.data[end] != _RANGLE
        and cursor.data[end] not in (_CR, _NL)
    ):
        end += 1

    # Skip closing >, or abort if we reached end of line
    if end < cursor.end and cursor.data[end] == _RANGLE:
        return cursor.span(start, end), end + 1

    # Unmatched
    return None


def _scan_link_id(cursor: Cursor, pos: int) -> tuple[Span | None, int]:
    """Scan for link id.

    Link ids occur in two possible contexts: link references `[text][id]` and
    link definitions `[id]: href`. If no id is found, this function returns
    `None`. The returned position either equals the given position, if no id
    was found, or the position immediately after the closing `]` of the id.
    """
    start = _skip_whitespace_newline(cursor, pos)
    if start == cursor.end:
        return None, start

    # Consume id, but only if it's not a footnote or wikilink
    if cursor.data[start] == _LBRACKET and (
        start + 1 >= cursor.end
        or cursor.data[start + 1] not in (_CARET, _LBRACKET)
    ):
        result = _scan_link_id_identifier(cursor, start + 1)
        if result is not None:
            return result

    # Unmatched
    return None, start


def _scan_link_id_identifier(
    cursor: Cursor, start: int
) -> tuple[Span | None, int] | None:
    """Scan for link id identifier."""
    end = start
    while end < cursor.end:
        char = cursor.data[end]

        # Skip escaped character
        if char == _BACKSLASH and end + 1 < cursor.end:
            end += 2
            continue

        # Terminate if we reach a [, as nested brackets are not allowed
        if char == _LBRACKET:
            return None

        # Terminate if we reach a closing ]
        if char == _RBRACKET:
            if end == start:
                return None, end + 1

            # Skip closing ] and return id
            return cursor.span(start, end), end + 1

        # Literal
        end += 1

    # Unmatched
    return None


# ---------------------------------------------------------------------------


def _scan_image_or_image_ref(cursor: Cursor) -> Link | LinkReference | None:
    """Scan for image or image reference.

    This function first attempts to parse a link or link reference, since both
    start with `[` and have the same link text syntax.
    """
    start = cursor.pos
    if cursor.data[start] != _BANG:
        return None
    if cursor.data[start + 1] != _LBRACKET:
        return None

    # Consume image alt
    text = _scan_link_text(cursor, start + 2)
    if text is None:
        return None

    # Consume image href in parenthesis, if present
    end = text.end - cursor.shift + 1
    if end < cursor.end and cursor.data[end] == _LPAREN:
        result = _scan_link_href(cursor, end)
        if result is not None:
            href, end = result

            # Advance cursor and return link
            cursor.advance(end - start)
            return Link(
                cursor.shift + start,
                cursor.shift + end,
                "image",
                text,
                href,
            )

    # Consume image id
    id, end = _scan_link_id(cursor, end)

    # Advance cursor and return image reference
    cursor.advance(end - start)
    return LinkReference(
        cursor.shift + start,
        cursor.shift + end,
        "image",
        text,
        id or text,
    )


def _scan_refs(
    cursor: Cursor, ref: Link | LinkReference
) -> Iterator[Reference]:
    """Scan for inner images inside the given link.

    This function is called after successfully parsing a link or link reference.
    It scans the link text for any inner images (inline or reference) and yields
    them as `Link` objects with kind `"image"`. This allows us to capture image
    references inside link texts.
    """
    if ref.kind != "link":
        return

    # Calculate the absolute positions of the link text
    start = ref.text.start - cursor.shift
    end = ref.text.end - cursor.shift

    # Scan the link or link reference text for inner images
    yield from _scan(Cursor(cursor.data[start:end], ref.text.start))


# ---------------------------------------------------------------------------


def _scan_autolink(cursor: Cursor) -> Link | None:
    """Scan for autolink."""
    start = cursor.pos
    if cursor.data[start] != _LANGLE:
        return None

    # Skip if schema doesn't match http:// or https://
    schema = cursor.data[start + 1 : start + 9].lower()
    if schema.startswith((b"https://", b"http://")):
        result = _scan_link_href_in_angle_brackets(cursor, start)
        if result is not None:
            href, end = result

            # Advance cursor and return link
            cursor.advance(end - start)
            return Link(
                cursor.shift + start,
                cursor.shift + end,
                "autolink",
                href,
                href,
            )

    # Unmatched
    return None


def _scan_wikilink(cursor: Cursor) -> Link | None:
    """Scan for wikilink.

    This function attempts to parse a wikilink, which starts with `[[` and ends
    with `]]`. The content between the brackets is the link target, which is
    also used as the link text. If the wikilink is followed by an `[id]`, it's
    treated as a link reference, to align with Python Markdown.
    """
    start = cursor.pos
    if cursor.data[start] != _LBRACKET or cursor.data[start + 1] != _LBRACKET:
        return None

    # Consume wikilink target
    end = _find_bracket(cursor, start + 2)
    if (
        end + 1 >= cursor.end
        or cursor.data[end] != _RBRACKET
        or cursor.data[end + 1] != _RBRACKET
    ):
        return None

    # Skip if we didn't find a target
    text = cursor.span(start + 2, end)
    if text.start == text.end:
        return None

    # Skip closing brackets
    end += 2

    # If the wikilink is followed by an [id], it's a regular link, LOL
    begin = _skip_whitespace_newline(cursor, end)
    if (
        begin < cursor.end
        and cursor.data[begin] == _LBRACKET
        and (begin + 1 >= cursor.end or cursor.data[begin + 1] != _CARET)
    ):
        close = _find_bracket(cursor, begin + 1)
        if close < cursor.end and cursor.data[close] == _RBRACKET:
            return None

    # Advance cursor and return link
    cursor.advance(end - start)
    return Link(
        cursor.shift + start,
        cursor.shift + end,
        "wikilink",
        text,
        text,
    )


# ---------------------------------------------------------------------------


def _scan_footnote_ref_or_def(
    cursor: Cursor,
) -> Reference | Iterator[Reference] | None:
    """Scan for footnote reference or definition."""
    start = cursor.pos
    if cursor.data[start] != _LBRACKET or cursor.data[start + 1] != _CARET:
        return None

    # Consume footnote id
    end = _find_bracket(cursor, start + 2)
    if end >= cursor.end or cursor.data[end] != _RBRACKET:
        return None

    id = cursor.span(start + 2, end)

    # Validate id - non-empty, no internal spaces
    text = cursor.data[id.start : id.end]
    if _SPACE in text or _TAB in text:
        return None

    # Skip closing bracket
    end += 1

    # Consume footnote definition
    if (
        end < cursor.end
        and cursor.data[end] == _COLON
        and cursor.at_line_start()
    ):
        return _scan_footnote_def(cursor, id, end)

    # Advance cursor and return link
    cursor.advance(end - start)
    return FootnoteReference(
        cursor.shift + start,
        cursor.shift + end,
        id,
    )


def _scan_footnote_def(
    cursor: Cursor,
    id: Span,  # noqa: A002
    end: int,
) -> Iterator[Reference]:
    """Scan for footnote definition and recurse into its body."""
    start = cursor.pos

    # Skip colon and leading whitespace to find body start
    end = _skip_whitespace(cursor, end + 1)
    body_start = end
    end = _skip_line(cursor, end)

    # Continuation lines: indented lines belong to the same definition
    while end < cursor.end and cursor.data[end] in _WHITESPACE:
        end = _skip_line(cursor, end)

    # Trim trailing newlines from the body span
    body_end = end
    while body_end > body_start and cursor.data[body_end - 1] in (_CR, _NL):
        body_end -= 1

    # Build and emit the definition
    body = cursor.span(body_start, body_end)

    # Advance cursor and return footnote definition
    cursor.advance(end - start)
    yield FootnoteDefinition(
        cursor.shift + start,
        cursor.shift + end,
        id,
        body,
    )

    # Scan the footnote body for inner references
    text = cursor.data[body_start:body_end]
    yield from _scan(Cursor(text, cursor.shift + body_start))


# ---------------------------------------------------------------------------


def _scan_fenced_code(cursor: Cursor, char: int) -> int | None:
    """Scan for fenced code block using ``` or ~~~.

    Follows Python Markdown's rules:

    * The closing fence must be the **same character** as the opening fence
      (backticks cannot close a tilde fence and vice versa).
    * The closing fence must be **exactly** the same length as the opening
      fence — longer or shorter fences do not close the block.
    * An **unclosed** fence is not a code block; parsing continues normally
      after the opening line.
    """
    pos = cursor.pos

    # Compute length of opening fence
    opening = 0
    while pos < cursor.end and cursor.data[pos] == char:
        opening += 1
        pos += 1

    # Skip if the fence is too short
    if opening < 3:  # noqa: PLR2004
        return None

    # Skip to end of line and search for closing fence
    pos = _skip_line(cursor, pos)
    while pos < cursor.end:
        start = _skip_whitespace(cursor, pos)

        # Compute length of closing fence
        end = start
        while end < cursor.end and cursor.data[end] == char:
            end += 1

        # Skip to next line and terminate if we found a closing fence
        if end - start == opening:
            rest = _skip_whitespace(cursor, end)
            if rest >= cursor.end or cursor.data[rest] in (_CR, _NL):
                return _skip_line(cursor, end)

        # Skip to next line
        pos = _skip_line(cursor, pos)

    # Unmatched
    return None


def _scan_inline_code(cursor: Cursor) -> int | None:
    """Scan for inline code block."""
    pos = cursor.pos

    # Count opening backticks
    opening = 0
    while pos < cursor.end and cursor.data[pos] == _BACKTICK:
        opening += 1
        pos += 1

    # Skip if there are no backticks
    if opening == 0:
        return None

    # Search for matching closing backticks
    while pos < cursor.end:
        if cursor.data[pos] == _BACKTICK:
            end = pos

            # Count closing backticks
            while end < cursor.end and cursor.data[end] == _BACKTICK:
                end += 1

            # Terminate if we found matching closing backticks
            if end - pos == opening:
                return end
            pos = end
        else:
            pos += 1

    # Unmatched
    return None


# ---------------------------------------------------------------------------


def _scan_math_inline(cursor: Cursor) -> int | None:
    """Scan for `$...$` math."""
    end = cursor.pos
    if end >= cursor.end or cursor.data[end] != _DOLLAR:
        return None

    # Skip opening $ and abort if it's followed by whitespace or another $
    end += 1
    if end >= cursor.end or (
        cursor.data[end] in (_DOLLAR, _SPACE, _TAB, _CR, _NL)
    ):
        return None

    # Scan for closing $
    while end < cursor.end and cursor.data[end] not in (_CR, _NL):
        if cursor.data[end] == _BACKTICK:
            return None
        if (
            cursor.data[end] == _DOLLAR
            # Closing $ must not be preceded by whitespace or $
            and cursor.data[end - 1] not in (_SPACE, _TAB, _DOLLAR)
            # Closing $ must not be followed by $ (would be part of $$)
            and (end + 1 >= cursor.end or cursor.data[end + 1] != _DOLLAR)
        ):
            return end + 1

        # Literal
        end += 1

    # Unmatched
    return None


def _scan_math_block(cursor: Cursor) -> int | None:
    """Scan for `$$ ... $$` math, both single-line and multi-line."""
    return _scan_math(cursor, b"$$", b"$$", multiline=True)


def _scan_math_block_brackets(cursor: Cursor) -> int | None:
    r"""Scan for `\[ ... \]` math, both single-line and multi-line."""
    return _scan_math(cursor, b"\\[", b"\\]", multiline=True)


def _scan_math_inline_parens(cursor: Cursor) -> int | None:
    r"""Scan for `\( ... \)` math, only single-line."""
    return _scan_math(cursor, b"\\(", b"\\)", multiline=False)


def _scan_math(
    cursor: Cursor, opening: bytes, closing: bytes, *, multiline: bool
) -> int | None:
    """Scan for opening and closing math delimiters."""
    start = cursor.pos
    if cursor.data[start : start + 2] != opening:
        return None

    # Consume opening delimiter and scan for closing delimiter
    start += 2
    while start < cursor.end:
        if not multiline and cursor.data[start] in (_CR, _NL):
            return None

        # Consume closing delimiter and return if we found it
        if cursor.data[start : start + 2] == closing:
            start += 2

            # Skip optional whitespace and newlines after closing delimiter
            if multiline:
                return _skip_line_ending(cursor, start)

            # Otherwise return immediately
            return start

        # Literal
        start += 1

    # Unmatched
    return None


# ---------------------------------------------------------------------------


def _scan_html_links(cursor: Cursor) -> Iterator[Link]:
    """Scan for HTML src and href attributes to extract links."""
    while cursor.pos < cursor.end:
        # Skip HTML comments
        if cursor.data[cursor.pos : cursor.pos + 4] == b"<!--":
            end = _scan_html_comment(cursor)
            if end is not None:
                cursor.advance(end - cursor.pos)
            else:
                cursor.pos = cursor.end
            continue

        # Scan HTML tags for links
        if cursor.data[cursor.pos] == _LANGLE:
            result = _scan_html_tag(cursor)
            if result is not None:
                end, links = result
                yield from links

                # Skip links
                cursor.advance(end - cursor.pos)
                continue

        # Literal
        cursor.advance(1)


def _scan_html_comment(cursor: Cursor) -> int | None:
    """Scan for an HTML comment."""
    pos = cursor.pos
    if cursor.data[pos : pos + 4] != b"<!--":
        return None

    # Search for closing -->
    pos = cursor.data.find(b"-->", pos + 4)
    if pos == -1:
        return None

    # Skip past -->
    return pos + 3


def _scan_html_attrs(
    cursor: Cursor, pos: int
) -> tuple[int, dict[bytes, Span]] | None:
    """Scans for HTML attributes."""
    attrs: dict[bytes, Span] = {}
    while pos < cursor.end and cursor.data[pos] != _RANGLE:
        if cursor.data[pos] in (_SPACE, _TAB, _CR, _NL, _SLASH):
            pos += 1
            continue

        # Skip if attribute name doesn't start with alpha, _, or :
        if not (
            _is_alpha(cursor.data[pos])
            or cursor.data[pos] in (ord(b"_"), ord(b":"))
        ):
            pos += 1
            continue

        # Consume attribute name
        start = pos
        while pos < cursor.end and (
            cursor.data[pos]
            not in (_SPACE, _TAB, _CR, _NL, _RANGLE, _SLASH, _EQUALS)
        ):
            pos += 1

        # Extract and canonicalize attribute name
        name = cursor.data[start:pos].lower()

        # Skip whitespace before =
        while pos < cursor.end and cursor.data[pos] in (_SPACE, _TAB, _CR, _NL):
            pos += 1

        # Boolean attribute - record presence with empty span
        if pos >= cursor.end or cursor.data[pos] != _EQUALS:
            attrs[name] = cursor.span(pos, pos)
            continue

        # Skip = and whitespace after it
        pos += 1
        while pos < cursor.end and cursor.data[pos] in (_SPACE, _TAB, _CR, _NL):
            pos += 1

        # Quoted value
        if pos < cursor.end and cursor.data[pos] in _QUOTES:
            quote = cursor.data[pos]
            start = pos + 1

            # Scan until closing quote
            pos += 1
            while pos < cursor.end and cursor.data[pos] != quote:
                pos += 1

            # Extract attribute value and skip closing quote
            attrs[name] = cursor.span(start, pos)
            if pos < cursor.end:
                pos += 1  # past closing quote

        # Unquoted value
        else:
            start = pos
            while pos < cursor.end and (
                cursor.data[pos] not in (_SPACE, _TAB, _CR, _NL, _RANGLE)
            ):
                pos += 1

            # Extract attribute value and skip closing quote
            attrs[name] = cursor.span(start, pos)

    # Unclosed tag
    if pos >= cursor.end:
        return None

    # Skip past >
    return pos + 1, attrs


def _scan_html_tag(cursor: Cursor) -> tuple[int, list[Link]] | None:
    """Scan for HTML opening tag, returning its end position and any links."""
    if cursor.pos >= cursor.end or cursor.data[cursor.pos] != _LANGLE:
        return None

    # Closing tag - don't consume
    end = cursor.pos + 1
    if end < cursor.end and cursor.data[end] == _SLASH:
        return None

    # Must start with ASCII letter
    if end >= cursor.end or not _is_alpha(cursor.data[end]):
        return None

    # Consume tag name
    while end < cursor.end and (
        _is_alnum(cursor.data[end]) or cursor.data[end] == _DASH
    ):
        end += 1

    # Must be followed by whitespace, >, or /
    if end >= cursor.end or (
        cursor.data[end] not in (_SPACE, _TAB, _CR, _NL, _RANGLE, _SLASH)
    ):
        return None

    # Parse attributes and extract links from src and href
    result = _scan_html_attrs(cursor, end)
    if result is not None:
        pos, attrs = result

        # Extract links from src and href attributes, if present
        links: list[Link] = []
        for key in (b"src", b"href"):
            span = attrs.get(key)
            if span is not None and span.start < span.end:
                links.append(Link(span.start, span.end, "html", span, span))

        # Skip past the end of the tag and return any links we found
        return pos, links

    # Unmatched
    return None


def _scan_html_block(cursor: Cursor) -> int | None:
    """Consume an HTML block that does not carry a `markdown` attribute."""
    if cursor.pos >= cursor.end or cursor.data[cursor.pos] != _LANGLE:
        return None

    # Consume tag name
    pos = cursor.pos + 1
    if pos >= cursor.end or not _is_alpha(cursor.data[pos]):
        return None
    start = pos
    while pos < cursor.end and _is_alnum(cursor.data[pos]):
        pos += 1

    # Only block-level tags suppress Markdown parsing inside them
    name = cursor.data[start:pos]
    if name.lower() not in _BLOCK_TAGS:
        return None
    start = pos

    # Parse opening tag attributes - must fit on a single line
    result = _scan_html_attrs(cursor, pos)
    if result is None:
        return None

    # Skip if there are newlines in the opening tag
    pos, attrs = result
    if b"\n" in cursor.data[start:pos] or b"\r" in cursor.data[start:pos]:
        return None

    # If markdown attribute is set to a valid value, the md_in_html extension
    # will parse the block's content as Markdown — hand back to the main loop
    span = attrs.get(b"markdown")
    if span is not None:
        value = cursor.data[span.start : span.end]
        if value in (b"", b"1", b"block", b"span"):
            return None

    # Inline form: opening and closing tag on the same line
    closing_tag = b"</" + name + b">"
    eol = _find_line_end(cursor, pos)

    # Search for closing tag on the same line
    close = cursor.data.find(closing_tag, pos, eol + 1)
    if close != -1:
        return _skip_line_ending(cursor, close + len(closing_tag))

    # Multi-line form: require a newline (with optional trailing whitespace)
    i = _skip_whitespace(cursor, pos)
    if i >= cursor.end or cursor.data[i] not in (_CR, _NL):
        return None
    if (
        cursor.data[i] == _CR
        and i + 1 < cursor.end
        and cursor.data[i + 1] == _NL
    ):
        i += 1
    pos = i + 1

    # Find </tag> at line start - using find is much faster than regex or manual
    # parsing and is still efficient since we only search for the closing tag
    while True:
        pos = cursor.data.find(closing_tag, pos)
        if pos == -1:
            return None

        # Search for closing tag
        start = _find_line_start(cursor, pos)
        if cursor.data[start:pos].strip() == b"":
            return _skip_line_ending(cursor, pos + len(closing_tag))

        # Continue
        pos += 1


# ---------------------------------------------------------------------------


def _scan_jinja(cursor: Cursor) -> int | None:
    """Scan for Jinja2 syntax."""
    start = cursor.pos
    if start + 1 >= cursor.end:
        return None

    # Determine closing delimiter based on second character
    char = cursor.data[start + 1]
    if char == _PERCENT:
        close = b"%}"
    elif char == _LCURLY:
        close = b"}}"
    elif char == _HASH:
        close = b"#}"
    else:
        return None

    # Scan for closing delimiter, allowing line endings but not blank lines
    end = start + 2
    newlines = 0
    while end < cursor.end:
        char = cursor.data[end]

        # Check for two consecutive line endings
        if char in (_CR, _NL):
            newlines += 1

            # Terminate if we encounter a blank line
            if newlines == 2:  # noqa: PLR2004
                return None
            if (
                char == _CR
                and end + 1 < cursor.end
                and cursor.data[end + 1] == _NL
            ):
                end += 1
        else:
            newlines = 0

        # Check for closing delimiter
        if cursor.data[end : end + 2] == close:
            return end + 2

        # Literal
        end += 1

    # Unmatched
    return None


# ---------------------------------------------------------------------------


def _scan_abbreviation(cursor: Cursor) -> int | None:
    """Scan for abbreviations."""
    start = cursor.pos
    if cursor.data[start : start + 2] != b"*[":
        return None

    # Find closing ]
    end = _find_bracket(cursor, start + 2)
    if end >= cursor.end or cursor.data[end] != _RBRACKET:
        return None

    # Skip closing ] and find :
    end += 1
    if end >= cursor.end or cursor.data[end] != _COLON:
        return None

    # Skip to end of line
    return _skip_line(cursor, end)


def _scan_tasklist_checkbox(cursor: Cursor) -> int | None:
    """Scan for tasklist checkbox."""
    pos = _skip_whitespace(cursor, cursor.pos)
    if pos >= cursor.end or cursor.data[pos] not in _MARKERS:
        return None

    # Skip list marker and following space
    pos += 1
    if pos >= cursor.end or cursor.data[pos] not in _WHITESPACE:
        return None

    # Skip whitespace before checkbox
    pos = _skip_whitespace(cursor, pos)

    # Skip valid checkbox values [ ], [x], and [X]
    if pos + 2 >= cursor.end:
        return None
    if cursor.data[pos] != _LBRACKET:
        return None
    if cursor.data[pos + 1] not in (_SPACE, ord("x"), ord("X")):
        return None
    if cursor.data[pos + 2] != _RBRACKET:
        return None

    # Return position after checkbox
    return pos + 3


# ---------------------------------------------------------------------------


def _is_toc_marker(cursor: Cursor, text: Span, end: int) -> bool:
    """Return whether a shortcut reference is a TOC marker block."""
    start = text.start - cursor.shift - 1
    if cursor.data[start + 1 : end - 1] != b"TOC":
        return False

    # Python Markdown treats the marker as a block, not inline text. A block can
    # be indented up to three spaces before it becomes a code block.
    if not cursor.at_line_start() or cursor.col > 3:  # noqa: PLR2004
        return False

    # The marker must be on a line by itself
    line = _find_line_start(cursor, start)
    if not _is_previous_line_blank(cursor, line):
        return False

    # Skip whitespace after the marker and ensure there's nothing else
    pos = _skip_whitespace(cursor, end)
    if pos < cursor.end and cursor.data[pos] not in (_CR, _NL):
        return False

    # The next line must be blank or non-existent
    pos = _skip_line(cursor, pos)
    return pos >= cursor.end or _is_blank_line(cursor, pos)


def _is_callout_marker(cursor: Cursor, text: Span, end: int) -> bool:
    """Return whether a shortcut reference is a GitHub callout marker."""
    start = text.start - cursor.shift - 1
    if cursor.data[start + 1 : end - 1] not in (
        b"!NOTE",
        b"!TIP",
        b"!IMPORTANT",
        b"!WARNING",
        b"!CAUTION",
    ):
        return False

    # Skip whitespace after the marker and ensure there's nothing else
    pos = _skip_whitespace(cursor, end)
    if pos < cursor.end and cursor.data[pos] not in (_CR, _NL):
        return False

    # Skip to the next line and ensure it's not blank
    found = False
    pos = _find_line_start(cursor, start)
    while pos < start:
        pos = _skip_whitespace(cursor, pos)
        if pos >= start or cursor.data[pos] != _RANGLE:
            return False
        found = True
        pos = _skip_whitespace(cursor, pos + 1)

    # The callout marker must be preceded by one or more > characters
    return found and pos == start


# ---------------------------------------------------------------------------


def _skip_whitespace(cursor: Cursor, pos: int) -> int:
    """Skip horizontal whitespace (spaces and tabs)."""
    while pos < cursor.end and cursor.data[pos] in _WHITESPACE:
        pos += 1
    return pos


def _skip_whitespace_newline(cursor: Cursor, pos: int) -> int:
    """Skip trailing horizontal whitespace and at most one newline."""
    pos = _skip_whitespace(cursor, pos)
    if pos < cursor.end and cursor.data[pos] in (_CR, _NL):
        if (
            cursor.data[pos] == _CR
            and pos + 1 < cursor.end
            and cursor.data[pos + 1] == _NL
        ):
            pos += 1
        pos = _skip_whitespace(cursor, pos + 1)
    return pos


def _skip_line_ending(cursor: Cursor, pos: int) -> int:
    """Skip trailing horizontal whitespace and an optional newline."""
    while pos < cursor.end and cursor.data[pos] in _WHITESPACE:
        pos += 1
    if pos < cursor.end and cursor.data[pos] == _CR:
        pos += 1
    if pos < cursor.end and cursor.data[pos] == _NL:
        pos += 1
    return pos


def _skip_line(cursor: Cursor, pos: int) -> int:
    """Skip to the next line, consuming the newline."""
    while pos < cursor.end and cursor.data[pos] not in (_CR, _NL):
        pos += 1
    if (
        pos < cursor.end
        and cursor.data[pos] == _CR
        and pos + 1 < cursor.end
        and cursor.data[pos + 1] == _NL
    ):
        pos += 1
    if pos < cursor.end:
        pos += 1
    return pos


def _is_blank_line(cursor: Cursor, pos: int) -> bool:
    """Return whether the line at the given position is blank."""
    end = _find_line_end(cursor, pos)
    return all(char in _WHITESPACE for char in cursor.data[pos:end])


def _is_previous_line_blank(cursor: Cursor, pos: int) -> bool:
    """Return whether the line before the given position is blank."""
    if pos == 0:
        return True

    # Find the end of the previous line, skipping any trailing newlines
    end = pos - 1
    if end > 0 and cursor.data[end - 1] == _CR:
        end -= 1

    # Find the start of the previous line and check if it's blank
    start = _find_line_start(cursor, end)
    return all(char in _WHITESPACE for char in cursor.data[start:end])


def _find_bracket(cursor: Cursor, pos: int) -> int:
    """Find the next `]` or newline."""
    while (
        pos < cursor.end
        and cursor.data[pos] != _RBRACKET
        and cursor.data[pos] not in (_CR, _NL)
    ):
        pos += 1
    return pos


def _find_line_end(cursor: Cursor, pos: int) -> int:
    """Find the next line ending or end of buffer."""
    while pos < cursor.end and cursor.data[pos] not in (_CR, _NL):
        pos += 1
    return pos


def _find_line_start(cursor: Cursor, pos: int) -> int:
    """Find the first byte after the previous line ending."""
    start = pos - 1
    while start >= 0 and cursor.data[start] not in (_CR, _NL):
        start -= 1
    return start + 1


# ----------------------------------------------------------------------------


def _is_alpha(byte: int) -> bool:
    """Check if byte is ASCII letter."""
    return (ord("A") <= byte <= ord("Z")) or (ord("a") <= byte <= ord("z"))


def _is_alnum(byte: int) -> bool:
    """Check if byte is ASCII letter or digit."""
    return _is_alpha(byte) or (ord("0") <= byte <= ord("9"))
