// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Clash results (COL-CLASH-001, COL-CLASH-002): the last clash test's definition and its results,
//! kept on the project (`rxd:clash_test`, `rxd:clash_results`) so they survive saving and
//! reopening. Results are matched between runs by their elements' GlobalIds, and each has a
//! status: *new* when first found, *active* when found again, *reviewed* or *approved* when the
//! user says so, and *resolved* when a run no longer finds it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store::Store;

pub const RESULTS_KEY: &str = "rxd:clash_results";
pub const TEST_KEY: &str = "rxd:clash_test";

/// A result's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    New,
    Active,
    Reviewed,
    Approved,
    Resolved,
}

impl Status {
    /// Still to be dealt with: new or active.
    pub fn open(self) -> bool {
        matches!(self, Status::New | Status::Active)
    }

    pub fn name(self) -> &'static str {
        match self {
            Status::New => "new",
            Status::Active => "active",
            Status::Reviewed => "reviewed",
            Status::Approved => "approved",
            Status::Resolved => "resolved",
        }
    }
}

/// A comment on a result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Comment {
    pub by: String,
    /// UTC, RFC 3339.
    pub at: String,
    pub text: String,
}

/// One clash, matched between runs by its elements' GlobalIds (`a` before `b`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Record {
    /// Stable for the pair: `c` and a short hash of the two GlobalIds.
    pub id: String,
    pub a: String,
    pub b: String,
    pub status: Status,
    /// Cubic metres, as last measured.
    pub volume: f64,
    pub depth: f64,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub centroid: [f64; 3],
    /// The run that first found it, and the last that did.
    pub first_run: u32,
    pub last_run: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<Comment>,
}

/// The clash test last run: what it tested, and its runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Test {
    /// IFC classes of set A; empty: every element.
    #[serde(default)]
    pub set_a: Vec<String>,
    /// IFC classes of set B; empty: every element (the same as set A when both are empty).
    #[serde(default)]
    pub set_b: Vec<String>,
    /// Touching within this (metres) is not a clash.
    pub tolerance: f64,
    /// Runs so far.
    pub runs: u32,
    /// When the last run finished (UTC, RFC 3339).
    #[serde(default)]
    pub last_run_at: String,
    /// Bodies the last run checked.
    #[serde(default)]
    pub bodies: u32,
    /// The compute job of the last run.
    #[serde(default)]
    pub job_id: String,
}

fn project(store: &Store) -> Option<&crate::element::Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

/// The stored results.
pub fn results(store: &Store) -> Vec<Record> {
    project(store)
        .and_then(|p| p.param(RESULTS_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// The stored test definition.
pub fn test(store: &Store) -> Option<Test> {
    project(store)
        .and_then(|p| p.param(TEST_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// The ID of a pair, the same whichever way round it is given.
pub fn pair_id(a: &str, b: &str) -> String {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    // FNV-1a, 64-bit: stable across platforms and versions.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in a.bytes().chain([0u8]).chain(b.bytes()) {
        h ^= u64::from(byte);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("c{h:016x}")
}

/// A clash found by a run.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Found {
    /// GlobalIds.
    pub a: String,
    pub b: String,
    pub volume: f64,
    pub depth: f64,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub centroid: [f64; 3],
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordRunParams {
    /// The test that ran.
    pub set_a: Vec<String>,
    pub set_b: Vec<String>,
    pub tolerance: f64,
    pub bodies: u32,
    pub job_id: String,
    pub found: Vec<Found>,
}

/// What a run changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunSummary {
    pub run: u32,
    /// Found for the first time.
    pub new: usize,
    /// Found again (including ones reopened after being resolved).
    pub found_again: usize,
    /// No longer found, so resolved.
    pub resolved: usize,
    /// Results still open (new or active).
    pub open: usize,
    pub total: usize,
}

/// Records a run (`clash.record_run`): merges its clashes with the stored results.
pub(crate) fn record_run(
    txn: &mut crate::txn::Transaction<'_>,
    p: RecordRunParams,
) -> Result<RunSummary> {
    let store = txn.store().clone();
    let project = project(&store)
        .map(crate::element::Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let mut test = test(&store).unwrap_or_default();
    let run = test.runs + 1;
    test.runs = run;
    test.set_a = p.set_a;
    test.set_b = p.set_b;
    test.tolerance = p.tolerance;
    test.bodies = p.bodies;
    test.job_id = p.job_id;
    test.last_run_at = crate::overlap_accept::now_rfc3339();
    let mut list = results(&store);
    let mut summary = RunSummary {
        run,
        ..Default::default()
    };
    let mut seen = std::collections::HashSet::new();
    for f in p.found {
        let (a, b) = if f.a <= f.b { (f.a, f.b) } else { (f.b, f.a) };
        let id = pair_id(&a, &b);
        seen.insert(id.clone());
        match list.iter_mut().find(|r| r.id == id) {
            Some(r) => {
                r.status = match r.status {
                    Status::New | Status::Resolved => Status::Active,
                    s => s,
                };
                r.volume = f.volume;
                r.depth = f.depth;
                r.min = f.min;
                r.max = f.max;
                r.centroid = f.centroid;
                r.last_run = run;
                summary.found_again += 1;
            }
            None => {
                list.push(Record {
                    id,
                    a,
                    b,
                    status: Status::New,
                    volume: f.volume,
                    depth: f.depth,
                    min: f.min,
                    max: f.max,
                    centroid: f.centroid,
                    first_run: run,
                    last_run: run,
                    comments: Vec::new(),
                });
                summary.new += 1;
            }
        }
    }
    for r in &mut list {
        if !seen.contains(&r.id) && r.status != Status::Resolved {
            r.status = Status::Resolved;
            summary.resolved += 1;
        }
    }
    // Largest first, resolved last.
    list.sort_by(|x, y| {
        (x.status == Status::Resolved)
            .cmp(&(y.status == Status::Resolved))
            .then(y.volume.total_cmp(&x.volume))
    });
    summary.open = list.iter().filter(|r| r.status.open()).count();
    summary.total = list.len();
    let results = serde_json::to_value(&list).expect("serialises");
    let test = serde_json::to_value(&test).expect("serialises");
    txn.modify(project, |e| {
        e.params.insert(RESULTS_KEY.into(), results);
        e.params.insert(TEST_KEY.into(), test);
    })?;
    Ok(summary)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetStatusParams {
    /// Result IDs (from `clash.last_report`).
    pub result_ids: Vec<String>,
    pub status: Option<Status>,
    /// A comment added to each.
    pub comment: Option<String>,
}

/// Sets results' status and adds a comment (`clash.set_status`).
pub(crate) fn set_status(txn: &mut crate::txn::Transaction<'_>, p: SetStatusParams) -> Result<()> {
    let store = txn.store().clone();
    let project = project(&store)
        .map(crate::element::Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let mut list = results(&store);
    for id in &p.result_ids {
        let r = list
            .iter_mut()
            .find(|r| &r.id == id)
            .ok_or_else(|| Error::new("clash.not_found", format!("no clash result {id}")))?;
        if let Some(s) = p.status {
            r.status = s;
        }
        if let Some(t) = p.comment.as_ref().filter(|t| !t.trim().is_empty()) {
            r.comments.push(Comment {
                by: txn.origin().label(),
                at: crate::overlap_accept::now_rfc3339(),
                text: t.trim().to_owned(),
            });
        }
    }
    let value = serde_json::to_value(&list).expect("serialises");
    txn.modify(project, |e| {
        e.params.insert(RESULTS_KEY.into(), value);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_ids_ignore_order() {
        assert_eq!(pair_id("A", "B"), pair_id("B", "A"));
        assert_ne!(pair_id("A", "B"), pair_id("A", "C"));
        assert_eq!(pair_id("A", "B").len(), 17);
    }
}
