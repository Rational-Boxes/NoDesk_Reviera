// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! Compiles the contract's `.proto` files with protox (pure Rust), so no `protoc` is needed.

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../schemas/compute");
    let files = [
        "reviera/compute/v1/compute.proto",
        "reviera/compute/v1/subscription.proto",
        "reviera/analysis/v1/analysis.proto",
        "reviera/testing/v1/testing.proto",
    ];
    for f in &files {
        println!("cargo:rerun-if-changed={}", root.join(f).display());
    }
    let fds = protox::compile(files, [&root]).expect("compile the contract's .proto files");
    // Type names, so options travel as `google.protobuf.Any` with their full names.
    let mut config = tonic_prost_build::Config::new();
    config.enable_type_names();
    tonic_prost_build::configure()
        .compile_fds_with_config(fds, config)
        .expect("generate Rust for the contract");
}
