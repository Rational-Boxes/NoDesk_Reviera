// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sustainability › Daylight & Solar › **Solar Study** (ANA-SOLAR-001, ANA-ENV-002): the study type
//! (insolation or PV potential), period, surfaces (roofs, masses or the selection), grid spacing
//! and PV settings, then Run: a compute job the window watches. When it is collected, *Solar
//! Results* lists each face's area, mean insolation, total and (PV) yield and payback, says
//! whether the study is stale, and *Show on Model* colours each grid point's square on its face
//! in 3D views, blue (least) to red (most), with the range in the status bar.

use reviera_core::solar;
use reviera_render::SceneMesh;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const SHOW: i32 = 7202;
const HIDE: i32 = 7901;
const EXPORT: i32 = 7902;

/// Blue → cyan → green → yellow → red at `t` in 0–1.
pub(crate) fn ramp(t: f64) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0) as f32;
    let stops: [[f32; 3]; 5] = [[0.15, 0.25, 0.9], [0.1, 0.8, 0.9], [0.2, 0.8, 0.2], [0.95, 0.85, 0.1], [0.9, 0.15, 0.1]];
    let x = t * 4.0;
    let i = (x.floor() as usize).min(3);
    let f = x - i as f32;
    let (a, b) = (stops[i], stops[i + 1]);
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f, 1.0]
}

fn parse_date(s: &str) -> Option<[u8; 2]> {
    let (d, m) = s.trim().split_once(['/', '.', '-'])?;
    let (d, m) = (d.trim().parse::<u8>().ok()?, m.trim().parse::<u8>().ok()?);
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then_some([m, d])
}

impl Gui {
    pub(crate) fn solar_dialog(&mut self) {
        let frame = self.frame;
        let f = super::reinforcement_ui::Form::new(&frame, "Solar Study");
        let kinds = vec!["Cumulative insolation".to_owned(), "PV potential".to_owned()];
        let kind = f.choice("Study", &kinds, 0);
        let from = f.text("From (day/month)", "1/1", "");
        let to = f.text("To (day/month)", "31/12", "");
        let surf = vec!["Every roof".to_owned(), "Every mass".to_owned(), "The selected roofs and masses".to_owned()];
        let surfaces = f.choice("Surfaces", &surf, 0);
        let grid = f.text("Grid spacing (m)", "1", "0.1 to 10 m");
        let cov = f.text("PV coverage", "0.8", "Share of each face covered by panels");
        let eff = f.text("PV efficiency", "0.18", "");
        let derate = f.text("System derate", "0.86", "");
        let cost = f.text("Cost per kW peak", "1200", "Placeholder (EXPERT_REVIEW E-36)");
        let price = f.text("Energy price per kWh", "0.25", "Placeholder (EXPERT_REVIEW E-36)");
        if !f.show("Uses the project location and weather file; every model element shades. It runs out of process.", "Run") {
            f.dialog.destroy();
            return;
        }
        let num = |t: &TextCtrl, d: f64| t.get_value().trim().replace(',', ".").parse::<f64>().unwrap_or(d);
        let pv = kind.get_selection() == Some(1);
        let surfaces_v: Value = match surfaces.get_selection() {
            Some(1) => json!("masses"),
            Some(2) => json!({ "elements": self.selection() }),
            _ => json!("roofs"),
        };
        let args = json!({
            "study_type": if pv { "pv" } else { "insolation" },
            "from": parse_date(&from.get_value()).unwrap_or([1, 1]),
            "to": parse_date(&to.get_value()).unwrap_or([12, 31]),
            "surfaces": surfaces_v,
            "grid": num(&grid, 1.0),
            "pv_settings": { "coverage": num(&cov, 0.8), "efficiency": num(&eff, 0.18), "derate": num(&derate, 0.86), "cost_per_kwp": num(&cost, 1200.0), "energy_price": num(&price, 0.25) },
        });
        f.dialog.destroy();
        match self.run("solar.run", args) {
            Ok(v) => self.watch_job(v["job_id"].as_str().unwrap_or_default().to_owned(), "solar.collect", "Solar study"),
            Err(e) => error(&frame, &format!("The study did not start:\n{e}")),
        }
    }

    /// Solar Results: the latest study's faces.
    pub(crate) fn solar_results(&mut self) {
        let frame = self.frame;
        let r = match self.run("solar.results", json!({})) {
            Ok(v) => v,
            Err(e) => return error(&frame, &e),
        };
        let stale = self.run("solar.status", json!({})).ok().is_some_and(|s| s["stale"] == true);
        let pv = !r["pv_total"].is_null();
        let rows: Vec<Vec<String>> = r["faces"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| {
                let opt = |k: &str, d: usize| f[k].as_f64().map_or(String::new(), |v| format!("{v:.d$}"));
                vec![
                    f["element"].to_string(),
                    f["key"].as_str().and_then(|k| k.rsplit('/').next()).unwrap_or("").to_owned(),
                    format!("{:.1}", f["area"].as_f64().unwrap_or(0.0)),
                    format!("{:.0}", f["mean"].as_f64().unwrap_or(0.0)),
                    format!("{:.0}", f["total"].as_f64().unwrap_or(0.0)),
                    opt("pv_kwh_year", 0),
                    opt("pv_payback_years", 1),
                ]
            })
            .collect();
        let mut intro = format!(
            "{} study, {}/{} to {}/{}: {} sun hours, {:.0} kWh on the faces{}.",
            if pv { "PV potential" } else { "Insolation" },
            r["from"][1],
            r["from"][0],
            r["to"][1],
            r["to"][0],
            r["sun_hours"],
            r["total"].as_f64().unwrap_or(0.0),
            r["pv_total"].as_f64().map_or(String::new(), |p| format!(", PV {p:.0} kWh a year"))
        );
        if stale {
            intro.push_str(" Stale: the model, site or weather changed since; run it again.");
        }
        let (code, _) = super::mep_loads_ui::list_dialog(
            &frame,
            "Solar Results",
            &intro,
            &[("Element", 70), ("Face", 50), ("m²", 60), ("kWh/m²", 70), ("kWh", 80), ("PV kWh/yr", 80), ("Payback yr", 80)],
            &rows,
            &[(SHOW, "Show on Model", "Colour each grid point on its face in 3D views"), (HIDE, "Hide", "Stop showing the study"), (EXPORT, "Export…", "Per-face results as CSV")],
            None,
        );
        match code {
            SHOW => {
                self.solar_shown = r["id"].as_str().map(str::to_owned);
                self.refresh_scene(&frame);
            }
            HIDE => {
                self.solar_shown = None;
                self.refresh_scene(&frame);
            }
            EXPORT => {
                let fd = FileDialog::builder(&frame).with_message("Export Solar Results").with_wildcard("CSV (*.csv)|*.csv").with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt).build();
                if fd.show_modal() == ID_OK
                    && let Some(path) = fd.get_path()
                    && let Err(e) = self.run("solar.export", json!({ "path": path }))
                {
                    error(&frame, &e);
                }
            }
            _ => {}
        }
    }

    /// The shown study's grid squares, coloured by value (added to the 3D scene).
    pub(crate) fn solar_meshes(&mut self) -> Vec<SceneMesh> {
        let Some(id) = self.solar_shown.clone() else { return Vec::new() };
        let Some(store) = self.store_now() else { return Vec::new() };
        let Some(st) = solar::data(&store).studies.into_iter().find(|s| s.id == id) else { return Vec::new() };
        let all: Vec<f64> = st.results.faces.iter().flat_map(|f| f.values.iter().copied()).collect();
        let (lo, hi) = all.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        if all.is_empty() {
            return Vec::new();
        }
        self.frame.set_status_text(&format!("Solar study shown: {lo:.0} (blue) to {hi:.0} (red) kWh/m²"), 0);
        let half = st.grid * 0.47;
        let mut out = Vec::new();
        for f in &st.results.faces {
            let n = f.normal;
            if n == [0.0; 3] {
                continue;
            }
            // Square axes on the face: u horizontal (or x for flat faces), v = n × u.
            let mut u = [-n[1], n[0], 0.0];
            let l = (u[0] * u[0] + u[1] * u[1]).sqrt();
            u = if l < 1e-6 { [1.0, 0.0, 0.0] } else { [u[0] / l, u[1] / l, 0.0] };
            let v = [n[1] * u[2] - n[2] * u[1], n[2] * u[0] - n[0] * u[2], n[0] * u[1] - n[1] * u[0]];
            for (p, val) in f.points.iter().zip(&f.values) {
                let lift = 0.01;
                let c = [p[0] + n[0] * lift, p[1] + n[1] * lift, p[2] + n[2] * lift];
                let corner = |a: f64, b: f64| [(c[0] + (u[0] * a + v[0] * b) * half) as f32, (c[1] + (u[1] * a + v[1] * b) * half) as f32, (c[2] + (u[2] * a + v[2] * b) * half) as f32];
                out.push(SceneMesh {
                    element: None,
                    class: "IfcAnnotation".into(),
                    positions: vec![corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)],
                    indices: vec![0, 1, 2, 0, 2, 3],
                    color: ramp(if hi > lo { (val - lo) / (hi - lo) } else { 0.5 }),
                });
            }
        }
        out
    }
}
