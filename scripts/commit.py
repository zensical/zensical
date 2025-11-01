#!/usr/bin/env python

# -----------------------------------------------------------------------------

# Copyright (c) Zensical LLC <https://zensical.org>

# SPDX-License-Identifier: MIT
# Third-party contributions licensed under CLA

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

import os, re, sys, tomllib  # noqa: E401

from dataclasses import dataclass
from glob import glob

# ----------------------------------------------------------------------------
# Classes
# ----------------------------------------------------------------------------


class ScopeError(ValueError):
    """Invalid commit scope error."""


class TypeError(ValueError):
    """Invalid commit type error."""


# ----------------------------------------------------------------------------


@dataclass
class Message:
    """
    Commit message.

    This class represents a commit message with a scope, type, and description.
    It provides methods to parse and validate commit messages according to our
    format, which is slightly different from the Conventional Commits standard,
    improving readability and consistency.
    """

    @classmethod
    def parse(cls, message: str) -> "Message":
        """
        Parse a commit message string into an object.
        """
        match = re.match(r"^([^:]+):([^\s]+) - (.+)$", message)
        if not match:
            raise ValueError("Required format: <scope>:<type> - <description>")

        # Extract components and return commit message
        scope, type, description = match.groups()
        return cls(scope=scope, type=type, description=description)

    def validate(self, scopes: dict[str, str]) -> None:
        """
        Validate the commit message against the given scopes and types.
        """
        if self.scope not in scopes:
            raise ScopeError(f"Invalid scope: {self.scope}")

        # Validate type
        if self.type not in TYPES:
            raise TypeError(f"Invalid type: {self.type}")

        # Validate description
        if self.description[0] != self.description[0].lower():
            raise ValueError("Commit message must be lowercased.")

        # Retrieve staged files
        with os.popen("git diff --cached --name-only") as p:
            output = p.read()

        # Validate if files are within scope
        for file in output.strip().split("\n"):
            if not f"./{file}".startswith(scopes[self.scope]):
                raise ValueError(
                    f"Invalid scope for file: "
                    f"{file} not in {scopes[self.scope]}"
                )

    scope: str
    """
    Commit scope.
    """

    type: str
    """
    Commit type.
    """

    description: str
    """
    Commit description.
    """


# ----------------------------------------------------------------------------
# Functions
# ----------------------------------------------------------------------------


def resolve(directory: str) -> dict[str, str] | None:
    """
    Return commit scopes for a cargo project.

    This function checks, if the given directory contains a `Cargo.toml` file,
    and if so, parses it to extract the workspace members. It then resolves the
    valid scopes, which are the names of the crates defined in the respective
    `Cargo.toml` files.
    """
    path = os.path.join(directory, "Cargo.toml")
    if not os.path.isfile(path):
        return

    # Open and parse the Cargo.toml file
    with open(path, "rb") as f:
        content = tomllib.load(f)

    # Return workspace members
    if "workspace" in content:
        scopes: dict[str, str] = {}

        # Get the list of member crates
        for member in content["workspace"].get("members", []):
            path = os.path.join(directory, member)
            for match in glob(path):
                nested = resolve(match)
                if nested:
                    scopes.update(nested)

        # Return commit scopes
        return scopes

    # Return crate
    package = content.get("package")
    if package and "name" in package:
        return {package["name"]: directory}


# ----------------------------------------------------------------------------
# Constants
# ----------------------------------------------------------------------------


TYPES = {
    "feature",
    "fix",
    "refactor",
    "docs",
    "perf",
    "test",
    "build",
    "style",
    "chore",
    "release",
}
"""
Commit types.
"""

# ----------------------------------------------------------------------------

BG_RED = "\033[41m"
"""
ANSI escape code for red background.
"""

FG_RED = "\033[31m"
"""
ANSI escape code for red foreground.
"""

RESET = "\033[0m"
"""
ANSI escape code to reset formatting.
"""

# ----------------------------------------------------------------------------
# Program
# ----------------------------------------------------------------------------


def main():
    """
    Commit message linter.
    """
    if len(sys.argv) < 2:
        print("No commit message provided.")
        sys.exit(1)

    # Commit message might be passed as string, or in a file
    commit = sys.argv[1]
    if os.path.isfile(commit):
        with open(sys.argv[1], "r") as f:
            message = f.read().strip()
    else:
        message = commit.strip()

    # Skip merge commits
    if message.startswith("Merge branch"):
        return sys.exit(0)

    # Resolve cargo workspace members and parse commit message
    scopes = resolve(os.path.curdir)
    scopes["workspace"] = "."
    try:
        msg = Message.parse(message)
        msg.validate(scopes)

    # If an error happened, print it
    except ValueError as e:
        print(f"{FG_RED}✘{RESET} {BG_RED} Error {RESET} {e}")
        print("")
        print("   Commit rejected.")
        print("")

        # Exit with error
        return sys.exit(1)


# ----------------------------------------------------------------------------

if __name__ == "__main__":
    main()
