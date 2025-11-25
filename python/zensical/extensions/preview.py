# Copyright (c) 2025 Zensical and contributors

# SPDX-License-Identifier: MIT
# Third-party contributions licensed under DCO

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

from markdown import Extension, Markdown
from markdown.treeprocessors import Treeprocessor
from urllib.parse import urlparse
from xml.etree.ElementTree import Element

from .links import LinksProcessor
from .utilities.filter import Filter

# -----------------------------------------------------------------------------
# Classes
# -----------------------------------------------------------------------------


class PreviewProcessor(Treeprocessor):
    """
    A Markdown treeprocessor to enable instant previews on links.

    Note that this treeprocessor is dependent on the `links` treeprocessor
    registered programmatically before rendering a page.
    """

    def __init__(self, md: Markdown, config: dict):
        """
        Initialize the treeprocessor.
        """
        super().__init__(md)
        self.config = config

    def run(self, root: Element):
        """
        Run the treeprocessor.
        """
        at = self.md.treeprocessors.get_index_for_name("zrelpath")

        # Hack: Python Markdown has no notion of where it is, i.e., which file
        # is being processed. This seems to be a deliberate design decision, as
        # it is not possible to access the file path of the current page, but
        # it might also be an oversight that is now impossible to fix. However,
        # since this extension is only useful in the context of Material for
        # MkDocs, we can assume that the _RelativePathTreeprocessor is always
        # present, telling us the file path of the current page. If that ever
        # changes, we would need to wrap this extension in a plugin, but for
        # the time being we are sneaky and will probably get away with it.
        processor = self.md.treeprocessors[at]
        if not isinstance(processor, LinksProcessor):
            raise TypeError("Links processor not registered")

        # Normalize configurations
        configurations = self.config["configurations"]
        configurations.append(
            {
                "sources": self.config.get("sources"),
                "targets": self.config.get("targets"),
            }
        )

        # Walk through all configurations - @todo refactor so that we don't
        # iterate multiple times over the same elements
        for configuration in configurations:
            if not configuration.get("sources"):
                if not configuration.get("targets"):
                    continue

            # Skip if page should not be considered
            filter = get_filter(configuration, "sources")
            if not filter(processor.path):
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

                # Skip headerlinks
                if "headerlink" in el.get("class", ""):
                    continue

                # Skip external links
                url = urlparse(href)
                if url.scheme or url.netloc:
                    continue

                # Include, if filter matches
                path = resolve(processor.path, url.path)
                if path and filter(path):
                    el.set("data-preview", "")


# -----------------------------------------------------------------------------


class PreviewExtension(Extension):
    """
    A Markdown extension to enable instant previews on links.

    This extensions allows to automatically add the `data-preview` attribute to
    internal links matching specific criteria, so Material for MkDocs renders a
    nice preview on hover as part of a tooltip. It is the recommended way to
    add previews to links in a programmatic way.
    """

    def __init__(self, *args, **kwargs):
        """
        Initialize the extension.
        """
        self.config = {
            "configurations": [[], "Filter configurations"],
            "sources": [{}, "Link sources"],
            "targets": [{}, "Link targets"],
        }
        super().__init__(*args, **kwargs)

    def extendMarkdown(self, md: Markdown):
        """
        Register Markdown extension.
        """
        md.registerExtension(self)

        # Create and register treeprocessor - we use the same priority as the
        # `relpath` treeprocessor, the latter of which is guaranteed to run
        # after our treeprocessor, so we can check the original Markdown URIs
        # before they are resolved to URLs.
        processor = PreviewProcessor(md, self.getConfigs())
        md.treeprocessors.register(processor, "preview", 0)


# -----------------------------------------------------------------------------
# Functions
# -----------------------------------------------------------------------------


def get_filter(settings: dict, key: str):
    """
    Get file filter from settings.
    """
    return Filter(config=settings.get(key, {}))  # type: ignore


def resolve(processor_path: str, url_path: str) -> str:
    """
    Resolve a relative URL path against the processor path.
    """
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


def makeExtension(**kwargs):
    """
    Register Markdown extension.
    """
    return PreviewExtension(**kwargs)
