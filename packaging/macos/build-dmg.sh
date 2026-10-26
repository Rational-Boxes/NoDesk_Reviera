#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Reviera contributors
#
# Builds "BIM Reviera.app" and a .dmg on macOS (run it on a Mac; it cannot cross-build).
#
#   packaging/macos/build-dmg.sh
#   REVIERA_VERSION=0.1.0-pre.1 packaging/macos/build-dmg.sh
#   SKIP_BREW=1 SKIP_IFCOS=1 packaging/macos/build-dmg.sh      # reuse deps and IfcOpenShell
#
# Prerequisites: Xcode command-line tools (clang, libclang for bindgen, codesign, hdiutil,
# iconutil, sips), Homebrew, and rustup (rust-toolchain.toml selects Rust 1.99).
#
# Output: dist/BIM-Reviera-<version>-macos-<arch>.dmg holding "BIM Reviera.app" and an
# Applications link. The app carries its non-system dylibs in Contents/Frameworks, so it runs
# without Homebrew. It is ad-hoc signed unless CODESIGN_IDENTITY is set.
#
# Optional signing and notarisation (all through environment variables):
#   CODESIGN_IDENTITY="Developer ID Application: Name (TEAMID)"   sign with hardened runtime
#   NOTARY_PROFILE=<keychain profile>   notarise the DMG with `xcrun notarytool submit
#                                       --keychain-profile` (create it once with
#                                       `xcrun notarytool store-credentials`), then staple.
#
# The build is for the host architecture only (Homebrew's libraries are single-architecture);
# the universal binary of TECH_STACK §5.4 needs both Homebrew prefixes and `lipo`, later.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
VERSION="${REVIERA_VERSION:-0.1.0-pre.1}"
# CFBundleVersion must be numbers and dots.
BUNDLE_VERSION="${VERSION%%-*}"
BUNDLE_ID="io.github.rational_boxes.Reviera"
APP_NAME="BIM Reviera"
ARCH="$(uname -m)"
WORK="$HERE/build"
APP="$WORK/$APP_NAME.app"
DIST="$ROOT/dist"
DMG="$DIST/BIM-Reviera-$VERSION-macos-$ARCH.dmg"
JOBS="$(sysctl -n hw.ncpu)"

[ "$(uname -s)" = Darwin ] || { echo "build-dmg.sh runs on macOS only" >&2; exit 1; }
command -v brew >/dev/null || { echo "Homebrew is required: https://brew.sh" >&2; exit 1; }
command -v cargo >/dev/null || { echo "rustup/cargo is required: https://rustup.rs" >&2; exit 1; }

step() { printf '\n==> %s\n' "$*"; }

# ------------------------------------------------------------------ 1. Homebrew dependencies
BREW_DEPS=(opencascade luajit boost eigen nlohmann-json cmake ninja pkgconf dylibbundler)
if [ -z "${SKIP_BREW:-}" ]; then
    step "Homebrew dependencies: ${BREW_DEPS[*]}"
    brew install "${BREW_DEPS[@]}"
fi
BREW="$(brew --prefix)"
OCC="$(brew --prefix opencascade)"
EIGEN="$(brew --prefix eigen)"
occ_version="$(sed -n 's/^#define OCC_VERSION_COMPLETE *"\(.*\)"/\1/p' "$OCC/include/opencascade/Standard_Version.hxx")"
echo "OpenCASCADE $occ_version (Reviera is developed against 7.9)"

# ------------------------------------------------------------------ 2. IfcOpenShell 0.8.5
# tools/build-ifcopenshell.sh is portable once OCCT's location and the job count are given; CMake
# finds Boost and Eigen under the Homebrew prefix.
IFCOS_PREFIX="$ROOT/third_party/install"
if [ -z "${SKIP_IFCOS:-}" ] || [ ! -f "$IFCOS_PREFIX/include/ifcparse/IfcFile.h" ]; then
    step "IfcOpenShell (tools/build-ifcopenshell.sh)"
    OCC_INCLUDE_DIR="$OCC/include/opencascade" \
    OCC_LIBRARY_DIR="$OCC/lib" \
    IFCOS_JOBS="$JOBS" \
    CMAKE_PREFIX_PATH="$BREW" \
        "$ROOT/tools/build-ifcopenshell.sh"
fi
IFCOS_LIB="$IFCOS_PREFIX/lib"
[ -f "$IFCOS_LIB/libIfcParse.a" ] || IFCOS_LIB="$IFCOS_PREFIX/lib64"

# reviera-ifc links boost_system. Boost 1.89 stopped building that (header-only) stub library;
# give the linker an empty one if Homebrew's Boost no longer has it.
if [ ! -e "$BREW/lib/libboost_system.dylib" ]; then
    echo "Boost has no libboost_system: linking an empty stand-in"
    printf '' | clang -dynamiclib -x c - -o "$IFCOS_LIB/libboost_system.dylib" \
        -install_name @rpath/libboost_system.dylib
fi

# ------------------------------------------------------------------ 3. cargo build --release
step "cargo build --release"
export OCC_INCLUDE_DIR="$OCC/include/opencascade"
export OCC_LIBRARY_DIR="$OCC/lib"
export REVIERA_IFCOS_PREFIX="$IFCOS_PREFIX"
# The C++ shims include Boost and Eigen headers; Boost and LuaJIT link from the Homebrew prefix.
export CPLUS_INCLUDE_PATH="$EIGEN/include/eigen3:$BREW/include${CPLUS_INCLUDE_PATH:+:$CPLUS_INCLUDE_PATH}"
export LIBRARY_PATH="$BREW/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
export PKG_CONFIG_PATH="$BREW/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
# Homebrew bottles target the running macOS, so the bundle cannot claim an older one.
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-$(sw_vers -productVersion | cut -d. -f1).0}"
(cd "$ROOT" && cargo build --release --locked -p reviera -p reviera-compute --bins)
BIN="$ROOT/target/release/reviera"

# ------------------------------------------------------------------ 4. The .app bundle
step "Assembling $APP_NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$BIN" "$APP/Contents/MacOS/reviera"
# The local compute host and the built-in runner, beside reviera (DM-RPC-004).
cp "$ROOT/target/release/reviera-compute-host" "$ROOT/target/release/reviera-analysis-builtin" \
    "$APP/Contents/MacOS/"

# `reviera` without arguments is the command-line tool; the bundle's executable opens the window.
cat > "$APP/Contents/MacOS/BIM Reviera" <<'EOF'
#!/bin/sh
# Launches the Reviera window; the files Finder passes are opened as models.
exec "$(dirname "$0")/reviera" gui "$@"
EOF
chmod 755 "$APP/Contents/MacOS/BIM Reviera"

# Samples where crates/reviera-ui/src/help.rs samples_dir() finds them (Contents/Resources).
mkdir -p "$APP/Contents/Resources/samples"
cp "$ROOT"/test-data/samples/samples.json "$ROOT"/test-data/samples/*.ifc \
   "$ROOT"/test-data/samples/*.lua "$ROOT"/test-data/samples/*.md "$APP/Contents/Resources/samples/"
# Samples' resources (their weather files), beside them.
cp -R "$ROOT"/test-data/samples/*.resources "$APP/Contents/Resources/samples/"
# The sample Component Library beside the samples (crates/reviera-ui/src/components.rs).
rm -rf "$APP/Contents/Resources/library" && cp -R "$ROOT/test-data/library" "$APP/Contents/Resources/library"
cp "$ROOT/LICENSE" "$APP/Contents/Resources/COPYING"
cp "$ROOT/packaging/THIRD-PARTY-NOTICES.txt" "$APP/Contents/Resources/"

# Icon: an iconset from the 1024 px rendering of packaging/icons/reviera.svg.
ICONSET="$WORK/reviera.iconset"
rm -rf "$ICONSET"; mkdir -p "$ICONSET"
SRC_PNG="$ROOT/packaging/icons/reviera-1024.png"
for s in 16 32 128 256 512; do
    sips -z "$s" "$s" "$SRC_PNG" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
    d=$((s * 2))
    sips -z "$d" "$d" "$SRC_PNG" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Reviera.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>BIM Reviera</string>
  <key>CFBundleIconFile</key><string>Reviera</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$BUNDLE_VERSION</string>
  <key>CFBundleVersion</key><string>$BUNDLE_VERSION</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>NSHumanReadableCopyright</key><string>Copyright © 2026 Reviera contributors. GPL-3.0-or-later.</string>
  <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>IFC Building Model</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSHandlerRank</key><string>Default</string>
      <key>CFBundleTypeIconFile</key><string>Reviera</string>
      <key>LSItemContentTypes</key><array><string>org.buildingsmart.ifc</string></array>
      <key>CFBundleTypeExtensions</key><array><string>ifc</string></array>
    </dict>
  </array>
  <key>UTImportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeIdentifier</key><string>org.buildingsmart.ifc</string>
      <key>UTTypeDescription</key><string>IFC Building Model</string>
      <key>UTTypeConformsTo</key><array><string>public.plain-text</string></array>
      <key>UTTypeTagSpecification</key>
      <dict>
        <key>public.filename-extension</key><array><string>ifc</string></array>
      </dict>
    </dict>
  </array>
</dict>
</plist>
EOF
plutil -lint "$APP/Contents/Info.plist"

# ------------------------------------------------------------------ 5. Bundle the dylibs
# Copies every non-system library (OCCT, Boost, LuaJIT, FreeType, TBB, ...) into Frameworks and
# rewrites the install names to @executable_path/../Frameworks.
step "Bundling dylibs"
dylibbundler --overwrite-dir --bundle-deps --create-dir \
    --fix-file "$APP/Contents/MacOS/reviera" \
    --dest-dir "$APP/Contents/Frameworks/" \
    --install-path @executable_path/../Frameworks/ \
    --search-path "$BREW/lib" --search-path "$OCC/lib" --search-path "$IFCOS_LIB"
# Nothing may still point into Homebrew.
if otool -L "$APP/Contents/MacOS/reviera" "$APP"/Contents/Frameworks/*.dylib | grep -E "$BREW|/usr/local/(opt|Cellar)" ; then
    echo "error: Homebrew paths remain in the bundle (see above)" >&2
    exit 1
fi

# ------------------------------------------------------------------ 6. Sign
step "Signing"
if [ -n "${CODESIGN_IDENTITY:-}" ]; then
    # The hardened runtime (needed for notarisation) forbids writable executable memory unless
    # entitled; LuaJIT's JIT compiler needs it.
    ENTITLEMENTS="$WORK/entitlements.plist"
    cat > "$ENTITLEMENTS" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.cs.allow-jit</key><true/>
  <key>com.apple.security.cs.allow-unsigned-executable-memory</key><true/>
</dict>
</plist>
PLIST
    # Libraries first, then the executables, then the bundle.
    find "$APP/Contents/Frameworks" -name '*.dylib' -exec \
        codesign --force --timestamp --options runtime -s "$CODESIGN_IDENTITY" {} +
    for exe in "$APP/Contents/MacOS/reviera" "$APP/Contents/MacOS/reviera-compute-host" \
        "$APP/Contents/MacOS/reviera-analysis-builtin" "$APP/Contents/MacOS/BIM Reviera"; do
        codesign --force --timestamp --options runtime --entitlements "$ENTITLEMENTS" \
            -s "$CODESIGN_IDENTITY" "$exe"
    done
    codesign --force --timestamp --options runtime --entitlements "$ENTITLEMENTS" \
        -s "$CODESIGN_IDENTITY" "$APP"
else
    # Ad-hoc: runs on this Mac, and elsewhere after the user allows it (right-click › Open).
    codesign --force --deep -s - "$APP"
fi
codesign --verify --deep --strict --verbose=2 "$APP"

# ------------------------------------------------------------------ 7. The disk image
step "Creating $DMG"
mkdir -p "$DIST"
STAGE="$WORK/dmg"
rm -rf "$STAGE"; mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
cp "$ROOT/packaging/THIRD-PARTY-NOTICES.txt" "$STAGE/"
rm -f "$DMG"
hdiutil create -volname "$APP_NAME $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$DMG"

if [ -n "${CODESIGN_IDENTITY:-}" ]; then
    codesign --force --timestamp -s "$CODESIGN_IDENTITY" "$DMG"
fi
if [ -n "${NOTARY_PROFILE:-}" ]; then
    [ -n "${CODESIGN_IDENTITY:-}" ] || { echo "notarisation needs CODESIGN_IDENTITY" >&2; exit 1; }
    step "Notarising"
    xcrun notarytool submit "$DMG" --keychain-profile "$NOTARY_PROFILE" --wait
    xcrun stapler staple "$DMG"
fi

ls -lh "$DMG"
echo "Done. Try it: open \"$APP\"   (or mount the DMG and drag the app to Applications)"
