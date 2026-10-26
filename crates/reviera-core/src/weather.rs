// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Weather for analysis (ANA-EM-001): an EPW file (EnergyPlus weather, an open published format)
//! read for its station and hourly records, with the design conditions derived from them, kept
//! with the project location (`rxd:location`, `weather`).
//!
//! - The file must hold 8 760 hourly records (8 784 in a leap year); a short or malformed file is
//!   refused, naming the line where it goes wrong.
//! - **Design conditions** from the hourly dry-bulb temperatures: heating dry-bulb at 99.6 % (the
//!   temperature at or above which 99.6 % of the hours lie), cooling dry-bulb at 0.4 % (exceeded
//!   in 0.4 % of the hours) with the mean wet-bulb of the hours at or above it, the mean daily
//!   range of the warmest month, and that month's clearness (global over extraterrestrial
//!   horizontal radiation). Percentiles are of the sorted hours, without interpolation.
//! - **Overrides**: a value the user types is kept, marked, over the file's (and survives the
//!   file being imported again).
//! - Wet-bulb comes from dry-bulb, relative humidity and pressure by the psychrometric balance
//!   (saturation pressure by the Magnus form), solved by bisection.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store::Store;
use crate::txn::Transaction;

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// The station an EPW file describes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Station {
    pub name: String,
    pub region: String,
    pub country: String,
    pub wmo: String,
    pub latitude: f64,
    pub longitude: f64,
    pub time_zone: f64,
    pub elevation: f64,
}

/// Design conditions (°C; daily range in K; clearness 0–1).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Design {
    pub heating_dry_bulb: Option<f64>,
    pub cooling_dry_bulb: Option<f64>,
    pub cooling_wet_bulb: Option<f64>,
    pub daily_range: Option<f64>,
    pub clearness: Option<f64>,
}

impl Design {
    /// `over`'s values laid over these.
    pub fn with(&self, over: &Design) -> Design {
        Design {
            heating_dry_bulb: over.heating_dry_bulb.or(self.heating_dry_bulb),
            cooling_dry_bulb: over.cooling_dry_bulb.or(self.cooling_dry_bulb),
            cooling_wet_bulb: over.cooling_wet_bulb.or(self.cooling_wet_bulb),
            daily_range: over.daily_range.or(self.daily_range),
            clearness: over.clearness.or(self.clearness),
        }
    }
}

/// The project's weather.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Weather {
    /// The weather file, relative to the project when it is beside it.
    pub file: Option<String>,
    pub station: Option<Station>,
    /// First and last year of the records.
    pub period: Option<[i32; 2]>,
    /// From the file.
    pub derived: Design,
    /// Typed by the user; kept over the file's.
    pub overrides: Design,
    /// Bumped on every change, so results computed from the weather can tell they are stale.
    pub revision: u64,
}

impl Weather {
    pub fn design(&self) -> Design {
        self.derived.with(&self.overrides)
    }
}

/// An EPW file read: its station, period and derived design conditions.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Epw {
    pub station: Station,
    pub period: [i32; 2],
    pub records: usize,
    pub design: Design,
}

/// Saturation vapour pressure over water (Pa) at `t` °C (Magnus form).
fn p_sat(t: f64) -> f64 {
    610.94 * (17.625 * t / (t + 243.04)).exp()
}

/// Humidity ratio (kg/kg) of air at total pressure `p` with vapour pressure `pv`.
fn w_of(pv: f64, p: f64) -> f64 {
    0.621_945 * pv / (p - pv).max(1.0)
}

/// Wet-bulb temperature (°C) from dry-bulb (°C), relative humidity (%) and pressure (Pa).
pub fn wet_bulb(t: f64, rh: f64, p: f64) -> f64 {
    let w = w_of(p_sat(t) * (rh / 100.0).clamp(0.0, 1.0), p);
    // Adiabatic saturation: W = ((2501 − 2.326 Twb) Ws(Twb) − 1.006 (T − Twb)) / (2501 + 1.86 T − 4.186 Twb).
    let f = |twb: f64| {
        let ws = w_of(p_sat(twb), p);
        ((2501.0 - 2.326 * twb) * ws - 1.006 * (t - twb)) / (2501.0 + 1.86 * t - 4.186 * twb) - w
    };
    let (mut lo, mut hi) = (t - 60.0, t);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if f(mid) > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    (lo + hi) / 2.0
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Reads an EPW file's text.
pub fn parse_epw(text: &str) -> Result<Epw> {
    let lines: Vec<&str> = text.lines().collect();
    let head = lines.first().copied().unwrap_or("");
    let f: Vec<&str> = head.split(',').map(str::trim).collect();
    if f.first().map(|s| s.to_uppercase()) != Some("LOCATION".into()) || f.len() < 10 {
        return Err(bad("weather.bad_epw", "line 1: an EPW file starts with a LOCATION line (city, region, country, source, WMO, latitude, longitude, time zone, elevation)"));
    }
    let num = |s: &str, what: &str, line: usize| s.trim().parse::<f64>().map_err(|_| bad("weather.bad_epw", format!("line {line}: the {what} {s:?} is not a number")));
    let station = Station {
        name: f[1].to_owned(),
        region: f[2].to_owned(),
        country: f[3].to_owned(),
        wmo: f[5].to_owned(),
        latitude: num(f[6], "latitude", 1)?,
        longitude: num(f[7], "longitude", 1)?,
        time_zone: num(f[8], "time zone", 1)?,
        elevation: num(f[9], "elevation", 1)?,
    };
    // Eight header lines, then the hourly records.
    if lines.len() < 9 {
        return Err(bad("weather.bad_epw", format!("the file ends at line {} before its hourly records", lines.len())));
    }
    // (month, day, dry-bulb, wet-bulb, global horizontal, extraterrestrial horizontal)
    let mut hours: Vec<(usize, usize, f64, f64, f64, f64)> = Vec::new();
    let mut years = (i32::MAX, i32::MIN);
    for (k, line) in lines.iter().enumerate().skip(8) {
        let n = k + 1;
        if line.trim().is_empty() {
            continue;
        }
        let c: Vec<&str> = line.split(',').collect();
        if c.len() < 16 {
            return Err(bad("weather.bad_epw", format!("line {n}: an hourly record has {} fields; it needs at least 16", c.len())));
        }
        let year = c[0].trim().parse::<i32>().map_err(|_| bad("weather.bad_epw", format!("line {n}: the year {:?} is not a number", c[0])))?;
        let month = num(c[1], "month", n)? as usize;
        let day = num(c[2], "day", n)? as usize;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(bad("weather.bad_epw", format!("line {n}: month {month}, day {day} is not a date")));
        }
        let t = num(c[6], "dry-bulb temperature", n)?;
        let rh = num(c[8], "relative humidity", n)?;
        let p = num(c[9], "pressure", n)?;
        if !(-90.0..70.0).contains(&t) {
            return Err(bad("weather.bad_epw", format!("line {n}: a dry-bulb temperature of {t} °C is out of range (a missing value?)")));
        }
        let p = if (31_000.0..120_000.0).contains(&p) { p } else { 101_325.0 };
        let ext = num(c[10], "extraterrestrial radiation", n)?;
        let glob = num(c[13], "global horizontal radiation", n)?;
        years = (years.0.min(year), years.1.max(year));
        hours.push((month, day, t, wet_bulb(t, rh.clamp(0.0, 100.0), p), glob.max(0.0), ext.max(0.0)));
    }
    let leap = hours.iter().any(|h| h.0 == 2 && h.1 == 29);
    let want = if leap && is_leap(years.1) || hours.len() == 8784 { 8784 } else { 8760 };
    if hours.len() != want {
        return Err(bad(
            "weather.short_epw",
            format!("the hourly records end at line {} after {} of {want} records", 8 + hours.len(), hours.len()),
        ));
    }
    let mut temps: Vec<f64> = hours.iter().map(|h| h.2).collect();
    temps.sort_by(f64::total_cmp);
    let n = temps.len();
    let heating = temps[((n as f64) * 0.004).floor() as usize];
    let cooling = temps[(((n as f64) * 0.996).ceil() as usize).min(n - 1)];
    let hot: Vec<f64> = hours.iter().filter(|h| h.2 >= cooling - 1e-9).map(|h| h.3).collect();
    let mcwb = hot.iter().sum::<f64>() / hot.len().max(1) as f64;
    // The warmest month by mean dry-bulb.
    let mut sums = [(0.0, 0usize); 13];
    for h in &hours {
        sums[h.0].0 += h.2;
        sums[h.0].1 += 1;
    }
    let warm = (1..=12).max_by(|a, b| {
        let m = |k: usize| if sums[k].1 > 0 { sums[k].0 / sums[k].1 as f64 } else { f64::MIN };
        m(*a).total_cmp(&m(*b))
    });
    let (mut range_sum, mut days, mut glob, mut ext) = (0.0, 0usize, 0.0, 0.0);
    if let Some(m) = warm {
        for d in 1..=31 {
            let day: Vec<f64> = hours.iter().filter(|h| h.0 == m && h.1 == d).map(|h| h.2).collect();
            if !day.is_empty() {
                range_sum += day.iter().copied().fold(f64::MIN, f64::max) - day.iter().copied().fold(f64::MAX, f64::min);
                days += 1;
            }
        }
        for h in hours.iter().filter(|h| h.0 == m && h.5 > 0.0) {
            glob += h.4;
            ext += h.5;
        }
    }
    let round = |v: f64, k: f64| (v * k).round() / k;
    Ok(Epw {
        station,
        period: [years.0, years.1],
        records: hours.len(),
        design: Design {
            heating_dry_bulb: Some(round(heating, 10.0)),
            cooling_dry_bulb: Some(round(cooling, 10.0)),
            cooling_wet_bulb: Some(round(mcwb, 10.0)),
            daily_range: (days > 0).then(|| round(range_sum / days as f64, 10.0)),
            clearness: (ext > 0.0).then(|| round((glob / ext).clamp(0.0, 1.0), 100.0)),
        },
    })
}

/// Great-circle distance (km) between two points in degrees.
pub fn distance_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, lo1, la2, lo2) = (a.0.to_radians(), a.1.to_radians(), b.0.to_radians(), b.1.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2) + la1.cos() * la2.cos() * ((lo2 - lo1) / 2.0).sin().powi(2);
    2.0 * 6371.0 * h.sqrt().asin()
}

pub fn get(store: &Store) -> Weather {
    crate::location::get(store).weather
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyParams {
    /// The weather file as kept (relative to the project when beside it).
    pub file: String,
    pub epw: EpwIn,
    /// Also take the station's latitude, longitude, elevation and time zone as the project's.
    #[serde(default)]
    pub set_location: bool,
}

/// An EPW file read, as `weather.import` passes it.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EpwIn {
    pub station: Station,
    pub period: [i32; 2],
    pub design: Design,
}

/// `weather.apply`: the imported file and its values; the user's overrides are kept.
pub(crate) fn apply(txn: &mut Transaction<'_>, p: ApplyParams) -> Result<Weather> {
    let mut l = crate::location::get(txn.store());
    let w = &mut l.weather;
    w.file = Some(p.file);
    w.station = Some(p.epw.station.clone());
    w.period = Some(p.epw.period);
    w.derived = p.epw.design;
    w.revision += 1;
    if p.set_location {
        let s = &p.epw.station;
        l.latitude = Some(s.latitude);
        l.longitude = Some(s.longitude);
        l.elevation = s.elevation;
        l.time_zone = Some(s.time_zone);
        if l.place.is_none() && !s.name.is_empty() {
            l.place = Some(s.name.clone());
        }
    }
    crate::location::put(txn, &l)?;
    Ok(l.weather)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesignParams {
    /// The values typed; a missing value is no override (the file's is used).
    pub overrides: Design,
}

/// `weather.set_design`: the user's design conditions over the file's.
pub(crate) fn set_design(txn: &mut Transaction<'_>, p: DesignParams) -> Result<Weather> {
    let o = &p.overrides;
    if let (Some(h), Some(c)) = (o.heating_dry_bulb, o.cooling_dry_bulb)
        && h >= c
    {
        return Err(bad("weather.bad_design", "the heating design temperature must be below the cooling one"));
    }
    if o.clearness.is_some_and(|c| !(0.0..=1.2).contains(&c)) || o.daily_range.is_some_and(|d| !(0.0..=40.0).contains(&d)) {
        return Err(bad("weather.bad_design", "clearness runs from 0 to 1.2 and the daily range from 0 to 40 K"));
    }
    let mut l = crate::location::get(txn.store());
    l.weather.overrides = p.overrides;
    l.weather.revision += 1;
    crate::location::put(txn, &l)?;
    Ok(l.weather)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// `weather.get`.
pub(crate) fn get_cmd(store: &Store, _p: Empty) -> Result<Weather> {
    Ok(get(store))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wet_bulb_by_the_psychrometric_balance() {
        // Saturated air: the wet-bulb is the dry-bulb.
        assert!((wet_bulb(20.0, 100.0, 101_325.0) - 20.0).abs() < 0.05);
        // 30 °C at 40 %: about 20 °C on a psychrometric chart.
        let w = wet_bulb(30.0, 40.0, 101_325.0);
        assert!((19.5..20.5).contains(&w), "{w}");
    }

    #[test]
    fn great_circle_distance() {
        // One degree of latitude is about 111 km.
        assert!((distance_km((0.0, 0.0), (1.0, 0.0)) - 111.19).abs() < 0.1);
    }
}
