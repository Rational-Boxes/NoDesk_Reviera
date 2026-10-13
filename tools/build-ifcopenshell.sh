#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Reviera contributors
#
# Builds the IfcOpenShell C++ libraries (IfcParse, IfcGeom with OpenCASCADE) that reviera-ifc
# links against. Sources, build tree and install prefix live under third_party/ (not committed).
#
#   tools/build-ifcopenshell.sh            # configure, build, install
#   IFCOS_JOBS=8 tools/build-ifcopenshell.sh
#
# Linux uses the distribution's OCCT (Fedora: opencascade-devel). Override OCC_INCLUDE_DIR and
# OCC_LIBRARY_DIR to use another build.
set -euo pipefail

IFCOS_TAG="${IFCOS_TAG:-ifcopenshell-python-0.8.5}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/third_party/IfcOpenShell"
BUILD="$ROOT/third_party/build/ifcopenshell"
PREFIX="$ROOT/third_party/install"
OCC_INCLUDE_DIR="${OCC_INCLUDE_DIR:-/usr/include/opencascade}"
OCC_LIBRARY_DIR="${OCC_LIBRARY_DIR:-/usr/lib64}"

if [ ! -d "$SRC/.git" ]; then
    git clone --depth 1 --branch "$IFCOS_TAG" https://github.com/IfcOpenShell/IfcOpenShell.git "$SRC"
fi

# Schemas: IFC4 is the default project schema, IFC4X3_ADD2 is offered in Local mode, and IFC2X3
# is read for older files (TECH_STACK §3.1, §3.7.3).
cmake -S "$SRC/cmake" -B "$BUILD" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
    -DBUILD_SHARED_LIBS=OFF \
    -DSCHEMA_VERSIONS="2x3;4;4x3_add2" \
    -DBUILD_IFCPYTHON=OFF \
    -DBUILD_CONVERT=ON \
    -DBUILD_GEOMSERVER=OFF \
    -DWITH_OPENCASCADE=ON \
    -DWITH_CGAL=OFF \
    -DCOLLADA_SUPPORT=OFF \
    -DGLTF_SUPPORT=OFF \
    -DHDF5_SUPPORT=OFF \
    -DIFCXML_SUPPORT=OFF \
    -DUSE_CCACHE=OFF \
    -DOCC_INCLUDE_DIR="$OCC_INCLUDE_DIR" \
    -DOCC_LIBRARY_DIR="$OCC_LIBRARY_DIR"

cmake --build "$BUILD" --parallel "${IFCOS_JOBS:-$(nproc)}"
cmake --install "$BUILD"
