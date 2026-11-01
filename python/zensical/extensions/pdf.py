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

"""Embed PDFs as native browser documents."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from markdown import Extension, Markdown
from markdown.treeprocessors import Treeprocessor

if TYPE_CHECKING:
    from xml.etree.ElementTree import Element

# ----------------------------------------------------------------------------
# Classes
# ----------------------------------------------------------------------------


class PdfTreeprocessor(Treeprocessor):
    """Replace images marked as PDFs with embed elements."""

    name = "pdf"

    def run(self, root: Element) -> None:
        """Convert images marked as PDFs in-place."""
        for image in root.iter("img"):
            if image.get("type") == "application/pdf" and image.get("src"):
                image.tag = "embed"


class PdfExtension(Extension):
    """Markdown extension compatible with `mkdocs-pdf`."""

    name = "zensical.extensions.pdf"

    def extendMarkdown(self, md: Markdown) -> None:
        """Register the PDF treeprocessor."""
        md.registerExtension(self)

        # Run after `attr_list` (8) and media (7.5), but before glightbox (7)
        processor = PdfTreeprocessor(md)
        md.treeprocessors.register(processor, processor.name, 7.4)


# ----------------------------------------------------------------------------
# Entry point
# ----------------------------------------------------------------------------


def makeExtension(**kwargs: Any) -> PdfExtension:
    """Register the PDF Markdown extension."""
    return PdfExtension(**kwargs)
