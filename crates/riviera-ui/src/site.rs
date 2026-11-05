// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! The site in the window (ARC-TOPO-001 … 005, ARC-SITE-001): Massing & Site › Toposolid (from
//! a closed loop of selected model lines at a typed height, or from a points file of x y z
//! rows), Sub-Divide (a selected toposolid and a loop of model lines), Graded Region with
//! Cut/Fill, Simplify, Site Settings, Property Line (a closed loop of model lines, or a table of
//! distances and bearings) and Site Component (a component type, then clicks on the ground).

use riviera_core::{ElementId, mesh_objects, model_lines, toposolids};
use serde_json::{Value, json};
use wxdragon::prelude::*;

use super::Gui;

impl Gui {
    fn selected_toposolid(&mut self) -> Option<ElementId> {
        let store = self.store_now()?;
        self.selection()
            .into_iter()
            .find(|id| store.get(*id).and_then(toposolids::recipe).is_some())
    }

    /// The selected model lines joined into one closed loop in plan.
    fn selected_loop(&mut self) -> Option<Vec<[f64; 2]>> {
        let store = self.store_now()?;
        let lines: Vec<Vec<[f64; 2]>> = self
            .selection()
            .into_iter()
            .filter_map(|id| store.get(id).and_then(model_lines::recipe))
            .map(|r| r.curve.points())
            .collect();
        if lines.is_empty() {
            return None;
        }
        super::in_place::closed_loop(&lines)
    }

    fn ask_length(&mut self, title: &str, prompt: &str, default: f64) -> Option<f64> {
        let (frame, value) = (self.frame, self.format_length(default));
        let d = TextEntryDialog::builder(&frame, prompt, title)
            .with_default_value(&value)
            .build();
        if d.show_modal() != ID_OK {
            return None;
        }
        self.app
            .execute(
                "units.parse",
                json!({ "text": d.get_value(), "data_type": "length" }),
                &super::user(),
            )
            .ok()
            .and_then(|o| o.result.as_f64())
    }

    fn site_run(&mut self, command: &str, args: Value, done: &str) -> Option<Value> {
        let frame = self.frame;
        match self.run(command, args) {
            Ok(v) => {
                self.refresh_scene(&frame);
                frame.set_status_text(done, 0);
                Some(v)
            }
            Err(e) => {
                super::error(&frame, &e);
                None
            }
        }
    }

    /// Massing & Site › Toposolid.
    pub(crate) fn create_toposolid(&mut self) {
        let frame = self.frame;
        let Some(level) = self
            .active_view()
            .and_then(|v| v.plan.clone())
            .map(|p| p.level)
        else {
            return frame.set_status_text("Open the plan of the level the site is on", 0);
        };
        if let Some(boundary) = self.selected_loop() {
            let Some(z) = self.ask_length("Toposolid", "Height of the ground:", 0.0) else {
                return;
            };
            let points: Vec<[f64; 3]> = boundary.iter().map(|p| [p[0], p[1], z]).collect();
            self.site_run(
                "toposolid.create",
                json!({ "level_id": level, "boundary": boundary, "points": points }),
                "Toposolid made: Simplify, Sub-Divide or Graded Region to shape it",
            );
            return;
        }
        // Points from a survey file: x y z per row (spaces, tabs or commas), metres.
        let d = FileDialog::builder(&frame)
            .with_message("Toposolid from Points")
            .with_wildcard("Points (*.csv;*.txt;*.xyz)|*.csv;*.txt;*.xyz|All files (*.*)|*.*")
            .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
            .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(Ok(text)) = d.get_path().map(std::fs::read_to_string) else {
            return super::error(&frame, "The file could not be read");
        };
        let points = parse_points(&text);
        if points.len() < 3 {
            return super::error(
                &frame,
                "No points: the file needs rows of x y z (or select a closed loop of model lines first)",
            );
        }
        let n = points.len();
        self.site_run(
            "toposolid.create",
            json!({ "level_id": level, "points": points }),
            &format!("Toposolid made from {n} points"),
        );
    }

    /// Massing & Site › Sub-Divide.
    pub(crate) fn subdivide_toposolid(&mut self) {
        let frame = self.frame;
        let (Some(t), Some(region)) = (self.selected_toposolid(), self.selected_loop()) else {
            return frame.set_status_text(
                "Select a toposolid and a closed loop of model lines on it",
                0,
            );
        };
        let Some(offset) = self.ask_length(
            "Sub-Divide",
            "Height above the surface (negative to excavate):",
            0.0,
        ) else {
            return;
        };
        self.site_run(
            "toposolid.subdivide",
            json!({ "toposolid_id": t, "region": region, "offset": offset }),
            "Sub-division made",
        );
    }

    /// Massing & Site › Graded Region.
    pub(crate) fn graded_region(&mut self) {
        let frame = self.frame;
        let Some(t) = self.selected_toposolid() else {
            return frame.set_status_text("Select the existing toposolid", 0);
        };
        let modes = [
            "Copy every point (then edit them)",
            "Keep only the boundary's points",
        ];
        let d = SingleChoiceDialog::builder(&frame, "The graded copy:", "Graded Region", &modes)
            .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let mode = if d.get_selection() == 1 {
            "perimeter_only"
        } else {
            "copy"
        };
        self.site_run(
            "toposolid.create_graded_region",
            json!({ "toposolid_id": t, "mode": mode }),
            "Graded copy made (the existing one is hidden): change its points, then Cut/Fill",
        );
    }

    /// Massing & Site › Cut/Fill.
    pub(crate) fn cut_fill(&mut self) {
        let frame = self.frame;
        let Some(t) = self.selected_toposolid() else {
            return frame.set_status_text("Select a graded toposolid", 0);
        };
        match self.run("toposolid.cut_fill", json!({ "toposolid_id": t })) {
            Ok(v) => {
                let f = |k: &str| v[k].as_f64().unwrap_or(0.0);
                let dialog = MessageDialog::builder(
                    &frame,
                    &format!(
                        "Cut: {:.2} m³\nFill: {:.2} m³\nNet: {:.2} m³",
                        f("cut"),
                        f("fill"),
                        f("net")
                    ),
                    "Cut/Fill",
                )
                .with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation)
                .build();
                dialog.show_modal();
            }
            Err(e) => super::error(&frame, &e),
        }
    }

    /// Massing & Site › Simplify Surface.
    pub(crate) fn simplify_toposolid(&mut self) {
        let frame = self.frame;
        let Some(t) = self.selected_toposolid() else {
            return frame.set_status_text("Select a toposolid", 0);
        };
        let Some(store) = self.store_now() else {
            return;
        };
        let n = store
            .get(t)
            .and_then(toposolids::recipe)
            .map_or(0, |r| r.points.len());
        let d = TextEntryDialog::builder(
            &frame,
            &format!("Points to keep (it has {n}):"),
            "Simplify Surface",
        )
        .with_default_value(&(n / 2).max(3).to_string())
        .build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some(target) = d.get_value().and_then(|v| v.trim().parse::<usize>().ok()) else {
            return frame.set_status_text("Type a number of points", 0);
        };
        if let Some(v) = self.site_run(
            "toposolid.simplify",
            json!({ "toposolid_id": t, "target_points": target }),
            "Simplified",
        ) {
            frame.set_status_text(
                &format!(
                    "Simplified to {} points; the surface is within {} of the old one",
                    v["points"].as_u64().unwrap_or(0),
                    self.format_length(v["max_deviation"].as_f64().unwrap_or(0.0))
                ),
                0,
            );
        }
    }

    /// Massing & Site › Site Settings.
    pub(crate) fn site_settings(&mut self) {
        let Some(store) = self.store_now() else {
            return;
        };
        let s = toposolids::settings(&store);
        let Some(interval) =
            self.ask_length("Site Settings", "Contour interval:", s.contour_interval)
        else {
            return;
        };
        let Some(passing) = self.ask_length(
            "Site Settings",
            "Contours pass through the elevation:",
            s.passing_elevation,
        ) else {
            return;
        };
        self.site_run(
            "site.set_settings",
            json!({ "contour_interval": interval, "passing_elevation": passing }),
            "Site settings changed (every fifth contour from that elevation is heavier)",
        );
    }

    /// Massing & Site › Property Line.
    pub(crate) fn property_line(&mut self) {
        let frame = self.frame;
        let args = if let Some(mut points) = self.selected_loop() {
            points.push(points[0]);
            json!({ "points": points })
        } else {
            let (a, b) = (self.format_length(30.0), self.format_length(20.0));
            let example = format!("{a} N 0°E\n{b} N 90°E\n{a} S 0°E\n{b} S 90°W\n");
            let Some(text) = super::notes::ask_text(
                &frame,
                "Property Line: one row per side — distance and bearing (N 45°30'00\" E, or an azimuth)",
                &example,
            ) else {
                return;
            };
            let mut rows = Vec::new();
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                let line = line.trim();
                let (d, bearing) = split_row(line);
                let Some(distance) = self
                    .run("units.parse", json!({ "text": d, "data_type": "length" }))
                    .ok()
                    .and_then(|v| v.as_f64())
                else {
                    return super::error(&frame, &format!("Not a distance: {d}"));
                };
                rows.push(json!({ "distance": distance, "bearing": bearing.trim() }));
            }
            json!({ "table": rows })
        };
        if let Some(v) = self.site_run("propertyline.create", args, "Property line made")
            && let Some(a) = v["area"].as_f64()
        {
            frame.set_status_text(&format!("Property line made: {a:.2} m²"), 0);
        }
    }

    /// The selected model lines joined end to end into one open chain, in plan.
    fn selected_chain(&mut self) -> Option<Vec<[f64; 2]>> {
        let store = self.store_now()?;
        let mut lines: Vec<Vec<[f64; 2]>> = self
            .selection()
            .into_iter()
            .filter_map(|id| store.get(id).and_then(model_lines::recipe))
            .map(|r| r.curve.points())
            .collect();
        if lines.is_empty() {
            return None;
        }
        let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-4;
        let mut chain = lines.remove(0);
        while !lines.is_empty() {
            let (head, tail) = (chain[0], *chain.last().expect("non-empty"));
            let k = lines.iter().position(|l| near(l[0], tail) || near(*l.last().expect("a line"), tail) || near(l[0], head) || near(*l.last().expect("a line"), head))?;
            let mut l = lines.remove(k);
            if near(l[0], tail) {
                chain.extend(l.into_iter().skip(1));
            } else if near(*l.last().expect("a line"), tail) {
                l.reverse();
                chain.extend(l.into_iter().skip(1));
            } else {
                if near(l[0], head) {
                    l.reverse();
                }
                l.pop();
                l.extend(chain);
                chain = l;
            }
        }
        Some(chain)
    }

    /// Massing & Site › Alignment (ARC-ALN-001): its points from the selected model lines or a
    /// typed table, then an optional profile.
    pub(crate) fn alignment(&mut self) {
        let frame = self.frame;
        let Some(level) = self.active_view().and_then(|v| v.plan.clone()).map(|p| json!(p.level)).or_else(|| self.run("level.list", json!({})).ok().map(|l| l[0]["id"].clone())) else {
            return super::error(&frame, "The model has no level.");
        };
        let nums = |line: &str| -> Vec<f64> { line.split([',', ' ', '\t']).filter_map(|s| s.trim().parse().ok()).collect() };
        let (points, radii): (Vec<[f64; 2]>, Vec<f64>) = if let Some(chain) = self.selected_chain() {
            let n = chain.len().saturating_sub(2);
            let Some(r) = super::notes::ask_text(&frame, "Alignment: the curve radius at each of the inner corners, in metres (0: none), one a row", &"0\n".repeat(n)) else { return };
            (chain, r.lines().filter_map(|l| l.trim().parse().ok()).collect())
        } else {
            let Some(text) = super::notes::ask_text(&frame, "Alignment: one point of intersection a row, x, y in metres, and the curve radius at inner corners (or select model lines first)", "0, 0\n100, 0, 30\n100, 100\n") else { return };
            let rows: Vec<Vec<f64>> = text.lines().map(nums).filter(|v| v.len() >= 2).collect();
            let n = rows.len();
            let radii = rows.iter().enumerate().filter(|(i, _)| *i > 0 && *i + 1 < n).map(|(_, r)| r.get(2).copied().unwrap_or(0.0)).collect();
            (rows.iter().map(|r| [r[0], r[1]]).collect(), radii)
        };
        let Some(profile) = super::notes::ask_text(&frame, "Alignment: its vertical profile, one point a row: chainage, elevation and the vertical curve's length (empty: flat)", "") else { return };
        let profile: Vec<Value> = profile
            .lines()
            .map(nums)
            .filter(|v| v.len() >= 2)
            .map(|v| json!({ "station": v[0], "elevation": v[1], "curve": v.get(2).copied().filter(|c| *c > 0.0) }))
            .collect();
        let args = json!({ "level": level, "points": points, "radii": radii, "profile": profile });
        if let Some(v) = self.site_run("alignment.create", args, "Alignment made") {
            frame.set_status_text(&format!("Alignment made: {:.2} m, chainage {:.2} to {:.2}", v["length"].as_f64().unwrap_or(0.0), v["start_station"].as_f64().unwrap_or(0.0), v["end_station"].as_f64().unwrap_or(0.0)), 0);
        }
    }

    /// Massing & Site › Site Component: the type, then clicks.
    pub(crate) fn start_site_component(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else {
            return;
        };
        let types = mesh_objects::types(&store);
        if types.is_empty() {
            return frame.set_status_text(
                "Import a tree, car or other object first (Architecture › Component › Import Object)",
                0,
            );
        }
        let names: Vec<String> = types
            .iter()
            .map(|(id, _, _)| {
                store
                    .get(*id)
                    .and_then(|e| e.name.clone())
                    .unwrap_or_default()
            })
            .collect();
        let labels: Vec<&str> = names.iter().map(String::as_str).collect();
        let d =
            SingleChoiceDialog::builder(&frame, "Component:", "Site Component", &labels).build();
        if d.show_modal() != ID_OK {
            return;
        }
        let Some((id, _, _)) = usize::try_from(d.get_selection())
            .ok()
            .and_then(|i| types.get(i))
        else {
            return;
        };
        self.site_pick = Some(*id);
        frame.set_status_text(
            "Site Component: click on the ground in a plan (it follows the toposolid there); Esc ends",
            0,
        );
    }

    pub(crate) fn site_click(&mut self, x: i32, y: i32) {
        let Some(t) = self.site_pick else { return };
        let (Some(plan), Some(p)) = (
            self.active_view().and_then(|v| v.plan.clone()),
            self.plan_xy(x, y),
        ) else {
            return self
                .frame
                .set_status_text("Site components are placed in plans", 0);
        };
        let Some(store) = self.store_now() else {
            return;
        };
        // The topmost toposolid under the click hosts it.
        let host = store
            .iter()
            .filter(|e| toposolids::recipe(e).is_some())
            .filter_map(|e| Some((e.id(), toposolids::height_at(&store, e, p)?)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|h| h.0);
        self.site_run(
            "sitecomponent.place",
            json!({ "type_id": t, "point": p, "host_id": host, "level": plan.level }),
            "Placed; click again, or Esc",
        );
    }
}

/// A table row: its distance (in the project's units, spaces allowed) and its bearing (from its
/// N or S, or else the last word: an azimuth).
fn split_row(line: &str) -> (&str, &str) {
    match line.find(['N', 'S', 'n', 's']) {
        Some(i) => (line[..i].trim(), line[i..].trim()),
        None => line
            .rsplit_once(char::is_whitespace)
            .map_or((line, ""), |(d, b)| (d.trim(), b.trim())),
    }
}

/// Rows of `x y z` (spaces, tabs or commas); other rows (a header) are skipped.
fn parse_points(text: &str) -> Vec<[f64; 3]> {
    text.lines()
        .filter_map(|l| {
            let v: Vec<f64> = l
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            (v.len() >= 3).then(|| [v[0], v[1], v[2]])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn points_files() {
        let p = super::parse_points("x,y,z\n0,0,1\n1 2 3\n\n4\t5\t6\n");
        assert_eq!(p, vec![[0.0, 0.0, 1.0], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
    }

    #[test]
    fn table_rows() {
        assert_eq!(
            super::split_row("30 000 N 45°30'00\" E"),
            ("30 000", "N 45°30'00\" E")
        );
        assert_eq!(super::split_row("98' 6\" 120.5"), ("98' 6\"", "120.5"));
    }
}
