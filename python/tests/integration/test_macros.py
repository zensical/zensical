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

"""Integration coverage for macros page selection and verbose output."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from textwrap import dedent
from typing import TYPE_CHECKING
from urllib.parse import parse_qs, urlsplit

import pytest
import yaml
from bs4 import BeautifulSoup

import zensical

if TYPE_CHECKING:
    from pathlib import Path


def _write_config(
    root: Path, options: dict[str, str | bool], *, toml: bool = False
) -> Path:
    (root / "content").mkdir(exist_ok=True)
    overrides = root / "overrides"
    overrides.mkdir(exist_ok=True)
    (overrides / "main.html").write_text("{{ page.content }}", encoding="utf-8")
    if toml:
        config = root / "zensical.toml"
        content = (
            '[project]\nsite_name = "Macros"\ndocs_dir = "content"\n'
            '[project.theme]\ncustom_dir = "overrides"\n'
            "[project.plugins.macros]\n"
        )
        content += "\n".join(
            f"{name} = {json.dumps(value)}" for name, value in options.items()
        )
    else:
        config = root / "mkdocs.yml"
        content = yaml.safe_dump(
            {
                "site_name": "Macros",
                "docs_dir": "content",
                "theme": {"custom_dir": "overrides"},
                "plugins": {"macros": options},
            }
        )
    config.write_text(content, encoding="utf-8")
    return config


@pytest.mark.parametrize("toml", [False, True], ids=["yaml", "toml"])
@pytest.mark.parametrize("target_location", ["docs", "project", "external"])
def test_default_include_directory_builds_symlinked_pages(
    tmp_path: Path, toml: bool, target_location: str
) -> None:
    project = tmp_path / "project"
    project.mkdir()
    config = _write_config(project, {"on_error_fail": True}, toml=toml)
    docs = project / "content"
    target_dir = {"docs": docs, "project": project, "external": tmp_path}
    target = target_dir[target_location] / "page.md"
    target.write_text("# Hello\n\nValue: {{ 1 + 1 }}\n", encoding="utf-8")

    # Two aliases are discovered alongside the target without ending the build.
    for name in ["index.md", "alias.md"]:
        try:
            (docs / name).symlink_to(target)
        except OSError as error:
            pytest.skip(f"symbolic links unavailable: {error}")

    zensical.build(str(config), {"clean": True, "strict": False})

    for output in ["index.html", "alias/index.html"]:
        html = (project / "site" / output).read_text(encoding="utf-8")
        assert "Value: 2" in html


@pytest.mark.parametrize("dependency", ["page", "include"])
def test_serve_rebuilds_after_symlinked_macro_dependency_changes(
    tmp_path: Path, dependency: str
) -> None:
    config = _write_config(tmp_path, {"on_error_fail": True})
    with config.open("a", encoding="utf-8") as stream:
        stream.write("dev_addr: 127.0.0.1:0\n")

    docs = tmp_path / "content"
    target = tmp_path / ("page.md" if dependency == "page" else "fragment.j2")
    target.write_text("First value: {{ 1 + 1 }}\n", encoding="utf-8")
    alias = docs / ("index.md" if dependency == "page" else "fragment.j2")
    try:
        alias.symlink_to(target)
    except OSError as error:
        pytest.skip(f"symbolic links unavailable: {error}")

    if dependency == "include":
        (docs / "index.md").write_text(
            '{% include "fragment.j2" %}\n', encoding="utf-8"
        )

    output = tmp_path / "site" / "index.html"
    with (tmp_path / "serve.log").open("w+", encoding="utf-8") as log:
        process = subprocess.Popen(  # noqa: S603
            [sys.executable, "-m", "zensical", "serve", "-f", str(config)],
            cwd=tmp_path,
            stdout=log,
            stderr=subprocess.STDOUT,
        )

        def wait_for(text: str) -> None:
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                try:
                    content = output.read_text(encoding="utf-8")
                except FileNotFoundError:
                    content = ""
                if text in content:
                    return
                if process.poll() is not None:
                    break
                time.sleep(0.02)

            log.seek(0)
            pytest.fail(f"serve did not rebuild: {log.read()}")

        try:
            wait_for("First value: 2")

            # Canonical targets are still watched after alias discovery.
            target.write_text("Updated value: {{ 1 + 1 }}\n", encoding="utf-8")
            timestamp = target.stat().st_mtime + 2
            os.utime(target, (timestamp, timestamp))

            wait_for("Updated value: 2")
            assert process.poll() is None
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


@pytest.mark.parametrize("toml", [False, True], ids=["yaml", "toml"])
def test_force_render_paths_uses_docs_paths_and_page_metadata(
    tmp_path: Path, toml: bool
) -> None:
    config = _write_config(
        tmp_path,
        {
            "render_by_default": False,
            "force_render_paths": (
                "/guides/\n!guides/drafts/\nguides/drafts/keep.md"
            ),
        },
        toml=toml,
    )
    pages = {
        "index.md": (None, False),
        "guides/index.md": (None, True),
        "guides/disabled.md": (False, False),
        "guides/drafts/page.md": (None, False),
        "guides/drafts/keep.md": (None, True),
        "outside.md": (True, True),
        "nested/guides/page.md": (None, False),
    }
    for path, (override, _) in pages.items():
        page = tmp_path / "content" / path
        page.parent.mkdir(parents=True, exist_ok=True)
        header = (
            ""
            if override is None
            else f"---\nrender_macros: {str(override).lower()}\n---\n"
        )
        page.write_text(header + "Value: {{ 1 + 1 }}\n", encoding="utf-8")

    zensical.build(str(config), {"clean": True, "strict": False})

    for path, (_, rendered) in pages.items():
        output = tmp_path / "site" / path.removesuffix(".md")
        output = (
            output.with_suffix(".html")
            if output.name == "index"
            else output / "index.html"
        )
        html = output.read_text(encoding="utf-8")
        assert ("Value: 2" if rendered else "Value: {{ 1 + 1 }}") in html, path


def test_force_render_paths_changes_between_builds(tmp_path: Path) -> None:
    for index, (pattern, rendered) in enumerate(
        [("", False), ("/index.md", True), ("!index.md", False)]
    ):
        config = _write_config(
            tmp_path,
            {
                "render_by_default": False,
                "force_render_paths": pattern,
            },
        )
        if index == 0:
            (tmp_path / "content" / "index.md").write_text(
                "Value: {{ 1 + 1 }}\n", encoding="utf-8"
            )
        zensical.build(str(config), {"clean": index == 0, "strict": False})
        html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
        assert ("Value: 2" if rendered else "Value: {{ 1 + 1 }}") in html


def test_verbose_chatter_reaches_cli_and_can_be_disabled(
    tmp_path: Path,
) -> None:
    for index, verbose in enumerate([False, True, False]):
        config = _write_config(
            tmp_path, {"verbose": verbose, "on_error_fail": True}
        )
        if index == 0:
            (tmp_path / "content" / "index.md").write_text(
                '{{ greet("World") }}\n', encoding="utf-8"
            )
            (tmp_path / "main.py").write_text(
                "def define_env(env):\n"
                '    chatter = env.start_chatting("Example", color="cyan")\n'
                '    chatter("Registered macros")\n'
                "    @env.macro\n"
                "    def greet(name):\n"
                '        chatter("Greeting:", name)\n'
                '        return "Hello " + name\n',
                encoding="utf-8",
            )
        result = subprocess.run(  # noqa: S603
            [sys.executable, "-m", "zensical.main", "build", "-f", str(config)],
            capture_output=True,
            text=True,
            check=True,
        )
        html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
        assert "Hello World" in html
        if verbose:
            assert "[macros - Example] - Registered macros" in result.stderr
            assert "[macros - Example] - Greeting: World" in result.stderr
            assert "Rendering page: index.md" in result.stderr
            assert "Loading local module:" in result.stderr
        else:
            assert "[macros -" not in result.stderr
        assert "[macros -" not in result.stdout
        assert "[macros -" not in html


@pytest.mark.parametrize("toml", [False, True], ids=["yaml", "toml"])
@pytest.mark.parametrize(
    ("mode", "expected"),
    [
        ("keep", "Value: {{ missing }}"),
        ("silent", "Value:"),
        ("strict", "Macro Rendering Error"),
        ("lax", "Value:"),
    ],
)
def test_undefined_modes_reach_rendered_pages(
    tmp_path: Path, toml: bool, mode: str, expected: str
) -> None:
    config = _write_config(tmp_path, {"on_undefined": mode}, toml=toml)
    (tmp_path / "content" / "index.md").write_text(
        "Value: {{ missing }}\n", encoding="utf-8"
    )

    zensical.build(str(config), {"clean": True, "strict": False})

    html = BeautifulSoup(
        (tmp_path / "site" / "index.html").read_text(encoding="utf-8"),
        "html.parser",
    )
    assert expected in html.get_text()
    if mode == "strict":
        assert "UndefinedError" in html.get_text()
        assert "index.md" in html.get_text()
    else:
        assert html.get_text(strip=True) == expected


@pytest.mark.parametrize("toml", [False, True], ids=["yaml", "toml"])
def test_default_includes_and_imports_are_refreshed_between_builds(
    tmp_path: Path, toml: bool
) -> None:
    config = _write_config(tmp_path, {"on_error_fail": True}, toml=toml)
    docs_dir = tmp_path / "content"
    (docs_dir / "guide").mkdir()
    (docs_dir / "guide" / "index.md").write_text(
        '{% include "fragment.j2" %}\n\n{% import "helpers.j2" as helpers %}'
        '{{ helpers.greet("Ada") }}\n',
        encoding="utf-8",
    )
    fragment = docs_dir / "fragment.j2"
    helpers = docs_dir / "helpers.j2"

    for index, label in enumerate(["First", "Updated"]):
        fragment.write_text(f"{label} fragment", encoding="utf-8")
        helpers.write_text(
            "{% macro greet(name) %}"
            + label
            + " greeting {{ name }}{% endmacro %}",
            encoding="utf-8",
        )

        # File timestamps are advanced without a delay
        # so cached renders are checked.
        for template in [fragment, helpers]:
            mtime = int(template.stat().st_mtime) + index + 1
            os.utime(template, (mtime, mtime))
        zensical.build(str(config), {"clean": index == 0, "strict": False})

        html = (tmp_path / "site" / "guide" / "index.html").read_text(
            encoding="utf-8"
        )
        assert f"{label} fragment" in html
        assert f"{label} greeting Ada" in html


@pytest.mark.parametrize("on_error_fail", [False, True])
def test_macros_receive_each_pages_title_and_project_context(
    tmp_path: Path, on_error_fail: bool
) -> None:
    config = _write_config(
        tmp_path, {"on_error_fail": on_error_fail}, toml=True
    )
    (tmp_path / "main.py").write_text(
        dedent("""\
        from pathlib import Path

        def define_env(env):
            @env.macro
            def describe():
                assert Path(env.project_dir).samefile(env.conf['root_dir'])
                assert env.variables['page'].title == env.page.title
                return 'Page: ' + env.page.title
    """),
        encoding="utf-8",
    )

    # Metadata, headings, and filename fallbacks are checked in the same build.
    pages = {
        "index.md": ("", "Home"),
        "metadata.md": (
            "---\ntitle: Metadata title\n---\n# Heading\n\n",
            "Metadata title",
        ),
        "heading.md": ("# Heading title\n\n", "Heading title"),
        "file-name.md": ("", "File name"),
        "templated.md": (
            "---\ntitle: Title {{ 2 + 3 }}\n---\n",
            "Title {{ 2 + 3 }}",
        ),
        "templated-heading.md": (
            "# Heading {{ 2 + 3 }}\n\n",
            "Heading {{ 2 + 3 }}",
        ),
    }
    for path, (prefix, _) in pages.items():
        (tmp_path / "content" / path).write_text(
            prefix + "{{ describe() }}\n", encoding="utf-8"
        )

    zensical.build(str(config), {"clean": True, "strict": False})

    for path, (_, title) in pages.items():
        output = (
            tmp_path
            / "site"
            / (
                "index.html"
                if path == "index.md"
                else path.removesuffix(".md") + "/index.html"
            )
        )
        assert f"Page: {title}" in output.read_text(encoding="utf-8"), path


@pytest.mark.parametrize("first_title", [None, "Get started"])
def test_macros_use_the_first_configured_navigation_title(
    tmp_path: Path, first_title: str | None
) -> None:
    config = _write_config(tmp_path, {"on_error_fail": True})
    settings = yaml.safe_load(config.read_text(encoding="utf-8"))
    first_reference = (
        "index.md" if first_title is None else {first_title: "index.md"}
    )
    settings["nav"] = [
        {"Section": [first_reference]},
        {"Duplicate": "index.md"},
    ]
    config.write_text(yaml.safe_dump(settings), encoding="utf-8")
    (tmp_path / "content" / "index.md").write_text(
        "---\ntitle: Metadata {{ 2 + 3 }}\n---\n{{ page_title() }}\n",
        encoding="utf-8",
    )
    (tmp_path / "main.py").write_text(
        dedent("""\
        def define_env(env):
            @env.macro
            def page_title():
                return env.page.title
    """),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": False})

    # The original title is read during body rendering,
    # before its own rendering pass.
    html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert f"<p>{first_title or 'Metadata {{ 2 + 3 }}'}</p>" in html
    assert "Duplicate" not in html


@pytest.mark.parametrize("title_source", ["metadata", "heading", "navigation"])
def test_title_macros_are_rendered_after_the_body(
    tmp_path: Path, title_source: str
) -> None:
    config = _write_config(tmp_path, {"on_error_fail": True})
    title = "Title {{ next_number() }}"
    header = f"---\ntitle: {title}\n---\n" if title_source == "metadata" else ""
    if title_source == "navigation":
        settings = yaml.safe_load(config.read_text(encoding="utf-8"))
        settings["nav"] = [{title: "index.md"}]
        config.write_text(yaml.safe_dump(settings), encoding="utf-8")
    (tmp_path / "overrides" / "main.html").write_text(
        "<title>{{ page.title }}</title>{% if page.meta is defined %}<aside>"
        "{{ page.meta.title | default('') }}</aside>{% endif %}"
        "{{ page.content }}{% for item in nav.items %}"
        "<nav>{{ item.title }}</nav>{% endfor %}",
        encoding="utf-8",
    )
    (tmp_path / "content" / "index.md").write_text(
        header + "# " + title + "\n", encoding="utf-8"
    )
    (tmp_path / "main.py").write_text(
        dedent("""\
        def define_env(env):
            calls = 0
            @env.macro
            def next_number():
                nonlocal calls
                calls += 1
                return calls
    """),
        encoding="utf-8",
    )

    # The rendered title is also retained when the Markdown cache is reused.
    for clean in (True, False):
        zensical.build(str(config), {"clean": clean, "strict": False})

        # The body is rendered first;
        # the effective title is rendered in a separate pass.
        html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
        assert "Title 1" in html
        assert "<title>Title 2</title>" in html
        assert "<nav>Title 2</nav>" in html
        if title_source == "metadata":
            assert "<aside>Title 2</aside>" in html


def test_title_uses_variables_and_metadata_updated_by_the_body(
    tmp_path: Path,
) -> None:
    config = _write_config(
        tmp_path, {"on_error_fail": True, "on_undefined": "strict"}
    )
    title = "{{ generated }} {{ count_examples() }} {{ description }}"
    (tmp_path / "content" / "index.md").write_text(
        '---\ntitle: "'
        + title
        + '"\n---\n{{ example("First") }}\n\n{{ example("Second") }}\n',
        encoding="utf-8",
    )
    (tmp_path / "overrides" / "main.html").write_text(
        "<title>{{ page.title }}</title>{% if page.meta is defined %}<aside>"
        "{{ page.meta.title }}</aside>{% endif %}{{ page.content }}",
        encoding="utf-8",
    )
    (tmp_path / "main.py").write_text(
        dedent("""\
        def define_env(env):
            examples = []
            @env.macro
            def example(name):
                examples.append(name)
                env.variables.generated = 'Collected'
                env.page.meta['description'] = 'examples'
                return name
            @env.macro
            def count_examples():
                return len(examples)
    """),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": False})

    html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert "<title>Collected 2 examples</title>" in html
    assert "<aside>Collected 2 examples</aside>" in html
    assert "<p>First</p>" in html
    assert "<p>Second</p>" in html


@pytest.mark.parametrize("page_path", ["index.md", "guide.md"])
@pytest.mark.parametrize(
    (
        "metadata_title",
        "navigation_title",
        "expected_metadata_title",
        "expected_navigation_title",
    ),
    [
        pytest.param(
            "Metadata {{ next_number() }}",
            None,
            "Metadata 2",
            "Metadata 2",
            id="metadata",
        ),
        pytest.param(
            "Metadata {{ next_number() }}",
            "Short",
            "Metadata 2",
            "Short",
            id="separate-navigation-label",
        ),
        pytest.param(
            "Metadata {{ next_number() }}",
            "Navigation {{ next_number() }}",
            "Metadata 3",
            "Navigation 2",
            id="both-templated",
        ),
        pytest.param(
            "Metadata {{ next_number() }}",
            "Metadata {{ next_number() }}",
            "Metadata 2",
            "Metadata 2",
            id="same-template",
        ),
        pytest.param(
            "Long metadata title",
            "Short",
            "Long metadata title",
            "Short",
            id="plain-metadata",
        ),
        pytest.param(
            "Long metadata title",
            "Navigation {{ next_number() }}",
            "Long metadata title",
            "Navigation 2",
            id="plain-metadata-templated-navigation",
        ),
    ],
)
def test_theme_uses_rendered_metadata_titles(
    tmp_path: Path,
    page_path: str,
    metadata_title: str,
    navigation_title: str | None,
    expected_metadata_title: str,
    expected_navigation_title: str,
) -> None:
    config = _write_config(
        tmp_path, {"on_error_fail": True, "on_undefined": "strict"}
    )
    settings = yaml.safe_load(config.read_text(encoding="utf-8"))

    # The bundled theme is used so browser titles,
    # headers, and feedback are covered.
    settings["theme"].pop("custom_dir")
    if navigation_title is not None:
        settings["nav"] = [{navigation_title: page_path}]
    settings["extra"] = {
        "analytics": {
            "feedback": {
                "title": "Was this helpful?",
                "ratings": [
                    {
                        "icon": "material/thumb-up",
                        "name": "Yes",
                        "data": 1,
                        "note": '<a href="https://example.com/feedback?'
                        'title={title}">Send feedback</a>',
                    }
                ],
            }
        }
    }
    config.write_text(yaml.safe_dump(settings), encoding="utf-8")
    (tmp_path / "content" / page_path).write_text(
        "---\n"
        + yaml.safe_dump({"title": metadata_title})
        + "---\n# Heading\n\nBody: {{ next_number() }}\n",
        encoding="utf-8",
    )
    (tmp_path / "main.py").write_text(
        dedent("""\
        def define_env(env):
            calls = 0
            original_title = env.page.meta['title']
            @env.macro
            def next_number():
                nonlocal calls
                assert env.page.meta['title'] == original_title
                calls += 1
                return calls
    """),
        encoding="utf-8",
    )

    # The rendered metadata title is also retained
    # when cached Markdown is reused.
    for clean in (True, False):
        zensical.build(str(config), {"clean": clean, "strict": False})

        output = "index.html" if page_path == "index.md" else "guide/index.html"
        html = BeautifulSoup(
            (tmp_path / "site" / output).read_text(encoding="utf-8"),
            "html.parser",
        )
        browser_title = html.select_one("title")
        header_title = html.select_one('[data-md-component="header-topic"]')
        feedback_link = html.select_one(".md-feedback__note a")
        assert browser_title is not None
        assert header_title is not None
        assert feedback_link is not None

        assert (
            browser_title.get_text(strip=True)
            == expected_metadata_title + " - Macros"
        )
        assert header_title.get_text(strip=True) == expected_metadata_title
        assert parse_qs(urlsplit(str(feedback_link["href"])).query) == {
            "title": [expected_metadata_title]
        }
        assert expected_navigation_title in [
            link.get_text(" ", strip=True)
            for link in html.select("a.md-nav__link")
        ]
        assert "Body: 1" in html.get_text()


def test_extra_title_does_not_replace_the_page_title(tmp_path: Path) -> None:
    config = _write_config(tmp_path, {"on_error_fail": True})
    settings = yaml.safe_load(config.read_text(encoding="utf-8"))
    settings["extra"] = {"title": "Generated {{ 2 + 3 }}"}
    config.write_text(yaml.safe_dump(settings), encoding="utf-8")
    (tmp_path / "content" / "index.md").write_text(
        "# Heading\n\n{{ page_title() }}\n", encoding="utf-8"
    )
    (tmp_path / "main.py").write_text(
        dedent("""\
        def define_env(env):
            @env.macro
            def page_title():
                return env.page.title
    """),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": False})

    # An extra variable named title is not used as page metadata
    # by MkDocs Macros.
    html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert "<p>Heading</p>" in html


def test_macro_title_does_not_use_nav_replaced_by_literate_nav(
    tmp_path: Path,
) -> None:
    config = _write_config(tmp_path, {"on_error_fail": True})
    settings = yaml.safe_load(config.read_text(encoding="utf-8"))
    settings["nav"] = [{"Discarded title": "index.md"}]
    settings["plugins"]["literate-nav"] = {}
    config.write_text(yaml.safe_dump(settings), encoding="utf-8")
    (tmp_path / "content" / "SUMMARY.md").write_text(
        "- [Generated title](index.md)\n", encoding="utf-8"
    )
    (tmp_path / "content" / "index.md").write_text(
        "---\ntitle: Source title\n---\n{{ page_title() }}\n", encoding="utf-8"
    )
    (tmp_path / "main.py").write_text(
        dedent("""\
        def define_env(env):
            @env.macro
            def page_title():
                return env.page.title
    """),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": True, "strict": False})

    # Generated navigation is resolved later;
    # discarded labels must not be exposed.
    html = (tmp_path / "site" / "index.html").read_text(encoding="utf-8")
    assert "<p>Source title</p>" in html
    assert "Discarded title" not in html
