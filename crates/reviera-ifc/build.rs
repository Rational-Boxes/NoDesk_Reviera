// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Compiles the C++ shim and links IfcOpenShell (static) with OpenCASCADE and Boost (shared).
//!
//! IfcOpenShell comes from `tools/build-ifcopenshell.sh`, installed under `third_party/install`;
//! override with `REVIERA_IFCOS_PREFIX`. OCCT comes from `OCC_INCLUDE_DIR` / `OCC_LIBRARY_DIR`
//! (defaults: Fedora's `opencascade-devel`).

use std::env;
use std::path::PathBuf;

/// IfcOpenShell libraries, dependants first.
const IFCOS_LIBS: &[&str] = &["IfcGeom", "geometry_kernel_opencascade", "IfcParse"];

/// The OCCT toolkits IfcGeom's OpenCASCADE kernel uses (as linked by IfcConvert).
const OCCT_LIBS: &[&str] = &[
    "TKBO",
    "TKBool",
    "TKFillet",
    "TKOffset",
    "TKHLR",
    "TKMesh",
    "TKPrim",
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

const BOOST_LIBS: &[&str] = &[
    "boost_date_time",
    "boost_iostreams",
    "boost_regex",
    "boost_system",
    "boost_thread",
];

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf();
    let prefix = env::var_os("REVIERA_IFCOS_PREFIX")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("third_party/install"));
    let occ_include =
        env::var("OCC_INCLUDE_DIR").unwrap_or_else(|_| "/usr/include/opencascade".into());
    let occ_lib = env::var("OCC_LIBRARY_DIR").unwrap_or_else(|_| "/usr/lib64".into());
    for var in ["REVIERA_IFCOS_PREFIX", "OCC_INCLUDE_DIR", "OCC_LIBRARY_DIR"] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    let include = prefix.join("include");
    if !include.join("ifcparse/IfcFile.h").exists() {
        panic!(
            "IfcOpenShell was not found under {}.\nBuild it with tools/build-ifcopenshell.sh, or set REVIERA_IFCOS_PREFIX.",
            prefix.display()
        );
    }
    let lib_dir = ["lib64", "lib"]
        .iter()
        .map(|d| prefix.join(d))
        .find(|d| d.join("libIfcParse.a").exists())
        .unwrap();

    cxx_build::bridge("src/ffi.rs")
        .file("cpp/shim.cpp")
        .std("c++17")
        // System includes, so warnings inside third-party headers are not reported.
        .flag("-isystem")
        .flag(include.to_str().unwrap())
        .flag("-isystem")
        .flag(&occ_include)
        .flag("-isystem")
        .flag("/usr/include/eigen3")
        .define("IFOPSH_WITH_OPENCASCADE", None)
        .define("HAS_SCHEMA_2x3", None)
        .define("HAS_SCHEMA_4", None)
        .define("HAS_SCHEMA_4x3_add2", None)
        .define("BOOST_ALL_NO_LIB", None)
        .compile("reviera_ifc_shim");

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-search=native={occ_lib}");
    for lib in IFCOS_LIBS {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    for lib in OCCT_LIBS.iter().chain(BOOST_LIBS) {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }
    println!("cargo:rerun-if-changed=cpp/shim.cpp");
    println!("cargo:rerun-if-changed=cpp/shim.h");
    println!("cargo:rerun-if-changed=src/ffi.rs");
    println!(
        "cargo:rerun-if-changed={}",
        lib_dir.join("libIfcParse.a").display()
    );
}
