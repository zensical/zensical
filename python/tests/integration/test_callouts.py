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

import pytest

import zensical

if TYPE_CHECKING:
    from pathlib import Path


_BUILD_OPTIONS: dict[str, Any] = {"clean": False, "strict": True}


def test_callouts_plugin_renders_obsidian_callout(tmp_path: Path) -> None:
    """The plugin entry enables PyMdown's callout syntax."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.md").write_text(
        "# Callouts\n\n> [!NOTE] Native title\n> Rendered body.\n",
        encoding="utf-8",
    )
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        "site_name: Callouts\n"
        "plugins:\n"
        "  - callouts:\n"
        "      aliases: false\n"
        "      breakless_lists: false\n"
        "      title_from_first_bold: true\n",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert '<div class="admonition note">' in html
    assert '<p class="admonition-title">Native title</p>' in html
    assert "Rendered body." in html


@pytest.mark.parametrize(
    ("config_name", "config_text"),
    [
        pytest.param(
            "mkdocs.yml",
            "site_name: GitHub alerts\nplugins:\n  - gh-admonitions\n",
            id="yaml",
        ),
        pytest.param(
            "zensical.toml",
            "[project]\n"
            'site_name = "GitHub alerts"\n'
            "[project.plugins.gh-admonitions]\n",
            id="toml",
        ),
    ],
)
def test_gh_admonitions_plugin_renders_github_alerts(
    tmp_path: Path, config_name: str, config_text: str
) -> None:
    """The gh-admonitions plugin entry renders GitHub alert syntax."""
    # Cover all five GitHub alert types in one page.
    alert_types = ("NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION")
    docs = tmp_path / "docs"
    docs.mkdir()
    alerts = [f"> [!{alert}]\n> Body for {alert}." for alert in alert_types]
    (docs / "index.md").write_text(
        "# GitHub alerts\n\n" + "\n\n".join(alerts),
        encoding="utf-8",
    )
    config = tmp_path / config_name
    config.write_text(config_text, encoding="utf-8")

    zensical.build(str(config), _BUILD_OPTIONS)

    html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    for alert in alert_types:
        assert f'<div class="admonition {alert.lower()}">' in html
        assert f'<p class="admonition-title">{alert.title()}</p>' in html
        assert f"Body for {alert}." in html
