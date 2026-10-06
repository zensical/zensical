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
from jinja2 import TemplateSyntaxError, UndefinedError
from markdown import Markdown

from tests.unit.extensions.conftest import soup
from zensical.extensions.context import (
    ContextExtension,
    ContextPreprocessor,
    Page,
)
from zensical.extensions.markdownextradata import MarkdownExtraDataExtension

if TYPE_CHECKING:
    from pathlib import Path


_EXTENSION = "zensical.extensions.markdownextradata"


@pytest.mark.parametrize(
    "md",
    [
        {
            "config": {
                "site_name": "Configured site",
                "extra": {
                    "site_name": "Extra site",
                    "customer": {"name": "Alice"},
                },
                "markdown_extensions": {_EXTENSION: {}},
            }
        }
    ],
    indirect=True,
)
def test_configuration_and_extra_variables(md: Markdown) -> None:
    html = md.convert(
        "{{ site_name }}: {{ customer.name }} / "
        "{{ extra.customer.name }} / {{ docs_dir }}"
    )

    assert soup(html).get_text(strip=True) == "Extra site: Alice / Alice / docs"


@pytest.mark.parametrize(
    "md",
    [
        {
            "config": {
                "extra": {"name": "Alice"},
                "markdown_extensions": {_EXTENSION: {}},
            }
        }
    ],
    indirect=True,
)
def test_jinja_blocks_filters_and_raw_content(md: Markdown) -> None:
    # Block syntax, raw HTML, and indented code are rendered in the same pass.
    html = md.convert(
        "{% for item in ['one', 'two'] %}\n- {{ item | upper }}\n"
        "{% endfor %}\n\n"
        "<div>{{ name }}</div>\n\n"
        "    {{ name }}\n\n"
        "{% raw %}{{ literal }}{% endraw %} {# hidden #}"
    )
    document = soup(html)

    assert [item.get_text(strip=True) for item in document.find_all("li")] == [
        "ONE",
        "TWO",
    ]
    assert document.div is not None
    assert document.div.get_text() == "Alice"
    assert document.code is not None
    assert document.code.get_text().strip() == "Alice"
    assert "{{ literal }}" in document.get_text()
    assert "hidden" not in document.get_text()


@pytest.mark.parametrize(
    "md",
    [
        {
            "config": {
                "extra": {"name": "Alice"},
                "markdown_extensions": {
                    "pymdownx.superfences": {},
                    _EXTENSION: {},
                },
            }
        }
    ],
    indirect=True,
)
def test_fenced_code_is_rendered_before_highlighting(md: Markdown) -> None:
    html = md.convert("```text\n{{ name }}\n```")

    code = soup(html).code

    assert code is not None
    assert code.get_text().strip() == "Alice"


@pytest.mark.parametrize(
    "md",
    [{"config": {"markdown_extensions": {_EXTENSION: {}}}}],
    indirect=True,
)
def test_missing_variables_use_debug_undefined(md: Markdown) -> None:
    html = md.convert("{{ missing }} / {{ missing | default('fallback') }}")

    assert soup(html).get_text(strip=True) == "{{ missing }} / fallback"

    with pytest.raises(UndefinedError):
        md.reset().convert("{{ missing.nested }}")


@pytest.mark.parametrize(
    "md",
    [
        {
            "page": {
                "title": "Hello {{ name }}",
                "meta": {"title": "Metadata {{ name }}"},
            },
            "config": {
                "extra": {"name": "Alice"},
                "markdown_extensions": {_EXTENSION: {}},
            },
        }
    ],
    indirect=True,
)
def test_effective_and_metadata_titles_are_rendered(md: Markdown) -> None:
    md.convert("# Hello {{ name }}")
    context = ContextPreprocessor.from_markdown(md)

    assert context is not None
    assert context.page.title == "Hello Alice"
    assert context.page.rendered_metadata_title == "Metadata Alice"
    assert context.page.meta["title"] == "Metadata {{ name }}"


@pytest.mark.parametrize(
    "md",
    [
        {
            "page": {"meta": {"title": "Hello {{ name }}"}},
            "config": {
                "extra": {"name": "Alice"},
                "markdown_extensions": {_EXTENSION: {}},
            },
        }
    ],
    indirect=True,
)
def test_metadata_title_is_used_when_no_explicit_title_is_set(
    md: Markdown,
) -> None:
    md.convert("Body")
    context = ContextPreprocessor.from_markdown(md)

    assert context is not None
    assert context.page.title == "Hello Alice"
    assert context.page.rendered_metadata_title == "Hello Alice"


@pytest.mark.parametrize(
    "md",
    [
        {
            "config": {
                "extra": {"name": "<Alice>"},
                "markdown_extensions": {
                    _EXTENSION: {
                        "jinja_options": {
                            "variable_start_string": "[[",
                            "variable_end_string": "]]",
                            "comment_start_string": "<!--",
                            "comment_end_string": "-->",
                            "autoescape": True,
                        }
                    }
                },
            }
        }
    ],
    indirect=True,
)
def test_custom_jinja_options(md: Markdown) -> None:
    html = md.convert(
        "[[ name ]] / {{ unchanged }} <!-- hidden --> {# visible #}"
    )

    assert (
        soup(html).get_text(strip=True)
        == "<Alice> / {{ unchanged }}  {# visible #}"
    )


@pytest.mark.parametrize(
    "md",
    [
        {
            "page": {"title": "{{ name }}"},
            "config": {
                "extra": {"name": "Alice"},
                "markdown_extensions": {_EXTENSION: {"enabled": False}},
            },
        }
    ],
    indirect=True,
)
def test_disabled_extension_retains_source_and_title(md: Markdown) -> None:
    html = md.convert("{{ name }}")
    context = ContextPreprocessor.from_markdown(md)

    assert soup(html).get_text(strip=True) == "{{ name }}"
    assert context is not None
    assert context.page.title == "{{ name }}"


def _markdown(
    tmp_path: Path, extra: dict[str, Any] | None = None, **options: Any
) -> Markdown:
    config = {
        "root_dir": str(tmp_path),
        "docs_dir": "content",
        "extra": extra or {},
    }
    return Markdown(
        extensions=[
            MarkdownExtraDataExtension(**options),
            ContextExtension(
                page=Page(url="/", path="index.md"), config=config
            ),
        ]
    )


def test_default_data_directories_and_nested_namespaces(tmp_path: Path) -> None:
    # Data from the documentation directory is applied after project data.
    (tmp_path / "_data").mkdir()
    (tmp_path / "content" / "_data" / "sections").mkdir(parents=True)
    (tmp_path / "_data" / "site.yaml").write_text(
        "name: Project\n", encoding="utf-8"
    )
    (tmp_path / "content" / "_data" / "site.yml").write_text(
        "name: Documentation\n", encoding="utf-8"
    )
    (tmp_path / "content" / "_data" / "sections" / "captions.json").write_text(
        '{"foo": "Caption"}', encoding="utf-8"
    )
    (tmp_path / "content" / "_data" / "1_example.yaml").write_text(
        "bar: 42\n", encoding="utf-8"
    )
    md = _markdown(
        tmp_path, {"site": {"name": "Configuration", "replaced": True}}
    )

    html = md.convert(
        "{{ site.name }} / {{ sections.captions.foo }} / "
        "{{ extra['1_example']['bar'] }} / "
        "{{ site.replaced | default('replaced') }}"
    )

    assert (
        soup(html).get_text(strip=True)
        == "Documentation / Caption / 42 / replaced"
    )


@pytest.mark.parametrize("absolute", [False, True])
def test_custom_directories_are_relative_to_project_and_applied_in_order(
    tmp_path: Path, absolute: bool
) -> None:
    (tmp_path / "first").mkdir()
    (tmp_path / "second").mkdir()
    (tmp_path / "_data").mkdir()
    (tmp_path / "first" / "values.yaml").write_text(
        "name: First\n", encoding="utf-8"
    )
    (tmp_path / "second" / "values.json").write_text(
        '{"name": "Second"}', encoding="utf-8"
    )
    (tmp_path / "_data" / "ignored.yaml").write_text(
        "name: Default\n", encoding="utf-8"
    )
    first = str(tmp_path / "first") if absolute else "first"
    md = _markdown(tmp_path, data=f"{first}, second, missing")

    html = md.convert("{{ values.name }} / {{ ignored | default('absent') }}")

    assert soup(html).get_text(strip=True) == "Second / absent"


def test_json_overrides_yaml_with_the_same_namespace(tmp_path: Path) -> None:
    (tmp_path / "_data").mkdir()
    (tmp_path / "_data" / "value.yaml").write_text("YAML", encoding="utf-8")
    (tmp_path / "_data" / "value.yml").write_text("YML", encoding="utf-8")
    (tmp_path / "_data" / "value.json").write_text('"JSON"', encoding="utf-8")
    md = _markdown(tmp_path)

    html = md.convert("{{ value }}")

    assert soup(html).get_text(strip=True) == "JSON"


def test_yaml_safe_loader_lists_scalars_and_nulls(tmp_path: Path) -> None:
    (tmp_path / "_data").mkdir()
    (tmp_path / "_data" / "values.yaml").write_text(
        "items: [Alice, Bob]\nempty: null\npair: [1, 2]\ndate: 2026-10-06\n",
        encoding="utf-8",
    )
    md = _markdown(tmp_path)

    html = md.convert(
        "{{ values['items'] | join(', ') }} / {{ values.empty }} / "
        "{{ values.pair | join('-') }} / {{ values.date }}"
    )

    assert (
        soup(html).get_text(strip=True)
        == "Alice, Bob / None / 1-2 / 2026-10-06"
    )


@pytest.mark.parametrize(
    "tag",
    [
        "!!python/name:builtins.str ''",
        "!!python/tuple [1, 2]",
        "!!python/object/apply:builtins.str [example]",
        "!!python/object/new:builtins.str [example]",
    ],
)
def test_python_yaml_tags_are_rejected(tmp_path: Path, tag: str) -> None:
    (tmp_path / "_data").mkdir()
    (tmp_path / "_data" / "value.yaml").write_text(
        f"value: {tag}\n", encoding="utf-8"
    )
    md = _markdown(tmp_path)

    with pytest.raises(yaml.constructor.ConstructorError):
        md.convert("Body")


@pytest.mark.parametrize(
    "location",
    [
        "absolute",
        "parent",
        "sibling_prefix",
        "directory_symlink",
        "default_directory_symlink",
        "default_docs_symlink",
    ],
)
def test_data_directories_outside_project_root_are_skipped(
    tmp_path: Path, location: str
) -> None:
    project = tmp_path / "project"
    project.mkdir()
    outside = tmp_path / "project-other"
    outside.mkdir()
    # Parsing would fail if a file outside the project were read.
    (outside / "value.yaml").write_text("value: [", encoding="utf-8")

    data = None
    if location == "absolute":
        data = str(outside)
    elif location == "parent":
        data = ".."
    elif location == "sibling_prefix":
        data = "../project-other"
    elif location == "directory_symlink":
        (project / "linked").symlink_to(outside, target_is_directory=True)
        data = "linked"
    elif location == "default_directory_symlink":
        (project / "_data").symlink_to(outside, target_is_directory=True)
    else:
        (outside / "_data").mkdir()
        (outside / "_data" / "value.yaml").write_text(
            "value: [", encoding="utf-8"
        )
        (project / "content").symlink_to(outside, target_is_directory=True)
    md = _markdown(project, data=data)

    html = md.convert("{{ value | default('absent') }}")

    assert soup(html).get_text(strip=True) == "absent"


@pytest.mark.parametrize("suffix", ["yaml", "json"])
def test_data_file_symlinks_outside_project_root_are_skipped(
    tmp_path: Path, suffix: str
) -> None:
    project = tmp_path / "project"
    (project / "_data").mkdir(parents=True)
    outside = tmp_path / f"outside.{suffix}"
    outside.write_text("[", encoding="utf-8")
    (project / "_data" / f"value.{suffix}").symlink_to(outside)
    md = _markdown(project)

    html = md.convert("{{ value | default('absent') }}")

    assert soup(html).get_text(strip=True) == "absent"


@pytest.mark.parametrize("directory_symlink", [False, True])
def test_symlinks_within_project_root_remain_supported(
    tmp_path: Path, directory_symlink: bool
) -> None:
    (tmp_path / "source").mkdir()
    source = tmp_path / "source" / "value.yaml"
    source.write_text("name: Alice\n", encoding="utf-8")
    if directory_symlink:
        (tmp_path / "_data").symlink_to(source.parent, target_is_directory=True)
    else:
        (tmp_path / "_data").mkdir()
        (tmp_path / "_data" / "value.yaml").symlink_to(source)
    md = _markdown(tmp_path)

    html = md.convert("{{ value.name }}")

    assert soup(html).get_text(strip=True) == "Alice"


def test_data_changes_are_seen_when_a_markdown_instance_is_reused(
    tmp_path: Path,
) -> None:
    (tmp_path / "_data").mkdir()
    data_file = tmp_path / "_data" / "value.yaml"
    data_file.write_text("First", encoding="utf-8")
    md = _markdown(tmp_path)

    assert soup(md.convert("{{ value }}")).get_text(strip=True) == "First"

    data_file.write_text("Second", encoding="utf-8")

    assert (
        soup(md.reset().convert("{{ value }}")).get_text(strip=True) == "Second"
    )


def test_rendering_does_not_mutate_project_extra(tmp_path: Path) -> None:
    extra = {"names": ["Alice"]}
    md = _markdown(
        tmp_path, extra, jinja_options={"extensions": ["jinja2.ext.do"]}
    )

    html = md.convert("{% do names.append('Bob') %}{{ names | join(', ') }}")

    assert soup(html).get_text(strip=True) == "Alice, Bob"
    assert extra == {"names": ["Alice"]}


def test_syntax_errors_are_reported_and_propagated(
    tmp_path: Path, caplog: pytest.LogCaptureFixture
) -> None:
    md = _markdown(tmp_path)

    with pytest.raises(TemplateSyntaxError):
        md.convert("{{ 1_example.foo }}")

    assert "'extra' dictionary" in caplog.text


def test_invalid_data_is_propagated(tmp_path: Path) -> None:
    (tmp_path / "_data").mkdir()
    (tmp_path / "_data" / "invalid.yaml").write_text(
        "values: [", encoding="utf-8"
    )
    md = _markdown(tmp_path)

    with pytest.raises(yaml.YAMLError):
        md.convert("Body")


def test_extension_can_render_without_zensical_context() -> None:
    md = Markdown(extensions=[_EXTENSION])

    html = md.convert("{{ 1 + 1 }}")

    assert soup(html).get_text(strip=True) == "2"
