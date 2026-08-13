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

from typing import TYPE_CHECKING

import zensical

if TYPE_CHECKING:
    from pathlib import Path

    import pytest


def test_validation_reports_issues_after_rendering(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    """Validation reports source and autoref issues after page rendering."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "# Hello\n\n[normal](missing.md)\n\n[autoref][missing-id]\n",
        encoding="utf-8",
    )
    (docs / "other.md").write_text("# Other\n", encoding="utf-8")
    (tmp_path / "watched.md").write_text(
        "# Watched support file\n", encoding="utf-8"
    )
    config = tmp_path / "zensical.toml"
    config.write_text(
        """
[project]
site_name = "Test"
watch = ["watched.md"]

[project.validation]
unresolved_references = true
""".lstrip(),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": False})

    captured = capfd.readouterr()
    assert "page does not exist" in captured.err
    assert "unresolved link reference" in captured.err
    assert "2 issues found" in captured.err
    assert captured.err.count("2 issues found") == 1

    (docs / "index.md").write_text("# Hello\n", encoding="utf-8")
    zensical.build(str(config), {"clean": True, "strict": False})

    captured = capfd.readouterr()
    assert "No issues found" in captured.err
    assert "2 issues found" not in captured.err
