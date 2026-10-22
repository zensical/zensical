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

"""Verify resources from overlapping documentation and theme directories."""

from __future__ import annotations

import subprocess
import sys
import time
from pathlib import Path

import pytest

import zensical
from zensical.config import get_builtin_theme_dir


@pytest.mark.parametrize("theme_dir", ["docs/overrides", "theme"])
def test_theme_assets_are_published_at_both_logical_paths(
    tmp_path: Path, theme_dir: str
) -> None:
    # The same theme is tested inside and outside the documentation root.
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Theme assets\n", encoding="utf-8")
    theme = tmp_path / theme_dir
    (theme / "assets").mkdir(parents=True)
    (theme / "assets/probe.css").write_text(
        "/* custom theme */", encoding="utf-8"
    )
    (theme / "main.html").write_text(
        '<html>CUSTOM_THEME_MARKER<link href="assets/probe.css">'
        "{{ page.content | safe }}</html>",
        encoding="utf-8",
    )
    config = tmp_path / "zensical.toml"
    config.write_text(
        '[project]\nsite_name = "Assets"\n'
        f'[project.theme]\ncustom_dir = "{theme_dir}"\n',
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": True})

    site = tmp_path / "site"
    assert (site / "assets/probe.css").read_text() == "/* custom theme */"
    assert "CUSTOM_THEME_MARKER" in (site / "index.html").read_text()
    if theme_dir.startswith("docs/"):
        assert (
            site / "overrides/assets/probe.css"
        ).read_text() == "/* custom theme */"

    # An ordinary docs asset takes precedence over the custom theme asset.
    (docs / "assets").mkdir()
    (docs / "assets/probe.css").write_text(
        "/* docs override */", encoding="utf-8"
    )

    zensical.build(str(config), {"clean": True, "strict": True})

    assert (site / "assets/probe.css").read_text() == "/* docs override */"


def test_nested_theme_asset_changes_and_fallbacks_in_serve(
    tmp_path: Path,
) -> None:
    docs = tmp_path / "docs"
    theme = docs / "overrides"
    asset = theme / "assets/images/favicon.png"
    asset.parent.mkdir(parents=True)
    asset.write_bytes(b"custom favicon")
    (docs / "index.md").write_text("# Theme assets\n", encoding="utf-8")
    config = tmp_path / "zensical.toml"
    config.write_text(
        '[project]\nsite_name = "Assets"\ndev_addr = "127.0.0.1:0"'
        '\n[project.theme]\ncustom_dir = "docs/overrides"\n',
        encoding="utf-8",
    )
    site = tmp_path / "site"
    log_path = tmp_path / "serve.log"

    with log_path.open("w", encoding="utf-8") as log:
        process = subprocess.Popen(  # noqa: S603
            [sys.executable, "-m", "zensical", "serve", "-f", str(config)],
            stdout=log,
            stderr=subprocess.STDOUT,
        )

        def wait_for_assets(expected: dict[str, bytes | None]) -> None:
            # Wait until all published asset copies match the current inputs.
            deadline = time.monotonic() + 15
            while True:
                matches = True
                for name, content in expected.items():
                    output = site / name
                    if content is None:
                        matches &= not output.exists()
                    else:
                        try:
                            matches &= output.read_bytes() == content
                        except FileNotFoundError:
                            matches = False
                if matches:
                    return
                assert process.poll() is None, log_path.read_text()
                assert time.monotonic() < deadline, log_path.read_text()
                time.sleep(0.02)

        try:
            # Both logical copies are published during discovery
            # and after an edit.
            wait_for_assets(
                {
                    "assets/images/favicon.png": b"custom favicon",
                    "overrides/assets/images/favicon.png": b"custom favicon",
                }
            )
            asset.write_bytes(b"edited favicon")
            wait_for_assets(
                {
                    "assets/images/favicon.png": b"edited favicon",
                    "overrides/assets/images/favicon.png": b"edited favicon",
                }
            )

            # A rename removes both old copies and reveals the bundled fallback.
            fallback = (
                Path(get_builtin_theme_dir()) / "assets/images/favicon.png"
            ).read_bytes()
            renamed = asset.with_name("renamed.png")
            asset.rename(renamed)
            wait_for_assets(
                {
                    "assets/images/favicon.png": fallback,
                    "overrides/assets/images/favicon.png": None,
                    "assets/images/renamed.png": b"edited favicon",
                    "overrides/assets/images/renamed.png": b"edited favicon",
                }
            )

            # Both logical copies are removed when the renamed asset is deleted.
            renamed.unlink()
            wait_for_assets(
                {
                    "assets/images/renamed.png": None,
                    "overrides/assets/images/renamed.png": None,
                }
            )
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
