// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Error reports for failures that would otherwise be silent.
//!
//! - A panic in a window event is caught by wxDragon and the event abandoned without a word; one
//!   elsewhere closes the application. Either way the panic hook writes a report, and the report
//!   is shown in a dialog as soon as the window is idle.
//! - A native crash (`native/crash.c`) writes a short report; Windows says so at once.
//! - A report not shown before the application closed is shown when it next opens.
//!
//! `REVIERA_CRASH_TEST=panic` (a failure in a window event) or `=segfault` (a native crash) makes
//! the window fail once it is up, to check the reporting on a platform.
//!
//! Reports are text files in `<data dir>/Reviera/crash-reports` (new ones in `new/` until shown).
//! A panic inside a command is not reported here: the command fails with an error the caller
//! sees, and its changes are rolled back.

use std::ffi::{CString, c_char};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use wxdragon::prelude::*;

unsafe extern "C" {
    fn reviera_crash_install(path: *const c_char, header: *const c_char, message: *const c_char);
    fn reviera_crash_mark();
}

/// Reports written in this session and not yet shown.
static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// `<data dir>/Reviera/crash-reports`, beside the recovery store.
pub fn dir() -> PathBuf {
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(home)
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
    };
    base.join("Reviera").join("crash-reports")
}

/// UTC `YYYY-MM-DD HH:MM:SS` and `YYYYMMDD-HHMMSS` for now.
fn now() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (h, mi, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    (
        format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC"),
        format!("{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}"),
    )
}

fn header(when: &str) -> String {
    format!(
        "NoDesk Reviera {} error report\nTime: {when}\nSystem: {} {}\n\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Installs the panic hook and the native crash handlers. Call once, before the window opens.
pub fn install() {
    let new = dir().join("new");
    let _ = std::fs::create_dir_all(&new);
    let (when, stamp) = now();
    let native = new.join(format!("crash-{stamp}-{}.txt", std::process::id()));
    let message = format!(
        "NoDesk Reviera has stopped because of an error.\n\nA report was saved to:\n{}\n\n\
         Unsaved work is kept: Reviera offers to recover it when it opens again.",
        native.display()
    );
    if let (Ok(p), Ok(h), Ok(m)) = (
        CString::new(native.to_string_lossy().as_bytes()),
        CString::new(header(&when)),
        CString::new(message),
    ) {
        // SAFETY: the strings are copied by the shim.
        unsafe { reviera_crash_install(p.as_ptr(), h.as_ptr(), m.as_ptr()) };
    }

    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if reviera_core::command::panic_is_recovered() {
            // Reported as the command's error.
            default(info);
            return;
        }
        let (when, stamp) = now();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".to_owned());
        let thread = std::thread::current();
        let report = format!(
            "{}Error: {message}\nAt: {}\nThread: {}\n\nBacktrace:\n{}\n",
            header(&when),
            info.location()
                .map_or_else(|| "unknown".to_owned(), |l| l.to_string()),
            thread.name().unwrap_or("unnamed"),
            trimmed(&std::backtrace::Backtrace::force_capture().to_string()),
        );
        let path = dir()
            .join("new")
            .join(format!("error-{stamp}-{}.txt", std::process::id()));
        if std::fs::write(&path, &report).is_ok() {
            if let Ok(mut p) = PENDING.lock() {
                p.push(path);
            }
            // A following abort is the same failure.
            // SAFETY: sets a flag.
            unsafe { reviera_crash_mark() };
        }
        default(info);
    }));
}

/// A backtrace from the frame that panicked: the hook's and the panic machinery's frames before
/// it are left out.
fn trimmed(backtrace: &str) -> String {
    let lines: Vec<&str> = backtrace.lines().collect();
    // Frames start `  N: name`. The machinery runs from the top to the first panic function
    // (`panic_fmt`, `begin_panic`, `core::panicking::…`) and the frames right after it that are
    // panic functions too.
    let is_frame = |l: &str| {
        l.trim_start()
            .split_once(':')
            .is_some_and(|(n, _)| n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty())
    };
    let is_panic = |l: &str| {
        l.contains("panic_fmt")
            || l.contains("begin_panic")
            || l.contains("core::panicking::")
            || l.contains("rust_begin_unwind")
    };
    let frames: Vec<usize> = (0..lines.len()).filter(|&i| is_frame(lines[i])).collect();
    let start = frames
        .iter()
        .position(|&i| is_panic(lines[i]))
        .and_then(|k| frames[k..].iter().find(|&&i| !is_panic(lines[i])).copied())
        .unwrap_or(0);
    lines[start..].join("\n")
}

/// Reports not shown yet: this session's, then any left by an earlier one.
fn unseen() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir().join("new"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    found.sort();
    found
}

/// Moves a shown report out of `new/`; returns where it is now.
fn mark_seen(path: &Path) -> PathBuf {
    let to = dir().join(path.file_name().unwrap_or_default());
    match std::fs::rename(path, &to) {
        Ok(()) => to,
        Err(_) => path.to_owned(),
    }
}

/// Shows the reports left by an earlier session that closed unexpectedly.
pub fn show_earlier(frame: &Frame) {
    let found = unseen();
    if found.is_empty() {
        return;
    }
    let intro = if found.len() == 1 {
        "Reviera closed unexpectedly last time. This is the report it saved:".to_owned()
    } else {
        format!(
            "Reviera closed unexpectedly. It saved {} reports; the latest is below:",
            found.len()
        )
    };
    let last = found.last().cloned().unwrap_or_default();
    let text = std::fs::read_to_string(&last).unwrap_or_default();
    let shown: Vec<PathBuf> = found.iter().map(|p| mark_seen(p)).collect();
    report_dialog(frame, &intro, &text, shown.last().map(PathBuf::as_path));
}

/// Shows the reports this session has written since the last look (call when idle).
pub fn show_pending(frame: &Frame) {
    let pending: Vec<PathBuf> = match PENDING.try_lock() {
        Ok(mut p) if !p.is_empty() => std::mem::take(&mut *p),
        _ => return,
    };
    let last = pending.last().cloned().unwrap_or_default();
    let text = std::fs::read_to_string(&last).unwrap_or_default();
    let shown: Vec<PathBuf> = pending.iter().map(|p| mark_seen(p)).collect();
    let intro = "Reviera hit an internal error and stopped what it was doing. The model may not \
                 show that action's result. Saving your work under a new name is a good \
                 precaution; please include this report if you file an issue.";
    report_dialog(frame, intro, &text, shown.last().map(PathBuf::as_path));
}

/// The failure `REVIERA_CRASH_TEST` asks for, if any.
pub fn test_failure() -> Option<String> {
    std::env::var("REVIERA_CRASH_TEST")
        .ok()
        .filter(|k| !k.is_empty())
}

/// Fails as `kind` says (see the module notes).
pub fn fail(kind: &str) {
    if kind == "segfault" {
        // SAFETY: deliberately not: a write to address 0 for the native handler to report.
        unsafe { std::ptr::write_volatile(std::ptr::null_mut::<u8>(), 1) };
    }
    panic!("a test failure (REVIERA_CRASH_TEST={kind})");
}

const ID_FOLDER: i32 = 7101;

fn report_dialog(frame: &Frame, intro: &str, report: &str, path: Option<&Path>) {
    let dialog = Dialog::builder(frame, "NoDesk Reviera: Error Report")
        .with_style(DialogStyle::DefaultDialogStyle | DialogStyle::ResizeBorder)
        .build();
    let sizer = BoxSizer::builder(Orientation::Vertical).build();
    let text = StaticText::builder(&dialog).with_label(intro).build();
    text.wrap(560);
    sizer.add(&text, 0, SizerFlag::All | SizerFlag::Expand, 10);
    let body = TextCtrl::builder(&dialog)
        .with_value(report)
        .with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly | TextCtrlStyle::DontWrap)
        .build();
    body.set_min_size(Size::new(600, 300));
    sizer.add(
        &body,
        1,
        SizerFlag::Left | SizerFlag::Right | SizerFlag::Expand,
        10,
    );
    if let Some(p) = path {
        let saved = StaticText::builder(&dialog)
            .with_label(&format!("Saved as {}", p.display()))
            .build();
        sizer.add(&saved, 0, SizerFlag::All | SizerFlag::Expand, 10);
    }
    let folder = Button::builder(&dialog)
        .with_id(ID_FOLDER)
        .with_label("Open Reports Folder")
        .build();
    let close = Button::builder(&dialog)
        .with_id(ID_OK)
        .with_label("Close")
        .build();
    let buttons = BoxSizer::builder(Orientation::Horizontal).build();
    buttons.add(&folder, 0, SizerFlag::All, 6);
    buttons.add_stretch_spacer(1);
    buttons.add(&close, 0, SizerFlag::All, 6);
    sizer.add_sizer(&buttons, 0, SizerFlag::Expand, 0);
    folder.on_click(move |_| {
        launch_default_application(&dir().to_string_lossy());
    });
    dialog.set_sizer_and_fit(sizer, true);
    dialog.set_affirmative_id(ID_OK);
    close.set_default();
    dialog.show_modal();
    dialog.destroy();
}
