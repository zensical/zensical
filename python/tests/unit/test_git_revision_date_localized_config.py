# Copyright (c) 2025-2026 Zensical and contributors
# SPDX-License-Identifier: MIT

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


def test_timeago_assets_are_added_only_when_enabled() -> None:
    project = {
        "extra": {"polyfills": []},
        "root_dir": ".",
        "extra_javascript": [],
        "extra_css": [],
    }

    _convert_plugins(
        {"git-revision-date-localized": {"type": "timeago"}}, project
    )

    assert (
        project["extra_javascript"][0]["path"]
        == "js/git-revision-date-localized.js"
    )
    assert project["extra_css"] == ["css/git-revision-date-localized.css"]

    project = {"extra": {"polyfills": []}, "root_dir": "."}
    _convert_plugins(
        {"git-revision-date-localized": {"enabled": False, "type": "timeago"}},
        project,
    )

    assert "extra_javascript" not in project
    assert "extra_css" not in project
