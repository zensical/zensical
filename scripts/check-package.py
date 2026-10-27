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

"""Check that core distributions depend on UI without bundling its assets."""

from __future__ import annotations

import sys
import tarfile
import tomllib
from email.parser import BytesParser
from pathlib import Path
from zipfile import ZipFile


def check(path: Path, requirement: str) -> None:
    """Check the contents and dependency metadata of one distribution."""
    if path.suffix == ".whl":
        with ZipFile(path) as archive:
            names = archive.namelist()
            metadata_path = next(
                name for name in names if name.endswith(".dist-info/METADATA")
            )
            metadata = archive.read(metadata_path)
    else:
        with tarfile.open(path) as archive:
            names = archive.getnames()
            metadata_path = next(
                name
                for name in names
                if name.endswith("/PKG-INFO") and name.count("/") == 1
            )
            stream = archive.extractfile(metadata_path)
            if stream is None:
                raise SystemExit(f"Missing package metadata: {path}")
            metadata = stream.read()

    bundled = [
        name
        for name in names
        if "/zensical/templates/" in f"/{name.rstrip('/')}/"
    ]
    if bundled:
        raise SystemExit(f"Bundled UI assets in {path}: {bundled[0]}")

    fields = BytesParser().parsebytes(metadata)
    dependencies = [
        value.replace(" ", "") for value in fields.get_all("Requires-Dist", [])
    ]
    if requirement not in dependencies:
        raise SystemExit(f"Missing {requirement} dependency in {path}")
    print(f"Checked {path.name}: UI dependency present; no bundled UI assets")


def main() -> None:
    """Check every wheel and source distribution in the artifact directory."""
    root = Path(__file__).resolve().parent.parent
    config = tomllib.loads((root / "pyproject.toml").read_text())
    requirement = next(
        value
        for value in config["project"]["dependencies"]
        if value.startswith("zensical-ui==")
    )
    directory = Path(sys.argv[1])
    paths = sorted(directory.glob("zensical-*.whl")) + sorted(
        directory.glob("zensical-*.tar.gz")
    )
    if not paths:
        raise SystemExit(f"No core distributions found in {directory}")
    for path in paths:
        check(path, requirement)


if __name__ == "__main__":
    main()
