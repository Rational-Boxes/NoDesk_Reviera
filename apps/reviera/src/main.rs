// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

use std::path::Path;
use std::process::ExitCode;

use reviera_core::CommandRegistry;
use reviera_store::LocalProject;

fn main() -> ExitCode {
    let registry = CommandRegistry::with_core();
    println!(
        "NoDesk Reviera {} ({} commands registered)",
        env!("CARGO_PKG_VERSION"),
        registry.len()
    );
    let Some(path) = std::env::args().nth(1) else {
        return ExitCode::SUCCESS;
    };
    match LocalProject::open(Path::new(&path)) {
        Ok((project, doc, report)) => {
            println!(
                "{path}: {} schema, {} elements ({} IDs from RXD, {} assigned), next ID {}",
                project.ifc().schema(),
                report.elements,
                report.ids_from_rxd,
                report.ids_assigned,
                doc.next_id()
            );
            for problem in &report.problems {
                println!("  warning: {problem}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{path}: {e}");
            ExitCode::FAILURE
        }
    }
}
