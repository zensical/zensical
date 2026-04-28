#!/bin/sh

# Copyright (c) 2025-2026 Zensical and contributors

# SPDX-License-Identifier: MIT
# All contributions are certified under the DCO

# Adjust the UID/GID of the zensical user to match the host user so that files
# generated inside the container (e.g. by `build`) are owned by the caller.
# Pass the desired values via the PUID and PGID environment variables:
#
#   docker run -e PUID=$(id -u) -e PGID=$(id -g) ...

PUID="${PUID:-1000}"
PGID="${PGID:-1000}"

# Validate that PUID and PGID are numeric
case "$PUID" in
    ''|*[!0-9]*) echo "Error: PUID must be numeric" >&2; exit 1 ;;
esac
case "$PGID" in
    ''|*[!0-9]*) echo "Error: PGID must be numeric" >&2; exit 1 ;;
esac

if [ "$(id -u)" = "0" ]; then
    # Update group and user IDs if they differ from the defaults
    if [ "$PGID" != "1000" ]; then
        delgroup zensical 2>/dev/null
        addgroup -g "$PGID" zensical
    fi
    if [ "$PUID" != "1000" ]; then
        deluser zensical 2>/dev/null
        adduser -D -u "$PUID" -G zensical zensical
    fi

    chown zensical:zensical /docs
    exec su-exec zensical "$@"
else
    exec "$@"
fi
