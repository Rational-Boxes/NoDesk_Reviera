// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Remote computation (spec Ch 2 §2.17): Reviera's client, distillations, the local compute
//! host and the built-in runner's job kinds. The contract itself, and the runner toolkit, are
//! in the MIT crate `reviera-compute-proto`.

pub mod client;
pub mod distill;
pub mod energyplus;
pub mod host;
pub mod kinds;
pub mod paths;

pub use client::{ComputeClient, ComputeError, LocalJob};
pub use reviera_compute_proto as proto;
