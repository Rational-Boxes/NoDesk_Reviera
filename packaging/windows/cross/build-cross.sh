#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Reviera contributors
#
# Cross-builds BIM Reviera for Windows x86-64 on Fedora, with the MinGW-w64 toolchain (GNU
# target, x86_64-pc-windows-gnu), and stages a portable folder and a ZIP:
#
#   packaging/windows/cross/build-cross.sh            # dependencies (cached), app, stage, zip
#   REVIERA_VERSION=0.1.0-pre.1 packaging/windows/cross/build-cross.sh
#
# Needs (Fedora): mingw64-gcc-c++ mingw64-boost mingw64-eigen3 mingw64-zlib
# mingw64-winpthreads-static, cmake, ninja-build, curl, and the Rust target
# (`rustup target add x86_64-pc-windows-gnu`). OpenCASCADE 7.9.3, LuaJIT and IfcOpenShell 0.8.5
# are cross-built from the same pinned sources as the Flatpak, under build/ (git-ignored).
# wxWidgets 3.3 is built by wxdragon-sys through mingw64.zigbuild-compatible.cmake.
#
# The installer (Inno Setup) is made here with ISCC under Wine (its own prefix under build/), or
# by packaging/windows/build-installer.ps1 on Windows.
set -euo pipefail

# The whole script is one function, read before it runs: editing this file while a build is
# running cannot change what that build does next.
main() {

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
BUILD="$HERE/build"
PREFIX="$BUILD/prefix"
DIST="$ROOT/dist"
VERSION="${REVIERA_VERSION:-0.1.0-pre.1}"
TRIPLE=x86_64-w64-mingw32
TARGET=x86_64-pc-windows-gnu
SYSROOT=/usr/$TRIPLE/sys-root/mingw
TOOLCHAIN="$HERE/mingw64.zigbuild-compatible.cmake"
JOBS="${JOBS:-$(nproc)}"

need() { command -v "$1" >/dev/null || { echo "missing: $1 ($2)" >&2; exit 1; }; }
need $TRIPLE-g++ "sudo dnf install mingw64-gcc-c++"
need cmake "sudo dnf install cmake"
need ninja "sudo dnf install ninja-build"
[ -d "$SYSROOT/include/boost" ] || { echo "missing: mingw64-boost" >&2; exit 1; }
[ -d "$SYSROOT/include/eigen3" ] || { echo "missing: mingw64-eigen3" >&2; exit 1; }
rustup target list --installed | grep -qx $TARGET \
    || { echo "missing: rustup target add $TARGET" >&2; exit 1; }

mkdir -p "$BUILD/src" "$PREFIX"
export REVIERA_CROSS_PREFIX="$PREFIX"

# fetch <name> <url> <sha256>: downloads and unpacks a pinned source tarball once.
fetch() {
    local name="$1" url="$2" sha="$3" archive="$BUILD/src/$1.tar.gz"
    if [ ! -d "$BUILD/src/$name" ]; then
        [ -f "$archive" ] || curl -fsSL -o "$archive" "$url"
        echo "$sha  $archive" | sha256sum -c --status || { echo "$name: checksum mismatch" >&2; exit 1; }
        mkdir -p "$BUILD/src/$name"
        tar -xf "$archive" -C "$BUILD/src/$name" --strip-components=1
    fi
}

# ---- OpenCASCADE 7.9.3 (shared DLLs; modelling only) ----
if [ ! -f "$PREFIX/lib/libTKernel.dll.a" ]; then
    fetch occt https://github.com/Open-Cascade-SAS/OCCT/archive/refs/tags/V7_9_3.tar.gz \
        5ecf094ec6b12d5413dfb851d8c3590c354058aee556e32e408bdfbf8c357d57
    cmake -S "$BUILD/src/occt" -B "$BUILD/occt" -G Ninja \
        -DCMAKE_TOOLCHAIN_FILE="$TOOLCHAIN" \
        -DCMAKE_BUILD_TYPE=Release \
        -DINSTALL_DIR="$PREFIX" -DINSTALL_DIR_LAYOUT=Unix -DINSTALL_DIR_LIB=lib \
        -DINSTALL_DIR_BIN=bin \
        -DBUILD_LIBRARY_TYPE=Shared \
        -DBUILD_MODULE_ApplicationFramework=OFF -DBUILD_MODULE_DataExchange=OFF \
        -DBUILD_MODULE_Draw=OFF -DBUILD_MODULE_Visualization=OFF \
        -DBUILD_MODULE_DETools=OFF -DBUILD_DOC_Overview=OFF \
        -DUSE_FREETYPE=OFF -DUSE_TK=OFF -DUSE_TCL=OFF -DUSE_TBB=OFF -DUSE_VTK=OFF \
        -DUSE_FREEIMAGE=OFF -DUSE_RAPIDJSON=OFF -DUSE_DRACO=OFF -DUSE_FFMPEG=OFF \
        -DUSE_OPENVR=OFF -DUSE_OPENGL=OFF -DUSE_GLES2=OFF -DUSE_D3D=OFF \
        -DINSTALL_SAMPLES=OFF -DINSTALL_TEST_CASES=OFF
    cmake --build "$BUILD/occt" --parallel "$JOBS"
    cmake --install "$BUILD/occt"
fi

# ---- LuaJIT (static, linked into the program) ----
if [ ! -f "$PREFIX/lib/libluajit.a" ]; then
    fetch luajit https://github.com/LuaJIT/LuaJIT/archive/707c12bf00dafdfd3899b1a6c36435dbbf6c7022.tar.gz \
        ac2defd1e9b4ab07ac0a52f2403318ffb0d54505b975dff0cfc49a927ab84d90
    make -C "$BUILD/src/luajit/src" clean >/dev/null
    make -C "$BUILD/src/luajit/src" -j"$JOBS" HOST_CC=gcc CROSS=$TRIPLE- TARGET_SYS=Windows \
        BUILDMODE=static libluajit.a
    install -Dm644 "$BUILD/src/luajit/src/libluajit.a" "$PREFIX/lib/libluajit.a"
fi

# ---- IfcOpenShell 0.8.5 (static libraries) ----
if [ ! -f "$PREFIX/lib/libIfcParse.a" ]; then
    fetch ifcopenshell https://github.com/IfcOpenShell/IfcOpenShell/archive/refs/tags/ifcopenshell-python-0.8.5.tar.gz \
        185b97d79c800603685c60abd1497df242050dbb117dcaccbaebdec0cdf25dd5
    # MinGW's real-number parser rejected a number that ends its token (Elevation 3.5, say).
    grep -q from_chars "$BUILD/src/ifcopenshell/src/ifcparse/IfcParse.cpp" \
        || patch -d "$BUILD/src/ifcopenshell" -p1 < "$HERE/patches/ifcopenshell-mingw-strtod.patch"
    cmake -S "$BUILD/src/ifcopenshell/cmake" -B "$BUILD/ifcopenshell" -G Ninja \
        -DCMAKE_TOOLCHAIN_FILE="$TOOLCHAIN" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$PREFIX" \
        -DCMAKE_CXX_FLAGS="-Wa,-mbig-obj" \
        -DBUILD_SHARED_LIBS=OFF \
        -DSCHEMA_VERSIONS="2x3;4;4x3_add2" \
        -DBUILD_IFCPYTHON=OFF -DBUILD_CONVERT=OFF -DBUILD_GEOMSERVER=OFF \
        -DWITH_OPENCASCADE=ON -DWITH_CGAL=OFF \
        -DCOLLADA_SUPPORT=OFF -DGLTF_SUPPORT=OFF -DHDF5_SUPPORT=OFF -DIFCXML_SUPPORT=OFF \
        -DUSE_CCACHE=OFF \
        -DOCC_INCLUDE_DIR="$PREFIX/include/opencascade" \
        -DOCC_LIBRARY_DIR="$PREFIX/lib" \
        -DBOOST_ROOT="$SYSROOT" \
        -DEIGEN_DIR="$SYSROOT/include/eigen3"
    cmake --build "$BUILD/ifcopenshell" --parallel "$JOBS"
    cmake --install "$BUILD/ifcopenshell"
fi

# ---- Reviera ----
export OCC_INCLUDE_DIR="$PREFIX/include/opencascade"
export OCC_LIBRARY_DIR="$PREFIX/lib"
export REVIERA_IFCOS_PREFIX="$PREFIX"
export REVIERA_EIGEN_INCLUDE="$SYSROOT/include/eigen3"
export REVIERA_BOOST_LIB_DIR="$SYSROOT/lib" REVIERA_BOOST_SUFFIX=-mt-x64
export LUA_LIB="$PREFIX/lib" LUA_LIB_NAME=luajit LUA_LINK=static
export CMAKE_TOOLCHAIN_FILE_x86_64_pc_windows_gnu="$TOOLCHAIN"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=$TRIPLE-gcc
export CC_x86_64_pc_windows_gnu=$TRIPLE-gcc CXX_x86_64_pc_windows_gnu=$TRIPLE-g++
export AR_x86_64_pc_windows_gnu=$TRIPLE-ar
# The IFC schema headers make very large objects (COFF big objects hold them); M_PI needs
# _USE_MATH_DEFINES with MinGW in strict C++17.
export CXXFLAGS_x86_64_pc_windows_gnu="-Wa,-mbig-obj -D_USE_MATH_DEFINES"
# libstdc++ is linked statically, like winpthread: with libstdc++-6.dll, std::call_once (used by
# std::async in IfcOpenShell's geometry iterator) called through a null pointer, because the
# exe's pthread_once and the DLL's once-callable were separate copies. -lstdc++ finds the
# stand-in libstdc++.dll.a first and is sent to the static library. wxdragon-sys's
# Linux-to-Windows path links `-lc++` (Zig's C++ library); a stand-in libc++.a points there too.
# (The OCCT DLLs keep using libstdc++-6.dll among themselves.)
mkdir -p "$BUILD/link-shims"
echo "INPUT($SYSROOT/lib/libstdc++.a)" > "$BUILD/link-shims/libstdc++.dll.a"
echo "INPUT($SYSROOT/lib/libstdc++.a)" > "$BUILD/link-shims/libc++.a"
# (Encoded, with 0x1f between arguments: the path may contain spaces.)
export CARGO_ENCODED_RUSTFLAGS="-L"$'\x1f'"native=$BUILD/link-shims"
(cd "$ROOT" && cargo build --release --target $TARGET -p reviera --bins)

# ---- Stage: the program, the DLLs it needs, samples and notices ----
STAGE="$DIST/windows/BIM-Reviera"
rm -rf "$STAGE"
mkdir -p "$STAGE/samples"
install -m755 "$ROOT/target/$TARGET/release/reviera.exe" "$STAGE/"
# The window without a console window: what the shortcuts and the file association start.
install -m755 "$ROOT/target/$TARGET/release/reviera-gui.exe" "$STAGE/"
# The DLLs it imports, transitively, from our prefix and the MinGW sysroot (not Windows').
search=("$PREFIX/bin" "$SYSROOT/bin")
queue=("$STAGE/reviera.exe")
while [ ${#queue[@]} -gt 0 ]; do
    f="${queue[0]}"; queue=("${queue[@]:1}")
    for dll in $($TRIPLE-objdump -p "$f" | sed -n 's/^\s*DLL Name: //p'); do
        [ -f "$STAGE/$dll" ] && continue
        for d in "${search[@]}"; do
            if [ -f "$d/$dll" ]; then
                install -m755 "$d/$dll" "$STAGE/"
                queue+=("$STAGE/$dll")
                break
            fi
        done
    done
done
# wxWidgets loads the WebView2 loader at run time (the help tab): the x64 one from the WebView2
# SDK its build downloaded.
loader=$(find "$ROOT/target/$TARGET/release" -name WebView2Loader.dll 2>/dev/null | head -1)
if [ -n "$loader" ]; then
    install -m755 "$loader" "$STAGE/"
else
    echo "warning: WebView2Loader.dll not found; the help tab will not open" >&2
fi
install -m644 "$ROOT"/test-data/samples/samples.json "$STAGE/samples/"
# The sample Component Library beside the samples (crates/reviera-ui/src/components.rs).
rm -rf "$STAGE/library" && cp -r "$ROOT/test-data/library" "$STAGE/library"
for n in $(python3 -c "import json;print(' '.join(s['name'] for s in json.load(open('$ROOT/test-data/samples/samples.json'))))"); do
    for ext in ifc lua md; do
        [ -f "$ROOT/test-data/samples/$n.$ext" ] && install -m644 "$ROOT/test-data/samples/$n.$ext" "$STAGE/samples/"
    done
done
install -m644 "$ROOT/LICENSE" "$STAGE/COPYING.txt"
install -m644 "$ROOT/packaging/THIRD-PARTY-NOTICES.txt" "$STAGE/"
install -m644 "$ROOT/packaging/icons/reviera.ico" "$STAGE/"

ZIP="$DIST/BIM-Reviera-$VERSION-windows-x64-portable.zip"
rm -f "$ZIP"
(cd "$DIST/windows" && python3 -m zipfile -c "$ZIP" BIM-Reviera)
echo "==> $ZIP ($(du -h "$ZIP" | cut -f1))"

# ---- The installer: Inno Setup under Wine, in a Wine prefix of the build's own ----
# (packaging/windows/cross/build/wine; install it once with
#  WINEPREFIX=… wine innosetup-7.1.0-x64.exe /VERYSILENT /CURRENTUSER "/DIR=C:\InnoSetup").
export WINEPREFIX="${WINEPREFIX:-$BUILD/wine}" WINEDEBUG="${WINEDEBUG:--all}"
ISCC="${REVIERA_ISCC:-$WINEPREFIX/drive_c/InnoSetup/ISCC.exe}"
if [ -f "$ISCC" ] && command -v wine >/dev/null; then
    wine "$ISCC" /Qp "/DAppVersion=$VERSION" "/DAppVersionNumeric=${VERSION%%-*}" \
        "/DStageDir=$(winepath -w "$STAGE")" "/DOutputDir=$(winepath -w "$DIST")" \
        "$(winepath -w "$ROOT/packaging/windows/reviera.iss")"
    ls -la "$DIST"/*.exe
else
    echo "==> No Inno Setup under Wine: the installer is not made (see packaging/README.md)"
fi
}

main "$@"
