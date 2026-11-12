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

"""Load user callbacks for export-only HTML preprocessing."""

from __future__ import annotations

import runpy
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from collections.abc import Callable


def load_preprocess(path: str) -> Callable[[str, str], str]:
    """Load a script's `preprocess(html, output)` callback once per config.

    The callback must return HTML as a string. Users choose and install any
    HTML parsing libraries their scripts require. The output argument is the
    site-relative Markdown path, including the filename.
    """
    try:
        # Source is executed directly so edits aren't hidden by cached bytecode.
        namespace = runpy.run_path(path)
    except Exception as error:
        raise ValueError(
            f"Could not load llmstxt preprocess script {path!r}: {error}"
        ) from error

    callback = namespace.get("preprocess")
    if not callable(callback):
        raise TypeError(
            f"llmstxt preprocess script {path!r} "
            "must define a callable named 'preprocess'"
        )
    return cast("Callable[[str, str], str]", callback)
