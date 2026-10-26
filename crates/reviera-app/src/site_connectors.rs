// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Site data connectors (ANA-ENV-010), beside PVGIS ([`crate::connectors`]); each is called only
//! when the user asks and can be switched off (`connectors.set`).
//!
//! - **Geocoding** (`geocode.search`): place names to coordinates from Nominatim
//!   (OpenStreetMap; data ODbL) or a compatible endpoint the user configures
//!   (`REVIERA_GEOCODER_URL`, such as a self-hosted Nominatim). Per Nominatim's usage policy:
//!   only searches the user starts, at most one a second, an identifying User-Agent, answers
//!   cached for the session, no autocomplete.
//! - **Elevation** (`elevation.fetch`): the ground height at the project location from
//!   Open-Meteo's elevation API (Copernicus DEM GLO-90, CC BY 4.0; free for non-commercial use,
//!   commercial use needs Open-Meteo's subscription), written to the location.
//! - **Grid carbon intensity** (`grid_intensity.fetch`): Great Britain's average grid carbon
//!   intensity over the last 30 days or year from the National Energy System Operator's Carbon
//!   Intensity API (no key, CC BY 4.0), as a grid factor for annual energy.
//!
//! The addresses can be replaced (`REVIERA_OPEN_METEO_URL`, `REVIERA_CARBON_INTENSITY_URL`);
//! plain http only for a local address.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::App;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

pub(crate) const NOMINATIM: &str = "https://nominatim.openstreetmap.org/";
pub(crate) const OPEN_METEO: &str = "https://api.open-meteo.com/v1/";
pub(crate) const CARBON_INTENSITY: &str = "https://api.carbonintensity.org.uk/";
/// An identifying User-Agent, as Nominatim's policy asks.
const USER_AGENT: &str = concat!("BIM-Reviera/", env!("CARGO_PKG_VERSION"), " (+", env!("CARGO_PKG_REPOSITORY"), ")");

/// A service's address: `env` when set, else `default`; https, or http for a local address.
pub(crate) fn base(env: &str, default: &str, service: &str) -> Result<String> {
    let base = std::env::var(env).ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| default.into());
    let local = ["http://127.0.0.1", "http://localhost", "http://[::1]"].iter().any(|p| base.starts_with(p));
    if !base.starts_with("https://") && !local {
        return Err(bad("connectors.insecure", format!("the {service} address must use https (plain http only for a local address)")));
    }
    Ok(if base.ends_with('/') { base } else { format!("{base}/") })
}

/// Percent-encodes a query value.
pub(crate) fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// GETs `url` as JSON; a refusal's message (`message` or `error.message`) is passed on.
fn get_json(service: &str, url: &str) -> Result<Value> {
    let client = reviera_compute_proto::subscription::public_client(Duration::from_secs(30)).map_err(|e| bad("connectors.tls", e.to_string()))?;
    let resp = client.get(url).header("User-Agent", USER_AGENT).header("Accept", "application/json").send().map_err(|e| bad("connectors.unreachable", format!("{service} did not answer: {e}")))?;
    let status = resp.status();
    let text = resp.text().map_err(|e| bad("connectors.unreachable", format!("{service}'s answer was cut off: {e}")))?;
    let v: Option<Value> = serde_json::from_str(&text).ok();
    if !status.is_success() {
        let why = v.as_ref().and_then(|v| v["message"].as_str().or_else(|| v["error"]["message"].as_str()).or_else(|| v["reason"].as_str()).map(str::to_owned)).unwrap_or_else(|| text.chars().take(200).collect());
        return Err(bad("connectors.refused", format!("{service} answered {}: {why}", status.as_u16())));
    }
    v.ok_or_else(|| bad("connectors.bad_answer", format!("{service} did not answer with JSON")))
}

// ---- Geocoding ----

/// A place found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Place {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// Its kind, such as `city` or `building`.
    pub kind: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    pub query: String,
    /// At most this many places (1–10, default 5).
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchOut {
    pub places: Vec<Place>,
    pub attribution: String,
    /// Whether the answer came from this session's cache.
    pub cached: bool,
}

type Cache = HashMap<(String, String, u32), Vec<Place>>;

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(Mutex::default)
}

/// Waits so that requests to the geocoder are at least a second apart.
fn pace() {
    static LAST: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    let mut last = LAST.get_or_init(Mutex::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(t) = *last {
        let gap = Duration::from_millis(1100);
        let since = t.elapsed();
        if since < gap {
            std::thread::sleep(gap - since);
        }
    }
    *last = Some(Instant::now());
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}

/// The places in a Nominatim `jsonv2` answer.
pub fn places(v: &Value) -> Vec<Place> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(Place {
                name: p["display_name"].as_str()?.to_owned(),
                lat: number(&p["lat"])?,
                lon: number(&p["lon"])?,
                kind: p["addresstype"].as_str().or_else(|| p["type"].as_str()).unwrap_or("").to_owned(),
            })
        })
        .collect()
}

pub(crate) struct GeocodeSearch;

impl HostCommand for GeocodeSearch {
    type Host = App;
    type Params = SearchParams;
    type Output = SearchOut;
    const META: CommandMeta = CommandMeta {
        name: "geocode.search",
        version: 1,
        summary: "Find a place",
        description: "Looks a place name or address up (ANA-ENV-010) with Nominatim (OpenStreetMap data, ODbL) or the geocoder in REVIERA_GEOCODER_URL, and returns up to `limit` places (1–10, default 5) with their coordinates; set the project location from one with location.set. Only for searches the user starts: at most one request a second, answers cached for the session, never as autocomplete. Needs the nominatim connector switched on.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, _: &mut App, _: &Caller, p: SearchParams) -> Result<SearchOut> {
        crate::connectors::require("nominatim")?;
        let q = p.query.split_whitespace().collect::<Vec<_>>().join(" ");
        if q.chars().count() < 2 {
            return Err(bad("geocode.empty", "type a place name or an address to look up"));
        }
        let limit = p.limit.unwrap_or(5).clamp(1, 10);
        let b = base("REVIERA_GEOCODER_URL", NOMINATIM, "geocoder")?;
        let attribution = "Data © OpenStreetMap contributors, ODbL 1.0 (openstreetmap.org/copyright)".to_owned();
        let key = (b.clone(), q.to_lowercase(), limit);
        if let Some(hit) = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
            return Ok(SearchOut { places: hit.clone(), attribution, cached: true });
        }
        pace();
        let v = get_json("The geocoder", &format!("{b}search?q={}&format=jsonv2&limit={limit}", encode(&q)))?;
        let found = places(&v);
        cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(key, found.clone());
        Ok(SearchOut { places: found, attribution, cached: false })
    }
}

// ---- Elevation ----

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElevationParams {
    /// Write it to the project location (default true).
    pub set: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ElevationOut {
    /// Metres above sea level.
    pub elevation: f64,
    pub set: bool,
    pub attribution: String,
}

pub(crate) struct ElevationFetch;

impl HostCommand for ElevationFetch {
    type Host = App;
    type Params = Option<ElevationParams>;
    type Output = ElevationOut;
    const META: CommandMeta = CommandMeta {
        name: "elevation.fetch",
        version: 1,
        summary: "Site elevation",
        description: "Fetches the ground elevation at the project location (ANA-ENV-010) from Open-Meteo's elevation API (Copernicus DEM GLO-90, 90 m grid, CC BY 4.0; free for non-commercial use) and, unless `set` is false, writes it to the location (location.set, one undoable step). Needs a location and the open_meteo connector switched on.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: Option<ElevationParams>) -> Result<ElevationOut> {
        crate::connectors::require("open_meteo")?;
        let set = p.and_then(|p| p.set).unwrap_or(true);
        let (lat, lon) = crate::connectors::site(app)?;
        let b = base("REVIERA_OPEN_METEO_URL", OPEN_METEO, "Open-Meteo")?;
        let v = get_json("Open-Meteo", &format!("{b}elevation?latitude={lat:.5}&longitude={lon:.5}"))?;
        let elevation = v["elevation"][0].as_f64().filter(|e| e.is_finite()).ok_or_else(|| bad("connectors.bad_answer", "Open-Meteo gave no elevation"))?;
        if set {
            app.execute("location.set", json!({ "lat": lat, "lon": lon, "elevation": elevation }), caller)?;
        }
        Ok(ElevationOut { elevation, set, attribution: "Elevation: Copernicus DEM GLO-90 via Open-Meteo.com, CC BY 4.0".into() })
    }
}

// ---- Grid carbon intensity ----

/// The civil date (year, month, day) of a day count from 1970-01-01.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn stamp(days: i64) -> String {
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}T00:00Z")
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntensityParams {
    /// `month` (the last 30 days, the default) or `year` (the last 360 days, in twelve
    /// 30-day windows).
    pub period: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IntensityOut {
    pub region: String,
    pub from: String,
    pub to: String,
    /// kg CO2e per kWh: the average, and the lowest and highest half-hour.
    pub average: f64,
    pub min: f64,
    pub max: f64,
    pub attribution: String,
}

pub(crate) struct GridIntensityFetch;

impl HostCommand for GridIntensityFetch {
    type Host = App;
    type Params = Option<IntensityParams>;
    type Output = IntensityOut;
    const META: CommandMeta = CommandMeta {
        name: "grid_intensity.fetch",
        version: 1,
        summary: "Grid carbon intensity",
        description: "Great Britain's average grid carbon intensity (ANA-ENV-010) from the Carbon Intensity API (National Energy System Operator, no key, CC BY 4.0) over the last 30 days (`period` month) or 360 days (year), with the lowest and highest half-hour, in kg CO2e/kWh — a grid factor for annual energy (energy.run options.grid_factor). For Great Britain only. Needs the uk_carbon_intensity connector switched on.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(&self, _: &mut App, _: &Caller, p: Option<IntensityParams>) -> Result<IntensityOut> {
        crate::connectors::require("uk_carbon_intensity")?;
        let windows = match p.and_then(|p| p.period).as_deref() {
            None | Some("month") => 1,
            Some("year") => 12,
            Some(o) => return Err(bad("grid_intensity.bad_period", format!("{o}: the period is month or year"))),
        };
        let b = base("REVIERA_CARBON_INTENSITY_URL", CARBON_INTENSITY, "Carbon Intensity API")?;
        let today = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64 / 86_400);
        let (mut sum, mut min, mut max) = (0.0, f64::MAX, f64::MIN);
        for w in 0..windows {
            let to = today - 30 * w;
            let v = get_json("The Carbon Intensity API", &format!("{b}intensity/stats/{}/{}", stamp(to - 30), stamp(to)))?;
            let i = &v["data"][0]["intensity"];
            let avg = i["average"].as_f64().ok_or_else(|| bad("connectors.bad_answer", "the Carbon Intensity API gave no average"))?;
            sum += avg;
            min = min.min(i["min"].as_f64().unwrap_or(avg));
            max = max.max(i["max"].as_f64().unwrap_or(avg));
        }
        Ok(IntensityOut {
            region: "Great Britain".into(),
            from: stamp(today - 30 * windows),
            to: stamp(today),
            average: sum / windows as f64 / 1000.0,
            min: min / 1000.0,
            max: max / 1000.0,
            attribution: "Carbon Intensity API, National Energy System Operator (carbonintensity.org.uk), CC BY 4.0".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(stamp(0), "1970-01-01T00:00Z");
        assert_eq!(stamp(20_752), "2026-10-26T00:00Z");
        assert_eq!(stamp(11_016), "2000-02-29T00:00Z");
    }

    #[test]
    fn encoding_and_places() {
        assert_eq!(encode("Rue de l'Église 3, Zürich"), "Rue%20de%20l%27%C3%89glise%203%2C%20Z%C3%BCrich");
        let v = json!([{ "display_name": "Berlin, Deutschland", "lat": "52.5173885", "lon": "13.3951309", "addresstype": "city" }, { "display_name": "broken" }]);
        assert_eq!(places(&v), vec![Place { name: "Berlin, Deutschland".into(), lat: 52.5173885, lon: 13.3951309, kind: "city".into() }]);
    }
}
