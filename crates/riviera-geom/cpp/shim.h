// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

#pragma once

#include "rust/cxx.h"

namespace riviera_geom {

struct Meshes;

Meshes mesh_boolean(uint8_t op, const Meshes &a, const Meshes &b, double tolerance);
rust::String occt_version();

} // namespace riviera_geom
