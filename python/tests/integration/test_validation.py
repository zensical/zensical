# Copyright (c) 2025-2026 Zensical and contributors

# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO

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
