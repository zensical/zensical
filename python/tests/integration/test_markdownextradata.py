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


_BUILD_OPTS: dict[str, Any] = {"clean": False, "strict": False}


@pytest.mark.parametrize("config_format", ["toml", "yaml"])
def test_builds_markdownextradata_from_project_config(
    tmp_path: Path, config_format: str
) -> None:
    """Check that a site build replaces variables in page titles and content."""
    (tmp_path / "docs").mkdir()
    (tmp_path / "data").mkdir()
    (tmp_path / "overrides").mkdir()
    (tmp_path / "overrides" / "main.html").write_text(
        '{% extends "base.html" %}{% block content %}{{ super() }}'
        '<div id="template-data">{{ config.extra.details.message }} / '
        "{{ config.extra.types.date }} / "
        '{{ config.extra.types.pair | join("-") }}</div>'
        "{% endblock %}",
        encoding="utf-8",
    )
    (tmp_path / "docs" / "index.md").write_text(
        "---\ntitle: Hello {{ customer.name }}\n---\n\n"
        "# Hello {{ customer.name }}\n\n"
        "{{ site_name }}: {{ customer.name }} / {{ details.message }} / "
        "{{ missing }}\n\n"
        "{{ types.date.strftime('%Y') }} / {{ types.pair | join('-') }} / "
        "{{ types.empty }}\n\n"
        "```text\n{{ customer.name }}\n```\n",
        encoding="utf-8",
    )
    (tmp_path / "data" / "details.json").write_text(
        '{"message": "External data"}', encoding="utf-8"
    )
    (tmp_path / "data" / "types.yaml").write_text(
        "date: 2026-10-06\npair: [1, 2]\nempty: null\n",
        encoding="utf-8",
    )
    config = tmp_path / (
        "zensical.toml" if config_format == "toml" else "mkdocs.yml"
    )
    config.write_text(
        '[project]\nsite_name = "Test"\n[project.theme]\ncustom_dir = '
        '"overrides"\n'
        '[project.plugins.markdownextradata]\ndata = "data"\n'
        '[project.extra.customer]\nname = "Alice"\n'
        if config_format == "toml"
        else "site_name: Test\ntheme:\n  custom_dir: overrides\n"
        "plugins:\n  - markdownextradata:\n      data: data\n"
        "extra:\n  customer:\n    name: Alice\n",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTS)

    html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert "Hello Alice" in html
    assert "Hello {{ customer.name }}" not in html
    assert "Test: Alice / External data / {{ missing }}" in html
    assert "2026 / 1-2 / None" in html
    assert (
        '<div id="template-data">External data / 2026-10-06 / 1-2</div>' in html
    )
