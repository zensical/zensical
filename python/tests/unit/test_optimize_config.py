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

from zensical.config import ConfigurationError, _convert_plugins


def _optimize(value: Any) -> list[dict[str, Any]]:
    return _convert_plugins(
        value, {"extra": {"polyfills": []}, "root_dir": "."}
    )["optimize"]["config"]


def test_absent_plugin_has_no_instances() -> None:
    assert _optimize([]) == []


@pytest.mark.parametrize(
    "entry",
    ["optimize", "material/optimize", {"optimize": None}, {"optimize": {}}],
)
def test_plugin_presence_enables_material_defaults(entry: Any) -> None:
    instances = _optimize([entry])

    assert len(instances) == 1
    options = instances[0]["config"]
    assert options["enabled"] is True
    assert options["optimize"] is True
    assert options["cache"] is True
    assert options["cache_dir"] == ".cache/plugin/optimize"
    assert options["concurrency"] >= 1
    assert options["optimize_png_speed"] == 3
    assert options["optimize_png_strip"] is True
    assert options["optimize_jpg_quality"] == 60
    assert options["optimize_jpg_progressive"] is True
    assert options["optimize_include"] == []
    assert options["optimize_exclude"] == []
    assert options["print_gain"] is True
    assert options["print_gain_summary"] is True


def test_preserves_instance_order_and_explicit_settings() -> None:
    instances = _optimize(
        [
            {
                "material/optimize/screenshots": {
                    "optimize_include": ["screenshots/*"],
                    "optimize_png_speed": 10,
                }
            },
            {
                "optimize/photos": {
                    "enabled": False,
                    "cache": False,
                    "optimize_jpg_quality": 0,
                }
            },
        ]
    )

    assert [instance["name"] for instance in instances] == [
        "optimize/screenshots",
        "optimize/photos",
    ]
    assert instances[0]["config"]["optimize_include"] == ["screenshots/*"]
    assert instances[0]["config"]["optimize_png_speed"] == 10
    assert instances[1]["config"]["enabled"] is False
    assert instances[1]["config"]["cache"] is False
    assert instances[1]["config"]["optimize_jpg_quality"] == 0


@pytest.mark.parametrize(
    "options",
    [
        {"enabled": "true"},
        {"optimize": None},
        {"cache": 1},
        {"concurrency": True},
        {"concurrency": 0},
        {"concurrency": 1.5},
        {"optimize_png_speed": 0},
        {"optimize_png_speed": 11},
        {"optimize_png_speed": True},
        {"optimize_jpg_quality": -1},
        {"optimize_jpg_quality": 101},
        {"optimize_jpg_quality": 60.5},
        {"optimize_png_strip": "yes"},
        {"optimize_jpg_progressive": 1},
        {"cache_dir": None},
        {"optimize_include": "*.png"},
        {"optimize_exclude": [1]},
        {"print_gain": 0},
        {"print_gain_summary": "false"},
        {"unknown": True},
    ],
)
def test_rejects_invalid_options(options: dict[str, Any]) -> None:
    with pytest.raises(ConfigurationError, match="optimize"):
        _optimize({"optimize": options})
