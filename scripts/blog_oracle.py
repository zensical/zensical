#!/usr/bin/env python

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

"""Build and compare semantic manifests for Material blog fixtures."""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import tempfile
from enum import Enum
from pathlib import Path
from typing import TYPE_CHECKING, Any
from urllib.parse import urljoin, urlparse

from bs4 import BeautifulSoup, Tag

if TYPE_CHECKING:
    from collections.abc import Iterable


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "python" / "tests" / "fixtures" / "blog"


def parse_args() -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--mkdocs",
        type=Path,
        required=True,
        help="path to the pinned Material environment's mkdocs executable",
    )
    parser.add_argument(
        "--zensical",
        type=Path,
        help="optional path to a Zensical executable to compare",
    )
    parser.add_argument(
        "--fixture",
        action="append",
        dest="fixtures",
        help="fixture name to run; may be repeated (default: all)",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="replace checked-in Material manifests",
    )
    return parser.parse_args()


class Engine(Enum):
    """Supported fixture builders."""

    MKDOCS = "MkDocs"
    ZENSICAL = "Zensical"


def _url(base: str, value: str | None) -> str | None:
    """Normalize an internal link while retaining external URLs."""
    if value is None:
        return None
    absolute = urljoin(base, value)
    parsed = urlparse(absolute)
    if parsed.netloc == "example.test":
        result = parsed.path
        if parsed.query:
            result += f"?{parsed.query}"
        if parsed.fragment:
            result += f"#{parsed.fragment}"
        return result
    return absolute


def _text(element: Tag | None) -> str | None:
    """Normalize the visible text of an element."""
    if element is None:
        return None
    return " ".join(element.stripped_strings)


def _fragment(
    element: Tag | None, base: str, *, normalize_urls: bool
) -> str | None:
    """Normalize a selected HTML fragment for semantic comparisons."""
    if element is None:
        return None
    clone = BeautifulSoup(str(element), "lxml").find(element.name)
    if clone is None:
        raise ValueError("selected HTML fragment could not be cloned")
    for value in clone.find_all(string=True):
        value.replace_with(re.sub(r"\s+", " ", str(value)))
    if normalize_urls:
        for node in clone.select("a[href], img[src]"):
            attribute = "href" if node.name == "a" else "src"
            value = node.get(attribute)
            if isinstance(value, str):
                node[attribute] = _url(base, value) or value
    return re.sub(r">\s+<", "><", str(clone)).strip()


def _link(element: Tag, base: str) -> dict[str, Any]:
    """Describe one rendered link."""
    return {
        "title": _text(element),
        "url": _url(base, element.get("href")),
    }


def _nav_items(container: Tag, base: str) -> list[dict[str, Any]]:
    """Extract one navigation level without depending on page objects."""
    root = container.find("ul", recursive=False)
    if root is None:
        return []
    items: list[dict[str, Any]] = []
    for entry in root.find_all("li", recursive=False):
        link = entry.find("a", recursive=False)
        container = entry.find("div", recursive=False)
        if link is None and container is not None:
            link = container.find("a", recursive=False)
        label = entry.find("label", recursive=False)
        title = _text(link or label)
        if not title:
            continue
        item: dict[str, Any] = {"title": title}
        if link is not None:
            item["url"] = _url(base, link.get("href"))
        child = entry.find("nav", recursive=False)
        if child is not None:
            children = _nav_items(child, base)
            if children:
                item["children"] = children
        items.append(item)
    return items


def _active_ancestors(nav: Tag | None) -> list[str]:
    """Extract visible active navigation ancestors in tree order."""
    if nav is None:
        return []
    ancestors: list[str] = []
    for entry in nav.select("li.md-nav__item--active"):
        label = entry.find(["a", "label"], recursive=False)
        title = _text(label)
        if title and title not in ancestors:
            ancestors.append(title)
    return ancestors


def _posts(
    soup: BeautifulSoup, base: str, *, engine: Engine
) -> list[dict[str, Any]]:
    """Extract ordered blog-view memberships and excerpt behavior."""
    posts: list[dict[str, Any]] = []
    for article in soup.select("article.md-post--excerpt"):
        content = article.select_one(".md-post__content")
        heading = content.find(["h1", "h2"]) if content else None
        heading_link = heading.find("a") if heading else None
        time = article.find("time")
        categories = [
            _link(link, base)
            for link in article.select(".md-post__meta a.md-meta__link")
        ]
        authors = [
            image.get("alt")
            for image in article.select(".md-post__authors img[alt]")
        ]
        action = article.select_one(".md-post__action a[href]")
        posts.append(
            {
                "title": _text(heading),
                "url": _url(
                    base,
                    heading_link.get("href") if heading_link else None,
                ),
                "date": time.get("datetime") if time else None,
                "authors": authors,
                "categories": categories,
                "pinned": article.select_one(".md-pin") is not None,
                "continue": _url(base, action.get("href")) if action else None,
                "content": _fragment(
                    content,
                    base,
                    normalize_urls=engine is Engine.ZENSICAL,
                ),
            }
        )
    return posts


def _pagination(soup: BeautifulSoup, base: str) -> dict[str, Any] | None:
    """Extract page number and pager links."""
    pagination = soup.select_one(".md-pagination")
    if pagination is None:
        return None
    current = pagination.select_one(".md-pagination__current")
    return {
        "current": int(_text(current) or "1"),
        "links": [
            _link(link, base)
            for link in pagination.select("a.md-pagination__link")
        ],
    }


def _page(path: Path, *, engine: Engine) -> dict[str, Any]:
    """Extract the stable, user-visible facts from one generated page."""
    soup = BeautifulSoup(path.read_text(encoding="utf-8"), "lxml")
    canonical = soup.select_one('link[rel="canonical"]')
    canonical_url = canonical.get("href") if canonical else None
    base = str(canonical_url or "https://example.test/")
    primary_nav = soup.select_one("nav.md-nav--primary")
    main = soup.select_one("article.md-content__inner")
    relations = {}
    for name in ("prev", "next"):
        relation = soup.select_one(f'head link[rel="{name}"]')
        relations[name] = _url(base, relation.get("href")) if relation else None
    headings = (
        [
            {
                "level": int(heading.name[1]),
                "id": heading.get("id"),
                "title": (
                    _heading_text(heading)
                    if engine is Engine.ZENSICAL
                    else _text(heading)
                ),
            }
            for heading in main.select("h1, h2, h3, h4, h5, h6")
        ]
        if main
        else []
    )
    links = (
        [
            _link(link, base)
            for link in main.select("a[href]")
            if "headerlink" not in link.get("class", [])
        ]
        if main
        else []
    )
    return {
        "document_title": _text(soup.title),
        "canonical": _url(base, str(canonical_url)) if canonical_url else None,
        "relations": relations,
        "active_ancestors": _active_ancestors(primary_nav),
        "navigation": _nav_items(primary_nav, base) if primary_nav else [],
        "headings": headings,
        "links": links,
        "posts": _posts(soup, base, engine=engine),
        "pagination": _pagination(soup, base),
    }


def _heading_text(heading: Tag) -> str | None:
    """Return visible heading text without permalink controls."""
    clone = BeautifulSoup(str(heading), "lxml").find(heading.name)
    if clone is None:
        return None
    for permalink in clone.select(".headerlink"):
        permalink.decompose()
    return _text(clone)


def extract(site: Path, *, engine: Engine) -> dict[str, Any]:
    """Create a deterministic manifest for a built fixture."""
    pages = {
        path.relative_to(site).as_posix(): _page(path, engine=engine)
        for path in sorted(site.rglob("*.html"))
        if path.name != "404.html"
    }
    root = pages.get("index.html") or next(iter(pages.values()), {})
    navigation = root.get("navigation", [])
    for page in pages.values():
        page.pop("navigation")
    outputs = [
        path.relative_to(site).as_posix()
        for path in sorted(site.rglob("*"))
        if path.is_file()
        and (
            path.suffix == ".html"
            or not path.relative_to(site).as_posix().startswith("assets/")
        )
        and path.name not in {"sitemap.xml", "sitemap.xml.gz"}
        and not (
            engine is Engine.ZENSICAL
            and (
                path.name
                in {
                    "__init__.py",
                    "mkdocs_theme.yml",
                    "objects.inv",
                    "search.json",
                }
                or "__pycache__" in path.parts
            )
        )
    ]
    return {"outputs": outputs, "navigation": navigation, "pages": pages}


def _fixture_names(selected: Iterable[str] | None) -> list[str]:
    """Resolve and validate the requested fixtures."""
    available = sorted(
        path.name
        for path in FIXTURES.iterdir()
        if path.is_dir() and (path / "mkdocs.yml").is_file()
    )
    names = list(selected or available)
    unknown = sorted(set(names) - set(available))
    if unknown:
        raise ValueError(f"unknown blog fixtures: {', '.join(unknown)}")
    return names


def build(
    executable: Path,
    fixture: Path,
    destination: Path,
    *,
    engine: Engine,
) -> None:
    """Build one fixture with the supplied reference environment."""
    config = fixture / "mkdocs.yml"
    if engine is Engine.ZENSICAL:
        source = config.read_text(encoding="utf-8")
        source = re.sub(r"(?m)^site_dir:.*\n", "", source)
        source += f"\nsite_dir: {destination.relative_to(fixture)}\n"
        config.write_text(source, encoding="utf-8")
        command = [
            str(executable),
            "build",
            "--clean",
            "--strict",
            "--config-file",
            str(config),
        ]
    else:
        command = [
            str(executable),
            "build",
            "--clean",
            "--strict",
            "--config-file",
            str(config),
            "--site-dir",
            str(destination),
        ]
    subprocess.run(
        command,
        check=True,
    )


def apply_mutation(fixture: Path, step: dict[str, Any]) -> None:
    """Apply one declarative mutation to a copied fixture."""
    for source, target in step.get("copy", {}).items():
        destination = fixture / target
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(fixture / source, destination)
    for target in step.get("remove", []):
        path = fixture / target
        if path.is_file():
            path.unlink()


def run_fixture(
    executable: Path,
    fixture: Path,
    root: Path,
    *,
    engine: Engine,
) -> dict[str, Any]:
    """Build one fixture or its ordered clean-build mutation sequence."""
    scenario = fixture / "scenario.json"
    if not scenario.is_file():
        destination = (
            fixture / "site"
            if engine is Engine.ZENSICAL
            else root / f"{fixture.name}-site"
        )
        build(
            executable,
            fixture,
            destination,
            engine=engine,
        )
        return extract(destination, engine=engine)

    steps = json.loads(scenario.read_text(encoding="utf-8"))
    snapshots = []
    for number, step in enumerate(steps):
        apply_mutation(fixture, step)
        destination = (
            fixture / "site"
            if engine is Engine.ZENSICAL
            else root / f"{fixture.name}-{number:02d}-site"
        )
        build(
            executable,
            fixture,
            destination,
            engine=engine,
        )
        snapshots.append(
            {
                "name": step["name"],
                "manifest": extract(destination, engine=engine),
            }
        )
    return {"steps": snapshots}


def compare(
    name: str,
    actual: dict[str, Any],
    *,
    update: bool,
    engine: Engine,
) -> bool:
    """Update or compare one checked-in semantic manifest."""
    expected_path = FIXTURES / name / "material.json"
    rendered = json.dumps(actual, indent=2, ensure_ascii=False) + "\n"
    if update:
        expected_path.write_text(rendered, encoding="utf-8")
        print(f"updated {expected_path.relative_to(ROOT)}")
        return True
    if not expected_path.is_file():
        print(f"missing {expected_path.relative_to(ROOT)}; run with --update")
        return False
    expected = json.loads(expected_path.read_text(encoding="utf-8"))
    if engine is Engine.ZENSICAL:
        expected = _normalize_generator_differences(
            expected,
            engine=Engine.MKDOCS,
        )
        actual = _normalize_generator_differences(
            actual,
            engine=Engine.ZENSICAL,
        )
    if expected == actual:
        print(f"matched {name} with {engine.value}")
        return True
    engine_name = engine.value.lower()
    temporary = (
        Path(tempfile.gettempdir()) / f"zensical-blog-{name}-{engine_name}.json"
    )
    temporary.write_text(rendered, encoding="utf-8")
    print(
        f"mismatch for {name} with {engine.value}; actual manifest: {temporary}"
    )
    return False


def _normalize_generator_differences(
    manifest: dict[str, Any],
    *,
    engine: Engine,
) -> dict[str, Any]:
    """Remove known non-blog differences between the two generators."""
    manifest = json.loads(json.dumps(manifest))

    # Mutation fixtures contain complete manifests at each step. Normalize
    # each snapshot through the same path as an ordinary fixture.
    for step in manifest.get("steps", []):
        step["manifest"] = _normalize_generator_differences(
            step["manifest"],
            engine=engine,
        )

    root = manifest.get("pages", {}).get("index.html")
    if root is not None:
        root.pop("document_title", None)
    for path, page in manifest.get("pages", {}).items():
        base = page.get("canonical") or "https://example.test/"
        if engine is Engine.ZENSICAL:
            for heading in page.get("headings", []):
                if heading.get("id") == "__skip":
                    heading["id"] = None
        if page.get("pagination") == {"current": 1, "links": []}:
            page["pagination"] = None
        if page.get("posts"):
            page.pop("relations", None)
            if engine is Engine.ZENSICAL and "/page/" in path:
                # Zensical reuses the logical view's navigation position for
                # pagination pages. Material leaves that final item inactive,
                # while retaining any containing section as active.
                page["active_ancestors"] = page.get("active_ancestors", [])[:-1]
        for post in page.get("posts", []):
            content = post.get("content")
            if not content:
                continue
            soup = BeautifulSoup(content, "lxml")
            element = (
                soup.body.find(recursive=False) if soup.body else soup.find()
            )
            post["content"] = _fragment(
                element,
                base,
                normalize_urls=True,
            )
    return manifest


def main() -> int:
    """Build selected fixtures and compare their manifests."""
    args = parse_args()
    mkdocs = args.mkdocs.resolve()
    if not mkdocs.is_file():
        raise FileNotFoundError(mkdocs)
    zensical = args.zensical.resolve() if args.zensical else None
    if zensical and not zensical.is_file():
        raise FileNotFoundError(zensical)
    succeeded = True
    with tempfile.TemporaryDirectory(prefix="zensical-blog-oracle-") as raw:
        root = Path(raw)
        for name in _fixture_names(args.fixtures):
            fixture = root / f"{name}-mkdocs"
            shutil.copytree(FIXTURES / name, fixture)
            manifest = run_fixture(
                mkdocs,
                fixture,
                root,
                engine=Engine.MKDOCS,
            )
            succeeded &= compare(
                name,
                manifest,
                update=args.update,
                engine=Engine.MKDOCS,
            )
            if zensical is None:
                continue
            if name == "collisions":
                print(
                    "skipped collisions with Zensical: diagnostics are expected"
                )
                continue
            fixture = root / f"{name}-zensical"
            shutil.copytree(FIXTURES / name, fixture)
            manifest = run_fixture(
                zensical,
                fixture,
                root,
                engine=Engine.ZENSICAL,
            )
            succeeded &= compare(
                name,
                manifest,
                update=False,
                engine=Engine.ZENSICAL,
            )
    return 0 if succeeded else 1


if __name__ == "__main__":
    raise SystemExit(main())
