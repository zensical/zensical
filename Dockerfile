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

# -----------------------------------------------------------------------------

FROM python:3.14-alpine3.23@sha256:dd4d2bd5b53d9b25a51da13addf2be586beebd5387e289e798e4083d94ca837a AS base

FROM base AS build

# Disable bytecode caching during build
ENV PYTHONDONTWRITEBYTECODE=1

# Install build dependencies and Rust toolchain
RUN apk upgrade --update-cache -a && \
    apk add --no-cache \
        curl \
        git \
        gcc \
        libffi-dev \
        musl-dev \
        uv && \
    curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"

# Copy files to prepare build
COPY scripts scripts

# Prepare build
RUN mkdir -p python/zensical
RUN python scripts/prepare.py

# Create a stub project, which will allow us to install dependencies and have
# them properly cached while changes to sources won't invalidate the cache
RUN mkdir -p crates && \
    cargo new --lib crates/zensical && \
    cargo add pyo3 \
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
COPY LICENSE.md LICENSE.md
COPY crates crates
COPY python python
COPY Cargo.lock Cargo.lock
COPY Cargo.toml Cargo.toml

# Build wheel
RUN --mount=type=cache,target=/root/.cache/uv \
    --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=target \
    uv build --wheel --out-dir /dist

# -----------------------------------------------------------------------------

FROM build AS bundle

# Install project wheel, runtime dependencies, and PyInstaller
RUN --mount=type=cache,target=/root/.cache/uv \
    uv pip install /dist/*.whl mkdocstrings-python pyinstaller

# Bundle into a self-contained directory (onedir avoids /tmp extraction overhead)
RUN uv run pyinstaller \
    --onedir \
    --name zensical \
    --distpath /bundle \
    --collect-all zensical \
    --collect-all markdown \
    --collect-all pymdownx \
    --collect-all pygments \
    --collect-all click \
    --collect-all yaml \
    --collect-all deepmerge \
    --collect-all tomli \
    --collect-all mkdocstrings \
    --collect-all griffe \
    $(uv run which zensical)

# -----------------------------------------------------------------------------

FROM alpine:3.23.4 AS image

# Add only the C runtime needed by the Rust extension, and tini as init
RUN apk upgrade --update-cache -a && \
    apk add --no-cache \
        libgcc \
        tini && \
    adduser -D -u 1000 zensical

# Set working directory
WORKDIR /docs
RUN chown zensical:zensical /docs

# Copy the self-contained PyInstaller bundle (no Python package required)
COPY --from=bundle /bundle/zensical /app

USER zensical
EXPOSE 8000

# Start preview server by default
ENTRYPOINT ["/sbin/tini", "--", "/app/zensical"]
CMD ["serve", "--dev-addr=0.0.0.0:8000"]
