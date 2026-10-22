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

from typing import Any

import pytest

from zensical.compat.privacy import matches, media_links
from zensical.config import ConfigurationError, _convert_plugins


def _privacy(value: Any) -> list[dict[str, Any]]:
    config = {"extra": {"polyfills": []}, "root_dir": "."}
    return _convert_plugins(value, config)["privacy"]["config"]


def test_absent_plugin_has_no_instances() -> None:
    assert _privacy([]) == []


def test_material_aliases_and_ordered_instances() -> None:
    instances = _privacy(
        [
            "material/privacy",
            {"privacy/images": {"assets": False}},
            {"material/privacy/links": {"enabled": False}},
        ]
    )

    assert [instance["name"] for instance in instances] == [
        "privacy",
        "privacy/images",
        "privacy/links",
    ]
    defaults = instances[0]["config"]
    assert defaults["assets_fetch"] is True
    assert defaults["assets_fetch_dir"] == "assets/external"
    assert defaults["cache_dir"] == ".cache/plugin/privacy"
    assert defaults["links_noopener"] is True
    assert instances[1]["config"]["assets"] is False
    assert instances[2]["config"]["enabled"] is False


@pytest.mark.parametrize(
    "options",
    [
        {"enabled": "yes"},
        {"assets_fetch": None},
        {"concurrency": True},
        {"concurrency": 0},
        {"log_level": "ignore"},
        {"assets_fetch_dir": "../outside"},
        {"assets_fetch_dir": "/outside"},
        {"assets_include": "example.com/*"},
        {"assets_exclude": [1]},
        {"assets_expr_map": {".css": "["}},
        {"assets_expr_map": {".css": "https://.*"}},
        {"links_attr_map": {"target": True}},
        {"links_attr_map": {"bad attribute": "value"}},
        {"unknown_option": True},
    ],
)
def test_rejects_invalid_settings(options: dict[str, Any]) -> None:
    with pytest.raises(ConfigurationError):
        _privacy({"privacy": options})


def test_legacy_options_are_adapted_to_canonical_options() -> None:
    with pytest.warns(
        DeprecationWarning, match="external_assets_dir.*assets_fetch_dir"
    ):
        instances = _privacy({"privacy": {"external_assets_dir": "downloads"}})

    assert instances[0]["config"]["assets_fetch_dir"] == "downloads"
    assert "external_assets_dir" not in instances[0]["config"]


def test_python_regex_overrides_keep_backreferences_and_utf8_offsets() -> None:
    content = 'é: "https://example.com/image.png"'

    matches = media_links(content, r'(["\'])(?P<url>https://[^"\']+)\1')

    start, end, value = matches[0]
    assert content.encode()[start:end].decode() == value
    assert value == "https://example.com/image.png"


def test_filters_keep_fnmatch_literal_braces_and_unclosed_brackets() -> None:
    assert matches("example.com/{one,two}", ["example.com/{one,two}"])
    assert not matches("example.com/one", ["example.com/{one,two}"])
    assert matches("example.com/[image", ["example.com/[image"])
