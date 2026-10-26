// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Sustainability › Daylight & Solar › **Daylight Factor** (ANA-LIGHT-002, ANA-ENV-006): the
//! sensor grid (height, spacing, border), the threshold, the room reflectances and the glazing
//! transmittance, then Run: a compute job the window watches. When it is collected, *Daylight
//! Results* lists each space's average, minimum, maximum, uniformity and share of sensors at the
//! threshold, says whether the study is stale, and *Show on Model* colours each sensor's square
//! in 3D views on a fixed scale (blue 0% to red at 2.5 × the threshold).

use reviera_core::daylight;
use reviera_render::SceneMesh;
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::{Gui, error};

const SHOW: i32 = 7202;
const HIDE: i32 = 7901;

impl Gui {
    pub(crate) fn daylight_dialog(&mut self) {
        let frame = self.frame;
        let last = self.run("daylight.results", json!({})).ok().map(|r| r["options"].clone()).unwrap_or(Value::Null);
        let d = daylight::DaylightOptions::default();
        let val = |k: &str, def: f64| format!("{}", last[k].as_f64().unwrap_or(def));
        let f = super::reinforcement_ui::Form::new(&frame, "Daylight Factor");
        let height = f.text("Sensor height (m)", &val("height", d.height), "Above each space's floor");
        let spacing = f.text("Sensor spacing (m)", &val("spacing", d.spacing), "0.1–5 m");
        let border = f.text("Border (m)", &val("border", d.border), "Sensors are kept this far from the walls");
        let threshold = f.text("Threshold (%)", &val("threshold", d.threshold), "The daylight factor a sensor should reach");
        let ceiling = f.text("Ceiling reflectance", &val("ceiling_reflectance", d.ceiling_reflectance), "0–0.95");
        let wall = f.text("Wall reflectance", &val("wall_reflectance", d.wall_reflectance), "0–0.95");
        let floor = f.text("Floor reflectance", &val("floor_reflectance", d.floor_reflectance), "0–0.95");
        let tau = f.text("Glazing transmittance", &val("glazing_transmittance", d.glazing_transmittance), "Visible transmittance of windows whose type gives none (Pset_DoorWindowGlazingType.VisibleLightTransmittance)");
        if !f.show("The CIE overcast sky seen through the windows from a grid in every enclosed space, plus the light reflected inside the room; runs out of process.", "Run") {
            f.dialog.destroy();
            return;
        }
        let num = |t: &TextCtrl, def: f64| t.get_value().trim().replace(',', ".").parse::<f64>().unwrap_or(def);
        let o = json!({
            "height": num(&height, d.height),
            "spacing": num(&spacing, d.spacing),
            "border": num(&border, d.border),
            "threshold": num(&threshold, d.threshold),
            "ceiling_reflectance": num(&ceiling, d.ceiling_reflectance),
            "wall_reflectance": num(&wall, d.wall_reflectance),
            "floor_reflectance": num(&floor, d.floor_reflectance),
            "glazing_transmittance": num(&tau, d.glazing_transmittance),
        });
        f.dialog.destroy();
        match self.run("daylight.run", json!({ "options": o })) {
            Ok(v) => self.watch_job(v["job_id"].as_str().unwrap_or_default().to_owned(), "daylight.collect", "Daylight factor"),
            Err(e) => error(&frame, &format!("The daylight study did not start:\n{e}")),
        }
    }

    pub(crate) fn daylight_results(&mut self) {
        let frame = self.frame;
        let r = match self.run("daylight.results", json!({})) {
            Ok(v) => v,
            Err(e) => return error(&frame, &e),
        };
        let stale = self.run("daylight.status", json!({})).ok().is_some_and(|s| s["stale"] == true);
        let pc = |v: &Value| format!("{:.1}", v.as_f64().unwrap_or(0.0));
        let rows: Vec<Vec<String>> = r["spaces"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                vec![
                    s["number"].as_str().unwrap_or("").to_owned(),
                    s["name"].as_str().unwrap_or("").to_owned(),
                    format!("{}", s["sensors"]),
                    pc(&s["average"]),
                    pc(&s["minimum"]),
                    pc(&s["maximum"]),
                    format!("{:.2}", s["uniformity"].as_f64().unwrap_or(0.0)),
                    format!("{:.0}%", 100.0 * s["above"].as_f64().unwrap_or(0.0)),
                    pc(&s["reflected"]),
                ]
            })
            .collect();
        let mut intro = format!(
            "Daylight factor under the CIE overcast sky: {:.1}% on average over every sensor; {:.0}% of the sensors reach {}%. Values in per cent; the reflected component is included in each.",
            r["average"].as_f64().unwrap_or(0.0),
            100.0 * r["above"].as_f64().unwrap_or(0.0),
            r["threshold"]
        );
        if stale {
            intro.push_str(" Stale: the model changed since; run it again.");
        }
        let (code, _) = super::mep_loads_ui::list_dialog(
            &frame,
            "Daylight Results",
            &intro,
            &[("Space", 60), ("Name", 110), ("Sensors", 60), ("Avg", 55), ("Min", 55), ("Max", 55), ("Unif.", 55), ("≥ thr.", 60), ("Refl.", 55)],
            &rows,
            &[(SHOW, "Show on Model", "Colour each sensor's square in 3D views"), (HIDE, "Hide", "Stop showing the study")],
            None,
        );
        match code {
            SHOW => {
                self.daylight_shown = r["id"].as_str().map(str::to_owned);
                self.refresh_scene(&frame);
            }
            HIDE => {
                self.daylight_shown = None;
                self.refresh_scene(&frame);
            }
            _ => {}
        }
    }

    /// The shown study's sensor squares (added to the 3D scene).
    pub(crate) fn daylight_meshes(&mut self) -> Vec<SceneMesh> {
        let Some(id) = self.daylight_shown.clone() else { return Vec::new() };
        let Some(store) = self.store_now() else { return Vec::new() };
        let Ok(st) = daylight::study(&store, Some(&id)) else { return Vec::new() };
        let top = (2.5 * st.results.threshold).max(1.0);
        self.frame.set_status_text(&format!("Daylight factor shown: 0% (blue) to {top:.1}% or more (red)"), 0);
        let half = (st.options.spacing * 0.47) as f32;
        let mut out = Vec::new();
        for s in &st.results.spaces {
            for (p, v) in s.points.iter().zip(&s.values) {
                let c = [p[0] as f32, p[1] as f32, p[2] as f32];
                out.push(SceneMesh {
                    element: None,
                    class: "IfcAnnotation".into(),
                    positions: vec![[c[0] - half, c[1] - half, c[2]], [c[0] + half, c[1] - half, c[2]], [c[0] + half, c[1] + half, c[2]], [c[0] - half, c[1] + half, c[2]]],
                    indices: vec![0, 1, 2, 0, 2, 3],
                    color: super::solar_ui::ramp(v / top),
                });
            }
        }
        out
    }
}
