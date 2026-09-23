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

import zensical

if TYPE_CHECKING:
    from pathlib import Path


_BUILD_OPTS: dict[str, Any] = {"clean": False, "strict": False}


def test_builds_table_reader_from_zensical_config(tmp_path: Path) -> None:
    """The plugin shim survives the Python-to-Rust configuration boundary."""
    (tmp_path / "docs").mkdir()
    (tmp_path / "tables").mkdir()
    (tmp_path / "docs" / "index.md").write_text(
        "# People\n\n"
        "{{ read_csv('people.csv') }}\n\n"
        "General expression: {{ 1 + 1 }}\n",
        encoding="utf-8",
    )
    (tmp_path / "tables" / "people.csv").write_text(
        "Name,Age\nAlice,30\n", encoding="utf-8"
    )
    config = tmp_path / "zensical.toml"
    config.write_text(
        "[project]\n"
        'site_name = "Test"\n'
        "[project.plugins.table-reader]\n"
        'data_path = "tables"\n',
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTS)

    html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert "<table>" in html
    assert "Alice" in html
    assert "{{ 1 + 1 }}" in html
