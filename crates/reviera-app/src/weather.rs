// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Importing a weather file (ANA-EM-001): `weather.import(path)` reads an EPW file, copies it
//! into the project's resource folder (`<project>.resources/weather/`, so the project stays
//! portable; an untitled project keeps the path given until it is saved), and applies its
//! station and design conditions (`weather.apply`). The project location takes the station's
//! when it has none. A station more than 50 km from the site, or more than 300 m above or below
//! it, is warned about and accepted.

use std::path::{Path, PathBuf};

use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// Warn beyond these (km, m).
const FAR_KM: f64 = 50.0;
const HIGH_M: f64 = 300.0;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportParams {
    /// The .epw file.
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ImportReport {
    /// The file as kept with the project.
    pub file: String,
    pub weather: reviera_core::weather::Weather,
    /// Kilometres from the site to the station, when the site has a location.
    pub distance_km: Option<f64>,
    pub warnings: Vec<String>,
}

pub(crate) struct WeatherImport;

fn resource_dir(app: &App) -> Option<PathBuf> {
    let d = app.document(app.active()?)?;
    if d.untitled {
        return None;
    }
    let path = d.project.path();
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    Some(path.parent()?.join(format!("{stem}.resources")))
}

impl HostCommand for WeatherImport {
    type Host = App;
    type Params = ImportParams;
    type Output = ImportReport;
    const META: CommandMeta = CommandMeta {
        name: "weather.import",
        version: 1,
        summary: "Import a weather file",
        description: "Reads an EPW weather file (`path`; 8 760 hourly records, 8 784 in a leap year, else refused naming the line) for its station and the design conditions derived from its hours: heating dry-bulb 99.6 %, cooling dry-bulb 0.4 % with mean coincident wet-bulb, the warmest month's daily range and clearness (ANA-EM-001). The file is copied into the project's resource folder (<project>.resources/weather). The user's overrides of the design conditions are kept. When the project has no location, it takes the station's. Warns when the station is more than 50 km away or 300 m higher or lower. Returns the weather as kept.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: ImportParams) -> Result<ImportReport> {
        let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
        let src = PathBuf::from(&p.path);
        let bytes = std::fs::read(&src).map_err(|e| bad("weather.read", format!("{}: {e}", src.display())))?;
        let epw = reviera_core::weather::parse_epw(&String::from_utf8_lossy(&bytes))?;
        let kept = match resource_dir(app) {
            Some(dir) => {
                let folder = dir.join("weather");
                std::fs::create_dir_all(&folder).map_err(|e| bad("weather.copy", format!("{}: {e}", folder.display())))?;
                let name = src.file_name().ok_or_else(|| bad("weather.read", "the path names no file"))?;
                let dest = folder.join(name);
                if dest != src {
                    std::fs::write(&dest, &bytes).map_err(|e| bad("weather.copy", format!("{}: {e}", dest.display())))?;
                }
                let base = dir.parent().unwrap_or(Path::new(""));
                dest.strip_prefix(base).map_or_else(|_| dest.display().to_string(), |r| r.display().to_string())
            }
            None => src.display().to_string(),
        };
        let store = app.document(doc).expect("active").doc.store().clone();
        let loc = reviera_core::location::get(&store);
        let mut warnings = Vec::new();
        let s = &epw.station;
        let distance = match (loc.latitude, loc.longitude) {
            (Some(la), Some(lo)) => {
                let d = reviera_core::weather::distance_km((la, lo), (s.latitude, s.longitude));
                if d > FAR_KM {
                    warnings.push(format!("the weather station {} is {d:.0} km from the site", s.name));
                }
                if (s.elevation - loc.elevation).abs() > HIGH_M {
                    warnings.push(format!("the weather station is {:.0} m {} the site", (s.elevation - loc.elevation).abs(), if s.elevation > loc.elevation { "above" } else { "below" }));
                }
                Some(d)
            }
            _ => None,
        };
        let out = app.execute(
            "weather.apply",
            json!({ "file": kept, "epw": { "station": epw.station, "period": epw.period, "design": epw.design }, "set_location": loc.latitude.is_none() }),
            caller,
        )?;
        let weather = serde_json::from_value(out.result).map_err(|e| bad("weather.internal", e.to_string()))?;
        Ok(ImportReport { file: kept, weather, distance_km: distance, warnings })
    }
}
