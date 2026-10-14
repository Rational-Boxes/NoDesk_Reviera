// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

#pragma once

#include "rust/cxx.h"

namespace reviera_geom {

struct Meshes;

Meshes mesh_boolean(uint8_t op, const Meshes &a, const Meshes &b, double tolerance);
rust::String occt_version();

} // namespace reviera_geom
