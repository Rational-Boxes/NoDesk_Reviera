// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The Terminal pane (UI-AGENT-004): a shell and command-line AI agents in real terminals, the
//! agents started connected to the open model through Reviera's MCP server.
//!
//! - Each tab runs its program in a pseudo-terminal (`portable-pty`: `openpty` on Linux and
//!   macOS, ConPTY on Windows) and keeps its screen in an xterm-compatible emulator (`vt100`),
//!   drawn here cell by cell in a monospace font: colours (16, 256 and true colour), bold,
//!   inverse, the cursor, the alternate screen; 10 000 lines of scrollback on the wheel.
//!   Keys are sent as the terminal sequences programs expect (application cursor keys when
//!   asked); Ctrl+Shift+C copies the screen and Ctrl+Shift+V pastes (bracketed when the program
//!   asked for it).
//! - **Agent presets** (Claude Code, Gemini CLI, Qwen Code, Codex CLI) and the user's own in
//!   `terminal-presets.json` in the settings folder. Each agent tab gets an MCP session on the
//!   window's [`reviera_mcp::bridge::Endpoint`] and a private session folder with the files its
//!   agent reads (an MCP config passed on its command line, a settings file named by an
//!   environment variable, or a settings file in the folder it starts in), all naming
//!   `reviera mcp-bridge --session-file <file>` as the MCP server. The user's own agent settings
//!   are never touched; the folder is removed when the tab closes.
//! - The session's calls run on the UI thread from the endpoint's queue, as the user's own
//!   commands do (undo, attribution); destructive ones wait for an approval dialog naming the
//!   tab.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use reviera_core::Permission;
use reviera_mcp::bridge::Endpoint;
use reviera_mcp::{Approval, AppHandle, Job, McpConfig};
use serde::{Deserialize, Serialize};
use wxdragon::prelude::*;

use super::{Gui, with};

const SCROLLBACK: usize = 10_000;

/// How an agent takes its MCP servers for one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum McpMethod {
    /// No MCP (a shell).
    None,
    /// A JSON file of `mcpServers` named in the arguments by `{mcp_json}`.
    JsonArgument,
    /// A JSON settings file of `mcpServers` named by the environment variable `env`.
    SettingsEnv,
    /// A settings file at `settings_path` (relative) in a session folder the agent starts in.
    SessionFolder,
    /// Placeholders in the arguments only (`{bridge}`, `{session_file}`).
    Arguments,
}

/// A terminal preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Preset {
    pub id: String,
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub mcp: McpMethod,
    /// SettingsEnv: the variable; SessionFolder: the settings file's path in the folder.
    #[serde(default)]
    pub setting: String,
}

/// The built-in presets, from each agent's published documentation (RESEARCH_LOG 2026-10-20,
/// S0835–S0839).
pub(crate) fn builtin_presets() -> Vec<Preset> {
    let shell = if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    };
    vec![
        Preset { id: "shell".into(), name: "Shell".into(), program: shell, args: Vec::new(), mcp: McpMethod::None, setting: String::new() },
        Preset {
            id: "claude".into(),
            name: "Claude Code".into(),
            program: "claude".into(),
            args: vec!["--mcp-config".into(), "{mcp_json}".into()],
            mcp: McpMethod::JsonArgument,
            setting: String::new(),
        },
        Preset {
            id: "gemini".into(),
            name: "Gemini CLI".into(),
            program: "gemini".into(),
            args: Vec::new(),
            mcp: McpMethod::SettingsEnv,
            setting: "GEMINI_CLI_SYSTEM_SETTINGS_PATH".into(),
        },
        Preset {
            id: "qwen".into(),
            name: "Qwen Code".into(),
            program: "qwen".into(),
            args: Vec::new(),
            mcp: McpMethod::SessionFolder,
            setting: ".qwen/settings.json".into(),
        },
        Preset {
            id: "codex".into(),
            name: "Codex CLI".into(),
            program: "codex".into(),
            args: vec![
                "-c".into(),
                "mcp_servers.reviera.command={bridge_toml}".into(),
                "-c".into(),
                "mcp_servers.reviera.args={bridge_args_toml}".into(),
            ],
            mcp: McpMethod::Arguments,
            setting: String::new(),
        },
    ]
}

fn presets_path() -> Option<PathBuf> {
    super::components::config_dir().map(|d| d.join("terminal-presets.json"))
}

/// The built-in presets with the user's (same id replaces, others added).
pub(crate) fn presets() -> Vec<Preset> {
    let mut all = builtin_presets();
    let own: Vec<Preset> = presets_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    for p in own {
        match all.iter_mut().find(|a| a.id == p.id) {
            Some(a) => *a = p,
            None => all.push(p),
        }
    }
    all
}

/// How agents start the bridge: a program and the arguments before `mcp-bridge` (in the
/// Flatpak, `flatpak run --command=reviera <app>`, since the agent runs on the host).
#[derive(Debug, Clone)]
pub(crate) struct Bridge {
    pub command: String,
    pub prefix: Vec<String>,
}

impl Bridge {
    pub(crate) fn current() -> Self {
        match flatpak_id() {
            Some(id) => Self { command: "flatpak".into(), prefix: vec!["run".into(), "--command=reviera".into(), id] },
            None => Self {
                command: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("reviera")).to_string_lossy().into_owned(),
                prefix: Vec::new(),
            },
        }
    }
}

/// The Flatpak's application id, inside the Flatpak.
fn flatpak_id() -> Option<String> {
    std::env::var("FLATPAK_ID").ok().filter(|s| !s.is_empty())
}

/// The files an agent session needs: written into `dir`, returning the program's arguments,
/// extra environment and working directory.
/// A session's start: the program's arguments, extra environment and working directory.
type Launch = (Vec<String>, Vec<(String, String)>, Option<PathBuf>);

fn session_files(p: &Preset, dir: &Path, bridge: &Bridge, endpoint: &str, secret: &str) -> std::io::Result<Launch> {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    let session_file = dir.join("session");
    std::fs::write(&session_file, format!("{endpoint}\n{secret}\n"))?;
    #[cfg(unix)]
    std::fs::set_permissions(&session_file, std::fs::Permissions::from_mode(0o600))?;
    let (bridge_s, file_s) = (bridge.command.clone(), session_file.to_string_lossy().into_owned());
    let mut bridge_args = bridge.prefix.clone();
    bridge_args.extend(["mcp-bridge".to_owned(), "--session-file".to_owned(), file_s.clone()]);
    let servers = serde_json::json!({
        "mcpServers": { "reviera": { "command": bridge_s, "args": bridge_args } }
    });
    // JSON strings and arrays of strings are valid TOML too.
    let toml = |s: &str| serde_json::to_string(s).expect("a string");
    let args_toml = serde_json::to_string(&bridge_args).expect("strings");
    let fill = |a: &str, json: &str| {
        a.replace("{mcp_json}", json)
            .replace("{bridge_toml}", &toml(&bridge_s))
            .replace("{bridge_args_toml}", &args_toml)
            .replace("{session_file_toml}", &toml(&file_s))
            .replace("{bridge}", &bridge_s)
            .replace("{session_file}", &file_s)
    };
    let mut env = Vec::new();
    let mut cwd = None;
    let json_path = dir.join("mcp.json");
    match p.mcp {
        McpMethod::None | McpMethod::Arguments => {}
        McpMethod::JsonArgument => std::fs::write(&json_path, servers.to_string())?,
        McpMethod::SettingsEnv => {
            std::fs::write(&json_path, servers.to_string())?;
            env.push((p.setting.clone(), json_path.to_string_lossy().into_owned()));
        }
        McpMethod::SessionFolder => {
            let f = dir.join(&p.setting);
            if let Some(parent) = f.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&f, servers.to_string())?;
            cwd = Some(dir.to_path_buf());
        }
    }
    let json_s = json_path.to_string_lossy().into_owned();
    let args = p.args.iter().map(|a| fill(a, &json_s)).collect();
    Ok((args, env, cwd))
}

/// One terminal tab.
pub(crate) struct Tab {
    pub id: u32,
    pub title: String,
    pub panel: Panel,
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    dirty: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    /// The agent's MCP session and its folder.
    secret: Option<String>,
    folder: Option<PathBuf>,
    size: (u16, u16),
    scroll: usize,
    shown_exit: bool,
}

/// A destructive call waiting for the user, with where to send the answer.
type Pending = (Approval, tokio::sync::oneshot::Sender<bool>);

pub(crate) struct TerminalPane {
    book: AuiNotebook,
    choice: Choice,
    pub tabs: Vec<Tab>,
    next: u32,
    timer: Timer<Frame>,
    endpoint: Option<Endpoint>,
    jobs: Option<mpsc::Receiver<Job>>,
    handle: Option<AppHandle>,
    approvals: Arc<Mutex<VecDeque<Pending>>>,
    presets: Vec<Preset>,
    font: Font,
    bold: Font,
    cell: (i32, i32),
}

fn nz(v: u16) -> std::num::NonZeroU16 {
    std::num::NonZeroU16::new(v.max(1)).expect("at least 1")
}

fn colour(c: vt100::Color, fg: bool, dark: bool) -> Colour {
    const ANSI: [(u8, u8, u8); 16] = [
        (0, 0, 0), (205, 49, 49), (13, 188, 121), (229, 229, 16), (36, 114, 200), (188, 63, 188), (17, 168, 205), (229, 229, 229),
        (102, 102, 102), (241, 76, 76), (35, 209, 139), (245, 245, 67), (59, 142, 234), (214, 112, 214), (41, 184, 219), (255, 255, 255),
    ];
    let (r, g, b) = match c {
        vt100::Color::Default => {
            match (fg, dark) {
                (true, true) => (220, 220, 220),
                (true, false) => (30, 30, 30),
                (false, true) => (24, 24, 26),
                (false, false) => (252, 252, 252),
            }
        }
        vt100::Color::Idx(i) if i < 16 => ANSI[i as usize],
        vt100::Color::Idx(i) if i < 232 => {
            let i = i - 16;
            let v = |k: u8| if k == 0 { 0 } else { 55 + k * 40 };
            (v(i / 36), v((i / 6) % 6), v(i % 6))
        }
        vt100::Color::Idx(i) => {
            let v = 8 + (i - 232) * 10;
            (v, v, v)
        }
        vt100::Color::Rgb(r, g, b) => (r, g, b),
    };
    Colour::new(r, g, b, 255)
}

impl TerminalPane {
    fn tab_index(&self, id: u32) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn current(&self) -> Option<usize> {
        let sel = self.book.get_selection();
        (sel >= 0).then_some(sel as usize).filter(|i| *i < self.tabs.len())
    }
}

impl Gui {
    /// View › User Interface › Terminal (Ctrl+Shift+T): shows or hides the pane, making it with a
    /// Shell tab on first use.
    pub(crate) fn toggle_terminal(gui: &Rc<RefCell<Gui>>) {
        if gui.borrow().terminal.is_some() {
            with(gui, |g| g.toggle_pane("terminal"));
            return;
        }
        let (frame, aui) = {
            let g = gui.borrow();
            (g.frame, g.aui)
        };
        let panel = Panel::builder(&frame).build();
        let presets = presets();
        let choice = Choice::builder(&panel).build();
        for p in &presets {
            choice.append(&p.name);
        }
        choice.set_selection(0);
        let open = Button::builder(&panel).with_label("Open").build();
        open.set_tooltip("Open a new tab with the chosen shell or agent; agents start connected to the open model");
        let close = Button::builder(&panel).with_label("Close Tab").build();
        let book = AuiNotebook::builder(&panel).build();
        let row = BoxSizer::builder(Orientation::Horizontal).build();
        row.add(&choice, 0, SizerFlag::All | SizerFlag::AlignCenterVertical, 3);
        row.add(&open, 0, SizerFlag::All, 3);
        row.add(&close, 0, SizerFlag::All, 3);
        let col = BoxSizer::builder(Orientation::Vertical).build();
        col.add_sizer(&row, 0, SizerFlag::Expand, 0);
        col.add(&book, 1, SizerFlag::Expand, 0);
        panel.set_sizer(col, true);
        aui.add_pane_with_info(
            &panel,
            AuiPaneInfo::new()
                .with_name("terminal")
                .with_caption("Terminal")
                .bottom()
                .layer(0)
                .best_size(900, 300)
                .min_size(300, 140)
                .close_button(true)
                .floatable(true),
        );
        aui.update();
        let (Some(font), Some(bold)) = (
            Font::builder().with_point_size(10).with_family(FontFamily::Teletype).build(),
            Font::builder().with_point_size(10).with_family(FontFamily::Teletype).with_weight(FontWeight::Bold).build(),
        ) else {
            return;
        };
        let timer = Timer::new(&frame);
        {
            let gui = gui.clone();
            timer.on_tick(move |_| {
                let mut ask: Option<(Frame, Pending)> = None;
                if let Ok(mut g) = gui.try_borrow_mut() {
                    ask = g.terminal_tick().map(|p| (g.frame, p));
                }
                // Asked with the window free, so the terminals and views keep drawing.
                if let Some((frame, (a, reply))) = ask {
                    let text = format!("{} wants to run {}.\n\nIt would leave {}.\n\nAllow it?", a.client, a.command, a.summary);
                    let d = MessageDialog::builder(&frame, &text, "Agent approval")
                        .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                        .build();
                    let _ = reply.send(d.show_modal() == ID_YES);
                }
            });
        }
        timer.start(30, false);
        {
            let gui = gui.clone();
            open.on_click(move |_| {
                let id = gui.borrow().terminal.as_ref().map(|t| t.choice.get_selection().unwrap_or(0) as usize).unwrap_or(0);
                Gui::open_terminal_tab(&gui, id);
            });
        }
        {
            let gui = gui.clone();
            close.on_click(move |_| {
                with(&gui, |g| {
                    if let Some(i) = g.terminal.as_ref().and_then(TerminalPane::current) {
                        g.close_terminal_tab(i, true);
                    }
                });
            });
        }
        gui.borrow_mut().terminal = Some(TerminalPane {
            book,
            choice,
            tabs: Vec::new(),
            next: 1,
            timer,
            endpoint: None,
            jobs: None,
            handle: None,
            approvals: Arc::default(),
            presets,
            font,
            bold,
            cell: (8, 16),
        });
        Gui::open_terminal_tab(gui, 0);
    }

    /// The endpoint, started on first use; its calls run on this thread from the timer.
    fn terminal_endpoint(&mut self) -> Result<(), String> {
        let Some(t) = &mut self.terminal else { return Err("no terminal".into()) };
        if t.endpoint.is_some() {
            return Ok(());
        }
        let (handle, jobs) = AppHandle::external();
        let dir = runtime_dir();
        let endpoint = Endpoint::start(handle.clone(), &dir).map_err(|e| e.to_string())?;
        t.endpoint = Some(endpoint);
        t.jobs = Some(jobs);
        t.handle = Some(handle);
        Ok(())
    }

    /// Opens a tab running preset `index`.
    pub(crate) fn open_terminal_tab(gui: &Rc<RefCell<Gui>>, index: usize) {
        let mut opened: Option<(Panel, u32)> = None;
        with(gui, |g| opened = g.start_terminal_tab(index));
        if let Some((panel, id)) = opened {
            Gui::bind_terminal_view(gui, panel, id);
        }
    }

    fn start_terminal_tab(&mut self, index: usize) -> Option<(Panel, u32)> {
        let frame = self.frame;
        let preset = self.terminal.as_ref()?.presets.get(index)?.clone();
        let agent = preset.mcp != McpMethod::None;
        // An agent: a session on the endpoint and its folder.
        let mut session: Option<(String, PathBuf)> = None;
        let mut extra: Launch = (preset.args.clone(), Vec::new(), None);
        let id = self.terminal.as_ref()?.next;
        let title = format!("{} {id}", preset.name);
        if agent {
            if let Err(e) = self.terminal_endpoint() {
                error(&frame, &format!("The MCP endpoint could not start:\n{e}"));
                return None;
            }
            let t = self.terminal.as_mut()?;
            let approvals = t.approvals.clone();
            let mut config = McpConfig::new(Permission::ModelEdit);
            config.client_name = Some(title.clone());
            config.toolsets = vec!["core".into(), "modeling".into(), "documentation".into()];
            config.agent_may_confirm = false;
            config.approver = Some(Arc::new(move |a: Approval| {
                let (tx, rx) = tokio::sync::oneshot::channel();
                approvals.lock().expect("approvals").push_back((a, tx));
                rx
            }));
            let endpoint = t.endpoint.as_ref()?;
            let secret = endpoint.issue(config);
            let folder = runtime_dir().join(format!("session-{}-{id}", std::process::id()));
            let bridge = Bridge::current();
            match session_files(&preset, &folder, &bridge, &endpoint.address, &secret) {
                Ok(e) => extra = e,
                Err(e) => {
                    endpoint.revoke(&secret);
                    error(&frame, &format!("The session's files could not be written:\n{e}"));
                    return None;
                }
            }
            session = Some((secret, folder));
        }
        let cwd = extra.2.clone().or_else(|| self.current_path().and_then(|p| p.parent().map(Path::to_path_buf))).or_else(|| std::env::var_os("HOME").map(PathBuf::from));
        let t = self.terminal.as_mut()?;
        t.next += 1;
        let panel = Panel::builder(&t.book).build();
        // Drawn whole, buffered, on every paint.
        panel.set_background_style(BackgroundStyle::Paint);
        super::wx::wants_chars(&panel);
        t.book.add_page(&panel, &title, true);
        let size = PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 };
        let pty = native_pty_system();
        let parser = Arc::new(Mutex::new(vt100::Parser::new(nz(size.rows), nz(size.cols), SCROLLBACK)));
        let pair = match pty.openpty(size) {
            Ok(p) => p,
            Err(e) => {
                error(&frame, &format!("A terminal could not be opened:\n{e}"));
                return None;
            }
        };
        let cmd = command(&preset, &extra, cwd.as_deref());
        let child = pair.slave.spawn_command(cmd);
        drop(pair.slave);
        let dirty = Arc::new(AtomicBool::new(true));
        let exited = Arc::new(AtomicBool::new(false));
        let child = match child {
            Ok(c) => Some(c),
            Err(e) => {
                // A missing program: say so in the tab, and change nothing else.
                let text = format!("\x1b[1m{}\x1b[0m could not be started: {e}\r\n\r\nIs `{}` installed and on your PATH? Presets are in {}.\r\n", preset.name, preset.program, presets_path().map(|p| p.display().to_string()).unwrap_or_default());
                parser.lock().expect("parser").process(text.as_bytes());
                exited.store(true, Ordering::Relaxed);
                if let Some((secret, folder)) = session.take() {
                    if let Some(e) = &t.endpoint {
                        e.revoke(&secret);
                    }
                    let _ = std::fs::remove_dir_all(folder);
                }
                None
            }
        };
        if child.is_some()
            && let Ok(mut reader) = pair.master.try_clone_reader()
        {
            let (p, d, x) = (parser.clone(), dirty.clone(), exited.clone());
            std::thread::Builder::new()
                .name("reviera-terminal".into())
                .spawn(move || {
                    let mut buf = [0u8; 8192];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                p.lock().expect("parser").process(&buf[..n]);
                                d.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    x.store(true, Ordering::Relaxed);
                    d.store(true, Ordering::Relaxed);
                })
                .ok();
        }
        let writer = pair.master.take_writer().unwrap_or_else(|_| Box::new(std::io::sink()));
        let (secret, folder) = match session {
            Some((s, f)) => (Some(s), Some(f)),
            None => (None, None),
        };
        t.tabs.push(Tab {
            id,
            title,
            panel,
            parser,
            writer,
            master: pair.master,
            child,
            dirty,
            exited,
            secret,
            folder,
            size: (24, 80),
            scroll: 0,
            shown_exit: false,
        });
        if !self.aui.is_pane_shown("terminal") {
            self.toggle_pane("terminal");
        }
        panel.set_focus();
        Some((panel, id))
    }

    fn bind_terminal_view(gui: &Rc<RefCell<Gui>>, panel: Panel, id: u32) {
        {
            let gui = gui.clone();
            panel.on_paint(move |_| {
                if let Ok(mut g) = gui.try_borrow_mut() {
                    g.paint_terminal(id);
                }
            });
        }
        {
            let gui = gui.clone();
            panel.on_size(move |e| {
                if let Ok(mut g) = gui.try_borrow_mut() {
                    g.resize_terminal(id);
                }
                e.skip(true);
            });
        }
        {
            let gui = gui.clone();
            panel.on_key_down(move |e| {
                let WindowEventData::Keyboard(k) = &e else { return };
                let (code, ctrl, shift, alt) = (k.get_key_code(), k.control_down(), k.shift_down(), k.alt_down());
                let mut used = false;
                if let Ok(mut g) = gui.try_borrow_mut() {
                    used = g.terminal_key(id, code.unwrap_or(0), ctrl, shift, alt);
                }
                if !used {
                    e.skip(true);
                }
            });
        }
        {
            let gui = gui.clone();
            panel.on_char(move |e| {
                let WindowEventData::Keyboard(k) = &e else { return };
                if let Some(c) = k.get_unicode_key().and_then(|u| u32::try_from(u).ok()).and_then(char::from_u32)
                    && let Ok(mut g) = gui.try_borrow_mut()
                {
                    let mut b = [0u8; 4];
                    g.terminal_write(id, c.encode_utf8(&mut b).as_bytes());
                }
            });
        }
        {
            let gui = gui.clone();
            panel.on_mouse_wheel(move |e| {
                let steps = super::wheel(&e);
                if let Ok(mut g) = gui.try_borrow_mut() {
                    g.terminal_scroll(id, steps);
                }
            });
        }
        {
            let p = panel;
            panel.on_mouse_left_down(move |_| p.set_focus());
        }
    }

    fn terminal_write(&mut self, id: u32, bytes: &[u8]) {
        let Some(t) = &mut self.terminal else { return };
        let Some(i) = t.tab_index(id) else { return };
        let tab = &mut t.tabs[i];
        if tab.scroll != 0 {
            tab.scroll = 0;
            tab.parser.lock().expect("parser").screen_mut().set_scrollback(0);
            tab.dirty.store(true, Ordering::Relaxed);
        }
        let _ = tab.writer.write_all(bytes);
        let _ = tab.writer.flush();
    }

    /// Keys that are not characters: arrows, Enter, editing keys, Ctrl combinations; copy and
    /// paste. True when used.
    fn terminal_key(&mut self, id: u32, code: i32, ctrl: bool, shift: bool, alt: bool) -> bool {
        let app_cursor = self
            .terminal
            .as_ref()
            .and_then(|t| t.tabs.iter().find(|x| x.id == id))
            .is_some_and(|t| t.parser.lock().expect("parser").screen().application_cursor());
        let arrow = |c: char| -> Vec<u8> {
            if ctrl || shift || alt {
                let m = 1 + (shift as u8) + 2 * (alt as u8) + 4 * (ctrl as u8);
                format!("\x1b[1;{m}{c}").into_bytes()
            } else if app_cursor {
                format!("\x1bO{c}").into_bytes()
            } else {
                format!("\x1b[{c}").into_bytes()
            }
        };
        if ctrl && shift && (code == 'C' as i32 || code == 'V' as i32) {
            if code == 'C' as i32 {
                self.terminal_copy(id);
            } else {
                self.terminal_paste(id);
            }
            return true;
        }
        let bytes: Vec<u8> = match code {
            WXK_RETURN | WXK_NUMPAD_ENTER => b"\r".to_vec(),
            WXK_BACK => if alt { b"\x1b\x7f".to_vec() } else { b"\x7f".to_vec() },
            WXK_TAB => if shift { b"\x1b[Z".to_vec() } else { b"\t".to_vec() },
            WXK_ESCAPE => b"\x1b".to_vec(),
            WXK_UP => arrow('A'),
            WXK_DOWN => arrow('B'),
            WXK_RIGHT => arrow('C'),
            WXK_LEFT => arrow('D'),
            WXK_HOME => arrow('H'),
            WXK_END => arrow('F'),
            WXK_PAGEUP => b"\x1b[5~".to_vec(),
            WXK_PAGEDOWN => b"\x1b[6~".to_vec(),
            WXK_INSERT => b"\x1b[2~".to_vec(),
            WXK_DELETE => b"\x1b[3~".to_vec(),
            k if (WXK_F1..=WXK_F12).contains(&k) => {
                const F: [&str; 12] = ["\x1bOP", "\x1bOQ", "\x1bOR", "\x1bOS", "\x1b[15~", "\x1b[17~", "\x1b[18~", "\x1b[19~", "\x1b[20~", "\x1b[21~", "\x1b[23~", "\x1b[24~"];
                F[(k - WXK_F1) as usize].as_bytes().to_vec()
            }
            // Ctrl+letter: the control code (some platforms do not send it as a character).
            k if ctrl && !alt && (('A' as i32)..=('Z' as i32)).contains(&k) => vec![(k as u8) & 0x1f],
            k if alt && !ctrl && (32..127).contains(&k) => {
                let c = if shift { k as u8 } else { (k as u8).to_ascii_lowercase() };
                vec![0x1b, c]
            }
            _ => return false,
        };
        self.terminal_write(id, &bytes);
        true
    }

    fn terminal_copy(&mut self, id: u32) {
        let Some(t) = &self.terminal else { return };
        let Some(tab) = t.tabs.iter().find(|x| x.id == id) else { return };
        let text = tab.parser.lock().expect("parser").screen().contents();
        let c = Clipboard::get();
        if c.open() {
            c.set_text(&text);
            c.close();
        }
        self.frame.set_status_text("The terminal's screen was copied", 0);
    }

    fn terminal_paste(&mut self, id: u32) {
        let c = Clipboard::get();
        let text = if c.open() {
            let t = c.get_text();
            c.close();
            t
        } else {
            None
        };
        let Some(text) = text else { return };
        let bracketed = self
            .terminal
            .as_ref()
            .and_then(|t| t.tabs.iter().find(|x| x.id == id))
            .is_some_and(|t| t.parser.lock().expect("parser").screen().bracketed_paste());
        if !bracketed && text.contains('\n') {
            let d = MessageDialog::builder(&self.frame, &format!("Paste {} lines into the terminal?", text.lines().count()), "Paste")
                .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                .build();
            if d.show_modal() != ID_YES {
                return;
            }
        }
        let body = text.replace("\r\n", "\r").replace('\n', "\r");
        let bytes = if bracketed { format!("\x1b[200~{body}\x1b[201~") } else { body };
        self.terminal_write(id, bytes.as_bytes());
    }

    fn terminal_scroll(&mut self, id: u32, steps: i32) {
        let Some(t) = &mut self.terminal else { return };
        let Some(i) = t.tab_index(id) else { return };
        let tab = &mut t.tabs[i];
        let mut p = tab.parser.lock().expect("parser");
        let want = (tab.scroll as i64 + steps as i64 * 3).clamp(0, SCROLLBACK as i64) as usize;
        p.screen_mut().set_scrollback(want);
        tab.scroll = p.screen().scrollback();
        tab.dirty.store(true, Ordering::Relaxed);
    }

    /// Fits the terminal to its panel.
    fn resize_terminal(&mut self, id: u32) {
        let Some(t) = &mut self.terminal else { return };
        let cell = t.cell;
        let Some(i) = t.tab_index(id) else { return };
        let tab = &mut t.tabs[i];
        let s = tab.panel.get_client_size();
        let cols = ((s.width - 4).max(cell.0) / cell.0.max(1)).clamp(10, 1000) as u16;
        let rows = ((s.height - 4).max(cell.1) / cell.1.max(1)).clamp(3, 500) as u16;
        if (rows, cols) != tab.size {
            tab.size = (rows, cols);
            tab.parser.lock().expect("parser").set_size(nz(rows), nz(cols));
            let _ = tab.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
            tab.dirty.store(true, Ordering::Relaxed);
        }
        tab.panel.refresh(false, None);
    }

    fn paint_terminal(&mut self, id: u32) {
        let dark = super::ribbon::dark();
        let Some(t) = &mut self.terminal else { return };
        let Some(i) = t.tab_index(id) else { return };
        let tab = &t.tabs[i];
        let dc = AutoBufferedPaintDC::new(&tab.panel);
        dc.set_font(&t.font);
        let (cw, ch) = dc.get_text_extent("M");
        let first = t.cell != (cw.max(1), ch.max(1));
        t.cell = (cw.max(1), ch.max(1));
        let size = tab.panel.get_client_size();
        let bg = colour(vt100::Color::Default, false, dark);
        dc.set_brush(bg, BrushStyle::Solid);
        dc.set_pen(bg, 1, PenStyle::Solid);
        dc.draw_rectangle(0, 0, size.width, size.height);
        let parser = tab.parser.lock().expect("parser");
        let screen = parser.screen();
        let (rows, cols) = screen.size();
        let (rows, cols) = (rows.get(), cols.get());
        let (cw, ch) = t.cell;
        let (ox, oy) = (2, 2);
        for row in 0..rows {
            let mut col = 0;
            while col < cols {
                let Some(cell) = screen.cell(row, col) else {
                    col += 1;
                    continue;
                };
                // A run of cells drawn alike.
                let key = (cell.fgcolor(), cell.bgcolor(), cell.bold(), cell.inverse());
                let start = col;
                let mut text = String::new();
                while col < cols {
                    let Some(c) = screen.cell(row, col) else { break };
                    if (c.fgcolor(), c.bgcolor(), c.bold(), c.inverse()) != key {
                        break;
                    }
                    if c.is_wide_continuation() {
                        col += 1;
                        continue;
                    }
                    let s = c.contents();
                    text.push_str(if s.is_empty() { " " } else { s });
                    col += 1;
                }
                let (mut fg, mut bgc) = (colour(key.0, true, dark), colour(key.1, false, dark));
                if key.3 {
                    std::mem::swap(&mut fg, &mut bgc);
                }
                let x = ox + start as i32 * cw;
                let y = oy + row as i32 * ch;
                if key.1 != vt100::Color::Default || key.3 {
                    dc.set_brush(bgc, BrushStyle::Solid);
                    dc.set_pen(bgc, 1, PenStyle::Solid);
                    dc.draw_rectangle(x, y, (col - start) as i32 * cw, ch);
                }
                if !text.trim().is_empty() {
                    dc.set_font(if key.2 { &t.bold } else { &t.font });
                    dc.set_text_foreground(fg);
                    dc.set_background_mode(BackgroundMode::Transparent);
                    dc.draw_text(&text, x, y);
                }
            }
        }
        if !screen.hide_cursor() && tab.scroll == 0 {
            let (r, c) = screen.cursor_position();
            let cur = colour(vt100::Color::Default, true, dark);
            dc.set_brush(cur, BrushStyle::Transparent);
            dc.set_pen(cur, 1, PenStyle::Solid);
            dc.draw_rectangle(ox + c as i32 * cw, oy + r as i32 * ch, cw, ch);
        }
        drop(parser);
        if first {
            self.resize_terminal(id);
        }
    }

    /// The timer: redraws changed tabs, marks exited ones, runs the MCP sessions' calls, and
    /// returns an approval to ask for (asked by the caller once the window is free).
    fn terminal_tick(&mut self) -> Option<Pending> {
        let frame = self.frame;
        // The agents' calls, as the user's own commands.
        let mut ran = 0;
        if let Some(rx) = self.terminal.as_ref().and_then(|t| t.jobs.as_ref()) {
            let jobs: Vec<Job> = std::iter::from_fn(|| rx.try_recv().ok()).take(64).collect();
            for job in jobs {
                reviera_mcp::run_job(job, &mut self.app);
                ran += 1;
            }
        }
        if ran > 0 {
            self.refresh_scene(&frame);
            self.properties_stale = true;
            self.browser_stale = true;
            self.tree.stale = true;
        }
        let pending = self.terminal.as_ref().and_then(|t| t.approvals.lock().expect("approvals").pop_front());
        let Some(t) = &mut self.terminal else { return pending };
        for (k, tab) in t.tabs.iter_mut().enumerate() {
            if tab.dirty.swap(false, Ordering::Relaxed) {
                tab.panel.refresh(false, None);
            }
            if tab.exited.load(Ordering::Relaxed) && !tab.shown_exit {
                tab.shown_exit = true;
                t.book.set_page_text(k, &format!("{} (ended)", tab.title));
                // Renaming the page lays the notebook out again: draw the screen once more.
                tab.panel.refresh(false, None);
                if let (Some(s), Some(e)) = (tab.secret.take(), &t.endpoint) {
                    e.revoke(&s);
                }
            }
        }
        pending
    }

    /// Closes tab `index`: its process ends (asking first while it runs) and its session and
    /// folder go.
    pub(crate) fn close_terminal_tab(&mut self, index: usize, ask: bool) {
        let frame = self.frame;
        let Some(t) = &mut self.terminal else { return };
        let Some(tab) = t.tabs.get(index) else { return };
        if ask && !tab.exited.load(Ordering::Relaxed) {
            let d = MessageDialog::builder(&frame, &format!("{} is still running. Close it?", tab.title), "Terminal")
                .with_style(MessageDialogStyle::YesNo | MessageDialogStyle::IconQuestion)
                .build();
            if d.show_modal() != ID_YES {
                return;
            }
        }
        let Some(t) = &mut self.terminal else { return };
        let mut tab = t.tabs.remove(index);
        if let Some(c) = &mut tab.child {
            let _ = c.kill();
        }
        if let (Some(s), Some(e)) = (tab.secret.take(), &t.endpoint) {
            e.revoke(&s);
        }
        if let Some(f) = tab.folder.take() {
            let _ = std::fs::remove_dir_all(f);
        }
        t.book.delete_page(index);
    }

    /// Ends every tab (Reviera is closing).
    pub(crate) fn close_all_terminals(&mut self) {
        while self.terminal.as_ref().is_some_and(|t| !t.tabs.is_empty()) {
            self.close_terminal_tab(0, false);
        }
        if let Some(t) = &mut self.terminal {
            t.timer.stop();
        }
    }
}

/// The command for a tab: its program, or in the Flatpak, `flatpak-spawn --host` running it on
/// the host (where the user's shell and agents are), with its environment and folder.
fn command(preset: &Preset, launch: &Launch, cwd: Option<&Path>) -> CommandBuilder {
    let mut env: Vec<(String, String)> = vec![("TERM".into(), "xterm-256color".into()), ("COLORTERM".into(), "truecolor".into())];
    env.extend(launch.1.iter().cloned());
    if flatpak_id().is_none() {
        let mut cmd = CommandBuilder::new(&preset.program);
        cmd.args(&launch.0);
        for (k, v) in &env {
            cmd.env(k, v);
        }
        if let Some(d) = cwd {
            cmd.cwd(d);
        }
        return cmd;
    }
    let mut cmd = CommandBuilder::new("flatpak-spawn");
    cmd.arg("--host");
    if let Some(d) = cwd {
        cmd.arg(format!("--directory={}", d.display()));
    }
    for (k, v) in &env {
        cmd.arg(format!("--env={k}={v}"));
    }
    if preset.mcp == McpMethod::None && preset.id == "shell" {
        // The host's own login shell, not the sandbox's.
        cmd.args(["sh", "-c", "exec \"${SHELL:-/bin/sh}\" -l"]);
    } else {
        cmd.arg(&preset.program);
        cmd.args(&launch.0);
    }
    cmd
}

/// Where the endpoint and session folders live: the user's runtime directory (private to them),
/// else the temporary directory. In the Flatpak, the app's own runtime directory, which the host
/// and every instance of the app share.
fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    match flatpak_id() {
        Some(id) => base.join("app").join(id).join("reviera"),
        None => base.join("reviera"),
    }
}

/// Removes session folders left by a crash (Reviera starting).
pub(crate) fn remove_stale_sessions() {
    let Ok(entries) = std::fs::read_dir(runtime_dir()) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let pid: Option<u32> = name.strip_prefix("session-").and_then(|r| r.split('-').next()).and_then(|p| p.parse().ok());
        if let Some(pid) = pid
            && pid != std::process::id()
            && !alive(pid)
        {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

fn alive(pid: u32) -> bool {
    if cfg!(target_os = "linux") {
        Path::new(&format!("/proc/{pid}")).exists()
    } else {
        // Elsewhere, kept until the next start after a reboot clears the runtime directory.
        true
    }
}

fn error(frame: &Frame, message: &str) {
    let d = MessageDialog::builder(frame, message, "BIM Reviera")
        .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconWarning)
        .build();
    d.show_modal();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_files_never_put_the_secret_on_the_command_line() {
        let dir = tempfile_dir();
        let bridge = Bridge { command: "/opt/reviera/reviera".into(), prefix: Vec::new() };
        for p in builtin_presets().into_iter().filter(|p| p.mcp != McpMethod::None) {
            let folder = dir.join(&p.id);
            let (args, env, cwd) = session_files(&p, &folder, &bridge, "/run/reviera/mcp.sock", "s3cret").unwrap();
            assert!(args.iter().all(|a| !a.contains("s3cret")), "{}: {args:?}", p.id);
            assert!(env.iter().all(|(_, v)| !v.contains("s3cret")), "{}", p.id);
            let session = std::fs::read_to_string(folder.join("session")).unwrap();
            assert_eq!(session, "/run/reviera/mcp.sock\ns3cret\n");
            match p.mcp {
                McpMethod::JsonArgument => {
                    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(folder.join("mcp.json")).unwrap()).unwrap();
                    assert_eq!(json["mcpServers"]["reviera"]["args"][0], "mcp-bridge");
                    assert!(args.contains(&folder.join("mcp.json").to_string_lossy().into_owned()));
                }
                McpMethod::SettingsEnv => assert_eq!(env[0].0, "GEMINI_CLI_SYSTEM_SETTINGS_PATH"),
                McpMethod::SessionFolder => {
                    assert!(folder.join(".qwen/settings.json").exists());
                    assert_eq!(cwd.as_deref(), Some(folder.as_path()));
                }
                McpMethod::Arguments => {
                    assert_eq!(args[1], "mcp_servers.reviera.command=\"/opt/reviera/reviera\"");
                    assert!(args[3].starts_with("mcp_servers.reviera.args=[\"mcp-bridge\",\"--session-file\""), "{args:?}");
                }
                McpMethod::None => {}
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(folder.join("session")).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600);
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    fn tempfile_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("reviera-term-test-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn in_the_flatpak_agents_start_the_bridge_through_flatpak_run() {
        let dir = tempfile_dir();
        let bridge = Bridge { command: "flatpak".into(), prefix: vec!["run".into(), "--command=reviera".into(), "io.github.rational_boxes.Reviera".into()] };
        let claude = builtin_presets().into_iter().find(|p| p.id == "claude").unwrap();
        session_files(&claude, &dir, &bridge, "/run/user/1/app/x/reviera/mcp.sock", "s").unwrap();
        let json: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("mcp.json")).unwrap()).unwrap();
        assert_eq!(json["mcpServers"]["reviera"]["command"], "flatpak");
        assert_eq!(json["mcpServers"]["reviera"]["args"][3], "mcp-bridge");
        let _ = std::fs::remove_dir_all(dir);
    }
}
