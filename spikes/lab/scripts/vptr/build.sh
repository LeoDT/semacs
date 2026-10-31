#!/usr/bin/env bash
# Build the spike-only virtual pointer helper (needs wayland-scanner, gcc and
# wayland-client development files).
set -euo pipefail
DIR=$(cd "$(dirname "$0")" && pwd)
BUILD="$DIR/build"
mkdir -p "$BUILD"
wayland-scanner client-header "$DIR/wlr-virtual-pointer-unstable-v1.xml" "$BUILD/vptr-client.h"
wayland-scanner private-code "$DIR/wlr-virtual-pointer-unstable-v1.xml" "$BUILD/vptr-protocol.c"
gcc -O1 -Wall -I"$BUILD" -o "$BUILD/vptr" "$DIR/vptr.c" "$BUILD/vptr-protocol.c" \
  $(pkg-config --cflags --libs wayland-client)
