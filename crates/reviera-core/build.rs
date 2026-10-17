// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Embeds the structural section catalogues (`data/sections/*.json`, STR-COL-001): a generated
//! list of `(file stem, contents)` pairs that `steel_sections::catalogs` reads.

use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/sections");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut out = String::from("&[\n");
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        let stem = f.file_stem().unwrap().to_string_lossy();
        out.push_str(&format!(
            "    ({stem:?}, include_str!({:?})),\n",
            f.display().to_string()
        ));
    }
    out.push(']');
    let target = Path::new(&std::env::var("OUT_DIR").unwrap()).join("section_catalogs.rs");
    std::fs::write(target, out).unwrap();
}
