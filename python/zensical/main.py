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

import os
import shutil
from pathlib import Path
from typing import Any, TYPE_CHECKING

import click
from click import ClickException

from zensical import StrictModeError, build, serve, version

if TYPE_CHECKING:
    from collections.abc import Callable

# ----------------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------------


def handle_strict_runtime_errors(
    func: Callable[..., None], *args: object, **kwargs: object
) -> None:
    """Call a Rust pymodule function; map strict-mode failure to a Click error.

    ``StrictModeError`` (from the native module) is turned into ``ClickException``
    so the process exits cleanly without a Python traceback. Any other
    exception is re-raised.
    """
    try:
        func(*args, **kwargs)  # type: ignore[operator]
    except StrictModeError as e:
        raise ClickException(str(e)) from e


# ----------------------------------------------------------------------------
# Commands
# ----------------------------------------------------------------------------


@click.version_option(version=version(), message="%(version)s")
@click.group()
def cli() -> None:
    """Zensical - A modern static site generator."""


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
    help="Treat warnings as errors.",
)
def execute_build(config_file: str | None, **kwargs: Any) -> None:
    """Build a project."""
    if config_file is None:
        for file in ["zensical.toml", "mkdocs.yml", "mkdocs.yaml"]:
            if os.path.exists(file):
                config_file = file
                break
        else:
            raise ClickException("No config file found in the current folder.")

    # Build project in Rust runtime, calling back into Python when necessary,
    # e.g., to parse MkDocs configuration format or render Markdown
    handle_strict_runtime_errors(
        build,
        os.path.abspath(config_file),
        kwargs.get("clean", False),
        kwargs.get("strict", False),
    )


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
    help="Treat warnings as errors.",
)
def execute_serve(config_file: str | None, **kwargs: Any) -> None:
    """Build and serve a project."""
    if config_file is None:
        for file in ["zensical.toml", "mkdocs.yml", "mkdocs.yaml"]:
            if os.path.exists(file):
                config_file = file
                break
        else:
            raise ClickException("No config file found in the current folder.")

    # Build project in Rust runtime, calling back into Python when necessary,
    # e.g., to parse MkDocs configuration format or render Markdown
    handle_strict_runtime_errors(serve, os.path.abspath(config_file), kwargs)


@cli.command(name="new")
@click.argument(
    "directory",
    type=click.Path(file_okay=False, dir_okay=True, writable=True),
    required=False,
)
def new_project(directory: str | None, **kwargs: Any) -> None:  # noqa: ARG001
    """Create a new template project in the current or given directory.

    Raises:
        ClickException: if the directory already contains a zensical.toml or a
            docs directory that is not empty, as well as when the path provided
            points to something that is not a directory.
    """
    working_dir = Path.cwd() if directory is None else Path(directory).resolve()
    if working_dir.is_file():
        raise ClickException(f"{working_dir} must be a directory, not a file.")

    config_file = working_dir / "zensical.toml"
    if config_file.exists():
        raise ClickException(f"{config_file} already exists.")

    working_dir.mkdir(parents=True, exist_ok=True)

    package_dir = Path(__file__).resolve().parent
    bootstrap = package_dir / "bootstrap"

    for src_file in bootstrap.rglob("*"):
        if src_file.is_file():
            rel_path = src_file.relative_to(bootstrap)
            dest_file = working_dir / rel_path
            if not dest_file.exists():
                dest_file.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(src_file, dest_file)


# ----------------------------------------------------------------------------
# Program
# ----------------------------------------------------------------------------

if __name__ == "__main__":  # pragma: no cover
    cli()
