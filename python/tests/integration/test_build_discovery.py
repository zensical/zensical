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

"""Check build completion and error reporting during file discovery."""

from __future__ import annotations

import os
import stat
import subprocess
import sys
import time
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from pathlib import Path


def _write_project(root: Path) -> None:
    """Create a project with a page and a custom template."""
    docs = root / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Complete page\n", encoding="utf-8")

    overrides = root / "overrides"
    overrides.mkdir()
    (overrides / "main.html").write_text(
        "<html><body>{{ page.content | safe }}</body></html>",
        encoding="utf-8",
    )
    (root / "zensical.toml").write_text(
        '[project]\nsite_name = "Discovery"\n'
        '[project.theme]\ncustom_dir = "overrides"\n',
        encoding="utf-8",
    )


def _build(root: Path) -> subprocess.CompletedProcess[str]:
    """Build the project and capture its exit status and diagnostics."""
    return subprocess.run(
        [sys.executable, "-m", "zensical", "build", "--clean", "--strict"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=15,
        check=False,
    )


@pytest.mark.skipif(os.name != "posix", reason="POSIX filenames are required")
@pytest.mark.parametrize(
    "filename",
    [b"bad\\name.txt", b"bad\xff.txt", b"C:asset.txt"],
    ids=["backslash", "non-utf8", "drive-prefix"],
)
@pytest.mark.xfail(
    reason="Invalid source path errors are lost by the file agent (#1027)",
    raises=AssertionError,
    strict=True,
)
def test_build_reports_invalid_source_filename(
    tmp_path: Path,
    filename: bytes,
) -> None:
    _write_project(tmp_path)

    # An invalid asset name is placed beside a valid Markdown page.
    path = os.fsencode(tmp_path / "docs") + b"/" + filename
    with open(path, "wb") as stream:
        stream.write(b"Asset\n")

    result = _build(tmp_path)

    # Discovery failure must be reported instead of successful empty output.
    assert result.returncode != 0, result.stdout
    assert "invalid source path" in result.stdout, result.stdout


@pytest.mark.skipif(os.name != "posix", reason="POSIX permissions are required")
@pytest.mark.parametrize(
    "directory",
    ["docs", "docs/locked"],
    ids=["root", "child"],
)
@pytest.mark.xfail(
    reason="Directory watch errors are swallowed (#1027)",
    raises=AssertionError,
    strict=True,
)
def test_build_reports_unreadable_directory(
    tmp_path: Path,
    directory: str,
) -> None:
    _write_project(tmp_path)
    locked = tmp_path / directory
    locked.mkdir(exist_ok=True)
    (locked / "hidden.md").write_text("# Unreadable page\n", encoding="utf-8")

    # A recursive scan is blocked until the permissions are restored.
    original_mode = stat.S_IMODE(locked.stat().st_mode)
    locked.chmod(0)
    try:
        if os.access(locked, os.R_OK | os.X_OK):
            pytest.skip("Directory permissions can be bypassed by this user")
        result = _build(tmp_path)
    finally:
        locked.chmod(original_mode)

    # Missing documentation must be reported even if theme assets were built.
    assert result.returncode != 0, result.stdout
    assert "Permission denied" in result.stdout, result.stdout


@pytest.mark.skipif(sys.platform != "linux", reason="IN_ATTRIB requires Linux")
@pytest.mark.parametrize("metadata", ["hardlink", "timestamp", "permissions"])
def test_build_completes_after_template_metadata_changes(
    tmp_path: Path,
    metadata: str,
) -> None:
    _write_project(tmp_path)
    template = tmp_path / "overrides/main.html"
    original = template.read_bytes()
    original_stat = template.stat()
    original_mode = stat.S_IMODE(original_stat.st_mode)
    alias = tmp_path / "template-alias"
    log_path = tmp_path / "build.log"

    with log_path.open("w", encoding="utf-8") as log:
        process = subprocess.Popen(
            [sys.executable, "-m", "zensical", "build", "--clean", "--strict"],
            cwd=tmp_path,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        try:
            deadline = time.monotonic() + 15
            while True:
                messages = log_path.read_text(encoding="utf-8")
                if "Build started" in messages:
                    break
                assert process.poll() is None, messages
                assert time.monotonic() < deadline, messages
                time.sleep(0.001)

            # Only metadata is changed during initial discovery. Hardlink
            # changes outside the theme reproduce concurrent UV installations.
            for iteration in range(50):
                if metadata == "hardlink":
                    os.link(template, alias)
                    alias.unlink()
                elif metadata == "timestamp":
                    timestamp = original_stat.st_mtime + iteration + 1
                    os.utime(template, (original_stat.st_atime, timestamp))
                else:
                    mode = original_mode
                    if iteration % 2:
                        mode ^= stat.S_IXUSR
                    template.chmod(mode)
                time.sleep(0.002)

            process.wait(timeout=15)
        finally:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=5)
            template.chmod(original_mode)

    messages = log_path.read_text(encoding="utf-8")

    # A complete page must be produced despite changes unrelated to content.
    assert template.read_bytes() == original
    if metadata == "hardlink":
        assert template.stat().st_mtime_ns == original_stat.st_mtime_ns
    assert process.returncode == 0, messages
    output = tmp_path / "site/index.html"
    assert output.is_file(), messages
    assert "Complete page" in output.read_text(encoding="utf-8")
