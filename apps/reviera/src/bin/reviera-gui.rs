// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `reviera-gui`: opens the Reviera window without a console window (Windows).
//!
//! `reviera.exe` is a console program, because it is also the command-line tool and the MCP
//! server; started from a shortcut it would bring an empty console window, whose closing ends the
//! program. This small launcher is a Windows (GUI) program: it starts `reviera.exe gui` beside it
//! with no console and returns. The shortcuts and the file association use it. Its arguments (a
//! file to open) are passed on.

#![windows_subsystem = "windows"]

use std::process::Command;

fn main() {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| {
            p.parent().map(|d| {
                d.join(if cfg!(windows) {
                    "reviera.exe"
                } else {
                    "reviera"
                })
            })
        })
        .unwrap_or_else(|| "reviera".into());
    let mut command = Command::new(exe);
    command.arg("gui").args(std::env::args_os().skip(1));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// The child is a console program run without a console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    if let Err(e) = command.spawn() {
        eprintln!("could not start reviera: {e}");
        std::process::exit(1);
    }
}
