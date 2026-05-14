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

import pytest

from zensical.markdown.render import _cleanup_toc_label

# ---------------------------------------------------------------------------
# Cleaning up TOC
# ---------------------------------------------------------------------------


class TestCleanupTocLabel:
    @pytest.mark.parametrize(
        ("html", "expected"),
        [
            # Links --------------------------------------------------------
            pytest.param(
                '<a href="#foo" id="foo">Heading</a>',
                "Heading",
                id="anchor_with_id_attr",  # id= is stripped before <a>
            ),
            pytest.param(
                '<a href="#x">Hello <em>world</em></a>',
                "Hello <em>world</em>",
                id="anchor_preserves_inner_content",
            ),
            pytest.param(
                '<a href="#x">\nLine one\nLine two\n</a>',
                "\nLine one\nLine two\n",
                id="multiline_anchor",
            ),
            pytest.param(
                '<a href="#a">First</a> and <a href="#b">Second</a>',
                "First and Second",
                id="multiple_anchors",
            ),
            # Abbreviations -----------------------------------------------
            pytest.param(
                '<abbr title="HyperText Markup Language">HTML</abbr>',
                "HTML",
                id="abbr_tag",
            ),
            pytest.param(
                'Use <abbr title="Cascading Style Sheets">CSS</abbr> for style',
                "Use CSS for style",
                id="abbr_preserves_surrounding_text",
            ),
            pytest.param(
                '<abbr title="HyperText Markup Language">HTML</abbr>'
                " and "
                '<abbr title="Cascading Style Sheets">CSS</abbr>',
                "HTML and CSS",
                id="multiple_abbreviations",
            ),
            pytest.param(
                '<abbr title="foo">\nAbbr\n</abbr>',
                "\nAbbr\n",
                id="multiline_abbr",
            ),
            # Combined and passthrough ------------------------------------
            pytest.param(
                '<a href="#x">Intro to '
                '<abbr title="HyperText Markup Language">HTML</abbr>'
                "</a>",
                "Intro to HTML",
                id="links_and_abbreviations",
            ),
            pytest.param(
                "Just plain text",
                "Just plain text",
                id="plain_text_unchanged",
            ),
        ],
    )
    def test_cleans(self, html: str, expected: str) -> None:
        assert _cleanup_toc_label(html) == expected
