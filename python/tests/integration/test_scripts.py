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

"""Verify extra script configuration and generated tags."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import pytest
import yaml

import zensical
from zensical.config import parse_config

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize(
    ("entry", "kind", "attributes"),
    [
        ("custom.mjs", "module", ' type="module"'),
        ("custom.MJS", None, ""),
        ("custom.mjs?v=1", None, ""),
        ("custom.mjs#fragment", None, ""),
        ("custom.js?file=.mjs", "module", ' type="module"'),
        ({"path": "custom.mjs", "async": True}, None, " async"),
        ({"path": "custom.mjs", "defer": True}, None, " defer"),
        (
            {
                "path": "custom.mjs",
                "type": "text/javascript",
                "async": True,
                "defer": True,
            },
            "text/javascript",
            ' type="text/javascript" async defer',
        ),
        ({"path": "custom.MJS", "type": "module"}, "module", ' type="module"'),
    ],
)
def test_extra_script_module_inference(
    tmp_path: Path,
    entry: str | dict[str, Any],
    kind: str | None,
    attributes: str,
) -> None:
    # Both template configuration and rendered attributes are checked.
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs/index.md").write_text("# Scripts", encoding="utf-8")
    (tmp_path / "theme").mkdir()
    (tmp_path / "theme/main.html").write_text(
        "{% for script in config.extra_javascript %}"
        '{% if script.type == "module" %}MODULE_CONFIG{% endif %}'
        "{{ script | script_tag | safe }}{% endfor %}",
        encoding="utf-8",
    )
    config_file = tmp_path / "mkdocs.yml"
    config_file.write_text(
        yaml.safe_dump(
            {
                "site_name": "Scripts",
                "theme": {"name": "material", "custom_dir": "theme"},
                "extra_javascript": [entry],
            }
        ),
        encoding="utf-8",
    )

    config = parse_config(str(config_file))
    zensical.build(str(config_file), {"clean": True, "strict": True})

    path = entry if isinstance(entry, str) else entry["path"]
    assert config["extra_javascript"][0]["type"] == kind
    output = (tmp_path / "site/index.html").read_text(encoding="utf-8")
    assert f'<script src="./{path}"{attributes}></script>' in output
    assert ("MODULE_CONFIG" in output) == (kind == "module")


@pytest.mark.parametrize(
    ("entry", "kind"),
    [
        ("custom.MJS", "module"),
        ({"path": "custom.mjs", "async": True}, "module"),
        ({"path": "custom.MJS", "type": "text/javascript"}, "text/javascript"),
    ],
)
def test_polyfill_module_inference_is_preserved(
    tmp_path: Path, entry: str | dict[str, Any], kind: str
) -> None:
    # Legacy inference is retained for polyfills independently of extra scripts.
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs/index.md").write_text("# Polyfills", encoding="utf-8")
    (tmp_path / "theme").mkdir()
    (tmp_path / "theme/main.html").write_text(
        "{% for script in config.extra.polyfills %}"
        "{{ script | script_tag | safe }}{% endfor %}",
        encoding="utf-8",
    )
    config_file = tmp_path / "mkdocs.yml"
    config_file.write_text(
        yaml.safe_dump(
            {
                "site_name": "Polyfills",
                "theme": {"name": "material", "custom_dir": "theme"},
                "extra": {"polyfills": [entry]},
            }
        ),
        encoding="utf-8",
    )

    zensical.build(str(config_file), {"clean": True, "strict": True})

    output = (tmp_path / "site/index.html").read_text(encoding="utf-8")
    assert f'type="{kind}"' in output
    assert (" async" in output) == (
        isinstance(entry, dict) and entry.get("async", False)
    )
