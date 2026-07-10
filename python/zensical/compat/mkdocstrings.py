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

from io import BytesIO
from pathlib import Path
from typing import TYPE_CHECKING, Any

from zensical.extensions.autorefs import get_autorefs_store

if TYPE_CHECKING:
    from mkdocstrings import (  # ty:ignore[unresolved-import]
        Handlers,
        MkdocstringsExtension,
    )


# ----------------------------------------------------------------------------
# Globals
# ----------------------------------------------------------------------------


HANDLERS: Handlers | None = None


# ----------------------------------------------------------------------------
# Classes
# ----------------------------------------------------------------------------


class ToolConfig:
    """Mock mkdocstrings tooling configuration."""

    def __init__(self, config_file_path: str | None = None) -> None:
        self.config_file_path = config_file_path


# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def get_mkdocstrings_extension(
    handlers: dict[str, Any] | None = None,
    *,
    custom_templates: str | None = None,
    enable_inventory: bool = True,  # noqa: ARG001
    default_handler: str = "python",
    locale: str = "en",
    config: dict[str, Any],
) -> MkdocstringsExtension:
    """Create the mkdocstrings Markdown extension."""
    from mkdocstrings import (  # noqa: PLC0415  # ty:ignore[unresolved-import]
        Handlers,
        MkdocstringsExtension,
    )

    autorefs = get_autorefs_store()

    global HANDLERS  # noqa: PLW0603
    if HANDLERS is None:
        root_dir = Path(config["root_dir"])
        config_file = root_dir / "zensical.toml"
        tool_config = ToolConfig(config_file_path=str(config_file))
        HANDLERS = Handlers(
            theme="material",
            default=default_handler,
            inventory_project=config["site_name"],
            inventory_version="0.0.0",
            handlers_config=handlers if handlers is not None else {},
            custom_templates=custom_templates,
            mdx=config["markdown_extensions"],
            mdx_config=config["mdx_configs"],
            locale=locale,
            tool_config=tool_config,
        )

        HANDLERS._download_inventories()
        url_map = autorefs._abs_url_map
        for identifier, url in HANDLERS._yield_inventory_items():
            url_map[identifier] = url

    return MkdocstringsExtension(handlers=HANDLERS, autorefs=autorefs)


def get_inventory(cached: bytes | None) -> bytes:
    """Get inventory bytes, merging cached entries with fresh handlers data."""
    try:
        from mkdocstrings import (  # noqa: PLC0415  # ty:ignore[unresolved-import]
            Inventory,
        )
    except ImportError:
        return cached or b""

    if HANDLERS is None:
        return cached or b""

    if not cached:
        return HANDLERS.inventory.format_sphinx()

    base = Inventory.parse_sphinx(BytesIO(cached))
    for name, item in HANDLERS.inventory.items():
        base[name] = item

    # Bug in mkdocstrings's `parse_sphinx` method
    # not parsing project and version (fixed in latest).
    base.project = HANDLERS.inventory.project
    base.version = HANDLERS.inventory.version

    return base.format_sphinx()


def reset() -> None:
    """Reset global state in-between rebuilds."""
    global HANDLERS  # noqa: PLW0603
    HANDLERS = None
