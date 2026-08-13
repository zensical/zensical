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

"""Generate a deterministic Zensical project for memory measurements."""

from __future__ import annotations

import argparse
from pathlib import Path


def parse_args() -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "directory",
        type=Path,
        help="new directory in which to create the fixture",
    )
    parser.add_argument(
        "--pages",
        type=int,
        default=400,
        help="number of Markdown pages to generate (default: 400)",
    )
    parser.add_argument(
        "--sections",
        type=int,
        default=12,
        help="second-level headings per page (default: 12)",
    )
    parser.add_argument(
        "--groups",
        type=int,
        default=20,
        help="directories across which pages are spread (default: 20)",
    )
    return parser.parse_args()


def page_contents(page: int, sections: int) -> str:
    """Create deterministic Markdown with metadata and search entries."""
    lines = [
        "---",
        f"title: Memory fixture page {page:04d}",
        "tags:",
        f"  - group-{page % 7}",
        f"  - topic-{page % 13}",
        "metadata:",
        f"  owner: team-{page % 5}",
        "  lifecycle: maintained",
        "---",
        "",
        f"# Memory fixture page {page:04d}",
        "",
    ]
    for section in range(sections):
        lines.extend(
            [
                f"## Section {section:02d}",
                "",
                (
                    f"Page {page:04d}, section {section:02d} contains "
                    "deterministic prose used to exercise Markdown parsing, "
                    "template rendering, navigation cloning, and search "
                    "index construction under a representative documentation "
                    "build."
                ),
                "",
                (
                    "The second paragraph deliberately contains "
                    "**formatting**, `inline code`, and enough text to make "
                    "retained content visible in process-level memory "
                    "measurements."
                ),
                "",
            ]
        )
    return "\n".join(lines)


def create_fixture(
    directory: Path, *, pages: int, sections: int, groups: int
) -> None:
    """Create the benchmark project, refusing to overwrite existing data."""
    if pages < 1:
        raise ValueError("--pages must be at least 1")
    if sections < 1:
        raise ValueError("--sections must be at least 1")
    if groups < 1:
        raise ValueError("--groups must be at least 1")
    if directory.exists():
        message = f"refusing to overwrite existing path: {directory}"
        raise FileExistsError(message)

    docs = directory / "docs"
    docs.mkdir(parents=True)
    (directory / "zensical.toml").write_text(
        """[project]
site_name = "Memory fixture"
site_url = "https://example.com/"

[project.theme]
language = "en"
""",
        encoding="utf-8",
    )

    for page in range(pages):
        group = docs / f"group-{page % groups:02d}"
        group.mkdir(exist_ok=True)
        path = group / f"page-{page:04d}.md"
        path.write_text(page_contents(page, sections), encoding="utf-8")


def main() -> int:
    """Generate a fixture from command-line arguments."""
    args = parse_args()
    create_fixture(
        args.directory.resolve(),
        pages=args.pages,
        sections=args.sections,
        groups=args.groups,
    )
    print(
        f"Created {args.pages} pages with {args.sections} sections each in "
        f"{args.directory.resolve()}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
