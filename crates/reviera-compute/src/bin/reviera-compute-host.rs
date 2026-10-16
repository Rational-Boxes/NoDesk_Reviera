// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The local compute host: started on demand by Reviera, exits when idle (spec DM-RPC-004).

fn main() -> std::process::ExitCode {
    reviera_compute::host::main()
}
