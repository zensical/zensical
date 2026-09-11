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

import json
import subprocess
import sys
import time
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

import pytest
import yaml

import zensical


def project(
    root: Path,
    plugin: str,
    options: dict,
    nav: list | None = None,
    extra_plugins: list | None = None,
) -> Path:
    docs = root / "docs"
    docs.mkdir(exist_ok=True)
    (docs / "index.md").write_text("# Home\n")
    package = root / "src" / "sample"
    package.mkdir(parents=True, exist_ok=True)
    for name, content in {
        "__init__.py": '"""Sample package."""\n',
        "public.py": '"""Public module documentation."""\n',
        "_private.py": '"""Private module documentation."""\n',
        "index.py": '"""Index module documentation."""\n',
    }.items():
        (package / name).write_text(content)
    config = {
        "site_name": "API test",
        "theme": {"features": ["content.action.edit"]},
        "repo_url": "https://example.com/repository",
        "edit_uri": "edit/main/docs",
        "plugins": [
            {plugin: options},
            {"mkdocstrings": {"handlers": {"python": {"paths": ["src"]}}}},
            *(extra_plugins or []),
        ],
    }
    if nav is not None:
        config["nav"] = nav
    path = root / "mkdocs.yml"
    path.write_text(yaml.safe_dump(config, sort_keys=False))
    return path


def build(path: Path, *, strict: bool = True) -> None:
    zensical.build(str(path), {"clean": False, "strict": strict})


@pytest.mark.parametrize(
    ("plugin", "options", "prefix"),
    [
        ("mkdocs-autoapi", {"autoapi_dir": "src"}, "autoapi"),
        ("api-autonav", {"modules": ["src/sample"]}, "reference"),
    ],
)
def test_generates_api_pages_and_navigation(
    tmp_path: Path, plugin: str, options: dict, prefix: str
) -> None:
    config = project(tmp_path, plugin, options)
    if plugin == "mkdocs-autoapi":
        # Upstream AutoAPI uses index.md for both index.py and __init__.py.
        (tmp_path / "src/sample/index.py").unlink()
    build(config)
    home = (tmp_path / "site/index.html").read_text()
    page = (tmp_path / f"site/{prefix}/sample/public/index.html").read_text()
    assert "Public module documentation" in page
    assert "API Reference" in home
    assert f"{prefix}/sample/public/" in home
    assert not (tmp_path / f"docs/{prefix}").exists()
    assert (
        tmp_path / f"site/{prefix}/sample/_private/index.html"
    ).exists() == (plugin == "mkdocs-autoapi")
    search = json.loads((tmp_path / "site/search.json").read_text())
    assert any(
        "Public module documentation" in doc["text"] for doc in search["items"]
    )


def test_autoapi_patterns_stubs_keep_and_manual_navigation(
    tmp_path: Path,
) -> None:
    config = project(
        tmp_path,
        "mkdocs-autoapi",
        {
            "autoapi_dir": "src/sample",
            "autoapi_file_patterns": ["*.pyi", "*.py"],
            "autoapi_ignore": ["index.py", "_private.py"],
            "autoapi_root": "api",
            "autoapi_add_nav_entry": False,
            "autoapi_keep_files": True,
        },
        nav=[{"Home": "index.md"}, {"Manual API": "api/"}],
    )
    (tmp_path / "src/sample/public.pyi").write_text('"""Stub docs."""\n')
    build(config)
    assert (
        tmp_path / "docs/api/sample/public.md"
    ).read_text() == "::: sample.public\n"
    summary = (tmp_path / "docs/api/summary.md").read_text()
    assert "[public](sample/public.md)" in summary
    assert "_private" not in summary
    assert not (tmp_path / "site/api/summary/index.html").exists()
    home = (tmp_path / "site/index.html").read_text()
    assert "Manual API" in home
    assert "API Reference" not in home


@pytest.mark.parametrize(
    "nav",
    [
        ["API Reference", {"Home": "index.md"}],
        [{"API Reference": "api/"}, {"Home": "index.md"}],
        [{"API Reference": [{"Intro": "intro.md"}]}, {"Home": "index.md"}],
        [{"Nested": [{"API Reference": "api"}]}, {"Home": "index.md"}],
    ],
)
def test_autonav_navigation_and_module_options(
    tmp_path: Path, nav: list
) -> None:
    config = project(
        tmp_path,
        "api-autonav",
        {
            "modules": ["src/sample"],
            "api_root_uri": "api",
            "nav_item_prefix": "MOD ",
            "show_full_namespace": True,
            "exclude_private": False,
            "exclude": ["sample.index", r"re:sample\._private$"],
            "module_options": {
                ".*": {"heading_level": 1},
                r"sample\.public$": {
                    "heading_level": 2,
                    "show_root_heading": True,
                },
            },
        },
        nav=nav,
    )
    (tmp_path / "docs/intro.md").write_text("# Introduction\n")
    build(config)
    page = (tmp_path / "site/api/sample/public/index.html").read_text()
    assert "MOD sample.public" in page
    assert '<h1 id="samplepublic"' in page
    assert not (tmp_path / "site/api/sample/index_py").exists()
    assert not (tmp_path / "site/api/sample/_private").exists()


@pytest.mark.parametrize("policy", ["raise", "warn", "skip"])
def test_namespace_policy(tmp_path: Path, policy: str) -> None:
    config = project(
        tmp_path,
        "api-autonav",
        {
            "modules": ["src/sample"],
            "on_implicit_namespace_package": policy,
        },
    )
    namespace = tmp_path / "src/sample/implicit"
    namespace.mkdir()
    (namespace / "child.py").write_text('"""Child."""\n')
    if policy == "raise":
        with pytest.raises(RuntimeError, match="implicit namespace package"):
            build(config)
    else:
        build(config, strict=False)
        assert not (tmp_path / "site/reference/sample/implicit").exists()
        assert (tmp_path / "site/reference/sample/public/index.html").exists()


def test_autonav_with_awesome_nav(tmp_path: Path) -> None:
    config = project(
        tmp_path,
        "api-autonav",
        {"modules": ["src/sample"]},
        extra_plugins=["awesome-nav"],
    )
    build(config)
    home = (tmp_path / "site/index.html").read_text()
    assert "API Reference" in home
    assert "reference/sample/public/" in home


def test_autonav_index_module_and_source_rebuild(tmp_path: Path) -> None:
    config = project(tmp_path, "api-autonav", {"modules": ["src/sample"]})
    build(config)
    assert (tmp_path / "site/reference/sample/index_py/index.html").exists()
    (tmp_path / "src/sample/public.py").write_text(
        '"""Updated module documentation."""\n'
    )
    build(config)
    page = (tmp_path / "site/reference/sample/public/index.html").read_text()
    assert "Updated module documentation" in page
    (tmp_path / "src/sample/public.py").unlink()
    build(config)
    assert not (tmp_path / "site/reference/sample/public/index.html").exists()


@pytest.mark.parametrize("plugin", ["mkdocs-autoapi", "api-autonav"])
def test_serve_discovers_added_renamed_and_removed_modules(
    tmp_path: Path, plugin: str
) -> None:
    options = (
        {"autoapi_dir": "src", "autoapi_ignore": ["**/index.py"]}
        if plugin == "mkdocs-autoapi"
        else {"modules": ["src/sample"]}
    )
    config = project(tmp_path, plugin, options)
    with config.open("a") as file:
        file.write("dev_addr: 127.0.0.1:0\n")
    prefix = "autoapi" if plugin == "mkdocs-autoapi" else "reference"
    output = tmp_path / f"site/{prefix}/sample/public/index.html"
    added = tmp_path / "src/sample/added.py"
    added_output = tmp_path / f"site/{prefix}/sample/added/index.html"
    renamed = tmp_path / "src/sample/renamed.py"
    renamed_output = tmp_path / f"site/{prefix}/sample/renamed/index.html"
    with (tmp_path / "serve.log").open("w+") as log:
        process = subprocess.Popen(  # noqa: S603
            [
                sys.executable,
                "-m",
                "zensical",
                "serve",
                "--config-file",
                str(config),
            ],
            cwd=tmp_path,
            stdout=log,
            stderr=subprocess.STDOUT,
        )

        def wait_for(condition: Callable[[], bool]) -> None:
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if condition():
                    return
                if process.poll() is not None:
                    break
                time.sleep(0.02)
            log.seek(0)
            raise AssertionError(log.read())

        try:
            wait_for(output.is_file)
            added.write_text('"""New module."""\n')
            wait_for(added_output.is_file)
            added.rename(renamed)
            wait_for(
                lambda: renamed_output.is_file() and not added_output.exists()
            )
            renamed.write_text('"""Changed module."""\n')
            wait_for(
                lambda: (
                    renamed_output.exists()
                    and "Changed module" in renamed_output.read_text()
                )
            )
            renamed.unlink()
            wait_for(lambda: output.is_file() and not renamed_output.exists())
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


def test_generated_pages_work_with_redirects_and_edit_links(
    tmp_path: Path,
) -> None:
    config = project(
        tmp_path,
        "mkdocs-autoapi",
        {
            "autoapi_dir": "src",
            "autoapi_ignore": ["**/index.py"],
        },
        extra_plugins=[
            {
                "redirects": {
                    "redirect_maps": {
                        "old-api.md": "autoapi/sample/public.md",
                        "old-home.md": "index.md",
                    }
                }
            }
        ],
    )
    build(config)
    page = (tmp_path / "site/autoapi/sample/public/index.html").read_text()
    assert (
        "https://example.com/repository/edit/main/src/sample/public.py" in page
    )
    assert (tmp_path / "site/old-api/index.html").exists()
    assert (tmp_path / "site/old-home/index.html").exists()


def test_autoapi_can_link_retained_docs_without_generating(
    tmp_path: Path,
) -> None:
    config = project(
        tmp_path,
        "mkdocs-autoapi",
        {
            "autoapi_generate_api_docs": False,
            "autoapi_add_nav_entry": "Saved API",
        },
    )
    directory = tmp_path / "docs/autoapi"
    directory.mkdir()
    (directory / "topic.md").write_text("# Saved topic\n")
    (directory / "summary.md").write_text("- [Saved topic](topic.md)\n")
    build(config)
    home = (tmp_path / "site/index.html").read_text()
    assert "Saved API" in home
    assert "autoapi/topic/" in home
    assert not (tmp_path / "site/autoapi/sample").exists()


def test_autonav_single_file_private_modules_and_flat_urls(
    tmp_path: Path,
) -> None:
    config = project(
        tmp_path,
        "api-autonav",
        {
            "modules": ["src/sample"],
            "exclude_private": False,
            "nav_item_prefix": "",
        },
    )
    with config.open("a") as file:
        file.write("use_directory_urls: false\n")
    build(config)
    assert (tmp_path / "site/reference/sample/_private.html").exists()
    page = (tmp_path / "site/reference/sample/public.html").read_text()
    assert "repository/edit/main/docs/reference" not in page
    data = yaml.safe_load(config.read_text())
    data["plugins"][0]["api-autonav"]["modules"] = ["src/standalone.py"]
    (tmp_path / "src/standalone.py").write_text('"""Standalone module."""\n')
    config.write_text(yaml.safe_dump(data))
    build(config)
    assert (tmp_path / "site/reference/standalone.html").exists()


def test_autoapi_uses_vba_identifiers_for_custom_file_patterns(
    tmp_path: Path,
) -> None:
    config = project(
        tmp_path,
        "mkdocs-autoapi",
        {
            "autoapi_dir": "src",
            "autoapi_file_patterns": ["*.bas"],
            "autoapi_keep_files": True,
        },
    )
    (tmp_path / "src/sample/code.bas").write_text("Sub Example()\nEnd Sub\n")
    data = yaml.safe_load(config.read_text())
    data["plugins"][1]["mkdocstrings"].update(
        enabled=False, default_handler="vba"
    )
    config.write_text(yaml.safe_dump(data))
    build(config)
    assert (
        tmp_path / "docs/autoapi/sample/code.md"
    ).read_text() == "::: sample/code.bas\n"


def test_autonav_preserves_root_order_and_uses_last_matching_options(
    tmp_path: Path,
) -> None:
    config = project(
        tmp_path,
        "api-autonav",
        {
            "modules": ["src/zeta", "src/alpha"],
            "nav_item_prefix": "",
            "module_options": {
                "zeta": {"heading_level": 3},
                ".*": {"heading_level": 2, "show_root_heading": True},
            },
        },
    )
    for name in ("zeta", "alpha"):
        directory = tmp_path / "src" / name
        directory.mkdir()
        (directory / "__init__.py").write_text(f'"""{name} docs."""\n')
    build(config)
    home = (tmp_path / "site/index.html").read_text()
    assert home.index('reference/zeta/" class="md-nav__link"') < home.index(
        'reference/alpha/" class="md-nav__link"'
    )
    page = (tmp_path / "site/reference/zeta/index.html").read_text()
    assert '<h2 id="zeta"' in page


def test_generators_replace_physical_collisions_and_inherit_metadata(
    tmp_path: Path,
) -> None:
    config = project(
        tmp_path,
        "api-autonav",
        {"modules": ["src/sample"]},
        extra_plugins=["meta"],
    )
    directory = tmp_path / "docs/reference/sample"
    directory.mkdir(parents=True)
    (directory / "public.md").write_text("# Physical collision\n")
    (directory / ".meta.yml").write_text(
        "description: Inherited API description\n"
    )
    build(config)
    page = (tmp_path / "site/reference/sample/public/index.html").read_text()
    assert "Public module documentation" in page
    assert "Physical collision" not in page
    assert "Inherited API description" in page


def test_disabled_api_generators_leave_docs_unchanged(tmp_path: Path) -> None:
    config = project(tmp_path, "mkdocs-autoapi", {"enabled": False})
    build(config)
    assert not (tmp_path / "site/autoapi").exists()
