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

"""Validate native Git revision date configuration."""

from __future__ import annotations

from typing import Any

import pytest

from zensical.config import ConfigurationError, _convert_plugins


def convert(settings: dict[str, Any] | None = None) -> dict[str, Any]:
    """Normalize one plugin configuration without importing upstream."""
    project = {"extra": {"polyfills": []}, "root_dir": "."}
    plugins = (
        [] if settings is None else [{"git-revision-date-localized": settings}]
    )
    return _convert_plugins(plugins, project)["git_revision_date_localized"][
        "config"
    ]


def test_presence_and_upstream_defaults() -> None:
    assert convert()["enabled"] is False

    settings = convert({})

    assert settings["enabled"] is True
    assert settings["enable_git_follow"] is True
    assert settings["enable_parallel_processing"] is True
    assert settings["enable_creation_date"] is False
    assert settings["fallback_to_build_date"] is False
    assert settings["strict"] is True
    assert settings["timezone"] == "UTC"
    assert settings["locale"] is None
    assert settings["type"] == "date"


@pytest.mark.parametrize(
    ("option", "value"),
    [
        ("enable_git_follow", "yes"),
        ("strict", 1),
        ("fallback_to_build_date", "false"),
        ("locale", 42),
        ("ignored_commits_file", []),
        ("timezone", False),
        ("exclude", "index.md"),
        ("type", "unknown"),
        ("unknown", True),
    ],
)
def test_invalid_options_are_rejected(option: str, value: Any) -> None:
    with pytest.raises(ConfigurationError, match="git-revision-date-localized"):
        convert({option: value})


@pytest.mark.parametrize(
    "settings",
    [None, {}, {"type": "timeago"}, {"enabled": False, "type": "timeago"}],
)
def test_custom_assets_are_preserved(settings: dict[str, Any] | None) -> None:
    project = {
        "extra": {"polyfills": []},
        "root_dir": ".",
        "extra_javascript": [{"path": "app.js", "type": "module"}],
        "extra_css": ["custom.css"],
    }
    plugins = []
    if settings is not None:
        plugins = [{"git-revision-date-localized": settings}]

    _convert_plugins(plugins, project)

    assert project["extra_javascript"] == [{"path": "app.js", "type": "module"}]
    assert project["extra_css"] == ["custom.css"]


def test_timeago_does_not_add_custom_asset_configuration() -> None:
    project = {"extra": {"polyfills": []}, "root_dir": "."}

    _convert_plugins(
        {"git-revision-date-localized": {"type": "timeago"}},
        project,
    )

    assert "extra_javascript" not in project
    assert "extra_css" not in project
