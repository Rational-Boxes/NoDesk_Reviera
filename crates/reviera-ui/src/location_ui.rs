// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Manage › Project Location › **Location** (DM-LOC-001): the place (name, latitude,
//! longitude, elevation, time zone, daylight saving), the project base point and the angle to
//! true north, the survey point with its shared coordinates, and the CRS, set together in one
//! undo step (`location.set_all`). **Report Coordinates** gives a typed point's internal,
//! project and shared coordinates. Plans show the project base point (a circle with a cross) and
//! the survey point (a triangle).

use reviera_core::location::{self, Location};
use reviera_render::plan::Lines;
use serde_json::json;
use wxdragon::prelude::*;

use super::{Gui, error};

fn num(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    format!("{r}")
}

fn triple(t: &str) -> Option<[f64; 3]> {
    let v: Vec<f64> = t.split([',', ';', ' ']).filter(|s| !s.trim().is_empty()).map(|s| s.trim().parse::<f64>()).collect::<Result<_, _>>().ok()?;
    match v[..] {
        [x, y] => Some([x, y, 0.0]),
        [x, y, z] => Some([x, y, z]),
        _ => None,
    }
}

impl Gui {
    pub(crate) fn location_dialog(&mut self) {
        let frame = self.frame;
        let Some(store) = self.store_now() else { return };
        let l = location::get(&store);
        let f = super::reinforcement_ui::Form::new(&frame, "Location and Coordinates");
        let place = f.text("Place", l.place.as_deref().unwrap_or(""), "A name for the location");
        let lat = f.text("Latitude (°)", &l.latitude.map_or(String::new(), num), "North positive, decimal degrees");
        let lon = f.text("Longitude (°)", &l.longitude.map_or(String::new(), num), "East positive, decimal degrees");
        let elev = f.text("Elevation (m)", &num(l.elevation), "Above sea level");
        let tz = f.text("Time zone (h)", &l.time_zone.map_or(String::new(), num), "Hours from UTC, e.g. 1 or -5");
        let dst = f.check("Daylight saving", "Use daylight saving time", l.daylight_saving);
        let base = f.text("Project base point", &format!("{}, {}, {}", num(l.project_base_point[0]), num(l.project_base_point[1]), num(l.project_base_point[2])), "x, y, z in internal coordinates (m)");
        let north = f.text("Angle to true north (°)", &num(l.true_north), "From project north, counter-clockwise");
        let sp = l.survey_point;
        let survey = f.text("Survey point", &format!("{}, {}, {}", num(sp.point[0]), num(sp.point[1]), num(sp.point[2])), "x, y, z in internal coordinates (m)");
        let shared = f.text("Shared coordinates", &format!("{}, {}, {}", num(sp.shared[0]), num(sp.shared[1]), num(sp.shared[2])), "Easting, northing, height at the survey point (m)");
        let crs = f.text("CRS", l.crs.as_deref().unwrap_or(""), "EPSG code, e.g. EPSG:25832; blank: none");
        if !f.show("Written to IFC on save: the site's latitude, longitude and elevation, true north, and with a CRS the map conversion.", "OK") {
            f.dialog.destroy();
            return;
        }
        let opt = |t: &TextCtrl| t.get_value().trim().replace(',', ".").parse::<f64>().ok();
        let read = || -> Result<Location, String> {
            Ok(Location {
                latitude: opt(&lat),
                longitude: opt(&lon),
                elevation: opt(&elev).unwrap_or(0.0),
                time_zone: opt(&tz),
                daylight_saving: dst.get_value(),
                place: Some(place.get_value().trim().to_owned()).filter(|s| !s.is_empty()),
                project_base_point: triple(&base.get_value()).ok_or("the project base point is x, y, z")?,
                survey_point: location::SurveyPoint {
                    point: triple(&survey.get_value()).ok_or("the survey point is x, y, z")?,
                    shared: triple(&shared.get_value()).ok_or("the shared coordinates are easting, northing, height")?,
                },
                true_north: opt(&north).unwrap_or(0.0),
                crs: Some(crs.get_value().trim().to_owned()).filter(|s| !s.is_empty()),
                weather: l.weather.clone(),
            })
        };
        let l = read();
        f.dialog.destroy();
        let result = l.map_err(|e| e.to_owned()).and_then(|l| self.run("location.set_all", json!({ "location": l })).map(|_| ()));
        match result {
            Ok(()) => {
                frame.set_status_text("Location and coordinates set", 0);
                self.refresh_scene(&frame);
                if let Some(doc) = self.doc {
                    self.plans_redraw(doc);
                }
            }
            Err(e) => error(&frame, &format!("The location was not set:\n{e}")),
        }
    }

    /// Report Coordinates: a typed internal point's project and shared coordinates.
    pub(crate) fn report_coordinates(&mut self) {
        let frame = self.frame;
        let d = TextEntryDialog::builder(&frame, "Point (x, y, z in internal coordinates, m):", "Report Coordinates").with_default_value("0, 0, 0").build();
        let v = (d.show_modal() == ID_OK).then(|| d.get_value()).flatten();
        d.destroy();
        let Some(p) = v.as_deref().and_then(triple) else { return };
        match self.run("coordinates.report", json!({ "point": p })) {
            Ok(r) => {
                let t = |k: &str| r[k].as_array().map(|a| a.iter().map(|x| format!("{:.4}", x.as_f64().unwrap_or(0.0))).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                let msg = format!("Internal: {}\nProject: {}\nShared (E, N, H): {}{}", t("internal"), t("project"), t("shared"), r["crs"].as_str().map(|c| format!("  ({c})")).unwrap_or_default());
                MessageDialog::builder(&frame, &msg, "Report Coordinates").with_style(MessageDialogStyle::OK | MessageDialogStyle::IconInformation).build().show_modal();
            }
            Err(e) => error(&frame, &e),
        }
    }

    /// The project base point and survey point symbols in a plan.
    pub(crate) fn location_lines(&mut self, index: usize) -> Vec<Lines> {
        let v = &self.views[index];
        let Some(plan) = v.plan.clone() else { return Vec::new() };
        let Some(store) = self.app.document(v.doc).map(|d| d.doc.store().clone()) else { return Vec::new() };
        let l = location::get(&store);
        let z = plan.range.cut as f32 + 0.004;
        let r = 2.5 * plan.scale / 1000.0;
        let mut base = Lines { color: [0.1, 0.35, 0.8, 1.0], width: 0.18, segments: Vec::new() };
        let mut survey = Lines { color: [0.15, 0.55, 0.2, 1.0], width: 0.18, segments: Vec::new() };
        let [bx, by, _] = l.project_base_point.map(|v| v as f32);
        for k in 0..24 {
            let (a0, a1) = (k as f32 / 24.0 * std::f32::consts::TAU, (k + 1) as f32 / 24.0 * std::f32::consts::TAU);
            base.segments.push([[bx + r * a0.cos(), by + r * a0.sin(), z], [bx + r * a1.cos(), by + r * a1.sin(), z]]);
        }
        base.segments.push([[bx - r, by, z], [bx + r, by, z]]);
        base.segments.push([[bx, by - r, z], [bx, by + r, z]]);
        let [sx, sy, _] = l.survey_point.point.map(|v| v as f32);
        let tri = [[sx, sy + r], [sx - r * 0.87, sy - r * 0.5], [sx + r * 0.87, sy - r * 0.5]];
        for i in 0..3 {
            let (a, b) = (tri[i], tri[(i + 1) % 3]);
            survey.segments.push([[a[0], a[1], z], [b[0], b[1], z]]);
        }
        vec![base, survey]
    }
}

impl Gui {
    /// Sustainability › Site › Weather (ANA-EM-001): the weather file and station, Import EPW…, and
    /// the design conditions (the file's shown beside; a typed value overrides it).
    pub(crate) fn weather_dialog(&mut self) {
        let frame = self.frame;
        loop {
            let Some(store) = self.store_now() else { return };
            let w = reviera_core::weather::get(&store);
            let station = w.station.as_ref().map_or("None — import an EPW file".to_owned(), |s| {
                format!("{} {} {} (WMO {}), {:.3}°, {:.3}°, {:.0} m", s.name, s.region, s.country, s.wmo, s.latitude, s.longitude, s.elevation)
            });
            let period = w.period.map_or(String::new(), |p| if p[0] == p[1] { format!("{}", p[0]) } else { format!("{}–{}", p[0], p[1]) });
            let f = super::reinforcement_ui::Form::new(&frame, "Weather");
            let file = f.text("Weather file", w.file.as_deref().unwrap_or(""), "Copied into the project's resource folder");
            file.set_editable(false);
            let st = f.text("Station", &station, &format!("Period of record: {period}"));
            st.set_editable(false);
            let d = &w.derived;
            let o = &w.overrides;
            // The file's value in the label; a typed value is the user's override.
            let field = |label: &str, over: Option<f64>, from: Option<f64>, unit: &str| {
                let shown = format!("{label} — file: {}{unit}", from.map_or("none".to_owned(), |v| format!("{v}")));
                f.text(&shown, &over.map_or(String::new(), |v| format!("{v}")), "Blank: the file's value; a typed value is kept as your override")
            };
            let heat = field("Heating dry-bulb 99.6 %", o.heating_dry_bulb, d.heating_dry_bulb, " °C");
            let cool = field("Cooling dry-bulb 0.4 %", o.cooling_dry_bulb, d.cooling_dry_bulb, " °C");
            let wet = field("Coincident wet-bulb", o.cooling_wet_bulb, d.cooling_wet_bulb, " °C");
            let range = field("Daily range", o.daily_range, d.daily_range, " K");
            let clear = field("Clearness", o.clearness, d.clearness, "");
            let import = f.check("Import", "Import an EPW file after OK", w.file.is_none());
            if !f.show("Typed values are kept as your overrides, also when the file is imported again.", "OK") {
                f.dialog.destroy();
                return;
            }
            let opt = |t: &TextCtrl| t.get_value().trim().replace(',', ".").parse::<f64>().ok();
            let overrides = json!({
                "heating_dry_bulb": opt(&heat), "cooling_dry_bulb": opt(&cool), "cooling_wet_bulb": opt(&wet),
                "daily_range": opt(&range), "clearness": opt(&clear),
            });
            let want_import = import.get_value();
            f.dialog.destroy();
            if let Err(e) = self.run("weather.set_design", json!({ "overrides": overrides })) {
                error(&frame, &e);
                continue;
            }
            if !want_import {
                return;
            }
            let fd = FileDialog::builder(&frame)
                .with_message("Import EPW Weather File")
                .with_wildcard("EnergyPlus weather (*.epw)|*.epw;*.EPW|All files (*.*)|*.*")
                .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
                .build();
            if fd.show_modal() != ID_OK {
                return;
            }
            let Some(path) = fd.get_path() else { return };
            match self.run("weather.import", json!({ "path": path })) {
                Ok(r) => {
                    let warns: Vec<String> = r["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_owned)).collect();
                    if !warns.is_empty() {
                        error(&frame, &format!("Imported, with warnings:\n{}", warns.join("\n")));
                    }
                }
                Err(e) => error(&frame, &format!("The weather file was not imported:\n{e}")),
            }
        }
    }
}
