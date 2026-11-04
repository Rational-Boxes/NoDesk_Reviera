// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Remote computation (spec Ch 2 §2.17): Riviera's client, distillations, the local compute
//! host and the built-in runner's job kinds. The contract itself, and the runner toolkit, are
//! in the MIT crate `riviera-compute-proto`.

pub mod client;
pub mod distill;
pub mod energyplus;
pub mod host;
pub mod kinds;
pub mod paths;

pub use client::{ComputeClient, ComputeError, LocalJob};
pub use riviera_compute_proto as proto;
