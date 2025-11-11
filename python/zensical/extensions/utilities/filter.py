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

from fnmatch import fnmatch

# -----------------------------------------------------------------------------
# Classes
# -----------------------------------------------------------------------------


class Filter:
    """
    A filter.
    """

    def __init__(self, config: dict):
        """
        Initialize the filter.

        Arguments:
            config: The filter configuration.
        """
        self.config = config

    def __call__(self, value: str) -> bool:
        """
        Filter a value.

        First, the inclusion patterns are checked. Regardless of whether they
        are present, the exclusion patterns are checked afterwards. This allows
        to exclude values that are included by the inclusion patterns, so that
        exclusion patterns can be used to refine inclusion patterns.

        Arguments:
            value: The value to filter.

        Returns:
            Whether the value should be included.
        """

        # Check if value matches one of the inclusion patterns
        if "include" in self.config:
            for pattern in self.config["include"]:
                if fnmatch(value, pattern):
                    break

            # Value is not included
            else:
                return False

        # Check if value matches one of the exclusion patterns
        if "exclude" in self.config:
            for pattern in self.config["exclude"]:
                if fnmatch(value, pattern):
                    return False

        # Value is not excluded
        return True

    # -------------------------------------------------------------------------

    config: dict
    """
    The filter configuration.
    """
