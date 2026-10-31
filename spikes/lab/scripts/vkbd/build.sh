#!/usr/bin/env bash
# Build the spike-only virtual keyboard helper (needs wayland-scanner, gcc,
# wayland-client and xkbcommon development files).
set -euo pipefail
DIR=$(cd "$(dirname "$0")" && pwd)
BUILD="$DIR/build"
mkdir -p "$BUILD"
wayland-scanner client-header "$DIR/virtual-keyboard-unstable-v1.xml" "$BUILD/virtual-keyboard-client.h"
wayland-scanner private-code "$DIR/virtual-keyboard-unstable-v1.xml" "$BUILD/virtual-keyboard-protocol.c"
gcc -O1 -Wall -I"$BUILD" -o "$BUILD/vkbd" "$DIR/vkbd.c" "$BUILD/virtual-keyboard-protocol.c" \
  $(pkg-config --cflags --libs wayland-client xkbcommon)
