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

from zensical.markdown.render import render

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _toc_contents(toc: list[dict]) -> list[str]:
    """Recursively collect the `content` field of every TOC item."""
    result = []
    for item in toc:
        result.append(item["content"])
        result.extend(_toc_contents(item["children"]))
    return result


# ---------------------------------------------------------------------------
# TOC cleanup
# ---------------------------------------------------------------------------


class TestTocCleanup:
    def test_abbreviations_stripped_from_toc_content(self) -> None:
        """Abbreviations defined in the page must not appear as <abbr> in TOC.

        The abbr Markdown extension runs before the TOC tree processor, so
        heading HTML stored in toc_tokens already contains <abbr> elements.
        _cleanup_toc_label must remove them, keeping only the plain text.
        """
        result = render(
            content=(
                "# Working with HTML\n"
                "\n"
                "Some content here.\n"
                "\n"
                "*[HTML]: HyperText Markup Language\n"
            ),
            path="index.md",
            url="/",
        )

        # Sanity-check: the rendered page body must contain <abbr> to confirm
        # the extension is actually active and expanding abbreviations.
        assert "<abbr" in result["content"]

        # The TOC must contain exactly one top-level entry.
        assert len(result["toc"]) == 1
        heading = result["toc"][0]

        # The TOC content must be plain text – no <abbr> tags.
        assert "<abbr" not in heading["content"]
        assert heading["content"] == "Working with HTML"

    def test_abbreviations_stripped_from_nested_toc(self) -> None:
        """Abbreviation stripping must apply at every nesting level."""
        result = render(
            content=(
                "# Top level\n"
                "\n"
                "## Using CSS\n"
                "\n"
                "Some content here.\n"
                "\n"
                "*[CSS]: Cascading Style Sheets\n"
            ),
            path="index.md",
            url="/",
        )

        # The rendered page body must contain <abbr>.
        assert "<abbr" in result["content"]

        # Collect content from all TOC levels.
        all_contents = _toc_contents(result["toc"])

        # No level should contain <abbr>.
        assert all("<abbr" not in c for c in all_contents)

        # The child heading must preserve the abbreviation text.
        child = result["toc"][0]["children"][0]
        assert child["content"] == "Using CSS"

    def test_images_stripped_from_toc_content(self) -> None:
        """Images defined in the page must not appear as <img> in TOC."""
        result = render(
            content="# ![icon](../../images/system-32.png) System\n",
            path="index.md",
            url="/",
        )

        # Sanity-check: the rendered page body must contain <img>.
        assert "<img" in result["content"]

        # The TOC must contain exactly one top-level entry.
        assert len(result["toc"]) == 1
        heading = result["toc"][0]

        # The TOC content must be plain text – no <img> tags.
        assert "<img" not in heading["content"]
        assert heading["content"].strip() == "System"

    def test_images_stripped_from_nested_toc(self) -> None:
        """Image stripping must apply at every nesting level."""
        result = render(
            content=(
                "# Top level\n\n## ![icon](../../images/system-32.png) System\n"
            ),
            path="index.md",
            url="/",
        )

        # The rendered page body must contain <img>.
        assert "<img" in result["content"]

        # Collect content from all TOC levels.
        all_contents = _toc_contents(result["toc"])

        # No level should contain <img>.
        assert all("<img" not in c for c in all_contents)
