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

# Integration tests for config loading: full round-trip via `zensical.build()`.
#
# Each test creates a minimal project in a fresh temp directory and invokes
# `zensical.build()`, which calls `Config::new()` on the Rust side. The
# assertions target the *side-effects* of config loading
# (e.g. theme loading: both config formats, success and error cases).

from __future__ import annotations

import json
import os
import subprocess
import sys
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from pathlib import Path


def _build_extra_templates(
    tmp_path: Path,
    templates: dict[str, str],
    names: list[str],
    *,
    strict: bool = False,
    page: bool = True,
) -> subprocess.CompletedProcess[str]:
    """Build an isolated project and capture its process result."""
    docs = tmp_path / "docs"
    docs.mkdir()
    if page:
        (docs / "index.md").write_text("# Home")
    for name, source in templates.items():
        path = docs / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source)
    config = tmp_path / "zensical.toml"
    config.write_text(
        '[project]\nsite_name = "Test"\nextra_templates = '
        + json.dumps(names)
        + "\n"
    )

    script = (
        f"import zensical; zensical.build({str(config)!r}, "
        f"{{'strict': {strict!r}, 'clean': False}})"
    )
    # The test-generated script is run through the active interpreter.
    return subprocess.run(  # noqa: S603
        [sys.executable, "-c", script],
        capture_output=True,
        text=True,
        env=os.environ.copy(),
        timeout=30,
        check=False,
    )


def test_nested_extra_templates_preserve_distinct_paths(tmp_path: Path) -> None:
    # Distinct output paths are required even when the basenames are shared.
    result = _build_extra_templates(
        tmp_path,
        {
            "one/custom.html": "one: {{ config.site_name }}",
            "two/custom.html": "two: {{ config.site_name }}",
        },
        ["one/custom.html", "two/custom.html"],
    )

    assert result.returncode == 0, result.stderr
    assert (tmp_path / "site/one/custom.html").read_text() == "one: Test"
    assert (tmp_path / "site/two/custom.html").read_text() == "two: Test"
    assert not (tmp_path / "site/custom.html").exists()


@pytest.mark.parametrize("strict", [False, True])
@pytest.mark.parametrize("page", [False, True])
def test_missing_extra_template_warns_even_without_pages(
    tmp_path: Path, strict: bool, page: bool
) -> None:
    result = _build_extra_templates(
        tmp_path, {}, ["nested/missing.html"], strict=strict, page=page
    )

    assert (result.returncode != 0) == strict, result.stderr
    message = "Template skipped: 'nested/missing.html' not found in docs_dir."
    assert message in result.stderr


@pytest.mark.parametrize("strict", [False, True])
def test_extra_template_syntax_error_keeps_source(
    tmp_path: Path, strict: bool
) -> None:
    # The original source is retained when template compilation fails.
    source = "{% if %}"
    result = _build_extra_templates(
        tmp_path,
        {"nested/custom.html": source},
        ["nested/custom.html"],
        strict=strict,
    )

    assert (result.returncode != 0) == strict, result.stderr
    assert "Error reading template 'nested/custom.html'" in result.stderr
    assert (tmp_path / "site/nested/custom.html").read_text() == source


def test_extra_template_runtime_error_remains_fatal(tmp_path: Path) -> None:
    result = _build_extra_templates(
        tmp_path, {"custom.html": "{{ missing_function() }}"}, ["custom.html"]
    )

    assert result.returncode != 0
    assert "Error reading template" not in result.stderr


def test_successful_empty_extra_template_stays_empty(tmp_path: Path) -> None:
    result = _build_extra_templates(
        tmp_path,
        {"custom.html": "{% if false %}hidden{% endif %}"},
        ["custom.html"],
    )

    assert result.returncode == 0, result.stderr
    assert (tmp_path / "site/custom.html").read_text() == ""
