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

from typing import TYPE_CHECKING, Any

import pytest

if TYPE_CHECKING:
    from markdown import Markdown

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _glightbox(**kwargs: object) -> dict[str, Any]:
    """Return md-fixture params with GlightboxExtension configured."""
    return {
        "config": {
            "markdown_extensions": {
                "zensical.extensions.glightbox": dict(kwargs),
            }
        }
    }


# ---------------------------------------------------------------------------
# Basic wrapping
# ---------------------------------------------------------------------------


class TestBasicWrapping:
    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_image_wrapped_in_anchor(self, md: Markdown) -> None:
        result = md.convert("![alt](image.png)")
        assert '<a class="glightbox"' in result
        assert 'href="image.png"' in result
        assert 'data-type="image"' in result
        assert "<img" in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_anchor_contains_img(self, md: Markdown) -> None:
        result = md.convert("![alt](image.png)")
        assert result.index('<a class="glightbox"') < result.index("<img")

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_src_used_as_href(self, md: Markdown) -> None:
        result = md.convert("![](photo.jpg)")
        assert 'href="photo.jpg"' in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_with_title_attribute(self, md: Markdown) -> None:
        result = md.convert('![alt](image.png "My Title")')
        assert "<img" in result
        assert 'href="image.png"' in result


# ---------------------------------------------------------------------------
# Skip classes
# ---------------------------------------------------------------------------


class TestSkipClasses:
    @pytest.mark.parametrize(
        ("md", "cls"),
        [
            pytest.param(_glightbox(), "emojione", id="emojione"),
            pytest.param(_glightbox(), "twemoji", id="twemoji"),
            pytest.param(_glightbox(), "gemoji", id="gemoji"),
            pytest.param(_glightbox(), "off-glb", id="off_glb"),
        ],
        indirect=["md"],
    )
    def test_builtin_skip_class_prevents_wrapping(
        self, md: Markdown, cls: str
    ) -> None:
        result = md.convert(f'<img src="image.png" class="{cls}" />')
        assert '<a class="glightbox"' not in result

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                _glightbox(skip_classes=["no-lightbox"]), id="custom_skip"
            )
        ],
        indirect=["md"],
    )
    def test_custom_skip_class_prevents_wrapping(self, md: Markdown) -> None:
        result = md.convert('<img src="image.png" class="no-lightbox" />')
        assert '<a class="glightbox"' not in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_non_skip_class_is_wrapped(self, md: Markdown) -> None:
        result = md.convert('<img src="image.png" class="hero" />')
        assert '<a class="glightbox"' in result


# ---------------------------------------------------------------------------
# Manual mode (auto=False)
# ---------------------------------------------------------------------------


class TestManualMode:
    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(auto=False), id="auto_off")],
        indirect=["md"],
    )
    def test_skips_plain_images(self, md: Markdown) -> None:
        result = md.convert("![alt](image.png)")
        assert '<a class="glightbox"' not in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(auto=False), id="auto_off")],
        indirect=["md"],
    )
    def test_wraps_on_glb_images(self, md: Markdown) -> None:
        result = md.convert('<img src="image.png" class="on-glb" />')
        assert '<a class="glightbox"' in result


# ---------------------------------------------------------------------------
# Auto caption
# ---------------------------------------------------------------------------


class TestAutoCaption:
    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(auto_caption=True), id="auto_caption_on")],
        indirect=["md"],
    )
    def test_uses_alt_as_title(self, md: Markdown) -> None:
        result = md.convert("![My Caption](image.png)")
        assert 'data-title="My Caption"' in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_disabled_by_default(self, md: Markdown) -> None:
        result = md.convert("![My Caption](image.png)")
        assert "data-title" not in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(auto_caption=True), id="auto_caption_on")],
        indirect=["md"],
    )
    def test_explicit_data_title_takes_precedence(self, md: Markdown) -> None:
        result = md.convert(
            '<img src="image.png" alt="alt" data-title="Override" />',
        )
        assert 'data-title="Override"' in result
        assert result.count("data-title=") == 1


# ---------------------------------------------------------------------------
# Caption position
# ---------------------------------------------------------------------------


class TestCaptionPosition:
    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_caption_position_forwarded_from_img(self, md: Markdown) -> None:
        result = md.convert(
            '<img src="image.png" data-caption-position="top" />'
        )
        assert 'data-desc-position="top"' in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_no_position_by_default(self, md: Markdown) -> None:
        result = md.convert("![alt](image.png)")
        assert "data-desc-position" not in result


# ---------------------------------------------------------------------------
# Width / height
# ---------------------------------------------------------------------------


class TestWidthHeight:
    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                _glightbox(width="800px", height="600px"),
                id="explicit_dimensions",
            )
        ],
        indirect=["md"],
    )
    def test_width_and_height_forwarded(self, md: Markdown) -> None:
        result = md.convert("![alt](image.png)")
        assert 'data-width="800px"' in result
        assert 'data-height="600px"' in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_auto_dimensions_omitted(self, md: Markdown) -> None:
        result = md.convert("![alt](image.png)")
        assert "data-width" not in result
        assert "data-height" not in result


# ---------------------------------------------------------------------------
# Auto-themed gallery grouping
# ---------------------------------------------------------------------------


class TestAutoThemedGalleryGrouping:
    @pytest.mark.parametrize(
        ("md", "src", "expected_gallery"),
        [
            pytest.param(
                _glightbox(auto_themed=True),
                "image.png#only-light",
                "light",
                id="only_light",
            ),
            pytest.param(
                _glightbox(auto_themed=True),
                "image.png#gh-light-mode-only",
                "light",
                id="gh_light_mode_only",
            ),
            pytest.param(
                _glightbox(auto_themed=True),
                "image.png#only-dark",
                "dark",
                id="only_dark",
            ),
            pytest.param(
                _glightbox(auto_themed=True),
                "image.png#gh-dark-mode-only",
                "dark",
                id="gh_dark_mode_only",
            ),
        ],
        indirect=["md"],
    )
    def test_gallery_grouping(
        self, md: Markdown, src: str, expected_gallery: str
    ) -> None:
        result = md.convert(f"![alt]({src})")
        assert f'data-gallery="{expected_gallery}"' in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_no_gallery_when_disabled(self, md: Markdown) -> None:
        result = md.convert("![alt](image.png#only-light)")
        assert "data-gallery" not in result


# ---------------------------------------------------------------------------
# Image already inside an anchor
# ---------------------------------------------------------------------------


class TestImageInsideAnchor:
    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_not_double_wrapped(self, md: Markdown) -> None:
        result = md.convert("[![alt](image.png)](https://example.com)")
        assert 'class="glightbox"' not in result
        assert 'href="https://example.com"' in result


# ---------------------------------------------------------------------------
# Postprocessor – stashed raw HTML
# ---------------------------------------------------------------------------


class TestPostprocessor:
    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_raw_html_image_wrapped(self, md: Markdown) -> None:
        result = md.convert('<img src="raw.png" />')
        assert '<a class="glightbox"' in result
        assert 'href="raw.png"' in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_glightbox(), id="default")],
        indirect=["md"],
    )
    def test_raw_html_skip_class_not_wrapped(self, md: Markdown) -> None:
        result = md.convert('<img src="raw.png" class="off-glb" />')
        assert 'class="glightbox"' not in result
