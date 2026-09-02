# Copyright (c) 2025-2026 Zensical and contributors

# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO

"""Integration tests for native MkDocs Material tags compatibility."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, Any

import pytest

import zensical

if TYPE_CHECKING:
    from pathlib import Path


_BUILD_OPTIONS: dict[str, Any] = {"clean": False, "strict": False}


def _write_project(root: Path, *, plugin: str = "") -> Path:
    """Create a small hierarchical tags project with observable templates."""
    docs = root / "docs"
    guide = docs / "guide"
    overrides = root / "overrides"
    guide.mkdir(parents=True)
    overrides.mkdir()
    (docs / "index.md").write_text(
        """\
# Catalog

<!-- material/tags -->

## After

Trailing content.

<div data-search-exclude>Secret listing-page text.</div>
""",
        encoding="utf-8",
    )
    (guide / "rust.md").write_text(
        """\
---
title: Rust page
tags:
  - Guide/Rust
  - Public
---
# Rust
""",
        encoding="utf-8",
    )
    (guide / "python.md").write_text(
        """\
---
title: Python page
tags:
  - Guide/Python
---
# Python
""",
        encoding="utf-8",
    )
    (overrides / "main.html").write_text(
        """\
{{ page.content }}
<tags>{% for tag in tags %}
<tag name="{{ tag.name }}" url="{{ tag.url or '' }}"
     hidden="{{ tag.hidden }}">{% for link in tag.links %}
<link title="{{ link.title }}" url="{{ link.url }}" />{% endfor %}
</tag>{% endfor %}
</tags>
<toc>{{ page.toc | tojson }}</toc>
""",
        encoding="utf-8",
    )
    config = root / "mkdocs.yml"
    config.write_text(
        f"""\
site_name: Tags
theme:
  name: material
  custom_dir: overrides
plugins:
  - search
  - material/tags:
      tags_hierarchy: true
      export: true
      export_file: ignored-tags.json
{plugin}
""",
        encoding="utf-8",
    )
    return config


def test_builds_listings_references_toc_and_search_without_export(
    tmp_path: Path,
) -> None:
    """One revision-complete listing drives all derived page facts."""
    config = _write_project(tmp_path)

    zensical.build(str(config), _BUILD_OPTIONS)

    listing = (tmp_path / "site" / "index.html").read_text()
    rust = (tmp_path / "site" / "guide" / "rust" / "index.html").read_text()
    search = json.loads((tmp_path / "site" / "search.json").read_text())

    assert "material/tags" not in listing
    assert "zensical:tags" not in listing
    assert '<h2 id="tag:guide">' in listing
    assert '<h3 id="tag:guide/rust">' in listing
    assert "Rust page" in listing
    assert "data-search-exclude" not in listing
    assert listing.index('id="tag:guide"') < listing.index('id="after"')
    assert 'name="Guide/Rust" url=".#tag:guide/rust"' in rust
    assert 'title="Catalog" url=".#tag:guide/rust"' in rust
    assert any("Rust page" in item["text"] for item in search["items"])
    assert not any(
        "Secret listing-page text" in item["text"] for item in search["items"]
    )
    assert not (tmp_path / "site" / "ignored-tags.json").exists()
    assert not (tmp_path / "site" / "tags.json").exists()


def test_inherits_tags_from_meta_file(tmp_path: Path) -> None:
    """Tags supplied by Material meta participate in page tag mappings."""
    _write_project(tmp_path)
    config = tmp_path / "zensical.toml"
    config.write_text(
        """\
[project]
site_name = "Tags"

[project.theme]
custom_dir = "overrides"

[project.plugins.search]

[project.plugins.tags]
tags_hierarchy = true

[project.plugins.meta]
""",
        encoding="utf-8",
    )
    guide = tmp_path / "docs" / "guide"
    (guide / ".meta.yml").write_text("tags: [Inherited]\n", encoding="utf-8")
    (guide / "rust.md").write_text(
        "---\ntitle: Rust page\n---\n# Rust\n", encoding="utf-8"
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    output = (tmp_path / "site" / "guide" / "rust" / "index.html").read_text()
    assert '<tag name="Inherited"' in output


def test_inline_selection_and_literal_html_discovery(tmp_path: Path) -> None:
    """Inline filters apply while escaped examples remain ordinary content."""
    config = _write_project(tmp_path)
    index = tmp_path / "docs" / "index.md"
    index.write_text(
        """\
# Catalog

<!-- material/tags { include: [Public], toc: false } -->

```html
<!-- material/tags -->
```
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    output = (tmp_path / "site" / "index.html").read_text()
    assert "Rust page" in output
    assert "Python page" not in output
    assert "&lt;!-- material/tags --&gt;" in output
    assert '"id":"tag:public"' not in output


def test_invalid_tag_metadata_reports_the_page(tmp_path: Path) -> None:
    """Mapping validation fails with the affected page in the diagnostic."""
    config = _write_project(tmp_path)
    (tmp_path / "docs" / "guide" / "rust.md").write_text(
        "---\ntags: scalar\n---\n# Rust\n",
        encoding="utf-8",
    )

    with pytest.raises(RuntimeError, match=r"guide/rust\.md"):
        zensical.build(str(config), _BUILD_OPTIONS)


def test_named_layout_uses_project_fragment_overrides(tmp_path: Path) -> None:
    """Named listing configuration retains Material's fragment contract."""
    config = _write_project(
        tmp_path,
        plugin="""\
      listings_map:
        cards:
          include: [Public]
          layout: cards
          toc: false
""",
    )
    (tmp_path / "docs" / "index.md").write_text(
        "# Catalog\n\n<!-- material/tags cards -->\n",
        encoding="utf-8",
    )
    fragments = tmp_path / "overrides" / "fragments" / "tags" / "cards"
    fragments.mkdir(parents=True)
    (fragments / "tag.html").write_text(
        '<x-tag data-name="{{ tag.name }}">{{ tag.name }}</x-tag>',
        encoding="utf-8",
    )
    (fragments / "listing.html").write_text(
        """\
<x-listing name="{{ listing.tag.name }}">{{ listing.content }}
{% for mapping in listing.mappings %}
<x-page href="{{ mapping.item.url }}">{{ mapping.item.title }}</x-page>
{% endfor %}</x-listing>
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    output = (tmp_path / "site" / "index.html").read_text()
    assert '<x-listing name="Public">' in output
    assert '<x-tag data-name="Public">Public</x-tag>' in output
    assert '<x-page href="guide/rust/">Rust page</x-page>' in output
    assert "Python page" not in output
    assert '"id":"tag:public"' not in output


def test_custom_templates_receive_complete_tag_and_mapping_objects(
    tmp_path: Path,
) -> None:
    """Fragments receive parent chains, mapping tags, and complete pages."""
    config = _write_project(tmp_path)
    (tmp_path / "docs" / "guide" / "rust.md").write_text(
        """\
---
title: Rust page
audience: developers
tags:
  - Guide/Rust
  - Public
---
# Rust
""",
        encoding="utf-8",
    )
    (tmp_path / "overrides" / "main.html").write_text(
        """\
{{ page.content }}
{% for tag in tags %}<ref name="{{ tag.name }}"
  parent="{{ tag.parent.name if tag.parent else '' }}" />{% endfor %}
""",
        encoding="utf-8",
    )
    fragments = tmp_path / "overrides" / "fragments" / "tags" / "default"
    fragments.mkdir(parents=True)
    (fragments / "tag.html").write_text(
        '<tag name="{{ tag.name }}" '
        'parent="{{ tag.parent.name if tag.parent else "" }}" />',
        encoding="utf-8",
    )
    (fragments / "listing.html").write_text(
        """\
{% macro render(tree) %}
<listing name="{{ tree.tag.name }}">
{{ tree.content }}
{% for mapping in tree.mappings %}<mapping
  title="{{ mapping.item.title }}"
  audience="{{ mapping.item.meta.audience }}"
  tags="{% for tag in mapping.tags %}{{ tag.name }};{% endfor %}" />
{% endfor %}
{% for child in tree %}{{ render(child) }}{% endfor %}
</listing>
{% endmacro %}
{{ render(listing) }}
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    listing = (tmp_path / "site" / "index.html").read_text()
    page = (tmp_path / "site" / "guide" / "rust" / "index.html").read_text()
    assert '<tag name="Guide/Rust" parent="Guide" />' in listing
    assert 'audience="developers"' in listing
    assert 'tags="Guide/Rust;Public;"' in listing
    assert '<ref name="Guide/Rust"\n  parent="Guide" />' in page


def test_later_nonempty_instance_can_populate_shared_variable(
    tmp_path: Path,
) -> None:
    """An empty earlier mapping does not claim shared template context."""
    docs = tmp_path / "docs"
    overrides = tmp_path / "overrides"
    docs.mkdir()
    overrides.mkdir()
    (docs / "index.md").write_text(
        "---\nsecond: [Visible]\n---\n# Home\n", encoding="utf-8"
    )
    (overrides / "main.html").write_text(
        "{% for tag in tags %}{{ tag.name }}{% endfor %}", encoding="utf-8"
    )
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Shared variable
theme:
  name: material
  custom_dir: overrides
plugins:
  - material/tags:
      tags_name_property: first
      tags_name_variable: tags
  - material/tags:
      tags_name_property: second
      tags_name_variable: tags
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    assert (tmp_path / "site" / "index.html").read_text() == "Visible"


def test_deprecated_tags_marker_is_ordinary_markdown(tmp_path: Path) -> None:
    """Only the native HTML comment directive creates a listing."""
    config = _write_project(tmp_path)
    (tmp_path / "docs" / "index.md").write_text(
        """\
# Catalog

[TAGS]
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    output = (tmp_path / "site" / "index.html").read_text()
    assert "[TAGS]" in output
    assert "Guide/Rust" not in output
    assert not (tmp_path / "site" / "ignored-tags.json").exists()


def test_top_level_listing_keeps_table_of_contents_order(
    tmp_path: Path,
) -> None:
    """A directive before the first heading remains first in the root TOC."""
    config = _write_project(tmp_path)
    (tmp_path / "docs" / "index.md").write_text(
        "<!-- material/tags -->\n\n# Catalog\n",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    output = (tmp_path / "site" / "index.html").read_text()
    toc = output.partition("<toc>")[2].partition("</toc>")[0]
    assert toc.index('"id":"tag:guide"') < toc.index('"id":"catalog"')


def test_tags_false_suppresses_template_references_but_keeps_listings(
    tmp_path: Path,
) -> None:
    """Mapping facts remain useful when page-level tag context is disabled."""
    config = _write_project(tmp_path, plugin="      tags: false\n")

    zensical.build(str(config), _BUILD_OPTIONS)

    listing = (tmp_path / "site" / "index.html").read_text()
    rust = (tmp_path / "site" / "guide" / "rust" / "index.html").read_text()
    assert "Rust page" in listing
    assert "Guide/Rust" not in rust


def test_non_clean_rebuild_retracts_listing_nodes_and_tag_links(
    tmp_path: Path,
) -> None:
    """Revision-complete selections retract stale derived facts."""
    config = _write_project(tmp_path)
    rust_source = tmp_path / "docs" / "guide" / "rust.md"
    listing_source = tmp_path / "docs" / "index.md"
    zensical.build(str(config), _BUILD_OPTIONS)

    rust_source.write_text(
        "---\ntitle: Rust page\ntags: [Public]\n---\n# Rust\n",
        encoding="utf-8",
    )
    zensical.build(str(config), _BUILD_OPTIONS)
    listing = (tmp_path / "site" / "index.html").read_text()
    rust = (tmp_path / "site" / "guide" / "rust" / "index.html").read_text()
    assert 'id="tag:guide/rust"' not in listing
    assert "Guide/Rust" not in rust
    assert 'name="Public" url=".#tag:public"' in rust

    listing_source.write_text("# Catalog\n", encoding="utf-8")
    zensical.build(str(config), _BUILD_OPTIONS)
    listing = (tmp_path / "site" / "index.html").read_text()
    rust = (tmp_path / "site" / "guide" / "rust" / "index.html").read_text()
    assert 'id="tag:public"' not in listing
    assert 'name="Public" url=""' in rust


def test_multiple_instances_keep_filters_properties_and_directives_isolated(
    tmp_path: Path,
) -> None:
    """Ordered instances retain independent mappings and listing ownership."""
    docs = tmp_path / "docs"
    private = docs / "private"
    overrides = tmp_path / "overrides"
    private.mkdir(parents=True)
    overrides.mkdir()
    (docs / "index.md").write_text(
        "# Public\n\n<!-- public/tags -->\n", encoding="utf-8"
    )
    (docs / "page.md").write_text(
        "---\ntags: [Public]\n---\n# Page\n", encoding="utf-8"
    )
    (private / "index.md").write_text(
        "# Private\n\n<!-- private/tags -->\n", encoding="utf-8"
    )
    (private / "secret.md").write_text(
        "---\ntitle: Secret\nlabels: [Internal]\n---\n# Secret\n",
        encoding="utf-8",
    )
    (overrides / "main.html").write_text(
        """\
{{ page.content }}
<tags>{% for tag in tags %}{{ tag.name }}={{ tag.url or '' }};
{% endfor %}</tags>
<labels>{% for tag in labels %}{{ tag.name }}={{ tag.url or '' }};
{% endfor %}</labels>
""",
        encoding="utf-8",
    )
    config = tmp_path / "mkdocs.yml"
    config.write_text(
        """\
site_name: Multiple tags
theme:
  name: material
  custom_dir: overrides
plugins:
  - material/tags:
      listings_directive: public/tags
      filters:
        exclude: [private/**]
  - material/tags:
      listings_directive: private/tags
      filters:
        include: [private/**]
      tags_name_property: labels
      tags_name_variable: labels
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    public = (tmp_path / "site" / "index.html").read_text()
    private_listing = (tmp_path / "site" / "private" / "index.html").read_text()
    secret = (
        tmp_path / "site" / "private" / "secret" / "index.html"
    ).read_text()
    assert "Page" in public
    assert "Secret" not in public
    assert "Secret" in private_listing
    assert "Page" not in private_listing
    assert "Internal=private/#tag:internal" in secret


def test_leading_hierarchy_separator_keeps_identity_and_listing_link(
    tmp_path: Path,
) -> None:
    """Empty root components do not lose hierarchy separators."""
    config = _write_project(tmp_path)
    (tmp_path / "docs" / "guide" / "rust.md").write_text(
        "---\ntitle: Leading\ntags: [/Child]\n---\n# Leading\n",
        encoding="utf-8",
    )
    (tmp_path / "docs" / "guide" / "python.md").write_text(
        "# Untagged\n", encoding="utf-8"
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    listing = (tmp_path / "site" / "index.html").read_text()
    page = (tmp_path / "site" / "guide" / "rust" / "index.html").read_text()
    assert '<h3 id="tag:/child">' in listing
    assert 'name="/Child" url=".#tag:/child"' in page


@pytest.mark.parametrize(
    ("option", "replacement"),
    [
        ("tags_compare", "tags_sort_by"),
        ("tags_compare_reverse", "tags_sort_reverse"),
        ("tags_pages_compare", "listings_sort_by"),
        ("tags_pages_compare_reverse", "listings_sort_reverse"),
        ("tags_file", "material/tags"),
        ("tags_extra_files", "material/tags"),
    ],
)
def test_rust_rejects_deprecated_tags_options(
    tmp_path: Path, option: str, replacement: str
) -> None:
    """The native configuration boundary owns deprecated-option errors."""
    config = _write_project(tmp_path, plugin=f"      {option}: value\n")

    with pytest.raises(ValueError, match=option) as error:
        zensical.build(str(config), _BUILD_OPTIONS)

    assert replacement in str(error.value)


@pytest.mark.parametrize("option", ["export_only", "tags_hierachy"])
def test_rust_rejects_unsupported_tags_options(
    tmp_path: Path, option: str
) -> None:
    """Unsupported behavior and misspellings cannot silently disappear."""
    config = _write_project(tmp_path, plugin=f"      {option}: true\n")

    with pytest.raises(ValueError, match=option):
        zensical.build(str(config), _BUILD_OPTIONS)


def test_scalar_configuration_and_metadata_match_python_names(
    tmp_path: Path,
) -> None:
    """Booleans and integral floats retain Python's public spelling."""
    config = _write_project(
        tmp_path,
        plugin="      tags_allowed: [true, 1, 1.0, 1.5]\n",
    )
    (tmp_path / "docs" / "guide" / "rust.md").write_text(
        "---\ntitle: Scalars\ntags: [true, 1, 1.0, 1.5]\n---\n# Scalars\n",
        encoding="utf-8",
    )
    (tmp_path / "docs" / "guide" / "python.md").write_text(
        "# Untagged\n", encoding="utf-8"
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    listing = (tmp_path / "site" / "index.html").read_text()
    page = (tmp_path / "site" / "guide" / "rust" / "index.html").read_text()
    for name, slug in [
        ("True", "true"),
        ("1", "1"),
        ("1.0", "10"),
        ("1.5", "15"),
    ]:
        assert f'id="tag:{slug}"' in listing
        assert f'name="{name}"' in page


def test_inline_listing_filters_use_python_scalar_names(tmp_path: Path) -> None:
    """Inline YAML applies the same scalar domain as plugin configuration."""
    config = _write_project(tmp_path)
    (tmp_path / "docs" / "index.md").write_text(
        "# Catalog\n\n<!-- material/tags { include: [true, 1.0] } -->\n",
        encoding="utf-8",
    )
    (tmp_path / "docs" / "guide" / "rust.md").write_text(
        "---\ntitle: Scalars\ntags: [true, 1.0, 1.5]\n---\n# Scalars\n",
        encoding="utf-8",
    )
    (tmp_path / "docs" / "guide" / "python.md").write_text(
        "# Untagged\n", encoding="utf-8"
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    listing = (tmp_path / "site" / "index.html").read_text()
    assert 'id="tag:true"' in listing
    assert 'id="tag:10"' in listing
    assert 'id="tag:15"' not in listing


def test_declarative_slug_callable_is_lowered_and_run_in_rust(
    tmp_path: Path,
) -> None:
    """Supported callable descriptors select native slug implementations."""
    config = _write_project(
        tmp_path,
        plugin="""\
      tags_slugify:
        object: pymdownx.slugs.slugify
        kwds:
          case: fold
""",
    )
    (tmp_path / "docs" / "guide" / "rust.md").write_text(
        "---\ntitle: Folded\ntags: [Straße]\n---\n# Folded\n",
        encoding="utf-8",
    )
    (tmp_path / "docs" / "guide" / "python.md").write_text(
        "# Untagged\n", encoding="utf-8"
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    listing = (tmp_path / "site" / "index.html").read_text()
    assert 'id="tag:strasse"' in listing


def test_python_slug_callable_is_identified_but_not_invoked(
    tmp_path: Path,
) -> None:
    """Python callable compatibility ends at native strategy selection."""
    config = _write_project(
        tmp_path,
        plugin="""\
      tags_slugify: !!python/object/apply:pymdownx.slugs.slugify
        kwds:
          case: fold
""",
    )
    (tmp_path / "docs" / "guide" / "rust.md").write_text(
        "---\ntitle: Folded\ntags: [Straße]\n---\n# Folded\n",
        encoding="utf-8",
    )
    (tmp_path / "docs" / "guide" / "python.md").write_text(
        "# Untagged\n", encoding="utf-8"
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    listing = (tmp_path / "site" / "index.html").read_text()
    assert 'id="tag:strasse"' in listing


def test_listing_marker_cannot_claim_a_user_comment(tmp_path: Path) -> None:
    """Generated marker allocation avoids existing page HTML."""
    config = _write_project(tmp_path)
    (tmp_path / "docs" / "index.md").write_text(
        """\
# Catalog

<!-- zensical:tags:0:0:0 -->

<!-- material/tags -->
""",
        encoding="utf-8",
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    output = (tmp_path / "site" / "index.html").read_text()
    assert output.count('id="tag:public"') == 1
    assert "<!-- zensical:tags:0:0:0 -->" in output


def test_nested_listing_fragments_receive_relative_base_url(
    tmp_path: Path,
) -> None:
    """Fragment context uses the listing owner's page depth."""
    config = _write_project(tmp_path)
    (tmp_path / "docs" / "guide" / "index.md").write_text(
        "# Nested\n\n<!-- material/tags -->\n", encoding="utf-8"
    )
    fragments = tmp_path / "overrides" / "fragments" / "tags" / "default"
    fragments.mkdir(parents=True)
    (fragments / "tag.html").write_text(
        "<tag-base>{{ base_url }}</tag-base>", encoding="utf-8"
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    output = (tmp_path / "site" / "guide" / "index.html").read_text()
    assert "<tag-base>..</tag-base>" in output


def test_listing_after_level_six_heading_never_emits_level_seven(
    tmp_path: Path,
) -> None:
    """A level-six predecessor falls back to its eligible ancestor."""
    config = _write_project(tmp_path)
    headings = "\n\n".join(
        f"{'#' * level} Level {level}" for level in range(1, 7)
    )
    (tmp_path / "docs" / "index.md").write_text(
        f"{headings}\n\n<!-- material/tags -->\n", encoding="utf-8"
    )

    zensical.build(str(config), _BUILD_OPTIONS)

    output = (tmp_path / "site" / "index.html").read_text()
    assert "<h7" not in output
    assert '<h6 id="tag:public">' in output
