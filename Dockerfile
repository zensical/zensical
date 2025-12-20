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
  curl \
  git \
  gcc \
  libffi-dev \
  musl-dev \
  tini \
  uv

# Install Rust toolchain
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"

# Copy files to prepare build
COPY scripts scripts

# Prepare build
RUN mkdir -p python/zensical
RUN python scripts/prepare.py

# Create a stub project, which will allow us to install dependencies and have
# them properly cached while changes to sources won't invalidate the cache
RUN mkdir crates
RUN cargo new --lib crates/zensical
RUN cargo add pyo3 \
    --manifest-path crates/zensical/Cargo.toml \
    --features extension-module

# Copy files to install dependencies - these will get installed into a virtual
# environment, which is fine, since uv can later reuse the cached versions
COPY pyproject.toml pyproject.toml
COPY README.md README.md
COPY uv.lock uv.lock

# Install dependencies
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --dev --no-install-project

# Copy files to build project
COPY crates crates
COPY python python
COPY Cargo.lock Cargo.lock
COPY Cargo.toml Cargo.toml

# Build project
RUN . /.venv/bin/activate
RUN --mount=type=cache,target=/root/.cache/uv \
    --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=target \
    uv pip install --system . -v

# -----------------------------------------------------------------------------

FROM scratch AS image

# Set version argument
ARG VERSION

# Annotate image with metadata
LABEL org.opencontainers.image.title="Zensical"
LABEL org.opencontainers.image.description="A modern static site generator"
LABEL org.opencontainers.image.documentation="https://zensical.org/docs/"
LABEL org.opencontainers.image.source="https://github.com/zensical/zensical"
LABEL org.opencontainers.image.url="https://github.com/zensical/zensical"
LABEL org.opencontainers.image.vendor="zensical"
LABEL org.opencontainers.image.version="${VERSION}"
LABEL org.opencontainers.image.license="MIT"

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
