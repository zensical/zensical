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

import zensical.config as config_module
from zensical.config import ConfigurationError

PYTHON_PLUGINS = (
    "search",
    "meta",
    "redirects",
    "minify",
    "literate-nav",
    "awesome-nav",
    "offline",
    "mike",
    "autorefs",
    "markdown-exec",
    "mkdocstrings",
    "glightbox",
    "macros",
)

SHIM_PLUGINS = (
    "autorefs",
    "markdown-exec",
    "mkdocstrings",
    "glightbox",
    "macros",
)


def _convert_plugins(value: Any) -> dict[str, dict[str, Any]]:
    config = {"extra": {"polyfills": []}}
    return config_module._convert_plugins(value, config)


def test_normalizes_defaults_and_complex_options() -> None:
    plugins = _convert_plugins(
        {
            "minify": {
                "js_files": "assets/app.js",
                "htmlmin_opts": {"remove_comments": True},
            },
            "literate-nav": {
                "markdown_extensions": [
                    "abbr",
                    {"toc": {"permalink": False}},
                ]
            },
            "awesome-nav": {"logs": {"no_matches": "error"}},
        }
    )

    minify = plugins["minify"]["config"]
    assert minify["enabled"] is True
    assert minify["js_files"] == ["assets/app.js"]
    assert minify["htmlmin_opts"]["remove_comments"] is True
    assert minify["htmlmin_opts"]["pre_tags"] == ["pre", "textarea"]

    literate_nav = plugins["literate_nav"]["config"]
    assert literate_nav["markdown_extensions"] == ["abbr", "toc"]
    assert literate_nav["mdx_configs"] == {
        "abbr": {},
        "toc": {"permalink": False},
    }

    awesome_nav = plugins["awesome_nav"]["config"]
    assert awesome_nav["logs"] == {
        "nav_override": None,
        "root_title": None,
        "root_hide": None,
        "no_matches": "error",
    }


def test_preserves_plugin_presence_semantics() -> None:
    plugins = _convert_plugins([])
    assert plugins["search"]["config"]["enabled"] is True
    for name in (
        "meta",
        "redirects",
        "minify",
        "literate_nav",
        "awesome_nav",
        "offline",
    ):
        assert plugins[name]["config"]["enabled"] is False
    assert plugins["tags"]["config"] == []
    assert "mike" not in plugins
    assert not set(SHIM_PLUGINS) & set(plugins)


def test_normalizes_mike_defaults() -> None:
    plugins = _convert_plugins({"mike": {}})
    assert plugins["mike"]["config"] == {
        "alias_type": "symlink",
        "redirect_template": None,
        "deploy_prefix": "",
        "canonical_version": None,
    }


@pytest.mark.parametrize("name", [*PYTHON_PLUGINS, "tags", "external"])
def test_plugin_configuration_must_be_a_mapping(name: str) -> None:
    with pytest.raises(
        ConfigurationError,
        match=rf"{name} configuration must be a mapping",
    ):
        _convert_plugins({name: []})


@pytest.mark.parametrize("name", PYTHON_PLUGINS)
def test_rejects_unknown_python_plugin_options(name: str) -> None:
    with pytest.raises(
        ConfigurationError,
        match=rf"unknown {name} option: unknown",
    ):
        _convert_plugins({name: {"unknown": True}})


@pytest.mark.parametrize("name", SHIM_PLUGINS)
def test_normalizes_null_shim_configuration(name: str) -> None:
    plugins = _convert_plugins({name: None})
    assert plugins[name]["config"] == {}


@pytest.mark.parametrize(
    ("name", "config"),
    [
        pytest.param("autorefs", {"enabled": False}, id="autorefs"),
        pytest.param(
            "markdown-exec",
            {"enabled": False, "ansi": "off", "languages": ["python"]},
            id="markdown-exec",
        ),
        pytest.param(
            "mkdocstrings",
            {
                "enabled": False,
                "handlers": {"python": {"options": {}}},
                "custom_templates": None,
                "enable_inventory": None,
                "default_handler": "python",
                "locale": "fr",
            },
            id="mkdocstrings",
        ),
        pytest.param(
            "glightbox",
            {
                "enabled": True,
                "width": "80%",
                "height": "auto",
                "skip_classes": ["no-lightbox"],
                "auto": False,
                "auto_themed": True,
                "auto_caption": True,
                "caption_position": "top",
                "touchNavigation": False,
                "loop": True,
                "effect": "fade",
                "zoomable": False,
                "draggable": False,
                "background": "black",
                "shadow": False,
                "manual": None,
            },
            id="glightbox",
        ),
        pytest.param(
            "macros",
            {
                "enabled": True,
                "module_name": "hooks",
                "modules": ["plugin.macros"],
                "include_yaml": {"data": "data.yml"},
                "include_dir": "includes",
                "render_by_default": False,
                "on_error_fail": True,
                "on_undefined": "strict",
                "verbose": True,
                "j2_block_start_string": "<%",
                "j2_block_end_string": "%>",
                "j2_variable_start_string": "<@",
                "j2_variable_end_string": "@>",
                "j2_comment_start_string": "<#",
                "j2_comment_end_string": "#>",
                "j2_extensions": ["jinja2.ext.do"],
            },
            id="macros",
        ),
    ],
)
def test_accepts_supported_shim_options(
    name: str, config: dict[str, Any]
) -> None:
    plugins = _convert_plugins({name: config})
    assert plugins[name]["config"] == config


@pytest.mark.parametrize(
    ("name", "config", "message"),
    [
        ("search", {"enabled": "yes"}, "enabled must be a boolean"),
        ("search", {"separator": 42}, "separator must be a string"),
        ("meta", {"meta_file": 42}, "meta_file must be a string"),
        (
            "redirects",
            {"redirect_maps": {"old.md": 42}},
            "redirect_maps must be a mapping of strings",
        ),
        ("minify", {"js_files": 42}, "js_files must be a string or a list"),
        (
            "minify",
            {"js_files": [42]},
            "js_files must be a string or a list",
        ),
        (
            "minify",
            {"htmlmin_opts": {"unknown": True}},
            "unknown minify htmlmin_opts option",
        ),
        (
            "minify",
            {"htmlmin_opts": {"pre_tags": [42]}},
            "pre_tags must be a list of strings",
        ),
        (
            "literate-nav",
            {"tab_length": 0},
            "tab_length must be a positive integer",
        ),
        (
            "literate-nav",
            {"markdown_extensions": [42]},
            "Markdown extensions must be strings or mappings",
        ),
        (
            "awesome-nav",
            {"filename": ""},
            "filename must be a non-empty string",
        ),
        (
            "awesome-nav",
            {"logs": {"no_matches": "debug"}},
            "no_matches must be info, warning or error",
        ),
        ("offline", {"enabled": "yes"}, "enabled must be a boolean"),
        ("mike", {"canonical_version": 42}, "must be a string or null"),
        ("autorefs", {"enabled": "yes"}, "enabled must be a boolean"),
        ("markdown-exec", {"ansi": "sometimes"}, "ansi must be"),
        (
            "markdown-exec",
            {"languages": ["ruby"]},
            "languages must be a list of supported language names",
        ),
        ("mkdocstrings", {"handlers": []}, "handlers must be a mapping"),
        ("glightbox", {"effect": "slide"}, "effect must be"),
        ("macros", {"include_yaml": [42]}, "include_yaml must be a list"),
        ("macros", {"on_undefined": "silent"}, "on_undefined must be"),
    ],
)
def test_rejects_invalid_plugin_options(
    name: str, config: dict[str, Any], message: str
) -> None:
    with pytest.raises(ConfigurationError, match=message):
        _convert_plugins({name: config})


@pytest.mark.parametrize(
    "value",
    [True, "search", [42], [{}], [{"search": {}, "offline": {}}]],
)
def test_rejects_invalid_plugin_collections(value: Any) -> None:
    with pytest.raises(ConfigurationError):
        _convert_plugins(value)
