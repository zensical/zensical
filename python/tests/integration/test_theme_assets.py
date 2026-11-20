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

import os
import subprocess
import sys
from pathlib import Path

import pytest
from click.testing import CliRunner

import zensical
from tests.integration.helpers import wait_for_serve
from zensical.config import get_builtin_theme_dir
from zensical.main import cli


@pytest.mark.parametrize("option", ["config", "cli", "cli-disable"])
@pytest.mark.parametrize("extension", ["toml", "yml"])
def test_ignore_theme_mtimes_reuses_cache(
    tmp_path: Path, option: str, extension: str
) -> None:
    docs = tmp_path / "docs"
    docs.mkdir()
    source = docs / "index.md"
    source.write_text("# Theme cache\n", encoding="utf-8")
    theme = tmp_path / "theme"
    theme.mkdir()
    template = theme / "main.html"
    template.write_text("First: {{ page.content }}", encoding="utf-8")
    if extension == "toml":
        config = tmp_path / "zensical.toml"
        setting = "false" if option == "cli" else "true"
        config.write_text(
            '[project]\nsite_name = "Cache"\n'
            f"ignore_theme_mtimes = {setting}\n"
            '[project.theme]\ncustom_dir = "theme"\n',
            encoding="utf-8",
        )
    else:
        config = tmp_path / "mkdocs.yml"
        setting = "false" if option == "cli" else "true"
        config.write_text(
            f"site_name: Cache\nignore_theme_mtimes: {setting}\n"
            "theme:\n  custom_dir: theme\n",
            encoding="utf-8",
        )
    args = ["build", "--strict", "-f", str(config)]
    if option == "cli":
        args.append("--ignore-theme-mtimes")
    elif option == "cli-disable":
        args.append("--no-ignore-theme-mtimes")
    runner = CliRunner()
    result = runner.invoke(cli, args)
    assert result.exit_code == 0, result.output
    output = tmp_path / "site/index.html"
    assert "First:" in output.read_text(encoding="utf-8")

    # Timestamp-only changes should preserve artifacts in content hash mode.
    cached = [
        path
        for path in (tmp_path / ".cache").iterdir()
        if path.name.isdecimal()
    ]
    assert cached
    timestamp = 1_700_000_000_000_000_000
    for path in cached:
        os.utime(path, ns=(timestamp, timestamp))
    template_timestamp = template.stat().st_mtime_ns + 1_000_000_000
    os.utime(template, ns=(template_timestamp, template_timestamp))

    result = runner.invoke(cli, args)
    assert result.exit_code == 0, result.output
    assert all(path.stat().st_mtime_ns == timestamp for path in cached) is (
        option != "cli-disable"
    )
    assert "First:" in output.read_text(encoding="utf-8")

    # Content hash mode also detects edits that preserve the mtime.
    template.write_text("Later: {{ page.content }}", encoding="utf-8")
    if option == "cli-disable":
        template_timestamp += 1_000_000_000
    os.utime(template, ns=(template_timestamp, template_timestamp))
    result = runner.invoke(cli, args)
    assert result.exit_code == 0, result.output
    assert "Later:" in output.read_text(encoding="utf-8")

    source.write_text("# Updated documentation\n", encoding="utf-8")
    result = runner.invoke(cli, args)
    assert result.exit_code == 0, result.output
    assert "Updated documentation" in output.read_text(encoding="utf-8")


@pytest.mark.parametrize(
    "flag", ["--ignore-theme-mtimes", "--no-ignore-theme-mtimes"]
)
def test_theme_mtime_cli_override_does_not_bypass_config_validation(
    tmp_path: Path, flag: str
) -> None:
    (tmp_path / "docs").mkdir()
    config = tmp_path / "mkdocs.yml"
    config.write_text('site_name: Cache\nignore_theme_mtimes: "true"\n')

    result = CliRunner().invoke(cli, ["build", "-f", str(config), flag])

    assert result.exit_code != 0
    assert "'ignore_theme_mtimes' must be a bool" in result.output


def test_same_second_template_edits_invalidate_cache(tmp_path: Path) -> None:
    # Cached output is produced with a known subsecond template timestamp.
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text("# Theme cache\n", encoding="utf-8")
    theme = tmp_path / "theme"
    theme.mkdir()
    template = theme / "main.html"
    template.write_text("First: {{ page.content }}", encoding="utf-8")
    timestamp = 1_700_000_000_000_000_000
    os.utime(template, ns=(timestamp, timestamp + 100_000_000))
    config = tmp_path / "zensical.toml"
    config.write_text(
        '[project]\nsite_name = "Cache"\n'
        '[project.theme]\ncustom_dir = "theme"\n',
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": True})

    output = tmp_path / "site/index.html"
    assert "First:" in output.read_text(encoding="utf-8")

    # An edit in the same second must be rendered without clearing the cache.
    template.write_text("Later: {{ page.content }}", encoding="utf-8")
    os.utime(template, ns=(timestamp, timestamp + 200_000_000))

    zensical.build(str(config), {"clean": False, "strict": True})

    assert "Later:" in output.read_text(encoding="utf-8")


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


@pytest.mark.parametrize("ignore_theme_mtimes", [False, True])
def test_nested_theme_asset_changes_and_fallbacks_in_serve(
    tmp_path: Path,
    ignore_theme_mtimes: bool,
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
    args = [sys.executable, "-m", "zensical", "serve", "-f", str(config)]
    if ignore_theme_mtimes:
        args.append("--ignore-theme-mtimes")

    with log_path.open("w", encoding="utf-8") as log:
        process = subprocess.Popen(  # noqa: S603
            args,
            stdout=log,
            stderr=subprocess.STDOUT,
        )

        def wait_for_assets(expected: dict[str, bytes | None]) -> None:
            def matches() -> bool:
                for name, content in expected.items():
                    output = site / name
                    if content is None:
                        if output.exists():
                            return False
                    elif output.read_bytes() != content:
                        return False
                return True

            wait_for_serve(matches, process, log_path.read_text)

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
