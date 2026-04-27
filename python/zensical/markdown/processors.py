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

from typing import TYPE_CHECKING

from markdown.postprocessors import Postprocessor
from markdown.preprocessors import Preprocessor
from markdown.treeprocessors import Treeprocessor

if TYPE_CHECKING:
    from zensical.markdown.extensions import MarkdownExt

# -----------------------------------------------------------------------------
# Classes
# -----------------------------------------------------------------------------


class PreprocessorExt(Preprocessor):
    """Subclass of `Preprocessor`.

    We need to subclass the `Preprocessor` to allow access to our modified
    `MarkdownExt` instance, which includes the page and configuration.
    """

    def __init__(self, md: MarkdownExt):
        """Initialize processor."""
        self.md: MarkdownExt = md


class PostprocessorExt(Postprocessor):
    """Subclass of `Postprocessor`.

    We need to subclass the `Postprocessor` to allow access to our modified
    `MarkdownExt` instance, which includes the page and configuration.
    """

    def __init__(self, md: MarkdownExt):
        """Initialize processor."""
        self.md: MarkdownExt = md


class TreeprocessorExt(Treeprocessor):
    """Subclass of `Treeprocessor`.

    We need to subclass the `Treeprocessor` to allow access to our modified
    `MarkdownExt` instance, which includes the page and configuration.
    """

    def __init__(self, md: MarkdownExt):
        """Initialize processor."""
        self.md: MarkdownExt = md
