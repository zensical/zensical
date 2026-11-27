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

from pathlib import Path

import pytest
from markdown import Markdown
from zensical_ui import get_theme_dir

from tests.unit.extensions.conftest import soup
from zensical.extensions.emoji import to_svg, twemoji


@pytest.fixture
def emoji_md() -> Markdown:
    return Markdown(
        extensions=["attr_list", "pymdownx.emoji"],
        extension_configs={
            "pymdownx.emoji": {
                "emoji_generator": to_svg,
                "emoji_index": twemoji,
            }
        },
    )


@pytest.mark.parametrize(
    ("shortcode", "tag"),
    [
        (":lucide-heart:", "span"),
        (":material-heart:", "span"),
        (":rocket:", "img"),
    ],
)
def test_icons_and_emoji_are_hidden_by_default(
    emoji_md: Markdown, shortcode: str, tag: str
) -> None:
    graphic = soup(emoji_md.convert(shortcode)).select_one(".twemoji")
    assert graphic is not None
    assert graphic.name == tag
    assert graphic["aria-hidden"] == "true"
    if tag == "img":
        assert graphic["alt"] == "🚀"
    else:
        assert graphic.find("svg") is not None


@pytest.mark.parametrize(
    "shortcode", [":lucide-heart:", ":material-heart:", ":rocket:"]
)
def test_attribute_lists_can_expose_and_label_icons(
    emoji_md: Markdown, shortcode: str
) -> None:
    source = (
        shortcode
        + '{ .xl aria-hidden="false" role="img" aria-label="Meaning" }'
    )
    graphic = soup(emoji_md.convert(source)).select_one(".twemoji")
    assert graphic is not None
    assert graphic["aria-hidden"] == "false"
    assert graphic["role"] == "img"
    assert graphic["aria-label"] == "Meaning"
    assert graphic["class"] == ["twemoji", "xl"]


def test_svg_markup_is_preserved(emoji_md: Markdown) -> None:
    original = (Path(get_theme_dir()) / ".icons/lucide/heart.svg").read_text()
    assert original in emoji_md.convert(":lucide-heart:")
