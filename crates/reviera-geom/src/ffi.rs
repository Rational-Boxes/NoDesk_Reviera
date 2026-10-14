// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The `cxx` bridge to `cpp/shim.cpp`. Every call returns `Result`, so OCCT's exceptions become
//! Rust errors instead of aborting.

#[cxx::bridge(namespace = "reviera_geom")]
pub(crate) mod bridge {
    /// Closed triangle meshes: positions `[x, y, z, …]`, triangles as index triples, and how
    /// many triangles each closed part has (in order).
    struct Meshes {
        verts: Vec<f64>,
        faces: Vec<u32>,
        parts: Vec<u32>,
    }

    unsafe extern "C++" {
        include!("reviera-geom/cpp/shim.h");

        /// `op`: 0 subtract (A − B), 1 intersect, 2 union. `tolerance` in metres.
        fn mesh_boolean(op: u8, a: &Meshes, b: &Meshes, tolerance: f64) -> Result<Meshes>;
        fn occt_version() -> String;
    }
}
