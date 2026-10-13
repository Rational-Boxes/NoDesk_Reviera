// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Compiles the GTK native-handle shim on Linux.

fn main() {
    println!("cargo:rerun-if-changed=native/gtk.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let gtk = pkg_config::Config::new()
        .probe("gtk+-3.0")
        .expect("GTK 3 development files (gtk3-devel)");
    // libwayland-client is present wherever GTK 3 was built with its Wayland backend.
    let wayland = pkg_config::Config::new().probe("wayland-client").ok();
    let mut build = cc::Build::new();
    build.file("native/gtk.c");
    for path in gtk
        .include_paths
        .iter()
        .chain(wayland.iter().flat_map(|w| &w.include_paths))
    {
        build.include(path);
    }
    build.compile("reviera_ui_native");
}
