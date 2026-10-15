// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Command-line entry point until the wxWidgets shell exists.
//!
//! - `reviera`: lists unsaved work left by a crash.
//! - `reviera FILE`: opens an IFC file and prints a summary.
//! - `reviera gui [FILE]`: the main window with the 3D viewport.
//! - `reviera shell [FILE]`: reads `<command> [JSON arguments]` lines and runs them as the user.
//! - `reviera --mcp-stdio [--permission LEVEL] [--toolsets a,b] [FILE]`: headless MCP server on
//!   stdin/stdout (spec DM-MCP-002). LEVEL is `read_only` (default), `model_edit` or `full_scripting`.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use reviera_app::{App, AppConfig};
use reviera_core::Caller;
use serde_json::{Value, json};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--mcp-stdio") {
        return mcp_stdio(&args[1..]);
    }
    // On Windows a double click on reviera.exe (a console of its own, no arguments) opens the
    // window too.
    if args.first().map(String::as_str) == Some("gui") || (args.is_empty() && own_console()) {
        release_console();
        return match reviera_ui::run(args.get(1).map(std::path::PathBuf::from)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => fail(&e),
        };
    }
    let mut app = App::new(AppConfig::default());
    println!(
        "NoDesk Reviera {} ({} commands)",
        env!("CARGO_PKG_VERSION"),
        app.registry().len()
    );
    match args.as_slice() {
        [] => report_recoverable(&app),
        [cmd, rest @ ..] if cmd == "shell" => shell(&mut app, rest.first()),
        [file] => summary(&mut app, file),
        _ => {
            eprintln!(
                "usage: reviera [FILE] | reviera shell [FILE] | reviera gui [FILE] | reviera --mcp-stdio [...]"
            );
            ExitCode::FAILURE
        }
    }
}

/// Windows: whether the console this program has was made for it alone (it was started from
/// Explorer or a shortcut, not from a terminal).
#[cfg(windows)]
fn own_console() -> bool {
    unsafe extern "system" {
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
    }
    let mut list = [0u32; 2];
    // SAFETY: the list holds two ids; the count of attached processes is returned.
    unsafe { GetConsoleProcessList(list.as_mut_ptr(), 2) == 1 }
}

#[cfg(not(windows))]
fn own_console() -> bool {
    false
}

/// Windows: closes the console window the window was started with, when nothing else uses it;
/// otherwise its closing would end the program. A terminal it was started from is left alone.
fn release_console() {
    #[cfg(windows)]
    if own_console() {
        unsafe extern "system" {
            fn FreeConsole() -> i32;
        }
        // SAFETY: detaches from the console; later writes to stdout and stderr go nowhere.
        unsafe { FreeConsole() };
    }
}

fn mcp_stdio(args: &[String]) -> ExitCode {
    use reviera_core::Permission;
    let mut permission = Permission::ReadOnly;
    let mut toolsets = Vec::new();
    let mut file = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--permission" => {
                permission = match it.next().map(String::as_str) {
                    Some("read_only") => Permission::ReadOnly,
                    Some("model_edit") => Permission::ModelEdit,
                    Some("full_scripting") => Permission::FullScripting,
                    other => return fail(&format!("unknown permission level {other:?}")),
                }
            }
            "--toolsets" => {
                toolsets = it
                    .next()
                    .map(|t| t.split(',').map(str::to_owned).collect())
                    .unwrap_or_default()
            }
            f if !f.starts_with("--") => file = Some(f.to_owned()),
            other => return fail(&format!("unknown option {other}")),
        }
    }
    // stdout carries the protocol; everything else goes to stderr.
    eprintln!(
        "NoDesk Reviera {} MCP server (stdio, {permission:?})",
        env!("CARGO_PKG_VERSION")
    );
    let app = reviera_mcp::AppHandle::spawn(move || {
        let mut app = App::new(AppConfig::default());
        if let Some(f) = file
            && let Err(e) = app.execute("file.open", json!({ "path": f }), &Caller::user())
        {
            eprintln!("{f}: {e}");
        }
        app
    });
    let mut config = reviera_mcp::McpConfig::new(permission);
    config.toolsets.extend(toolsets);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    match runtime.block_on(reviera_mcp::run_stdio(app, config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e.to_string()),
    }
}

fn report_recoverable(app: &App) -> ExitCode {
    match app.recoverable() {
        Ok(found) if found.is_empty() => ExitCode::SUCCESS,
        Ok(found) => {
            println!(
                "Unsaved work from an earlier session (restore with `recovery.restore` in `reviera shell`):"
            );
            for r in found {
                let path = r
                    .project_path
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "Untitled".into());
                let changes = r
                    .unsaved_changes
                    .map_or("unknown".into(), |n| n.to_string());
                println!(
                    "  {}  {path}  {changes} changes  {}",
                    r.key,
                    r.last_labels.join(" · ")
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}

fn summary(app: &mut App, file: &str) -> ExitCode {
    let user = Caller::user();
    let id = match app.execute("file.open", json!({"path": file, "read_only": true}), &user) {
        Ok(o) => o.result.as_u64().unwrap_or_default(),
        Err(e) => return fail(&e.to_string()),
    };
    let open = app.document(id).expect("just opened");
    let report = open.open_report.clone().unwrap_or_default();
    println!(
        "{file}: {} schema, {} elements ({} IDs from RXD, {} assigned), next ID {}",
        open.project.ifc().schema(),
        report.elements,
        report.ids_from_rxd,
        report.ids_assigned,
        open.doc.next_id()
    );
    for problem in &report.problems {
        println!("  warning: {problem}");
    }
    for note in &report.reconciled {
        println!("  from IFC: {note}");
    }
    ExitCode::SUCCESS
}

fn shell(app: &mut App, file: Option<&String>) -> ExitCode {
    let user = Caller::user();
    if let Some(f) = file
        && let Err(e) = app.execute("file.open", json!({ "path": f }), &user)
    {
        return fail(&e.to_string());
    }
    println!("Type `<command> [JSON arguments]`, `:commands [prefix]`, `:docs` or `:quit`.");
    let stdin = std::io::stdin();
    loop {
        print!("> ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            // End of input: keep unsaved work recoverable rather than discarding it.
            app.flush_all();
            if app.documents().iter().any(|d| d.dirty) {
                println!("\nUnsaved work is kept for recovery.");
            }
            return ExitCode::SUCCESS;
        }
        let line = line.trim();
        let (cmd, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        match cmd {
            "" => continue,
            ":quit" => match app.execute("app.exit", Value::Null, &user) {
                Ok(_) => return ExitCode::SUCCESS,
                Err(e) => println!("{e} (save, or use `app.exit {{\"force\": true}}`)"),
            },
            ":docs" => print_json(&json!(app.documents())),
            ":commands" => {
                for s in app
                    .registry()
                    .iter()
                    .filter(|s| s.name.starts_with(rest.trim()))
                {
                    println!("  {:<28} {}", s.name, s.summary);
                }
            }
            _ => {
                let args = if rest.trim().is_empty() {
                    Ok(Value::Null)
                } else {
                    serde_json::from_str(rest)
                };
                match args {
                    Ok(args) => match app.execute(cmd, args, &user) {
                        Ok(outcome) => print_json(&json!(outcome)),
                        Err(e) => print_json(&json!({ "error": e })),
                    },
                    Err(e) => println!("arguments are not valid JSON: {e}"),
                }
            }
        }
    }
}

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("{msg}");
    ExitCode::FAILURE
}
