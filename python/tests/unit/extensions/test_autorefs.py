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

from textwrap import dedent
from typing import TYPE_CHECKING, Any

import pytest

from zensical.extensions.autorefs import get_autorefs_store, reset

if TYPE_CHECKING:
    from collections.abc import Generator

    from markdown import Markdown


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _autorefs(exts: dict[str, dict[str, Any]] | None = None) -> dict[str, Any]:
    """Return md fixture params with AutorefsExtension configured."""
    return {
        "config": {
            "markdown_extensions": {
                **(exts or {}),
                "zensical.extensions.autorefs": {},
            }
        }
    }


def _autorefs_toc(*, page: dict[str, Any] | None = None) -> dict[str, Any]:
    """Return md fixture params with attr_list, toc, and AutorefsExtension."""
    param: dict[str, Any] = _autorefs({"attr_list": {}, "toc": {}})
    if page is not None:
        param["page"] = page
    return param


def _page(url: str = "page") -> dict[str, str]:
    """Return a page-override dict that sets the page URL."""
    return {"url": url, "path": f"{url}.html"}


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture(autouse=True)
def _reset_autorefs_store() -> Generator[None, None, None]:
    """Reset the global AutorefsStore around each test."""
    reset()
    yield
    reset()


# ---------------------------------------------------------------------------
# Inline processor
# ---------------------------------------------------------------------------


class TestInlineProcessor:
    """Tests for AutorefsInlineProcessor.

    The extension converts unresolved Markdown reference-style links into
    `<autoref identifier="...">` elements that the Rust side resolves later.
    """

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_autorefs(), id="default")],
        indirect=["md"],
    )
    def test_implicit_reference(self, md: Markdown) -> None:
        """`[Foo][]` produces an `<autoref>` element with identifier="Foo"."""
        result = md.convert("[Foo][]")
        assert 'identifier="Foo"' in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_autorefs(), id="default")],
        indirect=["md"],
    )
    def test_explicit_reference_with_formatted_text(self, md: Markdown) -> None:
        """`[**Foo**][Foo]` wraps the bold text inside the autoref element."""
        result = md.convert("[**Foo**][Foo]")
        assert 'identifier="Foo"' in result
        assert "<strong>Foo</strong>" in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_autorefs(), id="default")],
        indirect=["md"],
    )
    def test_implicit_backtick_reference_is_exact(self, md: Markdown) -> None:
        """``[`Foo`][]`` uses the code content as exact identifier (no slug)."""
        result = md.convert("[`Foo`][]")
        assert 'identifier="Foo"' in result
        assert "<code>Foo</code>" in result
        assert "slug=" not in result

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                _autorefs(
                    {"pymdownx.highlight": {}, "pymdownx.inlinehilite": {}}
                ),
                id="with_inlinehilite",
            )
        ],
        indirect=["md"],
    )
    def test_implicit_code_inlinehilite_plain_is_exact(
        self, md: Markdown
    ) -> None:
        """``[`pathlib.Path`][]`` with inlinehilite keep exact identifier."""
        result = md.convert("[`pathlib.Path`][]")
        assert 'identifier="pathlib.Path"' in result
        assert "slug=" not in result

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                _autorefs(
                    {
                        "pymdownx.highlight": {},
                        "pymdownx.inlinehilite": {"style_plain_text": "python"},
                    }
                ),
                id="with_inlinehilite_python_styled",
            )
        ],
        indirect=["md"],
    )
    def test_implicit_code_inlinehilite_styled_is_exact(
        self, md: Markdown
    ) -> None:
        """``[`pathlib.Path`][]`` with inlinehilite is still exact."""
        result = md.convert("[`pathlib.Path`][]")
        assert 'identifier="pathlib.Path"' in result
        assert "slug=" not in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_autorefs(), id="default")],
        indirect=["md"],
    )
    def test_reference_inside_code_not_converted(self, md: Markdown) -> None:
        """`` `[Foo][]` `` is not converted into an autoref."""
        result = md.convert("`[Foo][]`")
        assert "<autoref" not in result
        assert "<code>[Foo][]</code>" in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_autorefs(), id="default")],
        indirect=["md"],
    )
    def test_multiline_reference_uses_explicit_identifier(
        self, md: Markdown
    ) -> None:
        """References spanning two lines use explicit identifiers (no slug)."""
        result = md.convert("[Foo\nbar][foo-bar]")
        assert 'identifier="foo-bar"' in result
        assert "slug=" not in result

    @pytest.mark.parametrize(
        "md",
        [pytest.param(_autorefs(), id="default")],
        indirect=["md"],
    )
    def test_implicit_reference_with_space_is_slugified(
        self, md: Markdown
    ) -> None:
        """`[Foo bar][]` uses the text as identifier and adds a slug."""
        result = md.convert("[Foo bar][]")
        assert 'identifier="Foo bar"' in result
        assert 'slug="foo-bar"' in result

    @pytest.mark.parametrize(
        ("md", "markdown_ref", "exact_expected"),
        [
            pytest.param(_autorefs(), "[Foo][]", False, id="bare_implicit"),
            pytest.param(
                _autorefs(), "[\\`Foo][]", False, id="escaped_backtick_start"
            ),
            pytest.param(
                _autorefs(),
                "[\\`\\`Foo][]",
                False,
                id="two_escaped_backticks_start",
            ),
            pytest.param(
                _autorefs(),
                "[\\`\\`Foo\\`][]",
                False,
                id="mixed_escaped_backticks",
            ),
            pytest.param(
                _autorefs(), "[Foo\\`][]", False, id="escaped_backtick_end"
            ),
            pytest.param(
                _autorefs(),
                "[Foo\\`\\`][]",
                False,
                id="two_escaped_backticks_end",
            ),
            pytest.param(
                _autorefs(),
                "[\\`Foo\\`\\`][]",
                False,
                id="outer_escaped_backticks",
            ),
            pytest.param(
                _autorefs(),
                "[`Foo` `Bar`][]",
                False,
                id="two_separate_code_spans",
            ),
            pytest.param(
                _autorefs(), "[Foo][Foo]", True, id="explicit_identifier"
            ),
            pytest.param(_autorefs(), "[`Foo`][]", True, id="single_code_span"),
            pytest.param(
                _autorefs(),
                "[`Foo``Bar`][]",
                True,
                id="code_span_two_backticks",
            ),
            pytest.param(
                _autorefs(),
                "[`Foo```Bar`][]",
                True,
                id="code_span_three_backticks",
            ),
            pytest.param(
                _autorefs(),
                "[``Foo```Bar``][]",
                True,
                id="double_backtick_three",
            ),
            pytest.param(
                _autorefs(),
                "[``Foo`Bar``][]",
                True,
                id="double_backtick_single",
            ),
            pytest.param(
                _autorefs(),
                "[```Foo``Bar```][]",
                True,
                id="triple_backtick_double",
            ),
        ],
        indirect=["md"],
    )
    def test_mark_identifiers_as_exact(
        self, md: Markdown, markdown_ref: str, exact_expected: bool
    ) -> None:
        """Code/explicit identifiers have no slug; bare-text identifiers do."""
        output = md.convert(markdown_ref)
        if exact_expected:
            assert "slug=" not in output
        else:
            assert "slug=" in output


# ---------------------------------------------------------------------------
# Anchors tree processor
# ---------------------------------------------------------------------------


class TestAnchorsTreeprocessor:
    """Tests for AutorefsAnchorsTreeprocessor.

    The processor scans `<a id="...">` elements produced by the
    `attr_list` extension (e.g. `[](){#foo}`) and registers them as
    anchors or heading aliases in the `AutorefsStore`.
    """

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                _autorefs_toc(page=_page()),
                id="with_toc_attr_list",
            )
        ],
        indirect=["md"],
    )
    def test_register_anchors_and_aliases(self, md: Markdown) -> None:
        """Anchors preceding a heading become aliases for that heading."""
        md.convert(
            dedent("""\
                [](){#foo}
                ## Heading foo

                Paragraph 1.

                [](){#bar}
                Paragraph 2.

                [](){#alias1}
                [](){#alias2}
                ## Heading bar

                [](){#alias3}
                Text.
                [](){#alias4}
                ## Heading baz

                [](){#alias5}
                [](){#alias6}
                Decoy.
                ## Heading more1

                [](){#alias7}
                [decoy](){#alias8}
                [](){#alias9}
                ## Heading more2 {#heading-custom2}

                [](){#aliasSame}
                ## Same heading 1
                [](){#aliasSame}
                ## Same heading 2

                [](){#alias10}
            """),
        )
        store = get_autorefs_store()
        assert store._primary_url_map == {
            "foo": ["page#heading-foo"],
            "heading-foo": ["page#heading-foo"],
            "bar": ["page#bar"],
            "heading-bar": ["page#heading-bar"],
            "alias1": ["page#heading-bar"],
            "alias2": ["page#heading-bar"],
            "alias3": ["page#alias3"],
            "alias4": ["page#heading-baz"],
            "heading-baz": ["page#heading-baz"],
            "alias5": ["page#alias5"],
            "alias6": ["page#alias6"],
            "heading-more1": ["page#heading-more1"],
            "alias7": ["page#alias7"],
            "alias8": ["page#alias8"],
            "alias9": ["page#heading-custom2"],
            "heading-custom2": ["page#heading-custom2"],
            "alias10": ["page#alias10"],
            "aliasSame": ["page#same-heading-1", "page#same-heading-2"],
            "same-heading-1": ["page#same-heading-1"],
            "same-heading-2": ["page#same-heading-2"],
        }

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                {
                    "config": {
                        "markdown_extensions": {
                            "attr_list": {},
                            "toc": {},
                            "admonition": {},
                            "zensical.extensions.autorefs": {},
                        }
                    },
                    "page": _page(),
                },
                id="with_admonition",
            )
        ],
        indirect=["md"],
    )
    def test_register_anchors_inside_admonition(self, md: Markdown) -> None:
        """Anchors inside a nested block element are registered separately."""
        md.convert(
            dedent("""\
                [](){#alias1}
                !!! note
                    ## Heading foo

                    [](){#alias2}
                    ## Heading bar

                    [](){#alias3}
                ## Heading baz
            """),
        )
        store = get_autorefs_store()
        assert store._primary_url_map == {
            "heading-foo": ["page#heading-foo"],
            "heading-bar": ["page#heading-bar"],
            "heading-baz": ["page#heading-baz"],
            "alias1": ["page#alias1"],
            "alias2": ["page#heading-bar"],
            "alias3": ["page#alias3"],
        }


# ---------------------------------------------------------------------------
# Headings tree processor
# ---------------------------------------------------------------------------


class TestHeadingsTreeprocessor:
    """Tests for AutorefsHeadingsTreeprocessor.

    The processor scans headings that received an `id` attribute from the
    `toc` extension and registers them in the `AutorefsStore`.
    """

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                _autorefs_toc(page=_page()),
                id="with_toc",
            )
        ],
        indirect=["md"],
    )
    def test_register_heading(self, md: Markdown) -> None:
        """A single heading is registered under its toc-generated slug."""
        md.convert("## Foo")
        store = get_autorefs_store()
        assert "foo" in store._primary_url_map
        assert store._primary_url_map["foo"] == ["page#foo"]

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                _autorefs_toc(page=_page()),
                id="with_toc",
            )
        ],
        indirect=["md"],
    )
    def test_register_multiple_headings_at_different_levels(
        self, md: Markdown
    ) -> None:
        """All headings across all levels are individually registered."""
        md.convert("# Top\n\n## Middle\n\n### Bottom")
        store = get_autorefs_store()
        assert "top" in store._primary_url_map
        assert "middle" in store._primary_url_map
        assert "bottom" in store._primary_url_map

    @pytest.mark.parametrize(
        "md",
        [
            pytest.param(
                _autorefs_toc(page=_page()),
                id="with_toc",
            )
        ],
        indirect=["md"],
    )
    def test_heading_not_registered_without_id(self, md: Markdown) -> None:
        """Headings without an `id` attribute (no toc) are not registered."""
        # Use a raw HTML heading that has no id attribute.
        md.convert("<h2>No ID heading</h2>")
        store = get_autorefs_store()
        assert "no-id-heading" not in store._primary_url_map
        assert store._primary_url_map == {}
