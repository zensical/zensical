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

from dataclasses import dataclass
from io import BytesIO
from pathlib import Path
from typing import TYPE_CHECKING, Any, cast

from zensical.extensions.autorefs import get_autorefs_store

if TYPE_CHECKING:
    from mkdocstrings import (
        Handlers,
        MkdocstringsExtension,
    )


# ----------------------------------------------------------------------------
# Globals
# ----------------------------------------------------------------------------


HANDLERS: Handlers | None = None
_ENABLE_INVENTORY: bool | None = None


# ----------------------------------------------------------------------------
# Classes
# ----------------------------------------------------------------------------


class ToolConfig:
    """Mock mkdocstrings tooling configuration."""

    def __init__(self, config_file_path: str | None = None) -> None:
        self.config_file_path = config_file_path


@dataclass(frozen=True, order=True)
class _SortableBacklinkCrumb:
    """Backlink crumb with title-aware equality and ordering."""

    title: str
    url: str


# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def _python_handler_has_backlinks(
    handlers: dict[str, Any] | None,
) -> bool:
    """Return whether the Python handler enables backlinks."""
    if not isinstance(handlers, dict):
        return False
    handler = handlers.get("python")
    if not isinstance(handler, dict):
        return False
    options = handler.get("options")
    return isinstance(options, dict) and bool(options.get("backlinks", False))


def _get_handlers(
    handlers: dict[str, Any] | None = None,
    *,
    custom_templates: str | None = None,
    enable_inventory: bool | None = None,
    default_handler: str = "python",
    locale: str = "en",
    config: dict[str, Any],
) -> Handlers:
    """Create or return the handlers shared by the current build."""
    from mkdocstrings import (  # noqa: PLC0415
        Handlers,
    )

    global HANDLERS, _ENABLE_INVENTORY  # noqa: PLW0603
    _ENABLE_INVENTORY = enable_inventory
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
        url_map = get_autorefs_store()._abs_url_map
        for identifier, url in HANDLERS._yield_inventory_items():
            url_map[identifier] = url

    return HANDLERS


def _ensure_handlers() -> Handlers | None:
    """Initialize handlers when every Markdown page came from cache."""
    if HANDLERS is not None:
        return HANDLERS

    from zensical.config import get_config  # noqa: PLC0415

    config = get_config()
    options = config["mdx_configs"].get("zensical.extensions.mkdocstrings")
    if options is None or not options.get("enabled", True):
        return None
    options = dict(options)
    options.pop("enabled", None)
    return _get_handlers(**options, config=config)


def _get_backlink_handler(handler_name: str) -> tuple[Handlers, Any] | None:
    """Return a handler prepared to render backlinks on cached builds."""
    handlers = _ensure_handlers()
    if handlers is None:
        return None

    handler = handlers.get_handler(handler_name)
    if getattr(handler, "_md", None) is None:
        from markdown import Markdown  # noqa: PLC0415

        handler._update_env(Markdown(), config=handlers._tool_config)
    return handlers, handler


def get_mkdocstrings_extension(
    handlers: dict[str, Any] | None = None,
    *,
    custom_templates: str | None = None,
    enable_inventory: bool | None = None,
    default_handler: str = "python",
    locale: str = "en",
    config: dict[str, Any],
) -> MkdocstringsExtension:
    """Create the mkdocstrings Markdown extension."""
    from mkdocstrings import (  # noqa: PLC0415
        MkdocstringsExtension,
    )

    autorefs = get_autorefs_store()
    autorefs.record_backlinks = _python_handler_has_backlinks(handlers)
    handlers_instance = _get_handlers(
        handlers,
        custom_templates=custom_templates,
        enable_inventory=enable_inventory,
        default_handler=default_handler,
        locale=locale,
        config=config,
    )
    # Upstream annotates this as its full plugin; our store supplies the
    # compatible anchor-registration interface used by the extension.
    return MkdocstringsExtension(
        handlers=handlers_instance, autorefs=cast("Any", autorefs)
    )


def get_inventory(cached: bytes | None) -> bytes:
    """Get inventory bytes, merging cached entries with fresh handlers data."""
    if HANDLERS is None:
        return cached or b""

    try:
        from mkdocstrings import (  # noqa: PLC0415
            Inventory,
        )
    except ImportError:
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


def get_backlink_aliases(handler_name: str, identifier: str) -> list[str]:
    """Return aliases used by a handler for a backlink placeholder."""
    result = _get_backlink_handler(handler_name)
    if result is None:
        return []
    _, handler = result
    return list(handler.get_aliases(identifier))


def render_backlinks(
    handler_name: str,
    backlinks: list[tuple[str, list[list[tuple[str, str]]]]],
) -> str:
    """Turn serializable backlink crumbs into handler-rendered HTML."""
    if not backlinks:
        return ""
    result = _get_backlink_handler(handler_name)
    if result is None:
        return ""
    handlers, handler = result

    from inspect import signature  # noqa: PLC0415

    from mkdocs_autorefs import (  # noqa: PLC0415
        Backlink,
    )

    # Rust already returns these paths sorted and deduplicated. Preserve that
    # order instead of introducing Python's process-random set order again.
    # Our crumbs supply the title and URL fields expected by the handler.
    data = {
        backlink_type: tuple(
            Backlink(
                cast(
                    "Any",
                    tuple(
                        _SortableBacklinkCrumb(title=title, url=url)
                        for title, url in crumbs
                    ),
                )
            )
            for crumbs in backlink_list
        )
        for backlink_type, backlink_list in backlinks
    }
    kwargs = {}
    if "locale" in signature(handler.render_backlinks).parameters:
        kwargs["locale"] = handlers._locale
    return handler.render_backlinks(data, **kwargs)


def get_inventory_policy(cached_auto_enabled: bool) -> tuple[bool, bool]:
    """Decide whether to write `objects.inv`.

    Return two booleans: whether to write the file, and whether any handler
    enables it by default.

    We remember the second value for cached pages, whose handlers may not run
    again. We reuse it only if the project configuration has not changed.
    """
    from zensical.config import get_config  # noqa: PLC0415

    config = get_config()
    plugin = config.get("plugins", {}).get("mkdocstrings", {}).get("config", {})
    options = config.get("mdx_configs", {}).get(
        "zensical.extensions.mkdocstrings", plugin
    )
    auto_enabled = cached_auto_enabled or (
        HANDLERS is not None
        and any(handler.enable_inventory for handler in HANDLERS.seen_handlers)
    )
    setting = (
        _ENABLE_INVENTORY
        if HANDLERS is not None
        else options.get("enable_inventory")
    )
    enabled = options.get("enabled", True) and (
        auto_enabled if setting is None else setting
    )
    return enabled, auto_enabled


def reset() -> None:
    """Reset global state in-between rebuilds."""
    global HANDLERS, _ENABLE_INVENTORY  # noqa: PLW0603
    HANDLERS = None
    _ENABLE_INVENTORY = None
