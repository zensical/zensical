# Copyright (c) 2016-2025 Martin Donath <martin.donath@squidfunk.com>

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

# -----------------------------------------------------------------------------

FROM python:3.14-alpine3.23 AS build

# Disable bytecode caching during build
ENV PYTHONDONTWRITEBYTECODE=1

# Install build dependencies
RUN apk upgrade --update-cache -a
RUN apk add --no-cache \
  git \
  gcc \
  libffi-dev \
  musl-dev \
  tini \
  uv

# Copy files to prepare build
COPY scripts scripts

# Prepare build
RUN mkdir -p python/zensical
RUN python scripts/prepare.py

# Copy files to build project
COPY . .

# Build project
RUN uv pip install --system .

# -----------------------------------------------------------------------------

FROM scratch as image

# Copy relevant files from build
COPY --from=build /bin/sh /bin/sh
COPY --from=build /sbin/tini /sbin/tini
COPY --from=build /lib /lib
COPY --from=build /usr/lib /usr/lib
COPY --from=build /usr/local /usr/local

# Set working directory and expose preview server port
WORKDIR /docs
EXPOSE 8000

# Start preview server by default
ENTRYPOINT ["/sbin/tini", "--", "zensical"]
CMD ["serve", "--dev-addr=0.0.0.0:8000"]
