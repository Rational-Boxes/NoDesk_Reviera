// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Windows only: compiles `windows/reviera.rc` (the icon and the application manifest) and links
//! it into the program. Without the manifest Windows loads the old common controls library, which
//! lacks functions wxWidgets calls (GetWindowSubclass), and the program does not start.
//!
//! MinGW builds use `windres` (`<triple>-windres` when cross-compiling, or `WINDRES`); MSVC
//! builds use `rc.exe` (or `RC`) from a Visual Studio developer prompt.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=windows/reviera.rc");
    println!("cargo:rerun-if-changed=windows/reviera.exe.manifest");
    println!("cargo:rerun-if-changed=../../packaging/icons/reviera.ico");
    println!("cargo:rerun-if-env-changed=WINDRES");
    println!("cargo:rerun-if-env-changed=RC");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir")).join("windows");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let rc = dir.join("reviera.rc");
    let compiled = if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let res = out.join("reviera.res");
        let tool = std::env::var("RC").unwrap_or_else(|_| "rc.exe".into());
        let ok = Command::new(&tool)
            .current_dir(&dir)
            .arg("/nologo")
            .arg("/fo")
            .arg(&res)
            .arg(&rc)
            .status();
        (ok.is_ok_and(|s| s.success()), res, tool)
    } else {
        let obj = out.join("reviera-res.o");
        let target = std::env::var("TARGET").unwrap_or_default();
        let host = std::env::var("HOST").unwrap_or_default();
        let tool = std::env::var("WINDRES").unwrap_or_else(|_| {
            if target == host {
                "windres".into()
            } else {
                // x86_64-pc-windows-gnu → x86_64-w64-mingw32-windres
                format!("{}-w64-mingw32-windres", target.split('-').next().unwrap_or("x86_64"))
            }
        });
        let ok = Command::new(&tool)
            .current_dir(&dir)
            .arg("--input-format=rc")
            .arg("--output-format=coff")
            .arg("-i")
            .arg(&rc)
            .arg("-o")
            .arg(&obj)
            .status();
        (ok.is_ok_and(|s| s.success()), obj, tool)
    };
    let (ok, file, tool) = compiled;
    if !ok {
        panic!(
            "could not compile the Windows resources with `{tool}`; without the manifest the \
             program does not start on Windows (set WINDRES or RC)"
        );
    }
    println!("cargo:rustc-link-arg-bins={}", file.display());
}
