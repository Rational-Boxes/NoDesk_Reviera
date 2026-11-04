// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! The local compute host: started on demand by Riviera, exits when idle (spec DM-RPC-004).

fn main() -> std::process::ExitCode {
    riviera_compute::host::main()
}
