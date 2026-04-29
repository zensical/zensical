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

from markdown import Extension
from markdown.preprocessors import Preprocessor

if TYPE_CHECKING:
    from markdown import Markdown


# ----------------------------------------------------------------------------
# Classes
# ----------------------------------------------------------------------------


class Page:
    """A class representing a page being rendered."""

    def __init__(
        self,
        url: str,
        path: str,
        title: str | None = None,
        meta: dict | None = None,
    ):
        self.url = url
        self.path = path
        self.title: str | None = title
        self.meta: dict = meta if meta is not None else {}


class ContextPreprocessor(Preprocessor):
    """Preprocessor to store rendering context."""

    name = "rendering_context"

    def __init__(
        self,
        md: Markdown,
        page: Page,
        config: dict[str, Any],
    ):
        super().__init__(md)
        self.page = page
        self.config = config

    def run(self, lines: list[str]) -> list[str]:
        return lines

    @classmethod
    def from_markdown(cls, md: Markdown) -> ContextPreprocessor | None:
        """Lookup rendering context preprocessor from Markdown instance."""
        for processor in md.preprocessors:
            if isinstance(processor, cls):
                return processor
        return None


class ContextExtension(Extension):
    """Markdown extension to register rendering context."""

    name = "zensical.extensions.context"

    def __init__(self, **kwargs: Any):
        super().__init__()
        self._kwargs = kwargs

    def extendMarkdown(self, md: Markdown) -> None:
        """Register rendering context preprocessor."""
        # We must register the extension to ensure markdown-exec
        # is able to forward it to its inner Markdown instances
        md.registerExtension(self)
        md.preprocessors.register(
            ContextPreprocessor(md=md, **self._kwargs),
            ContextPreprocessor.name,
            0,
        )


def makeExtension(**kwargs: Any) -> ContextExtension:
    """Register Markdown extension."""
    return ContextExtension(**kwargs)
