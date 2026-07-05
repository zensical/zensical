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

import copy
from typing import TYPE_CHECKING, Any

import pytest
from bs4 import BeautifulSoup
from markdown import Markdown

from zensical.config import DEFAULT_MARKDOWN_EXTENSIONS, _apply_defaults
from zensical.extensions.context import ContextExtension, Page

if TYPE_CHECKING:
    from pathlib import Path


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def soup(html: str) -> BeautifulSoup:
    """Parse an HTML fragment into a tree for semantic assertions."""
    return BeautifulSoup(html, "lxml")


def _expand_keys(data: dict[str, Any]) -> dict[str, Any]:
    """Expand slash-separated keys in `data` into nested dicts, recursively.

    `{"a/b/c": 1, "a/b/d": 2}` becomes `{"a": {"b": {"c": 1, "d": 2}}}`.
    Keys with empty path segments (e.g. `"//"`) are left as-is.
    Expansion is applied recursively to dict values.
    """
    result: dict[str, Any] = {}
    for key, value in data.items():
        parts = key.split("/")
        if not all(parts):  # empty segment – treat whole key as literal
            result[key] = value
            continue
        if isinstance(value, dict):
            value = _expand_keys(value)  # noqa: PLW2901
        node = result
        for part in parts[:-1]:
            node = node.setdefault(part, {})
        node[parts[-1]] = value
    return result


def _deep_merge(base: Any, override: Any) -> Any:
    """Recursively merge `override` into `base`.

    Dicts are merged key-by-key; all other types are replaced by the override
    value. When `override` is `None`, `base` is returned unchanged.
    """
    if override is None:
        return base
    if isinstance(base, dict) and isinstance(override, dict):
        result = dict(base)
        for key, value in override.items():
            result[key] = _deep_merge(result.get(key), value)
        return result
    return override


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture(name="base_configs", scope="session")
def _fixture_base_configs(
    tmp_path_factory: pytest.TempPathFactory,
) -> dict[str, Any]:
    """Pre-compute both base configs through `_apply_defaults` once per session.

    Individual fixtures deep-copy from here, so every test gets a fresh,
    fully-processed config without re-running the expensive defaults pipeline.
    """
    root = tmp_path_factory.mktemp("conftest_base")
    (root / "docs").mkdir()
    path = str(root / "zensical.toml")

    minimal = _apply_defaults(
        {"site_name": "Demo", "markdown_extensions": {}},
        path,
    )
    recommended = _apply_defaults(
        {
            "site_name": "Demo",
            "markdown_extensions": DEFAULT_MARKDOWN_EXTENSIONS,
        },
        path,
    )
    return {"minimal": minimal, "recommended": recommended}


@pytest.fixture(
    name="config",
    params=[
        pytest.param("minimal", id="minimal_markdown"),
        pytest.param("recommended", id="recommended_markdown"),
    ],
)
def _fixture_config(
    request: pytest.FixtureRequest,
    base_configs: dict[str, Any],
) -> dict[str, Any]:
    """Return a fresh deep copy of the pre-processed config for this variant."""
    return copy.deepcopy(base_configs[request.param])


@pytest.fixture(name="md")
def _fixture_md(
    config: dict[str, Any],
    request: pytest.FixtureRequest,
    tmp_path: Path,
) -> Markdown:
    """Return a Markdown instance configured from the active project config."""
    # Always root the config at this test's tmp directory.
    config["root_dir"] = str(tmp_path)

    # Check test parametrization.
    fixture_param = getattr(request, "param", {})
    allowed_keys = {"page", "config"}
    unexpected_keys = set(fixture_param) - allowed_keys
    if unexpected_keys:
        raise ValueError(
            f"Unsupported md fixture params: {sorted(unexpected_keys)}. "
            "Use only 'page' and 'config'."
        )

    # Apply test-specific config overrides.
    if "config" in fixture_param:
        expanded = _expand_keys(fixture_param["config"])

        # Markdown extension overrides are handled separately because the
        # processed config stores them as a list + mdx_configs dict, not as
        # the nested dict that test params supply.
        md_ext_overrides: dict[str, Any] = expanded.pop(
            "markdown_extensions", {}
        )

        # Deep-merge all remaining overrides into the processed config.
        for key, value in expanded.items():
            config[key] = _deep_merge(config.get(key), value)

        # Add/override individual extensions.
        for ext_name, ext_cfg in md_ext_overrides.items():
            if ext_name not in config["markdown_extensions"]:
                config["markdown_extensions"].append(ext_name)
            config["mdx_configs"][ext_name] = _deep_merge(
                config["mdx_configs"].get(ext_name, {}),
                ext_cfg or {},
            )

    # Instantiate Markdown parser from the processed extension list and configs.
    md = Markdown(
        extensions=list(config["markdown_extensions"]),
        extension_configs=dict(config["mdx_configs"]),
    )

    # Register the rendering context extension.
    base_page: dict[str, Any] = {"url": "/", "path": "index.md"}
    page_overrides = fixture_param.get("page", {})
    page = Page(**{**base_page, **page_overrides})
    context_extension = ContextExtension(page=page, config=config)
    context_extension.extendMarkdown(md)

    return md
