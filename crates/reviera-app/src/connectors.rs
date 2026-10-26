// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Data connectors (ANA-ENV-010): open online services the user calls on purpose, each
//! switchable off in the per-user settings (`connectors.json`), none needed offline.
//!
//! - **PVGIS** (EU JRC, no key, data CC BY 4.0): `pvgis.tmy` fetches a typical meteorological
//!   year for the project location as EPW and imports it (`weather.import`); `pvgis.pv` asks
//!   PVcalc for a fixed PV system's yearly and monthly yield, a cross-check of the solar study's
//!   PV potential (ANA-ENV-002).
//!
//! The service address is PVGIS's own; `REVIERA_PVGIS_URL` replaces it (tests, mirrors), and
//! plain http is accepted only for a local address.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

const PVGIS: &str = "https://re.jrc.ec.europa.eu/api/v5_3/";

/// A connector as listed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Connector {
    pub name: String,
    pub enabled: bool,
    pub description: String,
    /// What a result made with it must credit.
    pub attribution: String,
    pub needs_key: bool,
}

const KNOWN: &[(&str, &str, &str)] = &[(
    "pvgis",
    "PVGIS (European Commission Joint Research Centre): typical meteorological years as EPW and PV yield estimates, worldwide except polar regions; no key",
    "PVGIS © European Union, 2001–2026; data under CC BY 4.0",
)];

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    enabled: BTreeMap<String, bool>,
}

fn settings_file() -> Result<PathBuf> {
    crate::subscriptions::config_dir().map(|d| d.join("connectors.json")).ok_or_else(|| bad("connectors.no_settings", "there is no per-user settings folder"))
}

fn load() -> Settings {
    settings_file().ok().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn enabled(name: &str) -> bool {
    load().enabled.get(name).copied().unwrap_or(true)
}

fn require(name: &str) -> Result<()> {
    if enabled(name) { Ok(()) } else { Err(bad("connectors.off", format!("the {name} connector is switched off (connectors.set)"))) }
}

pub(crate) struct ConnectorsList;

impl HostCommand for ConnectorsList {
    type Host = App;
    type Params = Option<Value>;
    type Output = Vec<Connector>;
    const META: CommandMeta = CommandMeta {
        name: "connectors.list",
        version: 1,
        summary: "List data connectors",
        description: "The online data connectors (ANA-ENV-010) with whether each is switched on, what it does, the attribution its data needs and whether it needs the user's key. Connectors only run when the user asks; nothing needs them offline.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, _: &mut App, _: &Caller, _: Option<Value>) -> Result<Vec<Connector>> {
        let s = load();
        Ok(KNOWN.iter().map(|(n, d, a)| Connector { name: (*n).into(), enabled: s.enabled.get(*n).copied().unwrap_or(true), description: (*d).into(), attribution: (*a).into(), needs_key: false }).collect())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub name: String,
    pub enabled: bool,
}

pub(crate) struct ConnectorsSet;

impl HostCommand for ConnectorsSet {
    type Host = App;
    type Params = SetParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "connectors.set",
        version: 1,
        summary: "Switch a data connector on or off",
        description: "Switches a data connector (`name`, e.g. pvgis) on or off for this user (per-user settings, connectors.json). A connector that is off refuses to run.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, _: &mut App, _: &Caller, p: SetParams) -> Result<()> {
        if !KNOWN.iter().any(|k| k.0 == p.name) {
            return Err(bad("connectors.unknown", format!("no connector is called {}", p.name)));
        }
        let mut s = load();
        s.enabled.insert(p.name, p.enabled);
        let path = settings_file()?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| bad("connectors.settings_unwritable", e.to_string()))?;
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&s).expect("serialises")).map_err(|e| bad("connectors.settings_unwritable", e.to_string()))
    }
}

/// The PVGIS address, checked.
fn pvgis_base() -> Result<String> {
    let base = std::env::var("REVIERA_PVGIS_URL").unwrap_or_else(|_| PVGIS.into());
    let local = ["http://127.0.0.1", "http://localhost", "http://[::1]"].iter().any(|p| base.starts_with(p));
    if !base.starts_with("https://") && !local {
        return Err(bad("connectors.insecure", "the PVGIS address must use https (plain http only for a local address)"));
    }
    Ok(if base.ends_with('/') { base } else { format!("{base}/") })
}

/// Fetches `tool` with `query`; PVGIS answers errors as JSON with a message.
fn pvgis_get(tool: &str, query: &[(&str, String)]) -> Result<String> {
    let client = reviera_compute_proto::subscription::public_client(Duration::from_secs(60)).map_err(|e| bad("connectors.tls", e.to_string()))?;
    // The values are numbers and plain words: no escaping needed.
    let qs: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let url = format!("{}{tool}?{}", pvgis_base()?, qs.join("&"));
    let resp = client.get(&url).send().map_err(|e| bad("connectors.unreachable", format!("PVGIS did not answer: {e}")))?;
    let status = resp.status();
    let text = resp.text().map_err(|e| bad("connectors.unreachable", format!("PVGIS's answer was cut off: {e}")))?;
    if !status.is_success() {
        let why = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["message"].as_str().map(str::to_owned)).unwrap_or_else(|| text.chars().take(200).collect());
        return Err(bad("connectors.refused", format!("PVGIS answered {}: {why}", status.as_u16())));
    }
    Ok(text)
}

fn site(app: &App) -> Result<(f64, f64)> {
    let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
    let loc = reviera_core::location::get(app.document(doc).expect("active").doc.store());
    match (loc.latitude, loc.longitude) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err(bad("connectors.no_location", "set the project location first (Sustainability › Site › Location)")),
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TmyParams {
    /// The years the typical months are chosen from (PVGIS's whole record when omitted).
    pub start_year: Option<u32>,
    pub end_year: Option<u32>,
}

pub(crate) struct PvgisTmy;

impl HostCommand for PvgisTmy {
    type Host = App;
    type Params = Option<TmyParams>;
    type Output = crate::weather::ImportReport;
    const META: CommandMeta = CommandMeta {
        name: "pvgis.tmy",
        version: 1,
        summary: "Weather from PVGIS",
        description: "Fetches a typical meteorological year for the project location from PVGIS (ANA-ENV-010; EU JRC, no key, CC BY 4.0) as an EPW file, keeps it with the saved project (<project>.resources/weather/pvgis-<lat>_<lon>.epw) and imports it (weather.import: station, design conditions; the user's overrides kept). `start_year`, `end_year` limit the years it draws on. Needs a saved project with a location and the connector switched on.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: Option<TmyParams>) -> Result<crate::weather::ImportReport> {
        require("pvgis")?;
        let p = p.unwrap_or_default();
        let (lat, lon) = site(app)?;
        let dir = crate::weather::resource_dir(app).ok_or_else(|| bad("connectors.untitled", "save the project first: the weather file is kept beside it"))?.join("weather");
        let mut q = vec![("lat", format!("{lat:.4}")), ("lon", format!("{lon:.4}")), ("outputformat", "epw".to_owned())];
        if let Some(y) = p.start_year {
            q.push(("startyear", y.to_string()));
        }
        if let Some(y) = p.end_year {
            q.push(("endyear", y.to_string()));
        }
        let text = pvgis_get("tmy", &q)?;
        if !text.starts_with("LOCATION") {
            return Err(bad("connectors.bad_answer", "PVGIS did not return an EPW file"));
        }
        std::fs::create_dir_all(&dir).map_err(|e| bad("weather.copy", format!("{}: {e}", dir.display())))?;
        let path = dir.join(format!("pvgis-{lat:.3}_{lon:.3}.epw"));
        std::fs::write(&path, text).map_err(|e| bad("weather.copy", format!("{}: {e}", path.display())))?;
        let out = app.execute("weather.import", json!({ "path": path }), caller)?;
        serde_json::from_value(out.result).map_err(|e| bad("connectors.internal", e.to_string()))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PvParams {
    /// kW peak.
    pub peak_power: f64,
    /// System losses, % (default 14).
    pub loss: Option<f64>,
    /// Tilt from horizontal, degrees (default 35).
    pub tilt: Option<f64>,
    /// Azimuth, degrees clockwise from north (default 180, south).
    pub azimuth: Option<f64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PvOut {
    /// kWh a year.
    pub yearly: f64,
    /// kWh in each month.
    pub monthly: Vec<f64>,
    /// kWh a year per kW peak.
    pub specific: f64,
    /// kWh/m² a year on the plane.
    pub irradiation: Option<f64>,
    pub attribution: String,
}

pub(crate) struct PvgisPv;

impl HostCommand for PvgisPv {
    type Host = App;
    type Params = PvParams;
    type Output = PvOut;
    const META: CommandMeta = CommandMeta {
        name: "pvgis.pv",
        version: 1,
        summary: "PV yield from PVGIS",
        description: "Asks PVGIS PVcalc (ANA-ENV-010; EU JRC, no key, CC BY 4.0) for a fixed, free-standing crystalline-silicon PV system at the project location: `peak_power` kWp, `loss` % (14), `tilt` degrees (35), `azimuth` degrees clockwise from north (180). Returns the yearly and monthly yield, kWh/kWp and the plane irradiation: a cross-check of the solar study's PV potential.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: PvParams) -> Result<PvOut> {
        require("pvgis")?;
        if !(p.peak_power > 0.0 && p.peak_power <= 100_000.0) {
            return Err(bad("connectors.bad_option", "peak_power must be above 0 and at most 100 000 kWp"));
        }
        let (lat, lon) = site(app)?;
        let loss = p.loss.unwrap_or(14.0);
        let tilt = p.tilt.unwrap_or(35.0);
        if !(0.0..=100.0).contains(&loss) || !(0.0..=90.0).contains(&tilt) {
            return Err(bad("connectors.bad_option", "loss must be 0–100% and tilt 0–90°"));
        }
        // PVGIS's aspect: 0 south, 90 west, −90 east.
        let aspect = (p.azimuth.unwrap_or(180.0) - 180.0 + 540.0) % 360.0 - 180.0;
        let q = [("lat", format!("{lat:.4}")), ("lon", format!("{lon:.4}")), ("peakpower", format!("{}", p.peak_power)), ("loss", format!("{loss}")), ("angle", format!("{tilt}")), ("aspect", format!("{aspect}")), ("outputformat", "json".to_owned())];
        let text = pvgis_get("PVcalc", &q)?;
        let v: Value = serde_json::from_str(&text).map_err(|e| bad("connectors.bad_answer", format!("PVGIS's answer is not JSON: {e}")))?;
        let totals = &v["outputs"]["totals"]["fixed"];
        let yearly = totals["E_y"].as_f64().ok_or_else(|| bad("connectors.bad_answer", "PVGIS's answer has no yearly yield (outputs.totals.fixed.E_y)"))?;
        let monthly = v["outputs"]["monthly"]["fixed"].as_array().map(|a| a.iter().filter_map(|m| m["E_m"].as_f64()).collect()).unwrap_or_default();
        Ok(PvOut { yearly, monthly, specific: yearly / p.peak_power, irradiation: totals["H(i)_y"].as_f64(), attribution: KNOWN[0].2.into() })
    }
}
