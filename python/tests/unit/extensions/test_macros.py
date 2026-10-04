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

import logging
import os
from io import StringIO
from textwrap import dedent
from typing import TYPE_CHECKING

import pandas
import pytest
from jinja2.exceptions import (
    TemplateNotFound,
    TemplateSyntaxError,
    UndefinedError,
)
from markdown import Markdown

from tests.unit.extensions.conftest import soup
from zensical.extensions.context import (
    ContextExtension,
    ContextPreprocessor,
    Page,
)
from zensical.extensions.macros import (
    MacroEnv,
    MacrosExtension,
    _fix_url,
    _load_module,
    _load_one_yaml,
    _merge_include_yaml,
    _pretty,
)
from zensical.extensions.table_reader import (
    _add_indentation,
    _convert_to_md_table,
    _get_fake_table_readers,
    _get_table_readers,
)

if TYPE_CHECKING:
    from pathlib import Path

    from pandas import DataFrame


_INCLUDE_CONFIG = {
    "config": {
        "markdown_extensions": {
            "zensical.extensions.macros": {
                "include_dir": "snippets",
                "on_error_fail": True,
            },
        },
    },
}


# ---------------------------------------------------------------------------
# Filters
# ---------------------------------------------------------------------------


class TestFilters:
    @pytest.mark.parametrize(
        ("url", "expected"),
        [
            pytest.param("page.html", "../page.html", id="relative_html"),
            pytest.param(
                "assets/image.png", "../assets/image.png", id="relative_asset"
            ),
            pytest.param(
                "https://example.org",
                "https://example.org",
                id="absolute_https",
            ),
            pytest.param(
                "mailto:test@example.org",
                "mailto:test@example.org",
                id="mailto",
            ),
        ],
    )
    def test_fix_url(self, url: str, expected: str) -> None:
        assert _fix_url(url) == expected

    @pytest.mark.parametrize(
        ("payload", "expected"),
        [
            pytest.param(
                [("alpha", "str", "hello")],
                "**alpha** | *str* | hello",
                id="single_row",
            ),
            pytest.param([], "", id="empty"),
        ],
    )
    def test_pretty(
        self, payload: list[tuple[str, str, str]], expected: str
    ) -> None:
        output = _pretty(payload)
        if expected:
            assert expected in output
        else:
            assert output == ""


# ---------------------------------------------------------------------------
# Defining environments
# ---------------------------------------------------------------------------


class TestMacroEnv:
    @pytest.mark.parametrize("verbose", [False, True])
    def test_chatter_respects_verbose(
        self, caplog: pytest.LogCaptureFixture, verbose: bool
    ) -> None:
        env = MacroEnv(verbose=verbose)
        chatter = env.start_chatting("Example", color="cyan")
        with caplog.at_level(logging.INFO, logger="zensical.extensions.macros"):
            chatter("Count:", 2)
        assert caplog.messages == (
            ["[macros - Example] - Count: 2"] if verbose else []
        )

    def test_conf_is_stored(self) -> None:
        conf = {"site_name": "My Site", "docs_dir": "/docs"}
        env = MacroEnv(conf=conf)
        assert env.conf is conf

    def test_conf_defaults_to_empty_dict(self) -> None:
        env = MacroEnv()
        assert env.conf == {}

    def test_variables_support_nested_dot_access_after_updates(self) -> None:
        env = MacroEnv()
        source = {"nested": {"entries": [{"label": "first"}]}}

        # Configured data is converted when the variable dictionary is updated.
        env.variables.update(source)
        entries = env.variables.nested.entries
        entries.extend([{"label": "second"}])
        combined = entries + [{"label": "third"}]  # noqa: RUF005

        assert [entry.label for entry in combined] == [
            "first",
            "second",
            "third",
        ]
        assert source == {"nested": {"entries": [{"label": "first"}]}}

    def test_registers_macros_and_filters(self) -> None:
        env = MacroEnv(conf={})

        @env.macro
        def twice(value: int) -> int:
            return value * 2

        @env.filter(name="rev")
        def reverse(value: str) -> str:
            return value[::-1]

        assert env.macros["twice"](4) == 8
        assert env.filters["rev"]("abc") == "cba"


@pytest.mark.parametrize("on_error_fail", [False, True])
@pytest.mark.parametrize("navigation_title", [None, "Short"])
def test_title_errors_respect_on_error_fail(
    on_error_fail: bool, navigation_title: str | None
) -> None:
    page = Page(
        url="/",
        path="index.md",
        title=navigation_title,
        meta={"title": "{{ missing_variable }}"},
    )
    md = Markdown(
        extensions=[
            ContextExtension(page=page, config={}),
            MacrosExtension(
                module_name="",
                on_undefined="strict",
                on_error_fail=on_error_fail,
            ),
        ]
    )

    # The body is valid; the error is raised only when the title is rendered.
    if on_error_fail:
        with pytest.raises(UndefinedError):
            md.convert("Body")
    else:
        assert md.convert("Body") == "<p>Body</p>"
        assert page.rendered_metadata_title is not None
        assert "Macro Rendering Error" in page.rendered_metadata_title
        assert "UndefinedError" in page.rendered_metadata_title
        assert "index.md" in page.rendered_metadata_title
        assert page.title == (navigation_title or page.rendered_metadata_title)

    assert page.meta["title"] == "{{ missing_variable }}"


@pytest.mark.parametrize("title", ["{# omitted #}", "{{ '' }}"])
@pytest.mark.parametrize("navigation_title", [None, "Short"])
def test_titles_can_render_to_an_empty_string(
    title: str, navigation_title: str | None
) -> None:
    page = Page(
        url="/", path="index.md", title=navigation_title, meta={"title": title}
    )
    md = Markdown(
        extensions=[
            ContextExtension(page=page, config={}),
            MacrosExtension(module_name="", on_error_fail=True),
        ]
    )

    md.convert("# Heading")

    assert page.title == (navigation_title or "")
    assert page.meta["title"] == title
    assert page.rendered_metadata_title == ""


def test_metadata_title_uses_configured_delimiters() -> None:
    page = Page(
        url="/",
        path="index.md",
        title="Short",
        meta={"title": "Metadata [[ 2 + 3 ]]"},
    )
    md = Markdown(
        extensions=[
            ContextExtension(page=page, config={}),
            MacrosExtension(
                module_name="",
                j2_variable_start_string="[[",
                j2_variable_end_string="]]",
                on_error_fail=True,
            ),
        ]
    )

    md.convert("Body")

    # The configured delimiters are also applied
    # to metadata with a separate navigation label.
    assert page.title == "Short"
    assert page.meta["title"] == "Metadata [[ 2 + 3 ]]"
    assert page.rendered_metadata_title == "Metadata 5"


@pytest.mark.parametrize(
    "md",
    [
        {
            "config": {
                "extra": {"shared": "extra", "nested": {"value": "original"}},
                "markdown_extensions": {
                    "zensical.extensions.macros": {
                        "include_yaml": ["vars.yaml"],
                        "on_error_fail": True,
                    },
                },
            },
            "page": {"title": "Current page", "meta": {"shared": "metadata"}},
        }
    ],
    indirect=True,
)
class TestMacroContext:
    def test_context_is_available_during_registration_and_rendering(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        # Configuration and YAML values are read while the module is registered.
        (tmp_path / "vars.yaml").write_text(
            "included: yaml\n", encoding="utf-8"
        )
        (tmp_path / "main.py").write_text(
            "from pathlib import Path\n"
            "def define_env(env):\n"
            "    env.variables.registered = env.variables.shared + '/' "
            "+ env.variables.included\n"
            "    env.variables.shared = 'module'\n"
            "    @env.macro\n"
            "    def describe():\n"
            "        assert Path(env.project_dir)"
            ".samefile(env.conf['root_dir'])\n"
            "        assert env.config['on_error_fail'] is True\n"
            "        assert env.variables.config['site_name'] == "
            "env.conf['site_name']\n"
            "        assert env.variables.page.title == env.page.title\n"
            "        return env.page.title + '/' + env.variables.shared\n",
            encoding="utf-8",
        )

        html = soup(
            md.convert("{{ registered }}\n\n{{ describe() }}\n\n{{ shared }}")
        )

        # Metadata overrides are applied to templates,
        # while module variables are retained.
        assert [p.get_text() for p in html.select("p")] == [
            "extra/yaml",
            "Current page/module",
            "metadata",
        ]

    def test_nested_variable_changes_are_local_to_the_page(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "vars.yaml").write_text("{}\n", encoding="utf-8")
        (tmp_path / "main.py").write_text(
            "def define_env(env):\n"
            "    env.variables.previous = env.variables['nested']['value']\n"
            "    env.variables.nested['value'] = 'changed'\n",
            encoding="utf-8",
        )

        # A new environment is created for each conversion,
        # without changing config.extra.
        first = soup(
            md.convert("{{ previous }} / {{ nested.value }}")
        ).get_text()
        second = soup(
            md.convert("{{ previous }} / {{ nested.value }}")
        ).get_text()

        assert first == "original / changed"
        assert second == "original / changed"

    def test_configured_nested_variables_are_available_through_attributes(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "vars.yaml").write_text(
            "included: yaml\n", encoding="utf-8"
        )
        (tmp_path / "main.py").write_text(
            "def define_env(env):\n"
            "    @env.macro\n"
            "    def describe():\n"
            "        return env.variables.nested.value + '/' "
            "+ env.variables.included\n",
            encoding="utf-8",
        )

        html = soup(md.convert("{{ describe() }}"))

        assert html.get_text() == "original/yaml"

    def test_template_page_is_a_shallow_copy(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "vars.yaml").write_text("{}\n", encoding="utf-8")
        (tmp_path / "main.py").write_text(
            "def define_env(env):\n"
            "    @env.macro\n"
            "    def update_page():\n"
            "        env.page.title = 'Macro title'\n"
            "        env.page.meta['note'] = 'shared'\n"
            "        return env.page.title\n",
            encoding="utf-8",
        )

        html = soup(
            md.convert(
                "{{ update_page() }} / {{ page.title }} / {{ page.meta.note }}"
            )
        )

        # A direct attribute change is kept local,
        # while the metadata dictionary is shared.
        assert html.get_text() == "Macro title / Current page / shared"

    def test_title_pass_respects_render_macros_changes(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "vars.yaml").write_text("{}\n", encoding="utf-8")
        (tmp_path / "main.py").write_text(
            "def define_env(env):\n"
            "    @env.macro\n"
            "    def stop_rendering():\n"
            "        env.page.meta['render_macros'] = False\n"
            "        return 'Body'\n",
            encoding="utf-8",
        )
        context = ContextPreprocessor.from_markdown(md)
        assert context is not None
        context.page.meta = context.page.meta.copy()
        context.page.title = "{{ missing_variable }}"
        context.page.meta["title"] = "Metadata {{ missing_variable }}"

        html = soup(md.convert("{{ stop_rendering() }}"))

        # Rendering is disabled by the body before the title pass is reached.
        assert html.get_text() == "Body"
        assert context.page.title == "{{ missing_variable }}"
        assert (
            context.page.rendered_metadata_title
            == "Metadata {{ missing_variable }}"
        )

    def test_nested_mutations_remain_visible_to_templates(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "vars.yaml").write_text("{}\n", encoding="utf-8")
        (tmp_path / "main.py").write_text(
            "def define_env(env):\n"
            "    state = {'value': 'initial', 'alias': 'initial'}\n"
            "    env.variables.state = state\n"
            "    @env.macro\n"
            "    def update():\n"
            "        env.variables.state['value'] = 'changed'\n"
            "        state['alias'] = 'changed'\n"
            "        return ''\n",
            encoding="utf-8",
        )

        # Both references are changed after the Jinja context has been created.
        html = soup(
            md.convert("{{ update() }}{{ state.value }} / {{ state.alias }}")
        )

        assert html.get_text() == "changed / changed"

    def test_template_config_changes_do_not_mutate_project_config(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "vars.yaml").write_text("{}\n", encoding="utf-8")
        (tmp_path / "main.py").write_text(
            "def define_env(env):\n"
            "    env.variables.config['site_name'] = 'Template name'\n"
            "    @env.macro\n"
            "    def site_name():\n"
            "        return env.conf['site_name']\n",
            encoding="utf-8",
        )

        html = soup(md.convert("{{ config.site_name }} / {{ site_name() }}"))

        assert html.get_text() == "Template name / Demo"

    def test_page_context_includes_the_effective_title(
        self, md: Markdown
    ) -> None:
        # The computed property is included in the built-in context diagnostic.
        html = md.convert("{{ context(page) | pretty }}")

        assert "Current page" in html
        assert "<strong>title</strong>" in html
        assert "_fallback_title" not in html
        assert "_title" not in html


# ---------------------------------------------------------------------------
# Loading YAML
# ---------------------------------------------------------------------------


class TestLoadYAML:
    @pytest.mark.parametrize(
        ("relative_path", "content", "expected"),
        [
            pytest.param(
                "ok.yaml", "a: 1\nb: x\n", {"a": 1, "b": "x"}, id="valid_dict"
            ),
            pytest.param(
                "not_dict.yaml", "- 1\n- 2\n", None, id="non_dict_returns_none"
            ),
        ],
    )
    def test_with_relative_paths(
        self,
        tmp_path: Path,
        relative_path: str,
        content: str,
        expected: dict | None,
    ) -> None:
        (tmp_path / relative_path).write_text(content, encoding="utf-8")
        loaded = _load_one_yaml(relative_path, tmp_path)
        assert loaded == expected

    def test_blocks_outside_project_root(self, tmp_path: Path) -> None:
        outside = tmp_path.parent / "outside.yaml"
        outside.write_text("x: 1\n", encoding="utf-8")
        loaded = _load_one_yaml(str(outside), tmp_path)
        assert loaded is None

    @pytest.mark.parametrize(
        "include_yaml",
        [
            pytest.param(["a.yaml", "b.yaml"], id="list"),
            pytest.param({"left": "a.yaml", "right": "b.yaml"}, id="dict"),
        ],
    )
    def test_merge_include(
        self,
        tmp_path: Path,
        include_yaml: list[str] | dict[str, str],
    ) -> None:
        (tmp_path / "a.yaml").write_text("x: 1\n", encoding="utf-8")
        (tmp_path / "b.yaml").write_text("y: 2\n", encoding="utf-8")
        variables: dict = {}
        _merge_include_yaml(include_yaml, tmp_path, variables)
        if isinstance(include_yaml, list):
            assert variables == {"x": 1, "y": 2}
        else:
            assert variables == {"left": {"x": 1}, "right": {"y": 2}}


# ---------------------------------------------------------------------------
# Loading modules / pluglets
# ---------------------------------------------------------------------------


class TestLoadModule:
    def test_from_local_file(self, tmp_path: Path) -> None:
        (tmp_path / "main.py").write_text(
            "def define_env(env):\n"
            "    env.variables['site_name'] = 'Demo'\n"
            "    @env.macro\n"
            "    def twice(x):\n"
            "        return x * 2\n"
            "    @env.filter\n"
            "    def shout(s):\n"
            "        return s.upper()\n",
            encoding="utf-8",
        )
        env = MacroEnv(conf={})
        _load_module(env, "main", tmp_path)
        assert env.variables["site_name"] == "Demo"
        assert env.macros["twice"](3) == 6
        assert env.filters["shout"]("hi") == "HI"

    @pytest.mark.parametrize(
        ("module_name", "file_rel_path"),
        [
            pytest.param("../evil", "../evil.py", id="path_traversal"),
            pytest.param("foo/bar", "foo/bar.py", id="forward_slash"),
            pytest.param("foo\\\\bar", "foo\\\\bar.py", id="backslash"),
        ],
    )
    def test_rejects_invalid_names(
        self,
        module_name: str,
        file_rel_path: str,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        define_env_source = (
            "def define_env(env):\n"
            "    env.variables['pwned'] = True\n"
            "    env.macros['evil'] = lambda: None\n"
            "    env.filters['bad'] = lambda x: x\n"
        )
        target = tmp_path / file_rel_path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(define_env_source, encoding="utf-8")
        monkeypatch.chdir(tmp_path)
        env = MacroEnv(conf={})
        _load_module(env, module_name)
        assert env.variables == {}
        assert env.macros == {}
        assert env.filters == {}


@pytest.mark.parametrize("module_name", ["", "main"])
def test_installed_modules_are_loaded_in_order_before_the_local_module(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, module_name: str
) -> None:
    monkeypatch.syspath_prepend(str(tmp_path))
    modules = [f"macros_first_{module_name}", f"macros_second_{module_name}"]

    # The same variable, macro, and filter are registered by all three modules.
    for name, label in [
        *zip(modules, ["first", "second"], strict=True),
        ("main", "local"),
    ]:
        (tmp_path / f"{name}.py").write_text(
            dedent(f"""\
            def define_env(env):
                env.variables.setdefault('load_order', []).append('{label}')
                env.variables.winner = '{label}'
                env.macro(lambda: '{label}', 'winner_macro')
                env.filter(lambda value: '{label}', 'winner_filter')
        """),
            encoding="utf-8",
        )

    md = Markdown(
        extensions=[
            ContextExtension(
                page=Page(url="/", path="index.md"),
                config={"root_dir": str(tmp_path)},
            ),
            MacrosExtension(
                modules=modules, module_name=module_name, on_error_fail=True
            ),
        ]
    )
    html = soup(
        md.convert(
            '{{ load_order | join(",") }} / {{ winner }} / '
            '{{ winner_macro() }} / {{ "" | winner_filter }}'
        )
    )

    if module_name:
        assert html.get_text() == "first,second,local / local / local / local"
    else:
        assert html.get_text() == "first,second / second / second / second"


@pytest.mark.parametrize("on_error_fail", [False, True])
@pytest.mark.parametrize(
    ("module_source", "error_type", "message"),
    [
        (None, ModuleNotFoundError, "unavailable_macros_pluglet"),
        (
            "import unavailable_macros_dependency\n",
            ModuleNotFoundError,
            "unavailable_macros_dependency",
        ),
        (
            "raise ImportError('broken macros import')\n",
            ImportError,
            "broken macros import",
        ),
    ],
)
def test_installed_module_import_errors_are_reported(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    module_source: str | None,
    error_type: type[Exception],
    message: str,
    on_error_fail: bool,
) -> None:
    monkeypatch.syspath_prepend(str(tmp_path))
    if module_source is not None:
        (tmp_path / "unavailable_macros_pluglet.py").write_text(
            module_source, encoding="utf-8"
        )
    md = Markdown(
        extensions=[
            MacrosExtension(
                module_name="",
                modules=["unavailable_macros_pluglet"],
                on_error_fail=on_error_fail,
            )
        ]
    )

    # Module imports are configuration errors,
    # regardless of the rendering policy.
    with pytest.raises(error_type, match=message):
        md.convert("Body")


def test_missing_default_local_module_is_optional(tmp_path: Path) -> None:
    md = Markdown(
        extensions=[
            ContextExtension(
                page=Page(url="/", path="index.md"),
                config={"root_dir": str(tmp_path)},
            ),
            MacrosExtension(on_error_fail=True),
        ]
    )

    assert md.convert("{{ 2 + 3 }}") == "<p>5</p>"


def test_default_local_module_dependency_errors_are_reported(
    tmp_path: Path,
) -> None:
    (tmp_path / "main.py").write_text(
        "import unavailable_local_macros_dependency\n", encoding="utf-8"
    )
    md = Markdown(
        extensions=[
            ContextExtension(
                page=Page(url="/", path="index.md"),
                config={"root_dir": str(tmp_path)},
            ),
            MacrosExtension(),
        ]
    )

    with pytest.raises(
        ModuleNotFoundError, match="unavailable_local_macros_dependency"
    ):
        md.convert("Body")


@pytest.mark.parametrize(
    "name",
    [
        "config",
        "environment",
        "files",
        "filters",
        "filters_builtin",
        "git",
        "macros",
        "navigation",
        "page",
        "plugin",
        "context",
        "now",
        "macros_info",
        "read_csv",
        "range",
        "custom_macro",
    ],
)
def test_metadata_overrides_variables_and_macros_in_body_and_title(
    tmp_path: Path, name: str
) -> None:
    (tmp_path / "main.py").write_text(
        f"def define_env(env):\n    env.variables[{name!r}] = 'module'\n    "
        f"env.macro(lambda: 'macro', {name!r})\n",
        encoding="utf-8",
    )
    template = "{{ " + name + " }}"
    page = Page(
        url="/", path="index.md", meta={name: "metadata", "title": template}
    )
    md = Markdown(
        extensions=[
            ContextExtension(
                page=page,
                config={"root_dir": str(tmp_path), "extra": {name: "extra"}},
            ),
            MacrosExtension(on_error_fail=True),
        ]
    )

    html = md.convert(template)

    # Metadata overrides are applied during both rendering passes.
    assert html == "<p>metadata</p>"
    assert page.title == "metadata"
    assert page.rendered_metadata_title == "metadata"


@pytest.mark.parametrize(
    "name",
    ["context", "now", "macros_info", "read_csv", "range", "custom_macro"],
)
@pytest.mark.parametrize("source", ["yaml", "extra", "module"])
def test_variables_override_registered_macros(
    tmp_path: Path, name: str, source: str
) -> None:
    (tmp_path / "vars.yaml").write_text(f"{name}: yaml\n", encoding="utf-8")
    module_source = (
        f"def define_env(env):\n    env.macro(lambda: 'macro', {name!r})\n"
    )
    if source == "module":
        module_source += f"    env.variables[{name!r}] = 'module'\n"
    (tmp_path / "main.py").write_text(module_source, encoding="utf-8")
    extra = {name: "extra"} if source in {"extra", "module"} else {}
    md = Markdown(
        extensions=[
            ContextExtension(
                page=Page(url="/", path="index.md"),
                config={"root_dir": str(tmp_path), "extra": extra},
            ),
            MacrosExtension(include_yaml=["vars.yaml"], on_error_fail=True),
        ]
    )

    html = md.convert("{{ " + name + " }}")

    # YAML, extra, and module variables are each given precedence
    # over macro globals.
    assert html == f"<p>{source}</p>"


@pytest.mark.parametrize("name", ["config", "environment", "git", "plugin"])
@pytest.mark.parametrize("source", ["extra", "module"])
def test_variables_override_standard_context(
    tmp_path: Path, name: str, source: str
) -> None:
    (tmp_path / "main.py").write_text(
        f"def define_env(env):\n    env.variables[{name!r}] = 'module'\n",
        encoding="utf-8",
    )
    md = Markdown(
        extensions=[
            ContextExtension(
                page=Page(url="/", path="index.md"),
                config={"root_dir": str(tmp_path), "extra": {name: "extra"}},
            ),
            MacrosExtension(
                module_name="main" if source == "module" else "",
                on_error_fail=True,
            ),
        ]
    )

    html = md.convert("{{ " + name + " }}")

    assert html == f"<p>{source}</p>"


@pytest.mark.parametrize("name", ["context", "now", "macros_info", "read_csv"])
def test_registered_macros_override_standard_macros(
    tmp_path: Path, name: str
) -> None:
    (tmp_path / "main.py").write_text(
        f"def define_env(env):\n    env.macro(lambda: 'custom', {name!r})\n",
        encoding="utf-8",
    )
    md = Markdown(
        extensions=[
            ContextExtension(
                page=Page(url="/", path="index.md"),
                config={"root_dir": str(tmp_path)},
            ),
            MacrosExtension(on_error_fail=True),
        ]
    )

    html = md.convert("{{ " + name + "() }}")

    assert html == "<p>custom</p>"


# ---------------------------------------------------------------------------
# Markdown preprocessor
# ---------------------------------------------------------------------------


class TestPreprocessor:
    @pytest.mark.parametrize(
        "md",
        [
            {
                "config": {
                    "markdown_extensions": {
                        "zensical.extensions.macros": {
                            "render_by_default": False,
                            "force_render_paths": (
                                "# Selected pages\n\n"
                                "guides/\n!guides/drafts/\nguides/drafts/keep.md\n"
                                "/only.md\nrender_*.md\n\\#literal.md\n\\!literal.md"
                            ),
                        }
                    }
                }
            }
        ],
        indirect=True,
    )
    @pytest.mark.parametrize(
        ("path", "override", "rendered"),
        [
            ("guides/index.md", None, True),
            (os.path.join("guides", "nested", "page.md"), None, True),
            ("guides/café.md", None, True),
            ("guides/drafts/page.md", None, False),
            ("guides/drafts/keep.md", None, True),
            ("guides/disabled.md", False, False),
            ("guides/drafts/keep.md", False, False),
            ("outside.md", True, True),
            ("outside.md", None, False),
            ("guides.md", None, False),
            ("only.md", None, True),
            ("nested/only.md", None, False),
            ("nested/render_example.md", None, True),
            ("#literal.md", None, True),
            ("!literal.md", None, True),
        ],
    )
    def test_force_render_paths_respects_page_overrides(
        self, md: Markdown, path: str, override: bool | None, rendered: bool
    ) -> None:
        context = ContextPreprocessor.from_markdown(md)
        assert context is not None
        context.page.path = path
        if override is not None:
            context.page.meta["render_macros"] = override

        html = soup(md.convert("Value: {{ 1 + 1 }}"))
        assert html.get_text() == (
            "Value: 2" if rendered else "Value: {{ 1 + 1 }}"
        )

    def test_force_render_paths_does_not_require_page_context(self) -> None:
        md = Markdown(
            extensions=[
                MacrosExtension(
                    render_by_default=False, force_render_paths="**"
                )
            ]
        )
        assert md.convert("Value: {{ 1 + 1 }}") == "<p>Value: {{ 1 + 1 }}</p>"

    @pytest.mark.parametrize("verbose", [False, True])
    @pytest.mark.parametrize(
        "md",
        [
            {
                "config": {
                    "markdown_extensions": {
                        "zensical.extensions.macros": {"on_error_fail": True}
                    }
                }
            }
        ],
        indirect=True,
    )
    def test_verbose_traces_module_loading_and_rendering(
        self,
        md: Markdown,
        tmp_path: Path,
        caplog: pytest.LogCaptureFixture,
        verbose: bool,
    ) -> None:
        md.preprocessors["macros"].config.verbose = verbose
        (tmp_path / "main.py").write_text(
            "def define_env(env):\n"
            '    chatter = env.start_chatting("Example")\n'
            '    chatter("Registered macros")\n'
            "    @env.macro\n"
            "    def greet(name):\n"
            '        chatter("Greeting:", name)\n'
            '        return "Hello " + name\n',
            encoding="utf-8",
        )
        with caplog.at_level(logging.INFO, logger="zensical.extensions.macros"):
            html = md.convert('{{ greet("World") }}')
        assert "Hello World" in html
        if verbose:
            assert (
                "[macros - render] - Rendering page: index.md"
                in caplog.messages
            )
            assert any(
                "Loading local module:" in line for line in caplog.messages
            )
            assert "[macros - Example] - Registered macros" in caplog.messages
            assert "[macros - Example] - Greeting: World" in caplog.messages
        else:
            assert caplog.messages == []

    @pytest.mark.parametrize(
        ("md", "expected_text"),
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": False
                            },
                        },
                    }
                },
                "Value: {{ 1 + 1 }}",
                id="disabled_keeps_template",
            ),
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                                "force_render_paths": "!**",
                            },
                        },
                    }
                },
                "Value: 2",
                id="enabled_renders",
            ),
        ],
        indirect=["md"],
    )
    def test_respects_render_by_default(
        self, md: Markdown, expected_text: str
    ) -> None:
        html = soup(md.convert("Value: {{ 1 + 1 }}"))
        p = html.select_one("p")
        assert p is not None
        assert p.get_text() == expected_text

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": False
                            },
                        },
                    },
                    "page": {"meta": {"render_macros": True}},
                },
                id="page_opt_in",
            ),
        ],
        indirect=["md"],
    )
    def test_renders_when_opted_in_by_page_meta(self, md: Markdown) -> None:
        html = soup(md.convert("Value: {{ 1 + 1 }}"))
        p = html.select_one("p")
        assert p is not None
        assert p.get_text() == "Value: 2"

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                                "include_yaml": ["vars.yaml"],
                                "module_name": "main",
                            },
                        },
                        "extra/who": "world",
                    },
                    "page": {"meta": {"render_macros": True}},
                },
                id="include_yaml_and_module",
            ),
        ],
        indirect=["md"],
    )
    def test_renders_with_include_yaml_and_module(
        self,
        md: Markdown,
        tmp_path: Path,
    ) -> None:
        (tmp_path / "vars.yaml").write_text("name: Ada\n", encoding="utf-8")
        (tmp_path / "main.py").write_text(
            "def define_env(env):\n"
            "    @env.macro\n"
            "    def greet(name):\n"
            "        return f'Hello {name}!'\n",
            encoding="utf-8",
        )
        html = soup(md.convert("{{ greet(name) }}\n\n{{ who }}"))
        text = html.get_text()
        assert "Hello Ada!" in text
        assert "world" in text

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "include_dir": "snippets",
                                "on_error_fail": True,
                            },
                        },
                    },
                },
                id="nested_include",
            ),
        ],
        indirect=["md"],
    )
    def test_renders_nested_include(
        self,
        md: Markdown,
        tmp_path: Path,
    ) -> None:
        include_dir = tmp_path / "snippets" / "sub"
        include_dir.mkdir(parents=True)
        include_dir.joinpath("fragment.md").write_text(
            "Included fragment text.", encoding="utf-8"
        )

        context = ContextPreprocessor.from_markdown(md)
        assert context is not None
        if os.name == "nt":
            # Rust canonicalizes the configuration path before parsing it.
            root = str(tmp_path)
            if not root.startswith("\\\\?\\"):
                context.config["root_dir"] = "\\\\?\\" + root

        html = soup(md.convert('{% include "sub/fragment.md" %}'))
        assert html.get_text() == "Included fragment text."

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True
                            },
                        },
                    }
                },
                id="render_by_default",
            ),
        ],
        indirect=["md"],
    )
    def test_syntax_errors_are_displayed_by_default(self, md: Markdown) -> None:
        html = soup(md.convert("Before\n\n{{ not_closed"))

        assert html.h1 is not None
        assert "Macro Syntax Error" in html.h1.get_text()
        assert "index.md" in html.get_text()
        assert "Line 3" in html.get_text()
        assert "unexpected end of template" in html.get_text()
        code = html.select_one("pre")
        assert code is not None
        assert code.get_text().strip() == "{{ not_closed"
        assert "Before" not in html.get_text()

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                                "on_error_fail": True,
                            },
                        },
                    }
                },
                id="on_error_fail",
            ),
        ],
        indirect=["md"],
    )
    def test_error_handling_raises_when_enabled(self, md: Markdown) -> None:
        with pytest.raises(TemplateSyntaxError):
            md.convert("{{ not_closed")

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True
                            },
                        },
                    },
                    "page": {
                        "meta": {
                            "render_macros": True,
                            "title": "Doc {{ 2 + 3 }}",
                        },
                    },
                },
                id="jinja_in_title",
            ),
        ],
        indirect=["md"],
    )
    def test_renders_title_after_body_without_changing_metadata(
        self, md: Markdown
    ) -> None:
        html = soup(md.convert("# {{ title }}"))
        context = ContextPreprocessor.from_markdown(md)
        assert context is not None

        assert html.h1 is not None
        assert html.h1.get_text().startswith("Doc {{ 2 + 3 }}")
        assert context.page.title == "Doc 5"
        assert context.page.meta["title"] == "Doc {{ 2 + 3 }}"

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                                "on_undefined": "strict",
                                "on_error_fail": True,
                            },
                        },
                    },
                },
                id="strict_on_error_fail",
            ),
        ],
        indirect=["md"],
    )
    def test_strict_undefined_raises(self, md: Markdown) -> None:
        with pytest.raises(UndefinedError):
            md.convert("{{ missing_variable }}")

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                                "module_name": "main",
                            },
                            "pymdownx.superfences": {},
                        },
                    },
                },
                id="superfences",
            ),
        ],
        indirect=["md"],
    )
    def test_fenced_code_block_processed_by_superfences(
        self,
        md: Markdown,
        tmp_path: Path,
    ) -> None:
        (tmp_path / "main.py").write_text(
            "def define_env(env):\n"
            "    @env.macro\n"
            "    def code_snippet(lang, code):\n"
            "        return f'```{lang}\\n{code}\\n```\\n'\n",
            encoding="utf-8",
        )
        html = soup(md.convert("{{ code_snippet('python', 'x = 1 + 1') }}"))
        code = html.select_one("code")
        assert code is not None
        # Language in class attr, not text
        assert "python" not in code.get_text()
        # Source broken into syntax-highlighted spans
        assert code.select("span")


@pytest.mark.parametrize(
    ("mode", "expected"),
    [
        ("keep", "Before {{ missing }} after"),
        ("silent", "Before  after"),
        ("strict", None),
        ("lax", "Before  after"),
    ],
)
def test_undefined_variable_modes(mode: str, expected: str | None) -> None:
    md = Markdown(
        extensions=[
            MacrosExtension(
                module_name="", on_undefined=mode, on_error_fail=True
            )
        ]
    )

    if expected is None:
        with pytest.raises(UndefinedError, match="missing"):
            md.convert("Before {{ missing }} after")
    else:
        assert md.convert("Before {{ missing }} after") == f"<p>{expected}</p>"


@pytest.mark.parametrize("mode", ["keep", "silent", "strict", "lax"])
@pytest.mark.parametrize(
    "expression",
    ["missing.attribute", "missing['item']", "missing()", "missing + 1"],
)
def test_undefined_operations_follow_mkdocs_macros_behavior(
    mode: str, expression: str
) -> None:
    md = Markdown(
        extensions=[
            MacrosExtension(
                module_name="", on_undefined=mode, on_error_fail=True
            )
        ]
    )
    template = "Before {{ " + expression + " }} after"

    # Attribute access is tolerated in lax mode; calls, indexing,
    # and arithmetic still fail.
    if mode == "lax" and expression == "missing.attribute":
        assert md.convert(template) == "<p>Before  after</p>"
    else:
        with pytest.raises(UndefinedError, match="missing"):
            md.convert(template)


def test_undefined_variables_are_kept_by_default() -> None:
    md = Markdown(
        extensions=[MacrosExtension(module_name="", on_error_fail=True)]
    )

    assert md.convert("{{ missing }}") == "<p>{{ missing }}</p>"


@pytest.mark.parametrize(
    ("source", "error_type", "message"),
    [
        (
            "Before {{ missing }} after",
            "UndefinedError",
            "'missing' is undefined",
        ),
        ("Before {{ 1 / 0 }} after", "ZeroDivisionError", "division by zero"),
    ],
)
def test_rendering_errors_are_displayed_with_a_traceback(
    source: str, error_type: str, message: str, caplog: pytest.LogCaptureFixture
) -> None:
    md = Markdown(
        extensions=[
            ContextExtension(
                page=Page(url="/guide/", path="guide.md"), config={}
            ),
            MacrosExtension(module_name="", on_undefined="strict"),
        ]
    )

    html = soup(md.convert(source))

    assert html.h1 is not None
    assert html.h1.get_text() == "Macro Rendering Error"
    assert "guide.md" in html.get_text()
    assert error_type in html.get_text()
    assert message in html.get_text()
    assert "Before" not in html.get_text()
    assert message in caplog.text

    traceback_code = html.select_one("pre code")
    assert traceback_code is not None
    assert "Traceback (most recent call last)" in traceback_code.get_text()


def test_macro_exceptions_are_displayed_as_text(tmp_path: Path) -> None:
    (tmp_path / "main.py").write_text(
        dedent("""\
        def define_env(env):
            @env.macro
            def fail():
                raise ValueError('<script>broken macro</script>')
    """),
        encoding="utf-8",
    )
    md = Markdown(
        extensions=[
            ContextExtension(
                page=Page(url="/", path="index.md"),
                config={"root_dir": str(tmp_path)},
            ),
            MacrosExtension(),
        ]
    )

    html = soup(md.convert("{{ fail() }}"))

    assert "Macro Rendering Error" in html.get_text()
    assert "ValueError" in html.get_text()
    assert "<script>broken macro</script>" in html.get_text()
    assert html.select("script") == []


@pytest.mark.parametrize("docs_path", ["default", "relative", "absolute"])
@pytest.mark.parametrize("include_dir", ["", "snippets"])
def test_includes_and_imports_default_to_docs_dir(
    tmp_path: Path, docs_path: str, include_dir: str
) -> None:
    docs_dir = tmp_path / ("docs" if docs_path == "default" else "content")
    docs_dir.mkdir()
    include_root = tmp_path / include_dir if include_dir else docs_dir
    (include_root / "nested").mkdir(parents=True)
    (include_root / "nested" / "fragment.md").write_text(
        "Included {{ who }}", encoding="utf-8"
    )
    (include_root / "helpers.j2").write_text(
        "{% macro greet(name) %}Hello {{ name }}{% endmacro %}",
        encoding="utf-8",
    )

    # A conflicting template in `docs_dir` is ignored
    # when `include_dir` is configured.
    if include_dir:
        (docs_dir / "helpers.j2").write_text("Wrong template", encoding="utf-8")
    config = {"root_dir": str(tmp_path), "extra": {"who": "Ada"}}
    if docs_path != "default":
        config["docs_dir"] = (
            str(docs_dir) if docs_path == "absolute" else "content"
        )
    md = Markdown(
        extensions=[
            ContextExtension(
                page=Page(url="/", path="index.md"), config=config
            ),
            MacrosExtension(
                module_name="", include_dir=include_dir, on_error_fail=True
            ),
        ]
    )

    html = soup(
        md.convert(
            '{% include "nested/fragment.md" %}\n\n'
            '{% import "helpers.j2" as helpers %}{{ helpers.greet(who) }}'
        )
    )

    assert [p.get_text() for p in html.select("p")] == [
        "Included Ada",
        "Hello Ada",
    ]


@pytest.mark.parametrize("include_dir", ["", "missing"])
def test_missing_include_directory_reports_template_not_found(
    tmp_path: Path, include_dir: str
) -> None:
    md = Markdown(
        extensions=[
            ContextExtension(
                page=Page(url="/", path="index.md"),
                config={"root_dir": str(tmp_path)},
            ),
            MacrosExtension(
                module_name="", include_dir=include_dir, on_error_fail=True
            ),
        ]
    )

    with pytest.raises(TemplateNotFound, match=r"missing\.md"):
        md.convert('{% include "missing.md" %}')


def test_syntax_errors_in_included_templates_report_the_correct_source(
    tmp_path: Path,
) -> None:
    (tmp_path / "docs").mkdir()
    fragment = tmp_path / "docs" / "invalid.md"
    fragment.write_text("First\nSecond\nThird\n{% if %}", encoding="utf-8")
    md = Markdown(
        extensions=[
            ContextExtension(
                page=Page(url="/", path="index.md"),
                config={"root_dir": str(tmp_path)},
            ),
            MacrosExtension(module_name=""),
        ]
    )

    # The error line is beyond the length of the page
    # that includes this template.
    html = soup(md.convert('{% include "invalid.md" %}'))

    assert "Macro Syntax Error" in html.get_text()
    assert str(fragment) in html.get_text()
    assert "Line 4" in html.get_text()
    code = html.select_one("pre code")
    assert code is not None
    assert code.get_text().strip() == "{% if %}"


@pytest.mark.parametrize(
    "md",
    [pytest.param(_INCLUDE_CONFIG, id="include_dir")],
    indirect=["md"],
)
class TestIncludeLoader:
    def test_renders_nested_include_and_refreshes_changed_source(
        self,
        md: Markdown,
        tmp_path: Path,
    ) -> None:
        include_dir = tmp_path / "snippets" / "sub"
        include_dir.mkdir(parents=True)
        fragment = include_dir / "fragment.md"
        fragment.write_text("First fragment.", encoding="utf-8")

        template = '{% include "sub/fragment.md" %}'
        assert soup(md.convert(template)).get_text() == "First fragment."

        fragment.write_text("Updated fragment.", encoding="utf-8")
        assert soup(md.convert(template)).get_text() == "Updated fragment."

    def test_missing_include_raises_template_not_found(
        self,
        md: Markdown,
        tmp_path: Path,
    ) -> None:
        (tmp_path / "snippets").mkdir()

        with pytest.raises(TemplateNotFound) as error:
            md.convert('{% include "missing.md" %}')

        assert error.value.name == "missing.md"

    def test_parent_traversal_is_rejected(
        self,
        md: Markdown,
        tmp_path: Path,
    ) -> None:
        (tmp_path / "snippets").mkdir()
        (tmp_path / "outside.md").write_text("Outside.", encoding="utf-8")

        with pytest.raises(TemplateNotFound):
            md.convert('{% include "../outside.md" %}')

    def test_template_error_reports_included_source(
        self,
        md: Markdown,
        tmp_path: Path,
    ) -> None:
        include_dir = tmp_path / "snippets"
        include_dir.mkdir()
        fragment = include_dir / "invalid.md"
        fragment.write_text("{% if %}", encoding="utf-8")

        with pytest.raises(TemplateSyntaxError) as error:
            md.convert('{% include "invalid.md" %}')

        assert error.value.filename is not None
        assert os.path.samefile(error.value.filename, fragment)


# ---------------------------------------------------------------------------
# Table helpers
# ---------------------------------------------------------------------------


# Unit tests for the table conversion helper functions.
class TestTableHelpers:
    def test_add_indentation_spaces(self) -> None:
        result = _add_indentation("line1\nline2", spaces=4)
        assert result == "    line1\n    line2"

    def test_add_indentation_tabs(self) -> None:
        result = _add_indentation("line1\nline2", tabs=2)
        assert result == "\t\tline1\n\t\tline2"

    def test_add_indentation_none_returns_unchanged(self) -> None:
        assert _add_indentation("hello") == "hello"

    def test_add_indentation_raises_when_both_specified(self) -> None:
        with pytest.raises(ValueError, match="spaces or tabs"):
            _add_indentation("x", spaces=2, tabs=1)

    def test_convert_to_md_table_basic(self) -> None:
        df: DataFrame = pandas.DataFrame(
            {"Name": ["Alice", "Bob"], "Age": [30, 25]}
        )
        result = _convert_to_md_table(df)
        assert "|" in result
        assert "Name" in result
        assert "Age" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_convert_to_md_table_escapes_pipes_in_cells(self) -> None:
        df: DataFrame = pandas.DataFrame({"Col": ["a|b", "c"]})
        result = _convert_to_md_table(df)
        assert r"a\|b" in result

    def test_convert_to_md_table_escapes_pipes_in_column_names(self) -> None:
        df: DataFrame = pandas.DataFrame({"Na|me": ["Alice"]})
        result = _convert_to_md_table(df)
        assert r"Na\|me" in result

    def test_convert_to_md_table_omits_index_by_default(self) -> None:
        # Custom index values must not leak into the output.
        # Verifies that the `index=False` default is applied.
        df: DataFrame = pandas.DataFrame(
            {"X": [1, 2]}, index=pandas.Index([100, 200])
        )
        result = _convert_to_md_table(df)
        assert "100" not in result
        assert "200" not in result

    def test_convert_to_md_table_requires_string_output(self) -> None:
        df: DataFrame = pandas.DataFrame({"X": [1, 2]})
        with pytest.raises(ValueError, match="produced no output"):
            _convert_to_md_table(df, buf=StringIO())


# ---------------------------------------------------------------------------
# Table readers
# ---------------------------------------------------------------------------


class TestTableReaders:
    def test_fake_readers_raise_runtime_error_when_pandas_missing(
        self,
    ) -> None:
        readers = _get_fake_table_readers()
        for reader in readers.values():
            with pytest.raises(RuntimeError, match="table reading requires"):
                reader("irrelevant.csv")

    # CSV
    def test_read_csv(self, tmp_path: Path) -> None:
        (tmp_path / "data.csv").write_text(
            "Name,Age\nAlice,30\nBob,25\n", encoding="utf-8"
        )
        readers = _get_table_readers(tmp_path)
        result = readers["read_csv"]("data.csv")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_csv_returns_dataframe(self, tmp_path: Path) -> None:
        (tmp_path / "data.csv").write_text("X,Y\n1,2\n3,4\n", encoding="utf-8")
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_csv"]("data.csv")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # JSON
    def test_read_json(self, tmp_path: Path) -> None:
        (tmp_path / "data.json").write_text(
            '[{"Name": "Alice", "Age": 30}, {"Name": "Bob", "Age": 25}]',
            encoding="utf-8",
        )
        readers = _get_table_readers(tmp_path)
        result = readers["read_json"]("data.json")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_json_returns_dataframe(self, tmp_path: Path) -> None:
        (tmp_path / "data.json").write_text(
            '[{"X": 1, "Y": 2}, {"X": 3, "Y": 4}]', encoding="utf-8"
        )
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_json"]("data.json")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # YAML
    def test_read_yaml(self, tmp_path: Path) -> None:
        (tmp_path / "data.yaml").write_text(
            "- Name: Alice\n  Age: 30\n- Name: Bob\n  Age: 25\n",
            encoding="utf-8",
        )
        readers = _get_table_readers(tmp_path)
        result = readers["read_yaml"]("data.yaml")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_yaml_returns_dataframe(self, tmp_path: Path) -> None:
        (tmp_path / "data.yaml").write_text(
            "- X: 1\n  Y: 2\n- X: 3\n  Y: 4\n", encoding="utf-8"
        )
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_yaml"]("data.yaml")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # Table (tab-separated)
    def test_read_table(self, tmp_path: Path) -> None:
        (tmp_path / "data.tsv").write_text(
            "Name\tAge\nAlice\t30\nBob\t25\n", encoding="utf-8"
        )
        readers = _get_table_readers(tmp_path)
        result = readers["read_table"]("data.tsv")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_table_returns_dataframe(self, tmp_path: Path) -> None:
        (tmp_path / "data.tsv").write_text(
            "X\tY\n1\t2\n3\t4\n", encoding="utf-8"
        )
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_table"]("data.tsv")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # FWF (fixed-width format)
    def test_read_fwf(self, tmp_path: Path) -> None:
        content = "Name     Age\nAlice    30\nBob      25\n"
        (tmp_path / "data.fwf").write_text(content, encoding="utf-8")
        readers = _get_table_readers(tmp_path)
        result = readers["read_fwf"]("data.fwf")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_fwf_returns_dataframe(self, tmp_path: Path) -> None:
        (tmp_path / "data.fwf").write_text(
            "X    Y\n1    2\n3    4\n", encoding="utf-8"
        )
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_fwf"]("data.fwf")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # Excel (.xlsx)
    def test_read_excel(self, tmp_path: Path) -> None:
        pytest.importorskip("openpyxl")
        df: DataFrame = pandas.DataFrame(
            {"Name": ["Alice", "Bob"], "Age": [30, 25]}
        )
        df.to_excel(tmp_path / "data.xlsx", index=False)
        readers = _get_table_readers(tmp_path)
        result = readers["read_excel"]("data.xlsx")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_excel_returns_dataframe(self, tmp_path: Path) -> None:
        pytest.importorskip("openpyxl")
        df_in: DataFrame = pandas.DataFrame({"X": [1, 3], "Y": [2, 4]})
        df_in.to_excel(tmp_path / "data.xlsx", index=False)
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_excel"]("data.xlsx")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # Feather
    def test_read_feather(self, tmp_path: Path) -> None:
        pytest.importorskip("pyarrow")
        df: DataFrame = pandas.DataFrame(
            {"Name": ["Alice", "Bob"], "Age": [30, 25]}
        )
        df.to_feather(tmp_path / "data.feather")
        readers = _get_table_readers(tmp_path)
        result = readers["read_feather"]("data.feather")
        assert "Name" in result
        assert "Alice" in result
        assert "Bob" in result

    def test_pd_read_feather_returns_dataframe(self, tmp_path: Path) -> None:
        pytest.importorskip("pyarrow")
        df_in: DataFrame = pandas.DataFrame({"X": [1, 3], "Y": [2, 4]})
        df_in.to_feather(tmp_path / "data.feather")
        readers = _get_table_readers(tmp_path)
        df: DataFrame = readers["pd_read_feather"]("data.feather")
        assert list(df.columns) == ["X", "Y"]
        assert len(df) == 2

    # End-to-end: CSV rendered through the Jinja2 / Markdown pipeline
    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                            },
                        },
                    }
                },
                id="end_to_end_csv",
            ),
        ],
        indirect=["md"],
    )
    def test_end_to_end_csv_via_template(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "scores.csv").write_text(
            "Player,Score\nAlice,100\nBob,80\n", encoding="utf-8"
        )
        html = soup(md.convert("{{ read_csv('scores.csv') }}"))
        table = html.select_one("table")
        assert table is not None
        headers = [th.get_text(strip=True) for th in table.select("th")]
        assert "Player" in headers
        assert "Score" in headers
        cells = [td.get_text(strip=True) for td in table.select("td")]
        assert "Alice" in cells

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "zensical.extensions.macros": {
                                "render_by_default": True,
                            },
                        },
                    }
                },
                id="end_to_end_pd_filter",
            ),
        ],
        indirect=["md"],
    )
    def test_end_to_end_pd_read_csv_with_convert_filter(
        self, md: Markdown, tmp_path: Path
    ) -> None:
        (tmp_path / "nums.csv").write_text("X,Y\n1,2\n3,4\n", encoding="utf-8")
        html = soup(
            md.convert("{{ pd_read_csv('nums.csv') | convert_to_md_table }}")
        )
        table = html.select_one("table")
        assert table is not None
        headers = [th.get_text(strip=True) for th in table.select("th")]
        assert "X" in headers
        assert "Y" in headers
