// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `mep_settings.show(kind)` (MEP-SET-004): one entry point to the MEP settings dialogs, for the
//! ribbon, Lua and MCP alike. With a window the dialog is queued for it to open; without one
//! (headless) the call succeeds and reports `ui_unavailable`.

use reviera_core::{Caller, CommandMeta, Effect, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::App;

/// The settings dialogs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingsKind {
    Mechanical,
    Electrical,
    ConductorCable,
    BuildingSpaceType,
    LoadClassification,
    DemandFactor,
    Fabrication,
    Pid,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowParams {
    pub kind: SettingsKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ShowResult {
    pub kind: SettingsKind,
    /// Whether the window will open it.
    pub shown: bool,
    /// Why not: `ui_unavailable` without a window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl App {
    /// The settings dialogs commands asked the window to open.
    pub fn take_settings_requests(&mut self) -> Vec<SettingsKind> {
        std::mem::take(&mut self.settings_requests)
    }
}

pub(crate) struct MepSettingsShow;

impl HostCommand for MepSettingsShow {
    type Host = App;
    type Params = ShowParams;
    type Output = ShowResult;
    const META: CommandMeta = CommandMeta {
        name: "mep_settings.show",
        version: 1,
        summary: "Open an MEP settings dialog",
        description: "Opens an MEP settings dialog (MEP-SET-004): `kind` mechanical, electrical, conductor_cable, building_space_type, load_classification, demand_factor, fabrication or pid. Only opens it; the edits are recorded by the settings' own commands, one transaction per dialog. Without a window (headless Lua or MCP) it returns `shown: false` with `reason: \"ui_unavailable\"`.",
        effects: &[Effect::Ui],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: ShowParams) -> Result<ShowResult> {
        if !app.has_window() {
            return Ok(ShowResult { kind: p.kind, shown: false, reason: Some("ui_unavailable".into()) });
        }
        app.settings_requests.push(p.kind);
        Ok(ShowResult { kind: p.kind, shown: true, reason: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_reports_ui_unavailable() {
        let mut app = App::new(crate::AppConfig { live: None, untitled_dir: std::env::temp_dir() });
        let out = app.execute("mep_settings.show", serde_json::json!({ "kind": "demand_factor" }), &Caller::user()).unwrap();
        assert_eq!(out.result["shown"], false);
        assert_eq!(out.result["reason"], "ui_unavailable");
        app.set_window(true);
        let out = app.execute("mep_settings.show", serde_json::json!({ "kind": "demand_factor" }), &Caller::user()).unwrap();
        assert_eq!(out.result["shown"], true);
        assert_eq!(app.take_settings_requests(), vec![SettingsKind::DemandFactor]);
        assert!(app.execute("mep_settings.show", serde_json::json!({ "kind": "nope" }), &Caller::user()).is_err());
    }
}
