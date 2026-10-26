// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The built-in analysis runner: `analysis.clash`, `analysis.structural`, `analysis.carbon`, `analysis.solar` and `test.fault`,
//! served on the remote computation contract (spec ANA-PLUG-003, ANA-CLASH-001, ANA-FEA-001).

use std::sync::Arc;

use reviera_compute::kinds::{CarbonKind, ClashKind, FaultKind, SolarKind, StructuralKind};
use reviera_compute_proto::runner::{Runner, main_with};

fn main() -> std::process::ExitCode {
    main_with(Runner::new(
        "reviera-analysis-builtin",
        env!("CARGO_PKG_VERSION"),
        vec![
            Arc::new(ClashKind),
            Arc::new(StructuralKind),
            Arc::new(CarbonKind),
            Arc::new(SolarKind),
            Arc::new(FaultKind),
        ],
    ))
}
