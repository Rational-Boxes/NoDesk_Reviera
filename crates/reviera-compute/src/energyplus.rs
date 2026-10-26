// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `analysis.energy.energyplus` (ANA-ENV-007): annual energy by EnergyPlus where it is
//! installed. EnergyPlus is never bundled or renamed; it is found by `REVIERA_ENERGYPLUS`, on
//! the PATH, or in its default folders, and its version comes from `energyplus --version`. The
//! job writes the epJSON ([`reviera_core::energyplus::build`], with the installed version) and
//! the weather file to a working folder, runs `energyplus -w in.epw -d out in.epJSON`, reads
//! `out/eplusout.sql` and returns the same results as the built-in `analysis.energy`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use reviera_compute_proto::blob::Blob;
use reviera_compute_proto::pb::KindInfo;
use reviera_compute_proto::runner::{JobContext, JobFailure, Kind, Output};
use reviera_core::energyplus::{EplusInput, Row};

use crate::kinds::{ENERGY_RESULTS, EPLUS, EPLUS_INPUT, JSON, LICENCE};

/// The program's file name.
fn program() -> &'static str {
    if cfg!(windows) { "energyplus.exe" } else { "energyplus" }
}

/// The `major.minor` version an EnergyPlus program reports.
pub fn version_of(exe: &Path) -> Option<String> {
    let out = Command::new(exe).arg("--version").stdin(Stdio::null()).output().ok()?;
    reviera_core::energyplus::version_of(&String::from_utf8_lossy(&out.stdout)).or_else(|| reviera_core::energyplus::version_of(&String::from_utf8_lossy(&out.stderr)))
}

/// The installed EnergyPlus and its version: `REVIERA_ENERGYPLUS` (the program), else the
/// PATH, else the newest of the default folders (`/usr/local/EnergyPlus-*`,
/// `/Applications/EnergyPlus-*`, `C:\EnergyPlusV*`).
pub fn find() -> Option<(PathBuf, String)> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("REVIERA_ENERGYPLUS").filter(|p| !p.is_empty()) {
        // Set explicitly: that one or none.
        let p = PathBuf::from(p);
        return version_of(&p).map(|v| (p, v));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join(program())));
    }
    let roots: &[(&str, &str)] = if cfg!(windows) { &[("C:\\", "EnergyPlusV")] } else if cfg!(target_os = "macos") { &[("/Applications", "EnergyPlus-")] } else { &[("/usr/local", "EnergyPlus-")] };
    for (root, prefix) in roots {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(prefix))).collect();
        dirs.sort();
        candidates.extend(dirs.into_iter().rev().map(|d| d.join(program())));
    }
    candidates.into_iter().filter(|p| p.is_file()).find_map(|p| version_of(&p).map(|v| (p, v)))
}

/// The report rows of a finished run's `eplusout.sql` (outside warm-up).
pub fn read_sql(path: &Path) -> rusqlite::Result<Vec<Row>> {
    let db = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut st = db.prepare(
        "SELECT d.KeyValue, d.Name, d.ReportingFrequency, t.Month, t.TimeIndex, r.Value FROM ReportData r \
         JOIN ReportDataDictionary d ON r.ReportDataDictionaryIndex = d.ReportDataDictionaryIndex \
         JOIN Time t ON r.TimeIndex = t.TimeIndex WHERE t.WarmupFlag IS NULL OR t.WarmupFlag = 0",
    )?;
    let rows = st.query_map([], |r| {
        Ok(Row {
            key: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            name: r.get(1)?,
            frequency: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            month: r.get::<_, Option<i64>>(3)?.unwrap_or(1).clamp(1, 12) as u8,
            time: r.get(4)?,
            value: r.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
        })
    })?;
    rows.collect()
}

/// The counts of warnings and severe errors in `eplusout.err`, and its severe lines.
pub fn errors(text: &str) -> (usize, usize, Vec<String>) {
    let warnings = text.lines().filter(|l| l.trim_start().starts_with("** Warning **")).count();
    let severe: Vec<String> = text.lines().filter(|l| { let t = l.trim_start(); t.starts_with("** Severe  **") || t.starts_with("**  Fatal  **") }).map(|l| l.trim().to_owned()).collect();
    (warnings, severe.len(), severe)
}

#[derive(Debug, Default)]
pub struct EnergyPlusKind;

impl Kind for EnergyPlusKind {
    fn info(&self) -> KindInfo {
        KindInfo {
            kind: EPLUS.into(),
            version: 1,
            runner: "reviera-analysis-builtin".into(),
            runner_version: env!("CARGO_PKG_VERSION").into(),
            licence: LICENCE.into(),
            capabilities: vec!["batch".into(), "annual-energy".into(), "energyplus".into()],
            accepted_formats: vec![format!("{JSON};schema={EPLUS_INPUT}")],
            supports_checkpoints: false,
            supports_sessions: false,
        }
    }

    fn run(&self, ctx: &JobContext, _options: Option<&prost_types::Any>, inputs: &[Blob]) -> Result<Output, JobFailure> {
        ctx.progress(0.0, "reading");
        let blob = inputs.iter().find(|b| b.reference.schema == EPLUS_INPUT).ok_or_else(|| JobFailure::new("invalid_input", "no EnergyPlus input in the inputs"))?;
        let mut input: EplusInput = serde_json::from_slice(&blob.raw).map_err(|e| JobFailure::new("invalid_input", e.to_string()))?;
        let (exe, version) = find().ok_or_else(|| JobFailure::new("runner_failed", "EnergyPlus was not found: install it from energyplus.net (or set REVIERA_ENERGYPLUS to the program)"))?;
        ctx.log(format!("EnergyPlus {version} at {}", exe.display()));
        input.epjson["Version"] = serde_json::json!({ "Version 1": { "version_identifier": version } });
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("reviera-energyplus-{}-{stamp}", std::process::id()));
        let fail = |e: std::io::Error| JobFailure::new("runner_failed", format!("{}: {e}", dir.display()));
        std::fs::create_dir_all(dir.join("out")).map_err(fail)?;
        let result = (|| {
            std::fs::write(dir.join("in.epJSON"), serde_json::to_vec_pretty(&input.epjson).map_err(|e| JobFailure::new("runner_failed", e.to_string()))?).map_err(fail)?;
            std::fs::write(dir.join("in.epw"), &input.epw).map_err(fail)?;
            ctx.progress(0.05, "simulating");
            let log = std::fs::File::create(dir.join("out").join("stdout.txt")).map_err(fail)?;
            let mut child = Command::new(&exe)
                .current_dir(&dir)
                .args(["-w", "in.epw", "-d", "out", "in.epJSON"])
                .stdin(Stdio::null())
                .stdout(log.try_clone().map_err(fail)?)
                .stderr(log)
                .spawn()
                .map_err(|e| JobFailure::new("runner_failed", format!("{}: {e}", exe.display())))?;
            let status = loop {
                if ctx.cancelled() {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(JobFailure::new("cancelled", "cancelled"));
                }
                match child.try_wait() {
                    Ok(Some(s)) => break s,
                    Ok(None) => std::thread::sleep(Duration::from_millis(200)),
                    Err(e) => return Err(JobFailure::new("runner_failed", e.to_string())),
                }
            };
            let err = std::fs::read_to_string(dir.join("out").join("eplusout.err")).unwrap_or_default();
            let (warnings, severe, lines) = errors(&err);
            ctx.log(format!("EnergyPlus finished ({status}): {warnings} warnings, {severe} severe errors"));
            for l in lines.iter().take(20) {
                ctx.warning(l.clone());
            }
            let sql = dir.join("out").join("eplusout.sql");
            if !status.success() || !sql.is_file() {
                let why = lines.iter().take(5).cloned().collect::<Vec<_>>().join("; ");
                return Err(JobFailure::new("runner_failed", format!("EnergyPlus did not finish ({status}){}", if why.is_empty() { String::new() } else { format!(": {why}") })));
            }
            ctx.progress(0.9, "reading results");
            let rows = read_sql(&sql).map_err(|e| JobFailure::new("runner_failed", format!("eplusout.sql: {e}")))?;
            let r = reviera_core::energyplus::results(&rows, &input);
            ctx.log(format!("{} zones, {:.0} m2: heating {:.0} kWh, cooling {:.0} kWh, EUI {:.0} kWh/m2", r.spaces.len(), r.floor_area, r.year.heating_load, r.year.cooling_load, r.eui));
            Ok(r)
        })();
        if std::env::var_os("REVIERA_KEEP_ENERGYPLUS").is_none() {
            let _ = std::fs::remove_dir_all(&dir);
        } else {
            ctx.log(format!("working folder kept: {}", dir.display()));
        }
        let r = result?;
        ctx.progress(1.0, "done");
        Ok(Output { bytes: serde_json::to_vec(&r).map_err(|e| JobFailure::new("runner_failed", e.to_string()))?, media_type: JSON.into(), schema: ENERGY_RESULTS.into() })
    }
}

/// Writes an `eplusout.sql` with EnergyPlus's report tables and the given rows (for tests and
/// stand-ins).
#[doc(hidden)]
pub fn write_sql(path: &Path, rows: &[Row]) -> rusqlite::Result<()> {
    let db = rusqlite::Connection::open(path)?;
    db.execute_batch(
        "CREATE TABLE ReportDataDictionary(ReportDataDictionaryIndex INTEGER PRIMARY KEY, IsMeter INTEGER, Type TEXT, IndexGroup TEXT, TimestepType TEXT, KeyValue TEXT, Name TEXT, ReportingFrequency TEXT, ScheduleName TEXT, Units TEXT);
         CREATE TABLE ReportData (ReportDataIndex INTEGER PRIMARY KEY, TimeIndex INTEGER, ReportDataDictionaryIndex INTEGER, Value REAL);
         CREATE TABLE Time (TimeIndex INTEGER PRIMARY KEY, Year INTEGER, Month INTEGER, Day INTEGER, Hour INTEGER, Minute INTEGER, Dst INTEGER, Interval INTEGER, IntervalType INTEGER, SimulationDays INTEGER, DayType TEXT, EnvironmentPeriodIndex INTEGER, WarmupFlag INTEGER);",
    )?;
    let mut dict: Vec<(String, String, String)> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let k = (r.key.clone(), r.name.clone(), r.frequency.clone());
        let d = match dict.iter().position(|x| *x == k) {
            Some(p) => p,
            None => {
                dict.push(k);
                db.execute("INSERT INTO ReportDataDictionary VALUES (?1, 0, 'Sum', 'Zone', 'Zone', ?2, ?3, ?4, '', 'J')", rusqlite::params![dict.len() as i64, r.key, r.name, r.frequency])?;
                dict.len() - 1
            }
        };
        db.execute("INSERT OR IGNORE INTO Time VALUES (?1, 2026, ?2, 1, 1, 0, 0, 60, 1, 1, 'Monday', 1, 0)", rusqlite::params![r.time, r.month as i64])?;
        db.execute("INSERT INTO ReportData VALUES (?1, ?2, ?3, ?4)", rusqlite::params![i as i64 + 1, r.time, d as i64 + 1, r.value])?;
    }
    // A warm-up row that must be ignored.
    db.execute("INSERT INTO Time VALUES (999999, 2026, 1, 1, 1, 0, 0, 60, 1, 1, 'Monday', 1, 1)", [])?;
    if !dict.is_empty() {
        db.execute("INSERT INTO ReportData VALUES (999999, 999999, 1, 1e15)", [])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_round_trip_skips_warm_up() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("eplusout.sql");
        let rows = vec![
            Row { key: "S1 IDEAL LOADS".into(), name: reviera_core::energyplus::HEATING.into(), frequency: "Monthly".into(), month: 1, time: 10, value: 3.6e9 },
            Row { key: "S1 IDEAL LOADS".into(), name: reviera_core::energyplus::HEATING.into(), frequency: "Monthly".into(), month: 2, time: 20, value: 1.8e9 },
        ];
        write_sql(&p, &rows).unwrap();
        let back = read_sql(&p).unwrap();
        assert_eq!(back, rows);
    }

    #[test]
    fn err_file_counts() {
        let t = "Program Version,EnergyPlus\n   ** Warning ** a\n   **   ~~~   ** more\n   ** Severe  ** bad surface\n   **  Fatal  ** stop\n";
        let (w, s, lines) = errors(t);
        assert_eq!((w, s), (1, 2));
        assert_eq!(lines[0], "** Severe  ** bad surface");
    }
}
