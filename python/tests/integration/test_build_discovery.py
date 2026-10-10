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

"""Check build output when filesystem events occur during discovery."""

from __future__ import annotations

import os
import subprocess
import sys
import time
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.skipif(
    sys.platform != "linux",
    reason="Startup event timing is verified with inotify",
)
@pytest.mark.parametrize(
    "target", ["zensical.toml", "overrides/main.html", "watched.txt"]
)
def test_build_completes_after_startup_metadata_events(
    tmp_path: Path, target: str
) -> None:
    # Config, template, and dependency reload events
    # are exercised independently.
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Complete page\n")
    overrides = tmp_path / "overrides"
    overrides.mkdir()
    (overrides / "main.html").write_text(
        "<html><body>{{ page.content | safe }}</body></html>"
    )
    (tmp_path / "watched.txt").write_text("Unchanged dependency\n")
    config = tmp_path / "zensical.toml"
    config.write_text(
        '[project]\nsite_name = "Discovery"\nwatch = ["watched.txt"]\n'
        '[project.theme]\ncustom_dir = "overrides"\n'
    )
    changed = tmp_path / target
    original = changed.read_bytes()
    log_path = tmp_path / "build.log"

    with log_path.open("w") as log:
        process = subprocess.Popen(
            [sys.executable, "-m", "zensical", "build", "--clean", "--strict"],
            cwd=tmp_path,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        try:
            deadline = time.monotonic() + 15
            while "Build started" not in log_path.read_text():
                assert process.poll() is None, log_path.read_text()
                assert time.monotonic() < deadline, log_path.read_text()
                time.sleep(0.001)

            # Only timestamps are changed inside the initial debounce window.
            for _ in range(5):
                os.utime(changed)
                time.sleep(0.002)
            process.wait(timeout=15)
        finally:
            if process.poll() is None:
                process.kill()
            process.wait()

    assert changed.read_bytes() == original
    assert process.returncode == 0, log_path.read_text()
    assert (tmp_path / "site" / "index.html").is_file(), log_path.read_text()
    assert "Complete page" in (tmp_path / "site" / "index.html").read_text()
