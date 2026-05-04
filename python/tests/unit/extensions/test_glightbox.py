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

from typing import Any

import pytest
from markdown import Markdown

from zensical.extensions.emoji import to_svg, twemoji
from zensical.extensions.glightbox import GlightboxExtension

MINIMAL_EXTENSIONS = {"attr_list": {}}

RECOMMENDED_EXTENSIONS = {
    "abbr": {},
    "admonition": {},
    "attr_list": {},
    "def_list": {},
    "footnotes": {},
    "md_in_html": {},
    "toc": {"permalink": True},
    "pymdownx.arithmatex": {"generic": True},
    "pymdownx.betterem": {},
    "pymdownx.caret": {},
    "pymdownx.details": {},
    "pymdownx.emoji": {
        "emoji_generator": to_svg,
        "emoji_index": twemoji,
    },
    "pymdownx.highlight": {
        "anchor_linenums": True,
        "line_spans": "__span",
        "pygments_lang_class": True,
    },
    "pymdownx.inlinehilite": {},
    "pymdownx.keys": {},
    "pymdownx.magiclink": {},
    "pymdownx.mark": {},
    "pymdownx.smartsymbols": {},
    "pymdownx.superfences": {
        "custom_fences": [{"name": "mermaid", "class": "mermaid"}]
    },
    "pymdownx.tabbed": {
        "alternate_style": True,
        "combine_header_slug": True,
    },
    "pymdownx.tasklist": {"custom_checkbox": True},
    "pymdownx.tilde": {},
}

_ACTIVE_EXTENSIONS = dict(MINIMAL_EXTENSIONS)


def _make_md(
    extensions: dict[str, Any] | None = None,
    **kwargs: object,
) -> Markdown:
    """Return a Markdown instance with GlightboxExtension registered."""
    extensions = dict(extensions or MINIMAL_EXTENSIONS)
    md = Markdown(
        extensions=list(extensions.keys()), extension_configs=extensions
    )
    GlightboxExtension(**kwargs).extendMarkdown(md)
    return md


def _convert(source: str, **kwargs: object) -> str:
    return _make_md(extensions=_ACTIVE_EXTENSIONS, **kwargs).convert(source)


@pytest.fixture(name="minimal_markdown")
def _fixture_minimal_markdown() -> dict[str, Any]:
    return dict(MINIMAL_EXTENSIONS)


@pytest.fixture(name="recommended_markdown")
def _fixture_recommended_markdown() -> dict[str, Any]:
    return dict(RECOMMENDED_EXTENSIONS)


@pytest.fixture(
    params=["minimal_markdown", "recommended_markdown"],
    ids=["minimal_markdown", "recommended_markdown"],
    autouse=True,
)
def _active_markdown(request: pytest.FixtureRequest) -> None:
    global _ACTIVE_EXTENSIONS  # noqa: PLW0603
    _ACTIVE_EXTENSIONS = dict(request.getfixturevalue(request.param))


# ---------------------------------------------------------------------------
# Basic wrapping
# ---------------------------------------------------------------------------


def test_image_is_wrapped_in_glightbox_anchor() -> None:
    result = _convert("![alt](image.png)")
    assert '<a class="glightbox"' in result
    assert 'href="image.png"' in result
    assert 'data-type="image"' in result
    assert "<img" in result


def test_anchor_wraps_img_not_standalone() -> None:
    result = _convert("![alt](image.png)")
    # The <a> must contain the <img>
    assert result.index('<a class="glightbox"') < result.index("<img")


def test_image_src_used_as_href() -> None:
    result = _convert("![](photo.jpg)")
    assert 'href="photo.jpg"' in result


def test_image_with_title_attribute() -> None:
    result = _convert('![alt](image.png "My Title")')
    assert "<img" in result
    assert 'href="image.png"' in result


# ---------------------------------------------------------------------------
# Skip classes
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("cls", ["emojione", "twemoji", "gemoji", "off-glb"])
def test_builtin_skip_classes_prevent_wrapping(cls: str) -> None:
    result = _convert(f'<img src="img.png" class="{cls}" />')
    assert '<a class="glightbox"' not in result


def test_custom_skip_class_prevents_wrapping() -> None:
    result = _convert(
        '<img src="img.png" class="no-lightbox" />',
        skip_classes=["no-lightbox"],
    )
    assert '<a class="glightbox"' not in result


def test_non_skip_class_is_wrapped() -> None:
    result = _convert('<img src="img.png" class="hero" />')
    assert '<a class="glightbox"' in result


# ---------------------------------------------------------------------------
# Manual mode (auto=False)
# ---------------------------------------------------------------------------


def test_manual_mode_skips_plain_images() -> None:
    result = _convert("![alt](image.png)", auto=False)
    assert '<a class="glightbox"' not in result


def test_manual_mode_wraps_on_glb_images() -> None:
    result = _convert('<img src="image.png" class="on-glb" />', auto=False)
    assert '<a class="glightbox"' in result


# ---------------------------------------------------------------------------
# auto_caption
# ---------------------------------------------------------------------------


def test_auto_caption_uses_alt_as_title() -> None:
    result = _convert("![My Caption](image.png)", auto_caption=True)
    assert 'data-title="My Caption"' in result


def test_auto_caption_disabled_by_default() -> None:
    result = _convert("![My Caption](image.png)")
    assert "data-title" not in result


def test_explicit_data_title_takes_precedence_over_auto_caption() -> None:
    # data-title on img (via raw HTML) should survive auto_caption=True
    result = _convert(
        '<img src="image.png" alt="alt" data-title="Override" />',
        auto_caption=True,
    )
    assert 'data-title="Override"' in result
    # alt should not overwrite the explicit title
    assert result.count("data-title=") == 1


# ---------------------------------------------------------------------------
# Caption position
# ---------------------------------------------------------------------------


def test_default_caption_position_via_raw_html() -> None:
    # caption_position on the image element itself is forwarded to the anchor
    result = _convert('<img src="image.png" data-caption-position="top" />')
    assert 'data-desc-position="top"' in result


def test_no_caption_position_by_default() -> None:
    result = _convert("![alt](image.png)")
    assert "data-desc-position" not in result


# ---------------------------------------------------------------------------
# Width / height
# ---------------------------------------------------------------------------


def test_width_and_height_are_forwarded() -> None:
    result = _convert("![alt](image.png)", width="800px", height="600px")
    assert 'data-width="800px"' in result
    assert 'data-height="600px"' in result


def test_default_auto_width_height_are_omitted() -> None:
    result = _convert("![alt](image.png)")
    assert "data-width" not in result
    assert "data-height" not in result


# ---------------------------------------------------------------------------
# auto_themed gallery grouping
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("src", "expected_gallery"),
    [
        ("image.png#only-light", "light"),
        ("image.png#gh-light-mode-only", "light"),
        ("image.png#only-dark", "dark"),
        ("image.png#gh-dark-mode-only", "dark"),
    ],
)
def test_auto_themed_gallery_grouping(src: str, expected_gallery: str) -> None:
    result = _convert(f"![alt]({src})", auto_themed=True)
    assert f'data-gallery="{expected_gallery}"' in result


def test_auto_themed_disabled_no_gallery() -> None:
    result = _convert("![alt](image.png#only-light)")
    assert "data-gallery" not in result


# ---------------------------------------------------------------------------
# Image already inside an anchor
# ---------------------------------------------------------------------------


def test_parsed_image_inside_anchor_is_not_double_wrapped() -> None:
    # When Markdown parses an image link ([![]()]()), the treeprocessor sees
    # the parent as an <a> and skips wrapping
    result = _convert("[![alt](image.png)](https://example.com)")
    assert 'class="glightbox"' not in result
    assert 'href="https://example.com"' in result


# ---------------------------------------------------------------------------
# Postprocessor – stashed raw HTML
# ---------------------------------------------------------------------------


def test_raw_html_image_is_wrapped() -> None:
    # Raw HTML images bypass the treeprocessor and are handled by the
    # postprocessor; markdown=1 block forces raw-HTML stashing
    result = _convert('<img src="raw.png" />')
    assert '<a class="glightbox"' in result
    assert 'href="raw.png"' in result


def test_raw_html_skip_class_not_wrapped() -> None:
    result = _convert('<img src="raw.png" class="off-glb" />')
    assert 'class="glightbox"' not in result
