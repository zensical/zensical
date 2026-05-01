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
from typing import TYPE_CHECKING, Any, cast
from xml.etree.ElementTree import Element, ParseError, fromstring, tostring

from zensical.markdown.extensions import ExtensionExt, MarkdownExt
from zensical.markdown.processors import PostprocessorExt, TreeprocessorExt

if TYPE_CHECKING:
    from zensical.markdown.extensions import MarkdownExt

# -----------------------------------------------------------------------------
# Constants
# -----------------------------------------------------------------------------

_RE = re.compile(r"<img\s[^>]*?>", re.IGNORECASE)
"""Match images in stashed raw HTML blocks."""

# -----------------------------------------------------------------------------
# Classes
# -----------------------------------------------------------------------------


class GlightboxTreeprocessor(TreeprocessorExt):
    """Wraps image elements in anchor tags to integrate with GLightbox."""

    SKIP_CLASSES: frozenset[str] = frozenset(
        {"emojione", "twemoji", "gemoji", "off-glb"}
    )

    def __init__(self, md: MarkdownExt, config: dict[str, object]):
        super().__init__(md)
        self.config = config

    def run(self, root: Element) -> None:
        """Walk the element tree and wrap images with anchors."""
        skip_classes = self.SKIP_CLASSES | frozenset(
            cast("list[str]", self.config.get("skip_classes") or [])
        )

        # Iterate over all images in the tree and wrap them with anchors
        for img in list(root.iter("img")):
            if not self._should_skip(img, skip_classes):
                self._wrap_with_anchor(img, root)

    def _should_skip(self, img: Element, skip_classes: frozenset[str]) -> bool:
        """Determine if this image should be excluded from wrapping."""
        classes = set(img.get("class", "").split())
        if classes & skip_classes:
            return True

        # If manual mode is enabled, only wrap images explicitly marked
        return not self.config.get("auto") and "on-glb" not in classes

    def _wrap_with_anchor(self, img: Element, root: Element) -> None:
        """Wrap an image with an anchor."""
        parent = self._find_parent(root, img)
        if parent is None or parent.tag == "a":
            return

        # Create anchor element with appropriate attributes based on the image
        # and config settings, then wrap the image with the anchor
        anchor = self._build_anchor(img)
        anchor.append(img)

        # If there's text adjacent to the image element, we need to clear it
        # from the image and move it to the anchor, see https://t.ly/dLA-l
        anchor.tail = img.tail
        img.tail = None

        # Remove image from current position and append to anchor, then insert
        # anchor back into the original position in the tree
        index = list(parent).index(img)
        parent.remove(img)
        parent.insert(index, anchor)

    def _build_anchor(self, img: Element) -> Element:
        """Construct the anchor from image attributes."""
        el = Element("a")
        el.set("class", "glightbox")
        el.set("href", img.get("data-src") or img.get("src") or "")
        el.set("data-type", "image")

        # Only set width/height if explicitly configured
        if width := self.config.get("width"):
            el.set("data-width", str(width))
        if height := self.config.get("height"):
            el.set("data-height", str(height))

        # Set image title
        auto_caption = bool(self.config.get("auto_caption", False))
        title = img.get("data-title") or (
            img.get("alt") if auto_caption else None
        )
        if title:
            el.set("data-title", title)

        # Set image description
        if description := img.get("data-description"):
            el.set("data-description", description)

        # Set image description position
        if (
            caption_position := (
                img.get("data-caption-position")
                or self.config.get("caption_position")
            )
        ) and caption_position != "bottom":
            el.set("data-desc-position", str(caption_position))

        # Set gallery grouping
        if gallery := self._resolve_gallery(img):
            el.set("data-gallery", gallery)

        # Remove sourced attributes from img now that they live on the anchor
        for attr in (
            "data-width",
            "data-height",
            "data-src",
            "data-title",
            "data-description",
            "data-caption-position",
            "data-gallery",
        ):
            if attr in img.attrib:
                img.attrib.pop(attr)

        # Return element
        return el

    def _resolve_gallery(self, img: Element) -> str:
        """Determine gallery group for an image."""
        src = img.get("data-src") or img.get("src") or ""

        # If auto-themed grouping is enabled, group images by light/dark mode
        # hints in the URL (e.g. from GitHub's light/dark mode image syntax)
        if self.config.get("auto_themed"):
            if "#only-light" in src or "#gh-light-mode-only" in src:
                return "light"
            if "#only-dark" in src or "#gh-dark-mode-only" in src:
                return "dark"

        # Explicit gallery grouping takes precedence over auto-themed grouping
        return img.get("data-gallery") or ""

    def _find_parent(self, root: Element, target: Element) -> Element | None:
        """Return the direct parent of target within the element tree."""
        return next(
            (parent for parent in root.iter() if target in list(parent)),
            None,
        )


class GlightboxPostprocessor(PostprocessorExt):
    """Wraps stashed images in anchors, delegating to the treeprocessor.

    This postprocessor uses a regular expression to find image tags in stashed
    raw HTML blocks and applies the same wrapping logic as the treeprocessor.
    Using a regular expression is cheaper and more resilient than trying to
    parse and modify the HTML with an actual parser.
    """

    def __init__(self, md: MarkdownExt, config: dict[str, object]):
        super().__init__(md)
        self._processor = GlightboxTreeprocessor(md, config)
        self._processed: set[int] = set()

        # Source classes to skip from postprocessor
        self._skip_classes = GlightboxTreeprocessor.SKIP_CLASSES | frozenset(
            cast("list[str]", config.get("skip_classes") or [])
        )

    def run(self, text: str) -> str:
        """Wrap images in stashed HTML blocks."""
        for i, raw in enumerate(self.md.htmlStash.rawHtmlBlocks):
            if i not in self._processed:
                self.md.htmlStash.rawHtmlBlocks[i] = _RE.sub(  # ty:ignore[no-matching-overload]
                    self._maybe_process, raw
                )
                self._processed.add(i)

        # Return text unmodified, as we only need to modify the stashed raw HTML
        # blocks, which will later be reinstated by the raw HTML postprocessor
        return text

    def _maybe_process(self, m: re.Match[str]) -> str:
        """Wrap a single matched image, delegating to the treeprocessor."""
        raw = m.group(0)
        try:
            fragment = raw if raw.endswith("/>") else raw[:-1] + "/>"
            img = fromstring(fragment)  # noqa: S314
        except ParseError:
            return raw

        # Skip if image should not be wrapped
        if self._processor._should_skip(img, self._skip_classes):
            return raw

        # Wrap image in anchor and return as string
        anchor = self._processor._build_anchor(img)
        anchor.append(img)
        return tostring(anchor, encoding="unicode", method="html")


# -----------------------------------------------------------------------------


class GlightboxExtension(ExtensionExt):
    """Markdown extension that wraps images in GLightbox anchor tags.

    This extension provides both a treeprocessor to wrap images in the normal
    Markdown flow and a postprocessor to handle images that are stashed as raw
    HTML, ensuring that all images are properly wrapped regardless of how they
    are processed by Markdown.
    """

    def __init__(self, **kwargs: object):
        """Initialize the extension."""
        self.config: dict[str, list[object]] = {
            "width": ["auto", "Width of the lightbox overlay."],
            "height": ["auto", "Height of the lightbox overlay."],
            "skip_classes": [
                [],
                "List of image CSS classes to exclude from lightbox wrapping.",
            ],
            "auto": [
                True,
                "Only wrap images that explicitly carry the on-glb CSS class.",
            ],
            "auto_themed": [
                False,
                "Group light/dark mode images into separate galleries.",
            ],
            "auto_caption": [
                False,
                "Use img alt attribute as the caption when no title is set.",
            ],
            "caption_position": [
                "bottom",
                "Default caption position: bottom, top, left, or right.",
            ],
        }
        super().__init__(**kwargs)

    def extendMarkdown(self, md: MarkdownExt) -> None:  # noqa: N802
        """Register Markdown extension."""
        md.registerExtension(self)

        # Register treeprocessor - run after `attr_list` (priority 8)
        treeprocessor = GlightboxTreeprocessor(md, self.getConfigs())
        md.treeprocessors.register(treeprocessor, "glightbox", 7)

        # Register postprocessor - run before `raw_html` (priority 30)
        postprocessor = GlightboxPostprocessor(md, self.getConfigs())
        md.postprocessors.register(postprocessor, "glightbox", 31)


# -----------------------------------------------------------------------------
# Functions
# -----------------------------------------------------------------------------


def makeExtension(**kwargs: Any) -> GlightboxExtension:
    """Register Markdown extension."""
    return GlightboxExtension(**kwargs)
