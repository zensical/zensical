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

"""Integration tests for MkDocs Material metadata inheritance."""

from __future__ import annotations

import json
import subprocess
import sys
import time
from typing import TYPE_CHECKING, Any

import pytest

import zensical

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path


_BUILD_OPTIONS: dict[str, Any] = {"clean": False, "strict": False}


def test_nested_metadata_and_front_matter_render_with_custom_name(
    tmp_path: Path,
) -> None:
    """Ancestor maps and lists merge before page values take precedence."""
    docs = tmp_path / "docs"
    guide = docs / "guide"
    overrides = tmp_path / "overrides"
    guide.mkdir(parents=True)
    overrides.mkdir()
    (docs / "defaults.yml").write_text(
        "scope:\n  root: root\nitems: [root]\ntitle: Root\n",
        encoding="utf-8",
    )
    (guide / "defaults.yml").write_text(
        "scope:\n  guide: guide\nitems: [guide]\ntitle: Guide\n",
        encoding="utf-8",
    )
    (guide / "page.md").write_text(
        """\
---
scope:
  page: page
items: [page]
title: Page
---
# Content
""",
        encoding="utf-8",
    )
    (overrides / "main.html").write_text("{{ page.meta }}", encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Metadata
theme:
  name: material
  custom_dir: overrides
plugins:
  - material/meta:
      meta_file: defaults.yml
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    output = tmp_path / "site" / "guide" / "page" / "index.html"
    assert json.loads(output.read_text()) == {
        "items": ["root", "guide", "page"],
        "scope": {"guide": "guide", "page": "page", "root": "root"},
        "title": "Page",
    }


def test_reports_metadata_type_conflicts_with_both_sources(
    tmp_path: Path,
) -> None:
    """Incompatible inherited and page values retain useful source spans."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / ".meta.yml").write_text("value: inherited\n", encoding="utf-8")
    (docs / "index.md").write_text(
        "---\nvalue: [page]\n---\n# Page\n", encoding="utf-8"
    )
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Metadata
theme:
  name: material
plugins:
  - material/meta
""",
        encoding="utf-8",
    )

    with pytest.raises(RuntimeError) as caught:
        zensical.build(str(config), _BUILD_OPTIONS)

    message = str(caught.value)
    assert "metadata types do not match" in message
    assert ".meta.yml" in message
    assert "index.md" in message


def test_serve_rebuilds_descendants_after_metadata_edit(
    tmp_path: Path,
) -> None:
    """A retained workflow refreshes its index and dependent pages."""
    docs = tmp_path / "docs"
    overrides = tmp_path / "overrides"
    docs.mkdir()
    overrides.mkdir()
    metadata = docs / ".meta.yml"
    metadata.write_text("value: first\n", encoding="utf-8")
    (docs / "index.md").write_text("# Page\n", encoding="utf-8")
    (overrides / "main.html").write_text("{{ page.meta }}", encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Metadata
dev_addr: 127.0.0.1:0
theme:
  name: material
  custom_dir: overrides
plugins:
  - material/meta
""",
        encoding="utf-8",
    )
    log = (tmp_path / "serve.log").open("w+", encoding="utf-8")
    process = subprocess.Popen(  # noqa: S603
        [
            sys.executable,
            "-m",
            "zensical",
            "serve",
            "--config-file",
            str(config),
        ],
        cwd=tmp_path,
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    output = tmp_path / "site" / "index.html"

    def rendered_meta() -> dict[str, Any] | None:
        try:
            return json.loads(output.read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            return None

    def wait_for(condition: Callable[[], bool], timeout: float = 10.0) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if condition():
                return
            if process.poll() is not None:
                log.flush()
                log.seek(0)
                raise AssertionError(
                    f"serve exited with status {process.returncode}: "
                    f"{log.read()}"
                )
            time.sleep(0.02)
        log.flush()
        log.seek(0)
        raise AssertionError(
            f"serve did not rebuild metadata descendants: {log.read()}"
        )

    try:
        wait_for(lambda: rendered_meta() == {"value": "first"})
        with metadata.open("r+", encoding="utf-8") as stream:
            stream.write("value: other\n")
            stream.truncate()
        wait_for(lambda: rendered_meta() == {"value": "other"})
        assert process.poll() is None
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        log.close()
