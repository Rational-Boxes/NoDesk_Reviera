// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Accepted overlaps (ARC-MOD-032): pairs the user has marked intended. They are held on the
//! project (`rxd:accepted_overlaps`) by GlobalId, with who accepted them and when, and the
//! overlap checks leave them out.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;

pub const KEY: &str = "rxd:accepted_overlaps";

/// An accepted pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Accepted {
    /// GlobalIds, in order.
    pub a: String,
    pub b: String,
    pub by: String,
    /// When (UTC, RFC 3339).
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Now, UTC, as RFC 3339 (`2026-10-13T17:05:00Z`).
fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn project(store: &Store) -> Option<&crate::element::Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

/// The accepted pairs.
pub fn accepted(store: &Store) -> Vec<Accepted> {
    project(store)
        .and_then(|p| p.param(KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Whether the overlap between two elements has been accepted.
pub fn is_accepted(store: &Store, a: ElementId, b: ElementId) -> bool {
    let (Some(ga), Some(gb)) = (store.get(a), store.get(b)) else {
        return false;
    };
    let (ga, gb) = (ga.guid().to_string(), gb.guid().to_string());
    accepted(store)
        .iter()
        .any(|p| (p.a == ga && p.b == gb) || (p.a == gb && p.b == ga))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptParams {
    pub a: ElementRef,
    pub b: ElementRef,
    /// Why it is intended.
    pub note: Option<String>,
    /// `false` takes the acceptance back.
    #[serde(default = "yes")]
    pub accepted: bool,
}

fn yes() -> bool {
    true
}

/// Accepts (or takes back) an overlap (`overlap.accept`).
pub(crate) fn accept(txn: &mut crate::txn::Transaction<'_>, p: AcceptParams) -> Result<()> {
    let store = txn.store().clone();
    let guid = |r: &ElementRef| {
        store
            .resolve(r)
            .and_then(|id| store.get(id))
            .map(|e| e.guid().to_string())
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))
    };
    let (a, b) = (guid(&p.a)?, guid(&p.b)?);
    let project = project(&store)
        .map(crate::element::Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    let mut list = accepted(&store);
    list.retain(|x| !((x.a == a && x.b == b) || (x.a == b && x.b == a)));
    if p.accepted {
        list.push(Accepted {
            a,
            b,
            by: txn.origin().label(),
            at: now(),
            note: p.note,
        });
    }
    let value = serde_json::to_value(&list).expect("serialises");
    txn.modify(project, |e| {
        e.params.insert(KEY.into(), value);
    })
}
