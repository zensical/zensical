#!/usr/bin/env python

# -----------------------------------------------------------------------------

# Copyright (c) 2025 Zensical and contributors

# SPDX-License-Identifier: MIT
# Third-party contributions licensed under DCO

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

import os
import subprocess

# ----------------------------------------------------------------------------
# Program
# ----------------------------------------------------------------------------


def main() -> int:
    """Prepare production build."""
    os.makedirs("tmp", exist_ok=True)

    # Clone UI repository into tmp directory
    repo_url = "https://github.com/zensical/ui.git"
    repo_tag = "v0.0.3"
    dest_dir = os.path.join("tmp", "ui")
    if not os.path.exists(dest_dir):
        subprocess.run(["git", "clone", repo_url, dest_dir], check=True)
        subprocess.run(["git", "checkout", repo_tag], cwd=dest_dir, check=True)

    # Determine base and dist directories
    base_dir = os.path.join("python", "zensical")
    dist_dir = os.path.join(dest_dir, "dist")

    # Check, if there are symbolic links and remove them
    if os.path.islink(os.path.join(base_dir, "templates")):
        os.unlink(os.path.join(base_dir, "templates"))

    # Remove .gitignore file from development setup
    path = os.path.join(base_dir, ".gitignore")
    if os.path.exists(path):
        os.remove(path)

    # Copy UI build artifacts
    path = os.path.join(base_dir, "templates")
    if os.path.exists(dist_dir):
        if os.path.exists(path):
            subprocess.run(["rm", "-rf", path], check=True)
        subprocess.run(["cp", "-r", dist_dir, path], check=True)

    return 0


# ----------------------------------------------------------------------------

if __name__ == "__main__":
    raise SystemExit(main())
