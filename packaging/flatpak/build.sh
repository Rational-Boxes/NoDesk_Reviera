#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Reviera contributors
#
# Builds the BIM Reviera Flatpak from the working tree and exports a single-file bundle:
#
#   packaging/flatpak/build.sh                  # build, then dist/BIM-Reviera-<ver>-<arch>.flatpak
#   packaging/flatpak/build.sh --install        # ... and install the bundle for this user
#   REVIERA_VERSION=0.1.0-pre.4 packaging/flatpak/build.sh
#
# Needs flatpak and flatpak-builder, plus python3 (to refresh cargo-sources.json when Cargo.lock
# changes). The GNOME 51 SDK and the LLVM extension are installed from Flathub (user
# installation) when missing. The first build compiles Boost, OCCT, IfcOpenShell, wxWidgets and
# the Rust workspace: about an hour; ccache and flatpak-builder's module cache make later builds
# much faster. State lives in packaging/flatpak/.flatpak-builder (git-ignored).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
APP_ID="io.github.rational_boxes.Reviera"
MANIFEST="$HERE/$APP_ID.yml"
STATE="$HERE/.flatpak-builder"
BUILD_DIR="$HERE/build-dir"
REPO="$HERE/repo"
DIST="$ROOT/dist"
ARCH="$(flatpak --default-arch)"
VERSION="${REVIERA_VERSION:-0.1.0-pre.4}"
BUNDLE="$DIST/BIM-Reviera-$VERSION-$ARCH.flatpak"

INSTALL=0
for arg in "$@"; do
    case "$arg" in
        --install) INSTALL=1 ;;
        -h|--help) sed -n '4,16p' "$0"; exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

# flatpak-cargo-generator, pinned by commit and checksum.
GEN_COMMIT=74697c75b630d7330e77250fc13cb5ea688d9479
GEN_SHA256=0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380
GEN_URL="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$GEN_COMMIT/cargo/flatpak-cargo-generator.py"

refresh_cargo_sources() {
    local sources="$HERE/cargo-sources.json" lock="$ROOT/Cargo.lock"
    if [ -f "$sources" ] && [ "$sources" -nt "$lock" ]; then
        return
    fi
    echo "==> Cargo.lock changed: regenerating cargo-sources.json"
    mkdir -p "$STATE/tools"
    local gen="$STATE/tools/flatpak-cargo-generator.py"
    if [ ! -f "$gen" ] || ! echo "$GEN_SHA256  $gen" | sha256sum -c --status; then
        curl -fsSL -o "$gen" "$GEN_URL"
        echo "$GEN_SHA256  $gen" | sha256sum -c --status \
            || { echo "flatpak-cargo-generator.py checksum mismatch" >&2; exit 1; }
    fi
    local venv="$STATE/tools/venv"
    if [ ! -x "$venv/bin/python" ]; then
        python3 -m venv "$venv"
        "$venv/bin/pip" install --quiet aiohttp tomlkit
    fi
    "$venv/bin/python" "$gen" "$lock" -o "$sources.tmp"
    mv "$sources.tmp" "$sources"
}

refresh_cargo_sources

if ! flatpak remotes --user --columns=name | grep -qx flathub; then
    flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
fi

echo "==> Building $APP_ID $VERSION ($ARCH)"
flatpak-builder \
    --user \
    --install-deps-from=flathub \
    --ccache \
    --force-clean \
    --state-dir="$STATE" \
    --repo="$REPO" \
    --default-branch=stable \
    --subject="BIM Reviera $VERSION" \
    "$BUILD_DIR" "$MANIFEST"

mkdir -p "$DIST"
echo "==> Exporting $BUNDLE"
flatpak build-bundle \
    --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
    "$REPO" "$BUNDLE" "$APP_ID" stable
ls -lh "$BUNDLE"

if [ "$INSTALL" = 1 ]; then
    flatpak install --user -y --noninteractive --reinstall --bundle "$BUNDLE"
    echo "Run it with: flatpak run $APP_ID gui"
fi
