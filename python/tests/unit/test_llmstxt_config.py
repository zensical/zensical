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

"""Configuration compatibility for native LLM text output."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import pytest

from zensical.config import ConfigurationError, _convert_plugins

if TYPE_CHECKING:
    from pathlib import Path


def convert(
    value: Any,
    site_url: str | None = "https://example.org/docs/",
    *,
    root_dir: str = ".",
) -> dict[str, Any]:
    """Normalize plugin settings with the project fields they use."""
    project = {
        "site_url": site_url,
        "extra": {"polyfills": []},
        "root_dir": root_dir,
    }
    return _convert_plugins(value, project)["llmstxt"]["config"]


def test_plugin_presence_controls_generation() -> None:
    assert convert([])["enabled"] is False
    assert convert(["llmstxt"])["enabled"] is True
    assert (
        convert({"llmstxt": {"enabled": False}}, site_url=None)["enabled"]
        is False
    )


def test_sections_preserve_order_patterns_and_descriptions() -> None:
    settings = convert(
        {
            "llmstxt": {
                "base_url": "https://other.org/v1",
                "markdown_description": "Long **description**.",
                "full_output": "ai/full.txt",
                "sections": {
                    "Z first": [{"index.md": "Home"}, "guide/*.md"],
                    "A second": ["other.md"],
                },
            }
        }
    )

    assert settings["autoclean"] is True
    assert settings["base_url"] == "https://other.org/v1"
    assert settings["full_output"] == "ai/full.txt"
    assert [section["title"] for section in settings["sections"]] == [
        "Z first",
        "A second",
    ]
    inputs = settings["sections"][0]["inputs"]
    assert inputs[0] == {
        "pattern": "index.md",
        "description": "Home",
        "expression": None,
    }
    assert inputs[1]["pattern"] == "guide/*.md"
    assert inputs[1]["expression"].startswith(r"\A(?s:")


def test_preprocess_path_is_resolved_relative_to_project(
    tmp_path: Path,
) -> None:
    script = tmp_path / "custom.py"
    script.write_text(
        "def preprocess(html, output): return html\n", encoding="utf-8"
    )

    settings = convert(
        {"llmstxt": {"preprocess": "custom.py"}}, root_dir=str(tmp_path)
    )

    assert settings["preprocess"] == str(script)
    assert convert(["llmstxt"])["preprocess"] is None


def test_disabled_preprocess_script_does_not_need_to_exist(
    tmp_path: Path,
) -> None:
    settings = convert(
        {"llmstxt": {"enabled": False, "preprocess": "missing.py"}},
        root_dir=str(tmp_path),
    )

    assert settings["preprocess"] == str(tmp_path / "missing.py")


def test_missing_preprocess_script_reports_configuration_error(
    tmp_path: Path,
) -> None:
    with pytest.raises(
        ConfigurationError, match="llmstxt preprocess script not found"
    ):
        convert(
            {"llmstxt": {"preprocess": "missing.py"}}, root_dir=str(tmp_path)
        )


@pytest.mark.parametrize(
    ("settings", "message"),
    [
        ({"enabled": "yes"}, "enabled must be a boolean"),
        ({"autoclean": 1}, "autoclean must be a boolean"),
        ({"base_url": 42}, "base_url must be a string"),
        ({"markdown_description": []}, "markdown_description must be a string"),
        ({"full_output": True}, "full_output must be a string"),
        ({"full_output": "../full.txt"}, "relative file path"),
        ({"full_output": "/full.txt"}, "relative file path"),
        ({"full_output": "full/../file.txt"}, "relative file path"),
        ({"full_output": "llms.txt"}, "must differ from llms.txt"),
        (
            {"preprocess": ""},
            "preprocess must be a non-empty script path or null",
        ),
        (
            {"preprocess": False},
            "preprocess must be a non-empty script path or null",
        ),
        (
            {"preprocess": 42},
            "preprocess must be a non-empty script path or null",
        ),
        (
            {"preprocess": ["custom.py"]},
            "preprocess must be a non-empty script path or null",
        ),
        (
            {"preprocess": {"function": "preprocess"}},
            "preprocess must be a non-empty script path or null",
        ),
        ({"sections": []}, "sections must be a mapping"),
        ({"sections": {"Pages": "index.md"}}, "map strings to lists"),
        ({"sections": {"Pages": [False]}}, "single-entry mappings"),
        (
            {"sections": {"Pages": [{"a.md": "A", "b.md": "B"}]}},
            "single-entry mappings",
        ),
        (
            {"sections": {"Pages": [{"a.md": 1}]}},
            "descriptions must be strings",
        ),
        ({"sections": {"Pages": [""]}}, "non-empty strings"),
        ({"unknown": True}, "unknown llmstxt option"),
    ],
)
def test_invalid_settings_report_configuration_errors(
    settings: dict[str, Any], message: str
) -> None:
    with pytest.raises(ConfigurationError, match=message):
        convert({"llmstxt": settings})


def test_enabled_plugin_requires_site_url() -> None:
    with pytest.raises(ConfigurationError, match="requires site_url"):
        convert(["llmstxt"], site_url=None)
