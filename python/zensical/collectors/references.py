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

import re
from dataclasses import dataclass
from functools import partial
from typing import TYPE_CHECKING, Literal, TypeAlias

from zensical.collectors.exclusions import exclusions
from zensical.utilities.span import Span

if TYPE_CHECKING:
    from collections.abc import Iterator
    from re import Match

# ----------------------------------------------------------------------------
# Classes
# ----------------------------------------------------------------------------


@dataclass
class Link(Span):
    """A link or image, e.g. `[text](href)` or `![alt](href)`."""

    kind: Literal["link", "image", "autolink", "wikilink"]
    """Discriminator identifying the variant."""

    text: Span
    """Span of visible link text (or image alt text)."""

    href: Span
    """Span of the link destination."""


# ----------------------------------------------------------------------------


@dataclass
class LinkReference(Span):
    """A link or image reference, e.g. `[text][id]` or `![alt][id]`."""

    kind: Literal["link", "image"]
    """Discriminator identifying the variant."""

    text: Span
    """Span of the visible link text (or image alt text)."""

    id: Span
    """Span of the link id."""


@dataclass
class LinkDefinition(Span):
    """A link or image definition, e.g. `[id]: href`."""

    id: Span
    """Span of the link id."""

    href: Span
    """Span of the link destination."""


# ----------------------------------------------------------------------------


@dataclass
class FootnoteReference(Span):
    """A footnote reference, e.g. `[^id]`."""

    id: Span
    """Span of the footnote id."""


@dataclass
class FootnoteDefinition(Span):
    """A footnote definition, e.g. `[^id]: body`."""

    id: Span
    """Span of the footnote id."""

    body: Span
    """Span of the footnote body, including multiline text."""


# ----------------------------------------------------------------------------
# Types
# ----------------------------------------------------------------------------

Reference: TypeAlias = (
    Link
    | LinkReference
    | LinkDefinition
    | FootnoteReference
    | FootnoteDefinition
)
"""
A reference extracted from Markdown.

References might include inline links and images, reference-style link usages,
reference definitions, footnote references, and footnote definitions. Each of
those reference carries a source span covering the full matched text, plus
additional metadata for the relevant subcomponents.
"""

# ----------------------------------------------------------------------------
# Constants
# ----------------------------------------------------------------------------

_RE = re.compile(
    rb"""
    # Escaped characters:
    #
    #   \[ \] \! \*
    #
    (?P<escape>
        \\[\[\]!*`]
    )
    |
    # Task list checkbox:
    #
    #   - [ ] text
    #   - [x] text
    #   - [X] text
    #
    (?P<task>
        ^[^\S\n]*[-*+][^\S\n]+\[[xX ]\]
    )
    |
    # Footnote definition:
    #
    #   [^id]: body
    #   [^id]:
    #       body on next line
    #
    (?P<footdef>
        ^[^\S\n]{0,3}\[\^(?P<footdef_id>[^\]]+)\]:[^\S\n]*
        (?P<footdef_body>[^\n]*(?:\n[^\S\n]+[^\n]+)*)
    )
    |
    # Link definition:
    #
    #   [id]: href
    #   [id]: href "optional title"
    #
    (?P<linkdef>
        ^[^\S\n]{0,3}\[(?P<linkdef_id>[^\]]+)\]:[^\S\n]+
        (?P<linkdef_href>\S+)[^\n]*$
    )
    |
    # Footnote reference:
    #
    #   [^id]
    #
    (?P<footref>
        \[\^(?P<footref_id>[^\]]+)\]
    )
    |
    # Wikilink:
    #
    #   [[text]]
    #
    (?P<wikilink>
        \[\[(?P<wikilink_text>[^\]]+)\]\]
    )
    |
    # Inline image:
    #
    #   ![alt](href)
    #   ![alt](href "title")
    #
    (?P<image>
        !\[(?P<image_alt>[^\]]*)\]
        \((?P<image_href>[^)\s]+)[^)]*\)
    )
    |
    # Image reference:
    #
    #   ![alt][id]
    #   ![alt]\n[id]   (label on next line)
    #   ![alt][]       (collapsed)
    #   ![alt]         (shortcut)
    #
    (?P<imageref>
        !\[(?P<imageref_alt>[^\]]*)\]
        (?:[^\S\n]*\n?[^\S\n]*\[(?![\^])(?P<imageref_id>[^\]]*)\])?
    )
    |
    # Inline link:
    #
    #   [text](href)
    #   [text](href "title")
    #
    (?P<link>
        \[(?P<link_text>[^\]]+)\]
        \((?P<link_href>[^)\s]+)[^)]*\)
    )
    |
    # Autolink:
    #
    #   <https://example.com>
    #
    (?P<autolink>
        <(?P<autolink_href>https?://[^>]+)>
    )
    |
    # Abbreviation definitions:
    #
    #   *[HTML]: Hyper Text Markup Language
    #
    (?P<abbr>
        ^\*\[(?P<abbr_text>[^\]]+)\]:[^\n]*$
    )
    |
    # Link reference:
    #
    #   [text][id]
    #   [text]\n[id]   (label on next line)
    #   [text][]       (collapsed)
    #   [text]         (shortcut)
    #
    (?P<linkref>
        \[(?P<linkref_text>[^\]]+)\]
        (?:[^\S\n]*\n?[^\S\n]*\[(?![\^])(?P<linkref_id>[^\]]*)\])?
    )
    """,
    re.VERBOSE | re.MULTILINE,
)
"""
Match link-like constructs in Markdown.

**Extraction and matching order**

References are extracted via the compiled regex. The regex uses alternation
with carefully ordered branches - specificity matters:

- **Escaped characters** - `[` `]` `!` `*` (skipped, not treated as references)
- **Task list checkbox** - `- [ ]` `- [x]` `- [X]`
- **Footnote definition** - `[^id]: body` (block-level, can span lines)
- **Link definition** - `[id]: href` (block-level, with optional title)
- **Footnote reference** - `[^id]` (inline)
- **Wikilink** - `[[target]]` (inline, standalone syntax)
- **Inline image** - `![alt](href)` (inline)
- **Image reference** - `![alt][id]`, `![alt][]`, `![alt]` (inline)
- **Inline link** - `[text](href)` (inline)
- **Autolink** - `<https://...>` (inline, angle-bracket syntax)
- **Abbreviations** - `*[HTML]: Hyper Text Markup Language`
- **Link reference** - `[text][id]`, `[text][]`, `[text]` (inline)

**Why order matters**

- Escaped characters are skipped and not treated as references.
- Block-level definitions (footnote, link) are checked before inline patterns
  to avoid overlaps with similar bracket syntax.
- Images must be checked before links: `![alt](href)` starts with `!`, but
  if not caught early, could conflict with `[text](href)` patterns.
- Autolinks are more specific (`<...>`) and checked before generic reference
  links (`[text][id]`).
- Wikilinks (`[[...]]`) are unambiguous and can appear earlier.

**Shortcut and collapsed references**

Link and image references support three forms:

- **Explicit**: `[text][id]` or `![alt][id]` - the id is explicit
- **Collapsed**: `[text][]` or `![alt][]` - empty brackets, shortcut
- **Shortcut**: `[text]` or `![alt]` - no second brackets, shortcut

In collapsed and shortcut forms, the reference id defaults to the visible text.
"""

# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def references(markdown: bytes, shift: int = 0) -> Iterator[Reference]:
    """Scan Markdown and yield all references.

    Performs a single left-to-right pass over the given Markdown by using an
    alternation regex, yielding one `Reference` per match in source order. No
    state is carried between matches. Results can be collected into a list
    or processed lazily.
    """
    exclude = iter(exclusions(markdown))
    current = next(exclude, None)
    for match in _RE.finditer(markdown):
        kind = match.lastgroup

        # Extract the start and end positions of the full match, and build a
        # partial function to extract spans from named capture groups
        start, end = shift + match.start(), shift + match.end()
        span = partial(_span, shift, match)

        # Advance past exclusions that end before this match
        while current and current.end <= start:
            current = next(exclude, None)

        # If the current exclusion covers the match, skip it
        if current and current.contains(start):
            continue

        # Inline link
        if kind == "link":
            text, href = span("link_text"), span("link_href")
            yield Link(start, end, "link", text, href)

        # Inline image
        elif kind == "image":
            text, href = span("image_alt"), span("image_href")
            yield Link(start, end, "image", text, href)

        # Image reference
        elif kind == "imageref":
            text = span("imageref_alt")
            id = _span_for_id(shift, match, "imageref_id") or text
            yield LinkReference(start, end, "image", text, id)

        # Link reference
        elif kind == "linkref":
            text = span("linkref_text")
            id = _span_for_id(shift, match, "linkref_id") or text
            yield LinkReference(start, end, "link", text, id)

        # Link definition
        elif kind == "linkdef":
            id, href = span("linkdef_id"), span("linkdef_href")
            yield LinkDefinition(start, end, id, href)

        # Footnote reference
        elif kind == "footref":
            id = span("footref_id")
            yield FootnoteReference(start, end, id)

        # Footnote definition
        elif kind == "footdef":
            id, body = span("footdef_id"), span("footdef_body")
            yield FootnoteDefinition(start, end, id, body)

            # Recurse into footnote body to extract nested references, adjusting
            # the shift to account for the position of the body
            yield from references(
                markdown[body.start : body.end], shift + body.start
            )

        # Wikilink
        elif kind == "wikilink":
            text = span("wikilink_text")
            yield Link(start, end, "wikilink", text, text)

        # Autolink
        elif kind == "autolink":
            href = span("autolink_href")
            yield Link(start, end, "autolink", href, href)


# ----------------------------------------------------------------------------


def _span(shift: int, match: Match[bytes], name: str) -> Span:
    """Build a span for a named capture group within a match."""
    start, end = match.start(name), match.end(name)
    return Span(shift + start, shift + end)


def _span_for_id(shift: int, match: Match[bytes], name: str) -> Span | None:
    """Build an id span for a named capture group within a match."""
    if match.group(name):
        return _span(shift, match, name)

    # Return nothing
    return None
