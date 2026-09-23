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
