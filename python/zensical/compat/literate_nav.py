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

"""Narrow Python-Markdown adapter for native literate navigation."""

from __future__ import annotations

from copy import deepcopy
from itertools import dropwhile
from typing import TYPE_CHECKING
from xml.etree import ElementTree

from markdown import Markdown
from markdown.preprocessors import Preprocessor
from markdown.treeprocessors import Treeprocessor

from zensical.config import get_config

if TYPE_CHECKING:
    from collections.abc import Callable

# ----------------------------------------------------------------------------
# Constants
# ----------------------------------------------------------------------------

# Private-use scalar framing text that must survive XML and HTML decoding.
_TEXT_ESCAPE = "\U000f0000"

# ----------------------------------------------------------------------------
# Classes
# ----------------------------------------------------------------------------


class _MarkerPreprocessor(Preprocessor):
    """Replace explicit navigation markers with a tree-visible placeholder."""

    def __init__(self, md: Markdown):
        super().__init__(md)
        self.placeholder: str | None = None

    def run(self, lines: list[str]) -> list[str]:
        for index, line in enumerate(lines):
            if line.strip() == "<!--nav-->":
                self.placeholder = self.md.htmlStash.store("")
                lines[index] = self.placeholder + "\n"
        return lines


class _CaptureTreeprocessor(Treeprocessor):
    """Capture the selected root list at literate-nav's processing phase."""

    def __init__(self, md: Markdown, marker: _MarkerPreprocessor):
        super().__init__(md)
        self.marker = marker
        self.nav: ElementTree.Element | None = None

    def run(self, root: ElementTree.Element) -> None:
        if self.marker.placeholder is None:
            candidates = reversed(root)
        else:
            candidates = dropwhile(
                lambda element: element.text != self.marker.placeholder,
                root,
            )
        for element in candidates:
            if element.tag in {"ul", "ol"}:
                self.nav = deepcopy(element)
                return


# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def render(content: str) -> str:
    """Capture literate navigation with its local Markdown configuration.

    This returns only the selected list subtree. Interpretation belongs to the
    native compatibility module, keeping filesystem and navigation semantics
    out of Python.
    """
    plugin = get_config()["plugins"]["literate_nav"]["config"]
    md = Markdown(
        extensions=plugin["markdown_extensions"],
        extension_configs=plugin["mdx_configs"],
        tab_length=plugin["tab_length"],
    )

    # Parse the complete document so definitions and extension state before
    # an explicit marker remain available to the selected navigation list.
    # Keep inline HTML and entities in the captured tree instead of replacing
    # them with placeholders that only a complete Markdown render can restore.
    md.inlinePatterns.deregister("html", strict=False)
    md.inlinePatterns.deregister("entity", strict=False)
    marker = _MarkerPreprocessor(md)
    capture = _CaptureTreeprocessor(md, marker)
    md.preprocessors.register(marker, "zensical_literate_nav_marker", 25)
    md.treeprocessors.register(capture, "zensical_literate_nav_capture", 19)
    md.convert(content)
    if capture.nav is None:
        return ""
    _encode_tree(capture.nav, md.treeprocessors["unescape"].unescape)
    return ElementTree.tostring(capture.nav, encoding="unicode")


def _escape_text(value: str) -> str:
    """Encode ampersands and the escape scalar without ambiguity."""
    return value.replace(_TEXT_ESCAPE, _TEXT_ESCAPE + "S").replace(
        "&", _TEXT_ESCAPE + "A"
    )


def _encode_tree(
    root: ElementTree.Element, unescape: Callable[[str], str]
) -> None:
    """Preserve text across XML serialization and Rust's HTML tokenizer."""
    for element in root.iter():
        if element.text:
            element.text = _escape_text(unescape(element.text))
        if element.tail:
            element.tail = _escape_text(unescape(element.tail))
