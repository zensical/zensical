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
import yaml

from zensical.config import (
    ConfigurationError,
    _apply_defaults,
    _convert_plugins,
)
from zensical.extensions.markdownextradata import MarkdownExtraDataExtension

if TYPE_CHECKING:
    from pathlib import Path


_EXTENSION = MarkdownExtraDataExtension.name


def _configure(tmp_path: Path, **settings: Any) -> dict[str, Any]:
    (tmp_path / "docs").mkdir(exist_ok=True)
    return _apply_defaults(
        {"site_name": "Test", "markdown_extensions": {}, **settings},
        str(tmp_path / "zensical.toml"),
    )


@pytest.mark.parametrize(
    "plugins",
    [
        ["markdownextradata"],
        {"markdownextradata": None},
        [{"markdownextradata": {}}],
        {"material/markdownextradata": {}},
    ],
)
def test_plugin_enables_extension(tmp_path: Path, plugins: Any) -> None:
    config = _configure(tmp_path, plugins=plugins)

    assert _EXTENSION in config["markdown_extensions"]
    assert config["mdx_configs"][_EXTENSION] == {}


def test_plugin_options_are_forwarded(tmp_path: Path) -> None:
    options = {
        "data": "custom",
        "jinja_options": {
            "comment_start_string": "<!--",
            "comment_end_string": "-->",
        },
    }

    config = _configure(tmp_path, plugins={"markdownextradata": options})

    assert config["mdx_configs"][_EXTENSION] == options
    assert config["plugins"]["markdownextradata"]["config"] == options


def test_direct_extension_options_take_precedence(tmp_path: Path) -> None:
    options = {"data": "direct"}

    config = _configure(
        tmp_path,
        plugins={"markdownextradata": {"data": "plugin"}},
        markdown_extensions={_EXTENSION: options},
    )

    assert config["markdown_extensions"].count(_EXTENSION) == 1
    assert config["mdx_configs"][_EXTENSION] == options


@pytest.mark.parametrize("direct", [False, True])
def test_disabled_support_does_not_load_or_watch_data(
    tmp_path: Path, direct: bool
) -> None:
    (tmp_path / "_data").mkdir()
    (tmp_path / "_data" / "invalid.yaml").write_text(
        "values: [", encoding="utf-8"
    )
    settings = (
        {
            "markdown_extensions": {_EXTENSION: {"enabled": False}},
            "plugins": {"markdownextradata": {}},
        }
        if direct
        else {"plugins": {"markdownextradata": {"enabled": False}}}
    )

    config = _configure(tmp_path, **settings)

    assert "invalid" not in config["extra"]
    assert config["watched_files"] == []
    if not direct:
        assert _EXTENSION not in config["markdown_extensions"]


@pytest.mark.parametrize("direct", [False, True])
def test_data_is_loaded_into_project_extra_and_watched(
    tmp_path: Path, direct: bool
) -> None:
    (tmp_path / "custom" / "nested").mkdir(parents=True)
    data_file = tmp_path / "custom" / "nested" / "value.json"
    data_file.write_text('{"name": "Alice"}', encoding="utf-8")
    (tmp_path / "custom" / "ignored.txt").write_text(
        "Ignored", encoding="utf-8"
    )
    options = {"data": "custom"}
    settings = (
        {"markdown_extensions": {_EXTENSION: options}}
        if direct
        else {"plugins": {"markdownextradata": options}}
    )

    config = _configure(tmp_path, **settings)

    assert config["extra"]["nested"]["value"] == {"name": "Alice"}
    assert config["watched_files"] == [
        (str(data_file), int(data_file.stat().st_mtime))
    ]


@pytest.mark.parametrize("direct", [False, True])
def test_external_data_is_neither_loaded_nor_watched(
    tmp_path: Path, direct: bool
) -> None:
    project = tmp_path / "project"
    data_directory = project / "_data"
    data_directory.mkdir(parents=True)
    local_file = data_directory / "local.yaml"
    local_file.write_text("name: Alice\n", encoding="utf-8")

    outside = tmp_path / "outside"
    outside.mkdir()
    external_file = outside / "external.yaml"
    external_file.write_text("value: [", encoding="utf-8")
    (data_directory / "external.yaml").symlink_to(external_file)
    options = {"data": "../outside, _data"}
    settings = (
        {"markdown_extensions": {_EXTENSION: options}}
        if direct
        else {"plugins": {"markdownextradata": options}}
    )

    config = _configure(project, **settings)

    assert config["extra"]["local"] == {"name": "Alice"}
    assert "external" not in config["extra"]
    assert config["watched_files"] == [
        (str(local_file), int(local_file.stat().st_mtime))
    ]


@pytest.mark.parametrize("direct", [False, True])
def test_python_yaml_tags_are_rejected_during_configuration(
    tmp_path: Path, direct: bool
) -> None:
    (tmp_path / "_data").mkdir()
    (tmp_path / "_data" / "value.yaml").write_text(
        "value: !!python/name:builtins.str ''\n", encoding="utf-8"
    )
    settings = (
        {"markdown_extensions": {_EXTENSION: {}}}
        if direct
        else {"plugins": {"markdownextradata": {}}}
    )

    with pytest.raises(yaml.constructor.ConstructorError):
        _configure(tmp_path, **settings)


def test_default_data_directories_follow_docs_dir(tmp_path: Path) -> None:
    (tmp_path / "content" / "_data").mkdir(parents=True)
    data_file = tmp_path / "content" / "_data" / "value.yaml"
    data_file.write_text("name: Alice\n", encoding="utf-8")

    config = _configure(
        tmp_path, docs_dir="content", plugins={"markdownextradata": {}}
    )

    assert config["extra"]["value"] == {"name": "Alice"}
    assert (str(data_file), int(data_file.stat().st_mtime)) in config[
        "watched_files"
    ]


@pytest.mark.parametrize("data", [None, "", "custom, another"])
def test_supported_data_settings(data: str | None) -> None:
    options = {"data": data, "jinja_options": {"trim_blocks": True}}

    config = _convert_plugins(
        {"markdownextradata": options}, {"extra": {}, "root_dir": "."}
    )

    assert config["markdownextradata"]["config"] == options


@pytest.mark.parametrize(
    ("options", "message"),
    [
        ({"data": []}, "data must be a string"),
        ({"data": 42}, "data must be a string"),
        ({"jinja_options": []}, "jinja_options must be a mapping"),
        ({"jinja_options": None}, "jinja_options must be a mapping"),
        ({"enabled": "yes"}, "enabled must be a boolean"),
        ({"unknown": True}, "unknown markdownextradata option: unknown"),
    ],
)
def test_invalid_plugin_options(options: dict[str, Any], message: str) -> None:
    with pytest.raises(ConfigurationError, match=message):
        _convert_plugins(
            {"markdownextradata": options}, {"extra": {}, "root_dir": "."}
        )
