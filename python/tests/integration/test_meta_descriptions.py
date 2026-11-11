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

"""Native meta descriptions from paragraphs and final HTML exports."""

from __future__ import annotations

import csv
import json
from textwrap import dedent, shorten
from typing import TYPE_CHECKING, Any

import pytest
import yaml
from bs4 import BeautifulSoup

import zensical

if TYPE_CHECKING:
    from pathlib import Path


def build(
    root: Path,
    options: dict[str, Any] | None = None,
    *,
    plugin: bool = True,
    strict: bool = False,
    directory_urls: bool = True,
) -> Path:
    """Build a project while retaining caches between builds."""
    settings = {
        "site_name": "Descriptions",
        "site_url": "https://example.org/docs/",
        "site_description": "Site fallback",
        "plugins": [{"meta-descriptions": options or {}}] if plugin else [],
        "theme": {"custom_dir": "overrides"},
        "use_directory_urls": directory_urls,
    }
    config = root / "mkdocs.yml"
    config.write_text(yaml.safe_dump(settings), encoding="utf-8")

    zensical.build(str(config), {"clean": False, "strict": strict})
    return root / "site"


def project(
    root: Path,
    content: str,
    *,
    template: str = "{{ page.meta | tojson }}",
) -> None:
    """Write a page and a small template exposing its metadata."""
    (root / "docs").mkdir()
    (root / "docs/index.md").write_text(content, encoding="utf-8")
    (root / "overrides").mkdir()
    (root / "overrides/main.html").write_text(template, encoding="utf-8")


def metadata(site: Path) -> dict[str, Any]:
    """Read page metadata exposed by the test template."""
    return json.loads((site / "index.html").read_text(encoding="utf-8"))


def exported_rows(site: Path) -> list[list[str]]:
    """Read exported descriptions with CSV quoting and newlines preserved."""
    output = site / "meta-descriptions.csv"
    with output.open(newline="", encoding="utf-8") as stream:
        return list(csv.reader(stream))


@pytest.mark.parametrize(
    ("content", "options", "expected"),
    [
        pytest.param(
            "# Title\n\nFirst **intro** with [link](https://example.org).\n"
            "\nSecond.\n",
            {},
            "First intro with link.",
            id="first-paragraph",
        ),
        pytest.param(
            "# Title\n\n## Section\n\nLater paragraph.\n",
            {},
            None,
            id="subsection-excluded",
        ),
        pytest.param(
            "---\ndescription: Explicit & exact\n---\n# Title\n\nIntro.\n",
            {"trim": True, "max_length": 5},
            "Explicit & exact",
            id="explicit-description",
        ),
        pytest.param(
            "---\ndescription: ''\n---\n# Title\n\nIntro.\n",
            {},
            "Intro.",
            id="empty-description",
        ),
        pytest.param(
            "---\ndescription: null\n---\n# Title\n\nIntro.\n",
            {},
            "Intro.",
            id="null-description",
        ),
        pytest.param(
            "# Title\n\nCafé naïve 🦀 additional words.\n",
            {"trim": True, "max_length": 12},
            "Café naïve 🦀",
            id="unicode-trimming",
        ),
        pytest.param(
            "# Title\n\nLongword tail.\n",
            {"trim": True, "max_length": 4},
            "Long",
            id="long-word",
        ),
        pytest.param(
            "# Title\n\nHello-world tail.\n",
            {"trim": True, "max_length": 8},
            "Hello-",
            id="hyphenated-word",
        ),
        pytest.param(
            "# Title\n\nShort.\n",
            {"fallback_if_short": True},
            None,
            id="short-paragraph-fallback",
        ),
        pytest.param(
            "# Title\n\nOne two three.\n",
            {
                "trim": True,
                "max_length": 5,
                "min_length": 5,
                "fallback_if_short": True,
            },
            "One",
            id="fallback-before-trimming",
        ),
        pytest.param(
            "# Title\n\n&\n",
            {"min_length": 5, "fallback_if_short": True},
            None,
            id="text-length-before-fallback",
        ),
        pytest.param(
            "# Title\n\nIntro.\n",
            {"enabled": False},
            None,
            id="disabled",
        ),
        pytest.param(
            "# Title\n\nA & B with <em>HTML</em>.\n",
            {},
            "A & B with HTML.",
            id="plain-text",
        ),
        pytest.param(
            "# Title\n\n<div class='admonition note'><p>Skip</p></div>\n"
            "\nReal introduction.\n",
            {},
            "Real introduction.",
            id="admonition-excluded",
        ),
    ],
)
def test_generation_and_options_survive_cached_builds(
    tmp_path: Path,
    content: str,
    options: dict[str, Any],
    expected: str | None,
) -> None:
    project(tmp_path, content)

    # Metadata must be retained by both the fresh and cached Markdown paths.
    for _ in range(2):
        site = build(tmp_path, options)

        assert metadata(site).get("description") == expected


def test_absent_plugin_does_not_generate_a_description(tmp_path: Path) -> None:
    project(tmp_path, "# Title\n\nIntro.\n")

    site = build(tmp_path, plugin=False)

    assert "description" not in metadata(site)


def test_inherited_descriptions_are_preserved(tmp_path: Path) -> None:
    project(tmp_path, "# Title\n\nIntro.\n")
    inherited = tmp_path / "docs/.meta.yml"
    inherited.write_text("description: Inherited\n", encoding="utf-8")
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        dedent("""\
        site_name: Descriptions
        plugins: [meta, meta-descriptions]
        theme:
          custom_dir: overrides
        """),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": False, "strict": False})

    assert metadata(tmp_path / "site")["description"] == "Inherited"


@pytest.mark.parametrize("directory_urls", [True, False])
def test_export_uses_final_templates_and_published_urls(
    tmp_path: Path, directory_urls: bool
) -> None:
    # The rendered description deliberately differs from page metadata.
    template = dedent("""\
        <html><head>
        <meta name="description"
          content="Rendered, &quot;quoted&quot; &amp; final">
        </head><body>{{ page.content }}</body></html>
    """)
    project(tmp_path, "# Home\n\nIntro.\n", template=template)
    guide = tmp_path / "docs/café.md"
    guide.write_text("# Café\n\nIntro.\n", encoding="utf-8")
    options = {"export_csv": True, "quiet": True}
    suffix = "caf%C3%A9/" if directory_urls else "caf%C3%A9.html"
    home_suffix = "" if directory_urls else "index.html"

    for _ in range(2):
        site = build(tmp_path, options, directory_urls=directory_urls)

        base_url = "https://example.org/docs/"
        assert exported_rows(site) == [
            ["Page", "Meta description"],
            [base_url + suffix, 'Rendered, "quoted" & final'],
            [base_url + home_suffix, 'Rendered, "quoted" & final'],
        ]


@pytest.mark.parametrize(
    ("introduction", "description"),
    [
        ("Generated introduction.", "Generated introduction."),
        (
            "Café & <em>naïve</em> 🦀, \"quotes\", 'apostrophe', &lt;tag&gt;.",
            "Café & naïve 🦀, \"quotes\", 'apostrophe', <tag>.",
        ),
    ],
)
def test_theme_receives_generated_and_fallback_descriptions(
    tmp_path: Path, introduction: str, description: str
) -> None:
    project(tmp_path, f"# Home\n\n{introduction}\n")
    (tmp_path / "overrides/main.html").unlink()
    missing = tmp_path / "docs/missing.md"
    missing.write_text("# Missing\n\n## Section\n\nLater.\n", encoding="utf-8")

    site = build(tmp_path, {"export_csv": True, "quiet": True})

    home = BeautifulSoup((site / "index.html").read_text(), "html.parser")
    missing_page = BeautifulSoup(
        (site / "missing/index.html").read_text(), "html.parser"
    )
    home_description = home.find("meta", attrs={"name": "description"})
    assert home_description is not None
    fallback = missing_page.find("meta", attrs={"name": "description"})
    assert fallback is not None
    assert home_description["content"] == description
    assert fallback["content"] == "Site fallback"
    assert exported_rows(site)[1:] == [
        ["https://example.org/docs/", description],
        ["https://example.org/docs/missing/", "Site fallback"],
    ]


def test_checks_count_plain_text_characters(tmp_path: Path) -> None:
    # Strict bounds are checked against text lengths, excluding HTML escaping.
    description = "& \"quoted\" 'text' <tag> 🦀."
    project(tmp_path, "# Home\n\n& \"quoted\" 'text' &lt;tag&gt; 🦀.\n")
    options = {
        "enable_checks": True,
        "min_length": len(description),
        "max_length": len(description),
        "quiet": True,
    }

    # The same exact bounds are applied to fresh and cached descriptions.
    for _ in range(2):
        site = build(tmp_path, options, strict=True)

        assert metadata(site)["description"] == description


@pytest.mark.parametrize(
    ("content", "message"),
    [
        ("# Missing\n\n## Section\n", "Meta description not found"),
        ("# Short\n\nTiny.\n", "shorter than 50"),
        (
            "---\ndescription: " + "x" * 161 + "\n---\n# Long\n",
            "longer than 160",
        ),
    ],
)
def test_checks_report_warnings_and_fail_strict_cached_builds(
    tmp_path: Path,
    capfd: pytest.CaptureFixture[str],
    content: str,
    message: str,
) -> None:
    project(tmp_path, content)
    options = {"enable_checks": True, "quiet": True}

    build(tmp_path, options)

    assert message in capfd.readouterr().err

    # Strictness is checked even when the rendered Markdown is cached.
    with pytest.raises(RuntimeError, match=message):
        build(tmp_path, options, strict=True)


def test_disabled_plugin_does_not_export_or_check(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    project(tmp_path, "# Home\n\nIntro.\n")
    options = {"enabled": False, "export_csv": True, "enable_checks": True}

    site = build(tmp_path, options, strict=True)

    assert not (site / "meta-descriptions.csv").exists()
    assert "meta-descriptions:" not in capfd.readouterr().err


def test_export_reports_missing_template_meta_tags(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    project(
        tmp_path,
        "# Home\n\nIntro.\n",
        template="<html><body>No meta tag</body></html>",
    )

    site = build(tmp_path, {"export_csv": True, "quiet": True})

    assert "Couldn't find meta descriptions" in capfd.readouterr().err
    assert exported_rows(site)[1] == ["https://example.org/docs/", ""]


def test_toml_configuration_is_forwarded_to_rust(tmp_path: Path) -> None:
    project(tmp_path, "# Title\n\nFirst paragraph with more words.\n")
    config = tmp_path / "zensical.toml"
    config.write_text(
        dedent("""\
        [project]
        site_name = "Descriptions"
        [project.theme]
        custom_dir = "overrides"
        [project.plugins.meta-descriptions]
        trim = true
        max_length = 15
        """),
        encoding="utf-8",
    )

    zensical.build(str(config), {"clean": False, "strict": False})

    assert metadata(tmp_path / "site")["description"] == "First paragraph"


@pytest.mark.parametrize("quiet", [True, False])
def test_summary_counts_are_retained_on_cached_builds(
    tmp_path: Path, capfd: pytest.CaptureFixture[str], quiet: bool
) -> None:
    project(tmp_path, "# Home\n\nGenerated description.\n")
    explicit = tmp_path / "docs/explicit.md"
    explicit.write_text(
        "---\ndescription: Explicit\n---\n# Explicit\n", encoding="utf-8"
    )
    (tmp_path / "docs/missing.md").write_text("# Missing\n", encoding="utf-8")

    for _ in range(2):
        build(tmp_path, {"quiet": quiet})

        logs = capfd.readouterr().err
        if quiet:
            assert "meta-descriptions:" not in logs
        else:
            assert "2 of 3 pages, 1 using the first paragraph" in logs


@pytest.mark.parametrize("width", [3, 5, 8, 12])
def test_trimming_matches_python_textwrap(tmp_path: Path, width: int) -> None:
    paragraphs = [
        "One extraordinarilylong word.",
        "a longword tail",
        "hello-world tail",
        "foo--bar tail",
        "a-b-c-d-e tail",
        "goof-ball tail",
        "d'Artagnan said hello",
        "Café naïve 🦀 additional words",
        "An ampersand & another",
        "A quote \" and apostrophe '",
        "Three-------hyphens tail",
        "one   two\nthree",
    ]
    project(tmp_path, "# Home\n\nIntroduction.\n")
    for index, paragraph in enumerate(paragraphs):
        source = tmp_path / "docs" / f"example-{index}.md"
        source.write_text(f"# Example\n\n{paragraph}\n", encoding="utf-8")

    site = build(tmp_path, {"trim": True, "max_length": width, "quiet": True})

    # The reference result is produced by shortening the unescaped text.
    for index, paragraph in enumerate(paragraphs):
        page = site / f"example-{index}/index.html"
        description = json.loads(page.read_text())["description"]

        assert description == shorten(paragraph, width, placeholder="")
