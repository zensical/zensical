# Copyright (c) 2025-2026 Zensical and contributors

# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO

"""Tests for the mkdocstrings compatibility helpers."""

from __future__ import annotations

import builtins
import sys
from types import ModuleType
from typing import TYPE_CHECKING

import pytest

from zensical.compat import mkdocstrings

if TYPE_CHECKING:
    from typing import Any


class _Handler:
    def __init__(self) -> None:
        self._md: Any = None
        self.updated_with: Any = None
        self.rendered_backlinks: Any = None

    def _update_env(self, markdown: Any, *, config: Any) -> None:
        self._md = markdown
        self.updated_with = config

    def get_aliases(self, identifier: str) -> tuple[str, ...]:
        return (f"{identifier}.alias",)

    def render_backlinks(self, backlinks: Any) -> str:
        self.rendered_backlinks = backlinks
        return "rendered"


class _Handlers:
    def __init__(self, handlers: list[_Handler]) -> None:
        self._handlers = handlers
        self._tool_config = object()

    def get_handler(self, name: str) -> _Handler:
        assert name == "python"
        return self._handlers[0]


def test_cached_inventory_does_not_import_mkdocstrings(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A warm build can copy its inventory without loading mkdocstrings."""
    original_import = builtins.__import__

    def guarded_import(name: str, *args: Any, **kwargs: Any) -> Any:
        if name == "mkdocstrings":
            raise AssertionError("mkdocstrings should not be imported")
        return original_import(name, *args, **kwargs)

    monkeypatch.setattr(mkdocstrings, "HANDLERS", None)
    monkeypatch.setattr(builtins, "__import__", guarded_import)

    assert (
        mkdocstrings.get_inventory(b"cached inventory") == b"cached inventory"
    )


@pytest.mark.parametrize(
    ("handlers", "enabled"),
    [
        (None, False),
        ({}, False),
        ({"python": {"options": {}}}, False),
        ({"python": {"options": {"backlinks": False}}}, False),
        ({"python": {"options": {"backlinks": "flat"}}}, True),
        ({"python": {"options": {"backlinks": "tree"}}}, True),
    ],
)
def test_python_handler_backlinks_must_be_enabled(
    handlers: Any, enabled: bool
) -> None:
    """Omitted and explicitly disabled backlinks do not enable collection."""
    assert mkdocstrings._python_handler_has_backlinks(handlers) is enabled


def test_unlinked_backlink_crumb_paths_sort_by_title() -> None:
    """Unlinked navigation sections still sort by their displayed title."""
    paths = [
        (
            mkdocstrings._SortableBacklinkCrumb("Getting started", ""),
            mkdocstrings._SortableBacklinkCrumb("Introduction", "intro/"),
        ),
        (
            mkdocstrings._SortableBacklinkCrumb("Reference", ""),
            mkdocstrings._SortableBacklinkCrumb("Python API", "api/"),
        ),
        (
            mkdocstrings._SortableBacklinkCrumb("Guide", ""),
            mkdocstrings._SortableBacklinkCrumb("User guide", "guide/"),
        ),
    ]

    assert [path[0].title for path in sorted(paths)] == [
        "Getting started",
        "Guide",
        "Reference",
    ]


def test_backlink_handler_is_initialized_for_cached_pages(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Backlink templates get their handler filters without collection."""
    handler = _Handler()
    handlers = _Handlers([handler])
    monkeypatch.setattr(mkdocstrings, "HANDLERS", handlers)

    assert mkdocstrings.get_backlink_aliases("python", "target") == [
        "target.alias"
    ]
    assert handler._md is not None
    assert handler.updated_with is handlers._tool_config


def test_render_backlinks_preserves_sorted_rust_input(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Rendering does not put deterministic Rust paths back into a set."""

    class Backlink:
        def __init__(self, crumbs: Any) -> None:
            self.crumbs = crumbs

    autorefs = ModuleType("mkdocs_autorefs")
    monkeypatch.setattr(autorefs, "Backlink", Backlink, raising=False)
    monkeypatch.setitem(sys.modules, "mkdocs_autorefs", autorefs)
    handler = _Handler()
    monkeypatch.setattr(mkdocstrings, "HANDLERS", _Handlers([handler]))

    assert (
        mkdocstrings.render_backlinks(
            "python",
            [
                (
                    "referenced-by",
                    [
                        [("Getting started", "getting-started/")],
                        [("Guide", "guide/")],
                    ],
                )
            ],
        )
        == "rendered"
    )
    paths = handler.rendered_backlinks["referenced-by"]
    assert isinstance(paths, tuple)
    assert [path.crumbs[0].title for path in paths] == [
        "Getting started",
        "Guide",
    ]
