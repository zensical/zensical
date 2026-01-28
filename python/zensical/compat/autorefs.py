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

if TYPE_CHECKING:
    from mkdocs_autorefs import AutorefsExtension


# ----------------------------------------------------------------------------
# Global variables
# ----------------------------------------------------------------------------
AUTOREFS: AutorefsPlugin | None = None


# ----------------------------------------------------------------------------
# Classes
# ----------------------------------------------------------------------------
class AutorefsPage:
    """Mock MkDocs pages."""

    def __init__(self, url: str, path: str):
        self.url = url
        self.path = path


class AutorefsPlugin:
    """Mock the autorefs plugin (data store)."""

    def __init__(self) -> None:
        self.current_page: AutorefsPage | None = None
        self.scan_toc: bool = True
        self.record_backlinks: bool = False

        self._primary_url_map: dict[str, list[str]] = {}
        self._secondary_url_map: dict[str, list[str]] = {}
        self._abs_url_map: dict[str, str] = {}
        self._title_map: dict[str, str] = {}

    def register_anchor(
        self,
        page: AutorefsPage,
        identifier: str,
        anchor: str | None = None,
        *,
        title: str | None = None,
        primary: bool = True,
    ) -> None:
        url = f"{page.url}#{anchor or identifier}"
        url_map = self._primary_url_map if primary else self._secondary_url_map
        if identifier in url_map:
            if url not in url_map[identifier]:
                url_map[identifier].append(url)
        else:
            url_map[identifier] = [url]
        if title and url not in self._title_map:
            self._title_map[url] = title

    def register_url(self, identifier: str, url: str) -> None:
        self._abs_url_map[identifier] = url


# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------
def get_autorefs_plugin() -> AutorefsPlugin:
    """Get the global autorefs instance."""
    global AUTOREFS  # noqa: PLW0603
    if AUTOREFS is None:
        AUTOREFS = AutorefsPlugin()
    return AUTOREFS


def get_autorefs_extension() -> AutorefsExtension | None:
    """Get the MkDocs Autorefs extension."""
    try:
        from mkdocs_autorefs import AutorefsExtension  # noqa: PLC0415
    except ImportError:
        return None
    return AutorefsExtension(get_autorefs_plugin())  # type: ignore[arg-type,unused-ignore]


def set_autorefs_page(url: str, path: str) -> None:
    """Set the current page for autorefs."""
    plugin = get_autorefs_plugin()
    plugin.current_page = AutorefsPage(url=url, path=path)


def get_autorefs_data() -> dict[str, Any]:
    """Get autorefs data.

    This function is called from Rust to replace the `<autoref>`
    elements written in the HTML output by both the autorefs
    Markdown extension (for manual cross-references) and the
    mkdocstrings extension (for automatic cross-references).
    """
    if AUTOREFS:
        return {
            "primary": AUTOREFS._primary_url_map,
            "secondary": AUTOREFS._secondary_url_map,
            "inventory": AUTOREFS._abs_url_map,
            "titles": AUTOREFS._title_map,
        }
    return {}


def reset() -> None:
    """Reset global state in-between rebuilds."""
    global AUTOREFS  # noqa: PLW0603
    AUTOREFS = None
