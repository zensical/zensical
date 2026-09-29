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
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
# FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
# IN THE SOFTWARE.

"""Compare generated Material and Zensical social cards and metadata."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

from bs4 import BeautifulSoup
from PIL import Image, ImageChops, ImageStat

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "python" / "tests" / "fixtures" / "social"


def arguments() -> argparse.Namespace:
    """Parse builder paths and optional cases."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mkdocs", type=Path, required=True)
    parser.add_argument("--zensical", type=Path, required=True)
    parser.add_argument("--case", action="append", dest="cases")
    parser.add_argument(
        "--output", type=Path, help="keep builds in this empty directory"
    )
    parser.add_argument(
        "--font-cache", type=Path, help="existing social font cache"
    )
    return parser.parse_args()


def build(executable: Path, project: Path, *, strict: bool) -> None:
    """Build one copied fixture in its own project directory."""
    command = [
        str(executable),
        "build",
        "--clean",
        "--config-file",
        "mkdocs.yml",
    ]
    if strict:
        command.append("--strict")
    result = subprocess.run(
        command, cwd=project, capture_output=True, text=True, check=False
    )
    (project / "build.log").write_text(
        f"$ {' '.join(command)}\n{result.stdout}{result.stderr}",
        encoding="utf-8",
    )
    if result.returncode:
        details = (result.stderr or result.stdout).strip().splitlines()
        summary = details[-1] if details else "no diagnostics"
        raise RuntimeError(
            f"{executable.name} build failed in {project} "
            f"({result.returncode}): {summary}"
        )


def manifest(site: Path) -> dict:
    """Extract only social metadata and generated image facts."""
    pages = {}
    for path in sorted(site.rglob("*.html")):
        if path.name == "404.html":
            continue
        soup = BeautifulSoup(path.read_text(encoding="utf-8"), "lxml")
        if soup.head is None:
            raise ValueError(f"generated page lacks a head element: {path}")
        tags = [
            [property_name, tag.get("content")]
            for tag in soup.head.find_all("meta", property=True)
            if isinstance(property_name := tag.get("property"), str)
            and property_name.startswith(("og:", "twitter:", "x:"))
        ]
        pages[path.relative_to(site).as_posix()] = tags

    cards = {}
    for path in sorted(site.rglob("*.png")):
        relative = path.relative_to(site).as_posix()
        if not relative.startswith(("assets/images/social/", "assets/cards/")):
            continue
        with Image.open(path) as image:
            cards[relative] = list(image.size)
    return {"pages": pages, "cards": cards}


def image_difference(first: Path, second: Path) -> float:
    """Measure mean RGB channel difference independent of PNG encoding."""
    with Image.open(first) as left, Image.open(second) as right:
        if left.size != right.size:
            return float("inf")
        difference = ImageChops.difference(
            left.convert("RGB"), right.convert("RGB")
        )
        return sum(ImageStat.Stat(difference).mean) / 3


def compare(name: str, material: Path, zensical: Path) -> bool:
    """Show semantic and visual differences for one fixture."""
    expected = manifest(material / "site")
    actual = manifest(zensical / "site")
    (material / "manifest.json").write_text(
        json.dumps(expected, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    (zensical / "manifest.json").write_text(
        json.dumps(actual, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    cards = set(expected["cards"]) | set(actual["cards"])
    differences = {
        path: round(
            image_difference(
                material / "site" / path, zensical / "site" / path
            ),
            3,
        )
        for path in sorted(set(expected["cards"]) & set(actual["cards"]))
    }
    matched = expected == actual and all(
        value <= 1 for value in differences.values()
    )
    print(
        f"{name}: {'MATCH' if matched else 'DIFF'}; "
        f"{len(expected['pages'])} pages, {len(cards)} card paths; "
        f"mean RGB error {max(differences.values(), default=0):.3f}"
    )
    if expected != actual:
        for key in ("pages", "cards"):
            for path in sorted(set(expected[key]) | set(actual[key])):
                if expected[key].get(path) != actual[key].get(path):
                    print(f"  {key}/{path}")
                    print(f"    Material: {expected[key].get(path)}")
                    print(f"    Zensical: {actual[key].get(path)}")
    for path, error in differences.items():
        if error > 1:
            print(f"  pixels/{path}: mean RGB error {error:.3f}")
    return matched


def snapshot(project: Path) -> dict:
    """Capture visible metadata and card contents between cached builds."""
    site = project / "site"
    current = manifest(site)
    return {
        "pages": current["pages"],
        "cards": {
            path: hashlib.sha256((site / path).read_bytes()).hexdigest()
            for path in current["cards"]
        },
    }


def apply_step(project: Path, step: dict) -> None:
    """Apply one exact source replacement in a copied fixture."""
    path = project / step["path"]
    source = path.read_text(encoding="utf-8")
    before = step["before"]
    if source.count(before) != 1:
        raise ValueError(f"expected one occurrence of {before!r} in {path}")
    path.write_text(source.replace(before, step["after"]), encoding="utf-8")


def run(root: Path, args: argparse.Namespace) -> bool:
    """Build selected fixtures in both engines and compare their outputs."""
    names = args.cases or sorted(
        path.name
        for path in FIXTURES.iterdir()
        if (path / "mkdocs.yml").is_file()
    )
    unknown = [
        name for name in names if not (FIXTURES / name / "mkdocs.yml").is_file()
    ]
    if unknown:
        raise ValueError(f"unknown social cases: {', '.join(unknown)}")
    matched = True
    for name in names:
        project = FIXTURES / name
        material = root / name / "material"
        zensical = root / name / "zensical"
        shutil.copytree(project, material)
        shutil.copytree(project, zensical)
        if args.font_cache and name in {
            "bundled",
            "custom-typography",
            "debug",
            "debug-no-grid",
            "layout-options",
            "logo-icon",
            "theme-defaults",
        }:
            for destination in (material, zensical):
                shutil.copytree(
                    args.font_cache,
                    destination / "social-cache/fonts",
                )
        strict = name not in {
            "debug",
            "debug-no-grid",
            "deprecated",
            "no-site-url",
        }
        try:
            build(args.mkdocs, material, strict=strict)
            build(args.zensical, zensical, strict=strict)
            matched &= compare(name, material, zensical)
            steps_file = project / "steps.json"
            if not steps_file.is_file():
                continue
            for index, step in enumerate(json.loads(steps_file.read_text())):
                previous = {
                    "material": snapshot(material),
                    "zensical": snapshot(zensical),
                }
                for destination in (material, zensical):
                    apply_step(destination, step)
                build(args.mkdocs, material, strict=strict)
                build(args.zensical, zensical, strict=strict)
                label = f"{name}/step-{index + 1}"
                matched &= compare(label, material, zensical)
                for engine, destination in (
                    ("material", material),
                    ("zensical", zensical),
                ):
                    current = snapshot(destination)
                    for kind in step["changes"]:
                        if previous[engine][kind] == current[kind]:
                            print(f"{label}: {engine} {kind} did not change")
                            matched = False
        except RuntimeError as error:
            print(f"{name}: ERROR; {error}")
            matched = False
            continue
    return matched


def main() -> int:
    """Run the selected compatibility matrix."""
    args = arguments()
    args.mkdocs = args.mkdocs.resolve()
    args.zensical = args.zensical.resolve()
    if args.font_cache:
        args.font_cache = args.font_cache.resolve()
    if args.output:
        root = args.output.resolve()
        root.mkdir(parents=True, exist_ok=True)
        if any(root.iterdir()):
            raise ValueError(f"output directory must be empty: {root}")
        print(f"Builds: {root}")
        return 0 if run(root, args) else 1
    with tempfile.TemporaryDirectory(prefix="zensical-social-parity-") as raw:
        return 0 if run(Path(raw), args) else 1


if __name__ == "__main__":
    raise SystemExit(main())
