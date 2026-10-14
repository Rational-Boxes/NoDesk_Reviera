// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Compiles the OCCT shim and links OpenCASCADE (shared). OCCT comes from `OCC_INCLUDE_DIR` /
//! `OCC_LIBRARY_DIR` (defaults: Fedora's `opencascade-devel`).

use std::env;

/// The OCCT toolkits the shim uses, dependants first.
const OCCT_LIBS: &[&str] = &[
    "TKBO",
    "TKBool",
    "TKMesh",
    "TKShHealing",
    "TKTopAlgo",
    "TKGeomAlgo",
    "TKBRep",
    "TKGeomBase",
    "TKG3d",
    "TKG2d",
    "TKMath",
    "TKernel",
];

fn main() {
    let occ_include =
        env::var("OCC_INCLUDE_DIR").unwrap_or_else(|_| "/usr/include/opencascade".into());
    let occ_lib = env::var("OCC_LIBRARY_DIR").unwrap_or_else(|_| "/usr/lib64".into());
    for var in ["OCC_INCLUDE_DIR", "OCC_LIBRARY_DIR"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    cxx_build::bridge("src/ffi.rs")
        .file("cpp/shim.cpp")
        .std("c++17")
        .flag("-isystem")
        .flag(&occ_include)
        .compile("reviera_geom_shim");
    println!("cargo:rustc-link-search=native={occ_lib}");
    for lib in OCCT_LIBS {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }
    println!("cargo:rerun-if-changed=cpp/shim.cpp");
    println!("cargo:rerun-if-changed=cpp/shim.h");
    println!("cargo:rerun-if-changed=src/ffi.rs");
}
