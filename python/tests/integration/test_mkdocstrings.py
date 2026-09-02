# Copyright (c) 2025-2026 Zensical and contributors

# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO

"""Integration tests for MkDocs-compatible mkdocstrings artifacts."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import zensical

if TYPE_CHECKING:
    from pathlib import Path


_BUILD_OPTIONS: dict[str, Any] = {"clean": False, "strict": False}


def test_object_inventory_is_restored_from_cache(tmp_path: Path) -> None:
    """The cached inventory is published when no handler updates it."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Home\n", encoding="utf-8")
    config = tmp_path / "zensical.toml"
    config.write_text('[project]\nsite_name = "Inventory"\n', encoding="utf-8")

    cache = tmp_path / ".cache"
    cache.mkdir()
    inventory = b"cached object inventory"
    (cache / "objects.inv").write_bytes(inventory)

    zensical.build(str(config), _BUILD_OPTIONS)

    assert (tmp_path / "site" / "objects.inv").read_bytes() == inventory
    assert (cache / "objects.inv").read_bytes() == inventory
