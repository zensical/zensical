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

from zensical.compat.autorefs import get_autorefs_plugin

if TYPE_CHECKING:
    from mkdocstrings import Handlers, MkdocstringsExtension


# ----------------------------------------------------------------------------
# Global variables
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
    config: dict[str, Any],
    path: str,
) -> MkdocstringsExtension:
    """Create the mkdocstrings Markdown extension."""
    from mkdocstrings import Handlers, MkdocstringsExtension  # noqa: PLC0415

    autorefs = get_autorefs_plugin()

    global HANDLERS  # noqa: PLW0603
    if HANDLERS is None:
        mkdocstrings_config = config["plugins"]["mkdocstrings"]["config"]
        tool_config = ToolConfig(config_file_path=path)
        HANDLERS = Handlers(
            theme="material",
            default=mkdocstrings_config.get("default_handler") or "python",
            inventory_project=mkdocstrings_config.get("inventory_project")
            or config["site_name"],
            inventory_version=mkdocstrings_config.get("inventory_version"),
            handlers_config=mkdocstrings_config.get("handlers"),
            custom_templates=mkdocstrings_config.get("custom_templates"),
            mdx=config["markdown_extensions"],
            mdx_config=config["mdx_configs"],
            locale=mkdocstrings_config.get("locale"),
            tool_config=tool_config,
        )

        HANDLERS._download_inventories()
        url_map = autorefs._abs_url_map
        for identifier, url in HANDLERS._yield_inventory_items():
            url_map[identifier] = url

    return MkdocstringsExtension(handlers=HANDLERS, autorefs=autorefs)


def get_inventory() -> bytes:
    """Get the objects.inv inventory as bytes.

    This function is called from Rust to write
    the objects.inv file in the site directory.
    """
    if HANDLERS:
        return HANDLERS.inventory.format_sphinx()
    return b""


def reset() -> None:
    """Reset global state in-between rebuilds."""
    global HANDLERS  # noqa: PLW0603
    HANDLERS = None
