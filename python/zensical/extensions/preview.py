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

import posixpath
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any
from urllib.parse import urlparse

from markdown import Extension
from markdown.treeprocessors import Treeprocessor

from zensical.extensions.context import ContextPreprocessor
from zensical.extensions.utilities.filter import Filter

if TYPE_CHECKING:
    from xml.etree.ElementTree import Element

    from markdown import Markdown

# -----------------------------------------------------------------------------
# Classes
# -----------------------------------------------------------------------------


@dataclass
class PreviewConfig:
    """Configuration for the Preview Markdown extension."""

    configurations: list[dict[str, Any]] = field(default_factory=list)
    sources: dict[str, Any] = field(default_factory=dict)
    targets: dict[str, Any] = field(default_factory=dict)


# -----------------------------------------------------------------------------


class PreviewProcessor(Treeprocessor):
    """A Markdown treeprocessor to enable instant previews on links.

    Get the source path from the rendering context before Rust resolves URLs.
    """

    name = "preview"

    def __init__(self, md: Markdown, config: PreviewConfig):
        """Initialize the treeprocessor."""
        super().__init__(md)
        self.config = config

    def run(self, root: Element) -> None:
        """Run the treeprocessor."""
        context = ContextPreprocessor.from_markdown(self.md)
        if context is None:
            raise TypeError("Rendering context not registered")
        source_path = context.page.path

        # Normalize configurations
        configurations = list(self.config.configurations)
        configurations.append(
            {
                "sources": self.config.sources,
                "targets": self.config.targets,
            }
        )

        # Walk through all configurations - @todo refactor so that we don't
        # iterate multiple times over the same elements
        for configuration in configurations:
            if not configuration.get("sources") and not configuration.get(
                "targets"
            ):
                continue

            # Skip if page should not be considered
            filter = get_filter(configuration, "sources")
            if not filter(source_path):
                continue

            # Walk through all links and add preview attributes
            filter = get_filter(configuration, "targets")
            for el in root.iter("a"):
                href = el.get("href")
                if not href:
                    continue

                # Skip footnotes
                if "footnote-ref" in el.get("class", ""):
                    continue
                if "footnote-backref" in el.get("class", ""):
                    continue

                # Skip headerlinks
                if "headerlink" in el.get("class", ""):
                    continue

                # Skip external links
                url = urlparse(href)
                if url.scheme or url.netloc:
                    continue

                # An empty url.path means we're targetting the current page
                url_path = url.path or source_path

                # Include, if filter matches
                path = resolve(source_path, url_path)
                if path and filter(path):
                    el.set("data-preview", "")


# -----------------------------------------------------------------------------


class PreviewExtension(Extension):
    """Markdown extension to enable instant previews on links.

    This extensions allows to automatically add the `data-preview` attribute to
    internal links matching specific criteria, so Material for MkDocs renders a
    nice preview on hover as part of a tooltip. It is the recommended way to
    add previews to links in a programmatic way.
    """

    name = "zensical.extensions.preview"

    def __init__(self, **kwargs: Any) -> None:
        """Initialize the extension."""
        self._kwargs = kwargs

    def extendMarkdown(self, md: Markdown) -> None:
        """Register Markdown extension."""
        md.registerExtension(self)

        # Original Markdown URIs are inspected before URL resolution in Rust.
        config = PreviewConfig(**self._kwargs)
        processor = PreviewProcessor(md, config)
        md.treeprocessors.register(processor, processor.name, 0)


# -----------------------------------------------------------------------------
# Functions
# -----------------------------------------------------------------------------


def get_filter(settings: dict, key: str) -> Filter:
    """Get file filter from settings."""
    return Filter(config=settings.get(key, {}))


def resolve(processor_path: str, url_path: str) -> str:
    """Resolve a relative URL path against the processor path."""
    # Remove the file name from the processor path to get the directory
    base_path = posixpath.dirname(processor_path)

    # Split the base path and URL path into segments
    base_segments = base_path.split("/")
    url_segments = url_path.split("/")

    # Process each segment in the URL path
    for segment in url_segments:
        if segment == "..":
            # Remove the last segment from the base path if possible
            if base_segments:
                base_segments.pop()
        elif segment and segment != ".":
            # Add non-empty, non-current directory segments
            base_segments.append(segment)

    # Join the base segments into the resolved path
    return posixpath.join(*base_segments)


def makeExtension(**kwargs: Any) -> PreviewExtension:
    """Register Markdown extension."""
    return PreviewExtension(**kwargs)
