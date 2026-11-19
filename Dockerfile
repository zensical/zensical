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

FROM python:3.14-alpine3.23@sha256:f6a7dec95e6205059dccfcb07faca3ca38729ea35df01231caf1d127bacedec8 AS base

FROM base AS build
ENV PYTHONDONTWRITEBYTECODE=1
ARG TARGETARCH
WORKDIR /build

# Cache build tools independently of application manifests and sources.
RUN apk upgrade --no-cache -a
RUN apk add --no-cache \
    curl \
    git \
    gcc \
    libffi-dev \
    musl-dev \
    uv

# Let rustup read the repository's pinned toolchain and requested components.
ENV PATH="/root/.cargo/bin:${PATH}"
COPY rust-toolchain.toml rust-toolchain.toml
RUN curl https://sh.rustup.rs -sSf | \
    sh -s -- -y --profile minimal --default-toolchain none \
    && rustup show

# Cache backend setup and requirements until Python dependency inputs change.
COPY pyproject.toml uv.lock ./
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --locked --only-group build \
    && mkdir -p /dist \
    && uv export --locked --no-dev --group docker --no-emit-project \
        --output-file /dist/requirements.txt

# Copy application inputs after the independently cached dependency setup.
COPY Cargo.toml Cargo.lock README.md LICENSE.md ./
COPY crates crates
COPY python python

# Enforce the Cargo lockfile and retain incremental compilation artifacts.
RUN --mount=type=cache,target=/root/.cache/uv \
    --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,id=zensical-target-${TARGETARCH},target=/build/target \
    uv build --no-build-isolation --python .venv/bin/python \
        --config-setting maturin.build-args=--locked \
        --wheel --out-dir /dist

# -----------------------------------------------------------------------------

FROM base AS runtime

# Refresh this stage in CI so OS updates leave the compiler cache intact.
RUN apk upgrade --no-cache -a \
    && apk add --no-cache \
        libgcc \
        tini

# -----------------------------------------------------------------------------

FROM runtime AS image

# Cache runtime dependencies independently of changes to the application wheel.
COPY --from=build /dist/requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir --require-hashes \
        --requirement /tmp/requirements.txt \
    && rm /tmp/requirements.txt

# Install the application using the already installed runtime dependencies.
COPY --from=build /dist/*.whl /dist/
RUN pip install --no-cache-dir --no-deps /dist/*.whl \
    && pip check \
    && rm -rf /dist

# Set working directory and expose preview server port.
WORKDIR /docs
EXPOSE 8000

# Start the preview server by default; callers can select build or another user.
ENTRYPOINT ["/sbin/tini", "--", "zensical"]
CMD ["serve", "--dev-addr=0.0.0.0:8000"]
