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
        *,
        fallback_title: str | None = None,
    ):
        self.url = url
        self.path = path
        self._title = title
        self._fallback_title = fallback_title
        self.meta: dict = meta if meta is not None else {}
        self._rendered_metadata_title: str | None = None

    @property
    def title(self) -> str | None:
        """The explicit title, current metadata title, or source fallback."""
        if self._title is not None:
            return self._title
        title = self.meta.get("title")
        return str(title) if title is not None else self._fallback_title

    @title.setter
    def title(self, title: str | None) -> None:
        self._title = title

    @property
    def title_override(self) -> str | None:
        """The title assigned by navigation or macros."""
        return self._title

    @property
    def rendered_metadata_title(self) -> str | None:
        """The rendered metadata title is returned for theme templates."""
        return self._rendered_metadata_title

    @rendered_metadata_title.setter
    def rendered_metadata_title(self, title: str | None) -> None:
        self._rendered_metadata_title = title


# This processor doesn't follow the usual pattern
# of receiving its configuration as a dataclass,
# because it only exists to store the rendering context.
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
        preprocessor = ContextPreprocessor(md=md, **self._kwargs)
        md.preprocessors.register(preprocessor, preprocessor.name, 0)


def makeExtension(**kwargs: Any) -> ContextExtension:
    """Register Markdown extension."""
    return ContextExtension(**kwargs)
