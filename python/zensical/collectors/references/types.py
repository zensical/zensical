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

from dataclasses import dataclass
from typing import Literal, TypeAlias

from zensical.utilities.span import Span

# ----------------------------------------------------------------------------
# Classes
# ----------------------------------------------------------------------------


@dataclass
class Link(Span):
    """A link or image, e.g. `[text](href)` or `![alt](href)`."""

    kind: Literal["link", "image", "autolink", "wikilink", "html"]
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

References might include inline links and images, link references and link
definitions, footnote references, and footnote definitions. Each of those
reference carries a source span covering the full matched text, possibly with
additional metadata for the relevant subcomponents.
"""
