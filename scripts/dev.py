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

import os, shutil, subprocess  # noqa: E401

# ----------------------------------------------------------------------------
# Program
# ----------------------------------------------------------------------------


def main():
    """
    Set up development environment.

    This script clones the Zensical UI repository, and symbolically links the
    build artifacts into the Python package directory for development use.
    """
    os.makedirs("tmp", exist_ok=True)

    # Clone UI repository into tmp directory
    repo_url = "https://github.com/zensical/ui.git"
    dest_dir = os.path.join("tmp", "ui")
    if not os.path.exists(dest_dir):
        subprocess.run(["git", "clone", repo_url, dest_dir], check=True)

    # Remove existing template directory if it exists
    path = os.path.join("python", "zensical", "templates")
    if os.path.exists(path) and not os.path.islink(path):
        shutil.rmtree(path)

    # Determine base and dist directories
    base_dir = os.path.join("python", "zensical")
    dist_dir = os.path.join(dest_dir, "dist")

    # Create a symbolic link to the UI source directory
    path = os.path.join(base_dir, "templates")
    if os.path.exists(dist_dir) and not os.path.exists(path):
        os.symlink(
            os.path.relpath(dist_dir, base_dir),
            path,
            target_is_directory=True,
        )

    # Create a .gitignore file to ignore templates directory
    path = os.path.join(base_dir, ".gitignore")
    if not os.path.exists(path):
        with open(path, "w") as f:
            f.write("templates\n")


# ----------------------------------------------------------------------------

if __name__ == "__main__":
    main()
