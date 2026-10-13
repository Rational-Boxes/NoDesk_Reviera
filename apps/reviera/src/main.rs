// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

fn main() {
    let registry = reviera_core::command::CommandRegistry::new();
    println!(
        "NoDesk Reviera {} ({} commands registered)",
        env!("CARGO_PKG_VERSION"),
        registry.len()
    );
}
