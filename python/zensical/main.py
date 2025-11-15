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

from __future__ import annotations

import click
import os
import shutil

from click import ClickException
from zensical import build, serve, version


# ----------------------------------------------------------------------------
# Commands
# ----------------------------------------------------------------------------


@click.version_option(version=version(), message="%(version)s")
@click.group()
def cli():
    """Zensical - A modern static site generator"""


@cli.command(name="build")
@click.option(
    "-f",
    "--config-file",
    type=click.Path(exists=True),
    default=None,
    help="Path to config file.",
)
@click.option(
    "-c",
    "--clean",
    default=False,
    is_flag=True,
    help="Clean cache.",
)
@click.option(
    "-s",
    "--strict",
    default=False,
    is_flag=True,
    help="Strict mode (currently unsupported).",
)
def execute_build(config_file: str | None, **kwargs):
    """
    Build a project.
    """
    if config_file is None:
        for file in ["zensical.toml", "mkdocs.yml", "mkdocs.yaml"]:
            if os.path.exists(file):
                config_file = file
                break
        else:
            raise ClickException("No config file found in the current folder.")
    if kwargs.get("strict", False):
        print("Warning: Strict mode is currently unsupported.")

    # Build project in Rust runtime, calling back into Python when necessary,
    # e.g., to parse MkDocs configuration format or render Markdown
    build(os.path.abspath(config_file), kwargs.get("clean"))


@cli.command(name="serve")
@click.option(
    "-f",
    "--config-file",
    type=click.Path(exists=True),
    default=None,
    help="Path to config file.",
)
@click.option(
    "-a",
    "--dev-addr",
    metavar="<IP:PORT>",
    help="IP address and port (default: localhost:8000).",
)
@click.option(
    "-o",
    "--open",
    default=False,
    is_flag=True,
    help="Open preview in default browser.",
)
@click.option(
    "-s",
    "--strict",
    default=False,
    is_flag=True,
    help="Strict mode (currently unsupported).",
)
def execute_serve(config_file: str | None, **kwargs):
    """
    Build and serve a project.
    """
    if config_file is None:
        for file in ["zensical.toml", "mkdocs.yml", "mkdocs.yaml"]:
            if os.path.exists(file):
                config_file = file
                break
        else:
            raise ClickException("No config file found in the current folder.")
    if kwargs.get("strict", False):
        print("Warning: Strict mode is currently unsupported.")

    # Build project in Rust runtime, calling back into Python when necessary,
    # e.g., to parse MkDocs configuration format or render Markdown
    serve(os.path.abspath(config_file), kwargs)


@cli.command(name="new")
@click.argument(
    "directory",
    type=click.Path(file_okay=False, dir_okay=True, writable=True),
    required=False,
)
def new_project(directory: str | None, **kwargs):
    """
    Create a new template project in the current directory or in the given
    directory.

    Raises:
        ClickException: if the directory already contains a zensical.toml or a
            docs directory that is not empty, as well as when the path provided
            points to something that is not a directory.
    """

    if directory is None:
        directory = "."
    docs_dir = os.path.join(directory, "docs")
    config_file = os.path.join(directory, "zensical.toml")
    github_dir = os.path.join(directory, ".github")

    if os.path.exists(directory):
        if not os.path.isdir(directory):
            raise (ClickException("Path provided is not a directory."))
        if os.path.exists(config_file):
            raise (ClickException(f"{config_file} already exists."))
        if os.path.exists(docs_dir):
            raise (ClickException(f"{docs_dir} already exists."))
        if os.path.exists(github_dir):
            raise (ClickException(f"{github_dir} already exists."))
    else:
        os.makedirs(directory)

    package_dir = os.path.dirname(os.path.abspath(__file__))
    shutil.copy(os.path.join(package_dir, "bootstrap/zensical.toml"), directory)
    shutil.copytree(
        os.path.join(package_dir, "bootstrap/docs"),
        os.path.join(directory, "docs"),
    )
    shutil.copytree(
        os.path.join(package_dir, "bootstrap/.github"),
        os.path.join(directory, ".github"),
    )


# ----------------------------------------------------------------------------
# Program
# ----------------------------------------------------------------------------

if __name__ == "__main__":  # pragma: no cover
    cli()
