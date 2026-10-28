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

"""Configuration compatibility for native meta descriptions."""

from __future__ import annotations

from typing import Any

import pytest

from zensical.config import ConfigurationError, _convert_plugins


def convert(value: Any) -> dict[str, Any]:
    """Normalize plugin settings with the required project fields."""
    project = {"extra": {"polyfills": []}, "root_dir": "."}
    return _convert_plugins(value, project)["meta_descriptions"]["config"]


def test_absent_plugin_is_disabled_with_upstream_defaults() -> None:
    assert convert([]) == {
        "enabled": False,
        "export_csv": False,
        "quiet": False,
        "enable_checks": False,
        "min_length": 50,
        "max_length": 160,
        "trim": False,
        "fallback_if_short": False,
    }


@pytest.mark.parametrize(
    "value",
    [
        ["meta-descriptions"],
        [{"meta-descriptions": None}],
        {"meta-descriptions": {}},
        {"material/meta-descriptions": None},
    ],
)
def test_plugin_presence_enables_generation(value: Any) -> None:
    assert convert(value)["enabled"] is True


def test_options_are_preserved_without_mutating_input() -> None:
    settings = {
        "enabled": False,
        "export_csv": True,
        "quiet": True,
        "enable_checks": True,
        "min_length": 0,
        "max_length": 100,
        "trim": True,
        "fallback_if_short": True,
    }
    original = settings.copy()

    normalized = convert({"meta-descriptions": settings})

    assert normalized == original
    assert settings == original


@pytest.mark.parametrize(
    "option",
    [
        "enabled",
        "export_csv",
        "quiet",
        "enable_checks",
        "trim",
        "fallback_if_short",
    ],
)
@pytest.mark.parametrize("value", [0, 1, "true", [], {}])
def test_boolean_options_are_validated(option: str, value: Any) -> None:
    message = f"meta-descriptions {option} must be a boolean"
    with pytest.raises(ConfigurationError, match=message):
        convert({"meta-descriptions": {option: value}})


@pytest.mark.parametrize("option", ["min_length", "max_length"])
@pytest.mark.parametrize("value", [True, -1, 1.5, "50", [], {}, 2**100])
def test_length_options_are_validated(option: str, value: Any) -> None:
    message = f"meta-descriptions {option} must be a non-negative integer"
    with pytest.raises(ConfigurationError, match=message):
        convert({"meta-descriptions": {option: value}})


def test_trim_width_is_independent_of_minimum_check_length() -> None:
    settings = convert({"meta-descriptions": {"max_length": 20, "trim": True}})

    assert settings["min_length"] == 50
    assert settings["max_length"] == 20


def test_zero_trim_width_is_rejected() -> None:
    with pytest.raises(ConfigurationError, match="max_length must be positive"):
        convert({"meta-descriptions": {"max_length": 0, "trim": True}})


def test_null_options_use_defaults() -> None:
    defaults = convert(["meta-descriptions"])

    settings = convert({"meta-descriptions": dict.fromkeys(defaults)})

    assert settings == defaults


@pytest.mark.parametrize("value", [[], True, "config", 42])
def test_plugin_configuration_must_be_a_mapping(value: Any) -> None:
    message = "meta-descriptions configuration must be a mapping"
    with pytest.raises(ConfigurationError, match=message):
        convert({"meta-descriptions": value})


def test_unknown_options_are_rejected() -> None:
    message = "unknown meta-descriptions option: unknown"
    with pytest.raises(ConfigurationError, match=message):
        convert({"meta-descriptions": {"unknown": True}})
