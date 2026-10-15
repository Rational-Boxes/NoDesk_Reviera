// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Compiles the native shims:
//!
//! - `native/gtk.c` (Linux): native handles for the viewport.
//! - `native/crash.c`: reports of native crashes (see `src/crash.rs`).
//! - `native/wxshim.cpp`: wxWidgets features wxDragon does not wrap yet. It is built against the
//!   wxWidgets that `wxdragon-sys` built (same headers and flags), which is why `wxdragon-sys` is a
//!   direct dependency: its build script (`links = "wxdragon"`) then runs before this one.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=native/gtk.c");
    println!("cargo:rerun-if-changed=native/wxshim.cpp");
    println!("cargo:rerun-if-changed=native/crash.c");
    println!("cargo:rerun-if-env-changed=WXDRAGON_SYS_BUILD_DIR");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "linux" {
        gtk_shim();
    }
    wx_shim(&os);
    cc::Build::new()
        .file("native/crash.c")
        .warnings(false)
        .compile("reviera_ui_crash");
}

fn gtk_shim() {
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

/// The directory wxdragon-sys builds wxWidgets into (mirrors its build script).
fn wxdragon_build_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("WXDRAGON_SYS_BUILD_DIR") {
        return dir.into();
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let profile = std::env::var("PROFILE").expect("PROFILE");
    out.ancestors()
        .find(|p| p.file_name().is_some_and(|n| *n == *profile))
        .expect("the profile directory above OUT_DIR")
        .join("wxdragon_sys_cmake_build")
}

/// The directory holding `wx/setup.h` under `dir`.
fn find_setup(dir: &Path) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.join("wx").join("setup.h").is_file()
                && path.file_name() != Some("include".as_ref())
            {
                return Some(path);
            }
            if let Some(found) = find_setup(&path) {
                return Some(found);
            }
        }
    }
    None
}

fn wx_shim(os: &str) {
    let dir = wxdragon_build_dir();
    let mut build = cc::Build::new();
    build.cpp(true).file("native/wxshim.cpp").std("c++17");
    let wx_config = dir.join("bin").join("wx-config");
    let flags = Command::new(&wx_config)
        .arg("--cxxflags")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    match flags {
        Some(flags) => {
            // The flags are `-I<dir>` and `-D<macro>[=value]`; paths may contain spaces, so split
            // at the option prefixes rather than at whitespace.
            let flags = format!(" {}", flags.trim());
            for part in flags.split(" -").skip(1) {
                if let Some(path) = part.strip_prefix('I') {
                    build.include(path.trim());
                } else if let Some(def) = part.strip_prefix('D') {
                    let (name, value) = def.trim().split_once('=').unwrap_or((def.trim(), ""));
                    build.define(name, (!value.is_empty()).then_some(value));
                } else {
                    build.flag(format!("-{}", part.trim()));
                }
            }
        }
        // No wx-config (MSVC builds): the headers and the setup.h of the build.
        None => {
            let include = dir.join("include").join("wx-3.3");
            let include = if include.is_dir() {
                include
            } else {
                dir.join("include")
            };
            build.include(&include);
            let lib = dir.join("lib");
            if let Some(setup) = find_setup(&lib) {
                build.include(setup);
            }
            match os {
                "windows" => {
                    build
                        .define("__WXMSW__", None)
                        .define("UNICODE", None)
                        .define("_UNICODE", None);
                }
                "macos" => {
                    build
                        .define("__WXOSX_COCOA__", None)
                        .define("__WXMAC__", None)
                        .define("__WXOSX__", None);
                }
                _ => {}
            }
        }
    }
    build.warnings(false).compile("reviera_ui_wx");
    // The shim's wxWidgets symbols resolve against the libraries wxdragon-sys links (core, aui),
    // apart from the ribbon library, which wxdragon-sys builds but does not link.
    link_ribbon(&dir);
}

/// Links the wxWidgets ribbon library built next to the others (its name depends on the port).
fn link_ribbon(dir: &Path) {
    for sub in ["lib64", "lib"] {
        let lib = dir.join(sub);
        let Ok(entries) = std::fs::read_dir(&lib) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.contains("ribbon") {
                continue;
            }
            let stem = name
                .strip_suffix(".a")
                .map(|n| n.strip_prefix("lib").unwrap_or(n))
                .or_else(|| name.strip_suffix(".lib"));
            if let Some(stem) = stem {
                println!("cargo:rustc-link-search=native={}", lib.display());
                println!("cargo:rustc-link-lib=static={stem}");
                return;
            }
        }
    }
    println!(
        "cargo::warning=the wxWidgets ribbon library was not found under {}",
        dir.display()
    );
}
