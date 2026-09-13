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
    "table-reader",
)

SHIM_PLUGINS = (
    "autorefs",
    "markdown-exec",
    "mkdocstrings",
    "glightbox",
    "macros",
    "table-reader",
)


def _convert_plugins(value: Any) -> dict[str, dict[str, Any]]:
    config = {"extra": {"polyfills": []}, "root_dir": "."}
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


def test_preserves_zensical_plugin_options() -> None:
    plugins = _convert_plugins(
        {
            "minify": {
                "enabled": False,
                "minify_inline_js": True,
                "minify_inline_css": True,
            },
            "glightbox": {"auto": False, "slide_effect": "fade"},
            "macros": {"include_yaml": {"data": "data.yml"}},
        }
    )

    minify = plugins["minify"]["config"]
    assert minify["enabled"] is False
    assert minify["minify_inline_js"] is True
    assert minify["minify_inline_css"] is True
    assert plugins["glightbox"]["config"] == {"auto": False}
    assert plugins["macros"]["config"]["include_yaml"] == {"data": "data.yml"}


def test_normalizes_mike_defaults() -> None:
    plugins = _convert_plugins({"mike": {}})
    assert plugins["mike"]["config"] == {
        "alias_type": "symlink",
        "redirect_template": None,
        "deploy_prefix": "",
        "canonical_version": None,
    }


@pytest.mark.parametrize("version_selector", [False, True])
def test_preserves_mike_version_selector(version_selector: bool) -> None:
    plugins = _convert_plugins({"mike": {"version_selector": version_selector}})

    assert plugins["mike"]["config"]["version_selector"] is version_selector


def test_silently_discards_unsupported_mike_options(
    capsys: pytest.CaptureFixture[str],
) -> None:
    plugins = _convert_plugins(
        {
            "mike": {
                "version_selector": False,
                "css_dir": "assets/css",
                "javascript_dir": "assets/js",
            }
        }
    )

    assert plugins["mike"]["config"] == {
        "alias_type": "symlink",
        "redirect_template": None,
        "deploy_prefix": "",
        "canonical_version": None,
        "version_selector": False,
    }
    assert capsys.readouterr().err == ""


@pytest.mark.parametrize("name", [*PYTHON_PLUGINS, "tags"])
def test_plugin_configuration_must_be_a_mapping(name: str) -> None:
    with pytest.raises(
        ConfigurationError,
        match=rf"{name} configuration must be a mapping",
    ):
        _convert_plugins({name: []})


@pytest.mark.parametrize(
    "name",
    [
        "external",  # does not exist
        "material/blog",  # exists but isn't supported yet
        "literate_nav",  # misspelling (`_` instead of `-`)
    ]
)
@pytest.mark.parametrize("data", [None, True, 42, "config", [], {42: object()}])
@pytest.mark.parametrize("as_list", [False, True])
def test_ignores_unsupported_plugins(
    name: str, data: Any, as_list: bool, capsys: pytest.CaptureFixture[str]
) -> None:
    value = {name: data}
    plugins = _convert_plugins([value] if as_list else value)

    assert plugins == _convert_plugins([])
    assert capsys.readouterr().err == ""


@pytest.mark.parametrize("name", ["external", "material/blog", "literate_nav"])
def test_ignores_unsupported_plugin_names(name: str) -> None:
    assert _convert_plugins([name]) == _convert_plugins([])


@pytest.mark.parametrize("prefix", ["", "material/"])
@pytest.mark.parametrize("value", [True, False, "auto", 42, [], {}, None])
@pytest.mark.parametrize(
    ("plugin", "option"),
    [
        ("autorefs", "resolve_closest"),
        ("autorefs", "link_titles"),
        ("autorefs", "strip_title_tags"),
        ("glightbox", "touchNavigation"),
        ("glightbox", "loop"),
        ("glightbox", "effect"),
        ("glightbox", "slide_effect"),
        ("glightbox", "zoomable"),
        ("glightbox", "draggable"),
        ("glightbox", "background"),
        ("glightbox", "shadow"),
        ("macros", "force_render_paths"),
        ("macros", "verbose"),
        ("mike", "css_dir"),
        ("mike", "javascript_dir"),
        ("mkdocstrings", "enable_inventory"),
        ("mkdocstrings", "watch"),
        ("search", "fields"),
        ("search", "indexing"),
        ("search", "jieba_dict"),
        ("search", "jieba_dict_user"),
        ("search", "lang"),
        ("search", "min_search_length"),
        ("search", "pipeline"),
        ("search", "prebuild_index"),
        ("table-reader", "base_path"),
        ("table-reader", "search_page_directory"),
        ("tags", "tags_compare"),
        ("tags", "tags_compare_reverse"),
        ("tags", "tags_pages_compare"),
        ("tags", "tags_pages_compare_reverse"),
        ("tags", "tags_file"),
        ("tags", "tags_extra_files"),
        ("tags", "export"),
        ("tags", "export_file"),
        ("tags", "export_only"),
    ],
)
def test_silently_discards_unimplemented_options(
    plugin: str,
    option: str,
    value: Any,
    prefix: str,
    capsys: pytest.CaptureFixture[str],
) -> None:
    data = {"enabled": False, option: value}
    plugins = _convert_plugins({prefix + plugin: data})

    assert plugins == _convert_plugins({plugin: {"enabled": False}})
    assert data == {"enabled": False, option: value}
    assert capsys.readouterr().err == ""


@pytest.mark.parametrize("name", [*PYTHON_PLUGINS, "tags"])
def test_rejects_unknown_python_plugin_options(name: str) -> None:
    with pytest.raises(
        ConfigurationError,
        match=rf"unknown {name} option: unknown",
    ):
        _convert_plugins({name: {"unknown": True}})


@pytest.mark.parametrize("plugin", ["search", "material/search"])
def test_silently_discards_unsupported_search_options(
    plugin: str, capsys: pytest.CaptureFixture[str]
) -> None:
    unsupported = {
        "fields": {"title": {"boost": 2}},
        "indexing": "titles",
        "jieba_dict": "dict.txt",
        "jieba_dict_user": "user-dict.txt",
        "lang": ["en", "de"],
        "min_search_length": 2,
        "pipeline": ["stemmer"],
        "prebuild_index": True,
    }
    configured = {
        "enabled": False,
        "separator": "[\\s-]+",
        **unsupported,
    }

    plugins = _convert_plugins({plugin: configured})

    assert plugins["search"]["config"] == {
        "enabled": False,
        "separator": "[\\s-]+",
    }
    assert capsys.readouterr().err == ""


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
        pytest.param(
            "table-reader",
            {
                "enabled": True,
                "data_path": "tables",
                "allow_missing_files": True,
                "select_readers": ["read_csv", "read_raw"],
            },
            id="table-reader",
        ),
    ],
)
def test_accepts_supported_shim_options(
    name: str, config: dict[str, Any]
) -> None:
    plugins = _convert_plugins({name: config})
    assert plugins[name]["config"] == config


@pytest.mark.parametrize(
    "option", ["resolve_closest", "link_titles", "strip_title_tags"]
)
@pytest.mark.parametrize(
    "value", [True, False, "auto", "external", 42, [], {}, None]
)
def test_silently_discards_unsupported_autorefs_options(
    option: str, value: Any, capsys: pytest.CaptureFixture[str]
) -> None:
    plugins = _convert_plugins({"autorefs": {"enabled": True, option: value}})
    assert plugins["autorefs"]["config"] == {"enabled": True}
    assert capsys.readouterr().err == ""


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
        (
            "mike",
            {"version_selector": "false"},
            "version_selector must be a boolean",
        ),
        ("autorefs", {"enabled": "yes"}, "enabled must be a boolean"),
        ("markdown-exec", {"ansi": "sometimes"}, "ansi must be"),
        (
            "markdown-exec",
            {"languages": ["ruby"]},
            "languages must be a list of supported language names",
        ),
        ("mkdocstrings", {"handlers": []}, "handlers must be a mapping"),
        (
            "glightbox",
            {"caption_position": "center"},
            "caption_position must be",
        ),
        ("macros", {"include_yaml": [42]}, "include_yaml must be a list"),
        ("macros", {"on_undefined": "silent"}, "on_undefined must be"),
        (
            "table-reader",
            {"enabled": "yes"},
            "enabled must be a boolean",
        ),
        (
            "table-reader",
            {"data_path": 42},
            "data_path must be a string",
        ),
        (
            "table-reader",
            {"allow_missing_files": "yes"},
            "allow_missing_files must be a boolean",
        ),
        (
            "table-reader",
            {"select_readers": "read_csv"},
            "select_readers must be a list",
        ),
        (
            "table-reader",
            {"select_readers": [42]},
            "select_readers must be a list",
        ),
        (
            "table-reader",
            {"select_readers": ["read_unknown"]},
            "unknown table-reader reader",
        ),
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
