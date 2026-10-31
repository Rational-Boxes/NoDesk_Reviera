// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Issues on a FileEngine model (COL-FE-009): the discussion threads people raise on the model,
//! in FileEngine's 3D viewer or here, each anchored to a viewpoint (camera, cut, the components
//! hidden or selected, a snapshot, measurements). They stay on the server, where the people
//! discussing them are: Reviera reads them when the model opens, keeps the last answer beside the
//! working copy (`issues.json`) to read offline, and shows them in the Issues pane. A viewpoint is
//! shown in a 3D view as a BCF one is ([`crate::bcf_topics`]); replies, resolving and new issues
//! go straight to the server. The model itself is not changed, so reading issues never makes it
//! unsaved and they are never uploaded inside it.

use reviera_core::bcf::{self, Viewpoint};
use reviera_core::{Caller, CommandMeta, Effect, ElementRef, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::fileengine::Link;
use crate::{App, DocumentId};

/// The cache beside the working copy.
const CACHE: &str = "issues.json";

/// FileEngine's model anchors as its 3D viewer writes them (xeokit's BCF viewpoints with their
/// default settings): a clipping plane's direction points to the side kept, where BCF's points to
/// the side clipped away. Reviera turns them round reading and writing, so a cut looks the same in
/// both. A later anchor schema that follows BCF is read as it is.
const VIEWER_ANCHOR: &str = "fileengine.anchor.v1";

fn turned(mut v: Viewpoint) -> Viewpoint {
    for p in &mut v.clipping_planes {
        p.direction = p.direction.map(|d| -d);
    }
    v
}

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

macro_rules! host {
    ($ty:ident, $name:literal, $summary:literal, $desc:literal, [$($eff:ident),*], $params:ty, $out:ty, |$app:ident, $caller:ident, $p:ident| $body:block) => {
        pub(crate) struct $ty;
        impl HostCommand for $ty {
            type Host = App;
            type Params = $params;
            type Output = $out;
            const META: CommandMeta = CommandMeta { name: $name, version: 1, summary: $summary, description: $desc, effects: &[$(Effect::$eff),*], transaction_name: "" };
            #[allow(unused_variables)]
            fn run(&self, $app: &mut App, $caller: &Caller, $p: $params) -> Result<$out> $body
        }
    };
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct IssueComment {
    pub id: String,
    pub author: String,
    /// ISO 8601.
    pub date: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Issue {
    pub thread_id: String,
    /// The thread's title, else the first line of its opening comment.
    pub title: String,
    /// `open` or `resolved`.
    pub status: String,
    pub opened_by: String,
    pub created: String,
    pub updated: String,
    /// The model version it was raised on (empty: not recorded).
    pub version: String,
    pub resolved_by: Option<String>,
    pub comments: Vec<IssueComment>,
    /// Its viewpoint, when it has one (an issue in the model, not a comment on the file).
    pub viewpoint: Option<Viewpoint>,
    /// The components it is about (IFC GlobalIds).
    pub objects: Vec<String>,
    /// Measurements taken with it, as text (`1.010 m`).
    pub measurements: Vec<String>,
    pub has_snapshot: bool,
    /// FileEngine marks a viewpoint stale when the model changed under it.
    pub stale: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IssuesOut {
    pub connection_id: String,
    pub uid: String,
    pub model: String,
    /// The model version open here.
    pub version: String,
    pub issues: Vec<Issue>,
    pub open: usize,
    /// Read from the copy kept beside the working copy, the server being out of reach.
    pub offline: bool,
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

/// A title from a comment: its first line, without the @mentions it starts with.
fn first_line(s: &str, max: usize) -> String {
    let line = s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let mut line = line.trim_start();
    while line.starts_with('@') {
        let rest = line.find(char::is_whitespace).map_or("", |k| &line[k..]).trim_start();
        if rest.is_empty() {
            break;
        }
        line = rest.trim_start_matches([',', ':']).trim_start();
    }
    if line.chars().count() > max { format!("{}…", line.chars().take(max - 1).collect::<String>()) } else { line.to_owned() }
}

/// One thread as an issue.
pub(crate) fn issue_of(t: &Value) -> Issue {
    let comments: Vec<IssueComment> = t["comments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["deleted"] != true)
        .map(|c| IssueComment { id: text(&c["id"]), author: text(&c["author"]), date: text(&c["created_at"]), text: if c["redacted"] == true { "(redacted)".into() } else { text(&c["body"]) } })
        .collect();
    let anchor = &t["anchor"];
    let model = anchor["kind"] == "model-viewpoint";
    let viewpoint = model.then(|| {
        let v = bcf::from_json(&anchor["viewpoint"]);
        if anchor["schema"] == VIEWER_ANCHOR { turned(v) } else { v }
    });
    let mut objects: Vec<String> = anchor["object_refs"].as_array().into_iter().flatten().filter_map(|o| o["id"].as_str()).map(str::to_owned).collect();
    if let Some(v) = &viewpoint {
        for g in &v.selection {
            if !objects.contains(g) {
                objects.push(g.clone());
            }
        }
    }
    let measurements = anchor["measurements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let v = m["value"].as_f64()?;
            Some(match m["type"].as_str() {
                Some("angle") => format!("{v:.1}°"),
                Some("area") => format!("{v:.3} m²"),
                _ => format!("{v:.3} m"),
            })
        })
        .collect();
    let title = match text(&t["title"]) {
        s if !s.trim().is_empty() => s,
        _ => comments.first().map(|c| first_line(&c.text, 100)).filter(|s| !s.is_empty()).unwrap_or_else(|| "(untitled)".into()),
    };
    Issue {
        thread_id: text(&t["id"]),
        title,
        status: text(&t["status"]),
        opened_by: text(&t["opened_by"]),
        created: text(&t["created_at"]),
        updated: text(&t["updated_at"]),
        version: text(&t["version"]),
        resolved_by: t["resolved_by"].as_str().map(str::to_owned),
        comments,
        viewpoint,
        objects,
        measurements,
        has_snapshot: anchor["viewpoint"]["snapshot"]["snapshot_data"].as_str().is_some_and(|s| !s.is_empty()),
        stale: t["anchor_stale"] == true,
    }
}

/// The open document's FileEngine link (or `document_id`'s).
fn linked(app: &App, document_id: Option<DocumentId>) -> Result<(DocumentId, Link, std::path::PathBuf)> {
    let id = match document_id {
        Some(id) => id,
        None => app.active().ok_or_else(|| bad("document.none", "no document is open"))?,
    };
    let path = app.get(id)?.project.path().to_path_buf();
    let link = crate::fileengine::link_of(&path).ok_or_else(|| bad("fileengine.not_linked", "this model is not on FileEngine (File › Open From FileEngine…)"))?;
    Ok((id, link, path))
}

fn cache_path(path: &std::path::Path) -> std::path::PathBuf {
    path.with_file_name(CACHE)
}

/// The model's threads, from the server (kept beside the working copy), else from that copy.
fn threads(app: &mut App, link: &Link, path: &std::path::Path, refresh: bool) -> Result<(Vec<Value>, bool)> {
    let cached = || -> Option<Vec<Value>> { serde_json::from_slice::<Vec<Value>>(&std::fs::read(cache_path(path)).ok()?).ok() };
    if !refresh && let Some(t) = cached() {
        return Ok((t, false));
    }
    match app.fileengine.with(&link.connection_id, |c| c.threads(&link.uid)) {
        Ok(t) => {
            let _ = std::fs::write(cache_path(path), serde_json::to_vec(&t).expect("serialises"));
            Ok((t, false))
        }
        Err(e) if e.code == "fileengine.unreachable" => cached().map(|t| (t, true)).ok_or(e),
        Err(e) => Err(e),
    }
}

pub(crate) fn list(app: &mut App, document_id: Option<DocumentId>, refresh: bool) -> Result<IssuesOut> {
    let (_, link, path) = linked(app, document_id)?;
    let (threads, offline) = threads(app, &link, &path, refresh)?;
    let mut issues: Vec<Issue> = threads.iter().map(issue_of).collect();
    // Open first, newest first.
    issues.sort_by(|a, b| (a.status != "open").cmp(&(b.status != "open")).then_with(|| b.updated.cmp(&a.updated)));
    let open = issues.iter().filter(|i| i.status == "open").count();
    Ok(IssuesOut { connection_id: link.connection_id, uid: link.uid, model: link.name, version: link.version, issues, open, offline })
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssuesParams {
    pub document_id: Option<DocumentId>,
    /// Ask the server again (default true); false reads the copy kept beside the working copy.
    #[serde(default = "yes")]
    pub refresh: bool,
}

fn yes() -> bool {
    true
}

host!(FeIssues, "fileengine.issues", "A FileEngine model's issues",
    "The issues people raised on the open FileEngine model (COL-FE-009): its discussion threads, open first, \
     each with its title, status, who opened and resolved it, the version it was raised on, its comments, its \
     viewpoint (camera, clipping planes, selection and visibility by IFC GlobalId) and the components and \
     measurements it is about. Kept beside the working copy, so they can be read offline (`offline` says so). \
     The model is not changed. fileengine.open reads them too.",
    [Network, Read], IssuesParams, IssuesOut, |app, caller, p| {
    list(app, p.document_id, p.refresh)
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueParams {
    pub document_id: Option<DocumentId>,
    pub thread_id: String,
}

fn thread(app: &mut App, document_id: Option<DocumentId>, id: &str) -> Result<(Link, Value)> {
    let (_, link, path) = linked(app, document_id)?;
    let (threads, _) = threads(app, &link, &path, false)?;
    let t = threads.into_iter().find(|t| t["id"].as_str() == Some(id)).ok_or_else(|| bad("fileengine.no_issue", format!("no issue {id:?} on this model; fileengine.issues lists them")))?;
    Ok((link, t))
}

host!(FeIssueSnapshot, "fileengine.issue_snapshot", "An issue's snapshot",
    "The picture taken with an issue's viewpoint (COL-FE-009), as base64 PNG or JPEG, or null.",
    [Read], IssueParams, Option<String>, |app, caller, p| {
    let (_, t) = thread(app, p.document_id, &p.thread_id)?;
    let data = t["anchor"]["viewpoint"]["snapshot"]["snapshot_data"].as_str().unwrap_or_default();
    Ok(data.split_once("base64,").map(|(_, b)| b.to_owned()).or_else(|| (!data.is_empty() && !data.starts_with("data:")).then(|| data.to_owned())))
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowParams {
    pub document_id: Option<DocumentId>,
    pub thread_id: String,
    /// The 3D view to show it in; the active one when 3D, else the default 3D view.
    pub target_view_id: Option<ElementRef>,
}

host!(FeShowIssue, "fileengine.show_issue", "Show a FileEngine issue",
    "Shows an issue's viewpoint in a 3D view (COL-FE-009), as bcf.apply_viewpoint does: its camera and cut \
     as one undoable step, then its components hidden or isolated, and the components it is about selected. \
     Returns what it applied and the GlobalIds not in the model.",
    [Edit], ShowParams, crate::bcf_files::ImportReport, |app, caller, p| {
    let (_, t) = thread(app, p.document_id, &p.thread_id)?;
    let issue = issue_of(&t);
    let mut vp = issue.viewpoint.ok_or_else(|| bad("fileengine.no_viewpoint", "this is a comment on the file, with no viewpoint"))?;
    if vp.selection.is_empty() {
        vp.selection = issue.objects;
    }
    crate::bcf_files::show(app, caller, &vp, &p.target_view_id)
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommentParams {
    pub document_id: Option<DocumentId>,
    pub thread_id: String,
    pub text: String,
}

host!(FeCommentIssue, "fileengine.comment_issue", "Reply to a FileEngine issue",
    "Adds a comment to an issue on the server (COL-FE-009), as the signed-in user; `@name@example.com` \
     mentions notify people there. Returns the issue as it now is.",
    [Network], CommentParams, Issue, |app, caller, p| {
    let text = p.text.trim().to_owned();
    if text.is_empty() {
        return Err(bad("fileengine.invalid_input", "the comment is empty"));
    }
    let (_, link, _) = linked(app, p.document_id)?;
    app.fileengine.with(&link.connection_id, |c| c.add_comment(&p.thread_id, &text))?;
    refreshed(app, p.document_id, &p.thread_id)
});

fn refreshed(app: &mut App, document_id: Option<DocumentId>, id: &str) -> Result<Issue> {
    list(app, document_id, true)?.issues.into_iter().find(|i| i.thread_id == id).ok_or_else(|| bad("fileengine.no_issue", format!("issue {id:?} is gone")))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveParams {
    pub document_id: Option<DocumentId>,
    pub thread_id: String,
    /// true resolves it (on the version open here), false opens it again.
    pub resolved: bool,
}

host!(FeResolveIssue, "fileengine.resolve_issue", "Resolve or reopen a FileEngine issue",
    "Resolves an issue on the server, recording the model version open here, or opens it again (COL-FE-009). \
     FileEngine lets the person who raised it, or anyone who may write the model, do so. Returns the issue.",
    [Network], ResolveParams, Issue, |app, caller, p| {
    let (_, link, _) = linked(app, p.document_id)?;
    let status = if p.resolved { "resolved" } else { "open" };
    app.fileengine.with(&link.connection_id, |c| c.set_thread_status(&p.thread_id, status, p.resolved.then_some(link.version.as_str())))?;
    refreshed(app, p.document_id, &p.thread_id)
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewIssueParams {
    pub document_id: Option<DocumentId>,
    /// The opening comment; its first line is the title when `title` is not given.
    pub text: String,
    pub title: Option<String>,
    /// The 3D view whose camera, cut, hidden elements and selection it keeps (the active one when 3D).
    pub view_id: Option<ElementRef>,
}

host!(FeNewIssue, "fileengine.new_issue", "Raise an issue on a FileEngine model",
    "Raises an issue on the open FileEngine model (COL-FE-009): a thread on the server with the opening \
     comment, anchored to a 3D view's viewpoint (camera, section box and slices, hidden or isolated elements, \
     the selection) and its snapshot, on the version open here. People see it in FileEngine's 3D viewer. \
     Returns the issue.",
    [Network, Read], NewIssueParams, Issue, |app, caller, p| {
    let text = p.text.trim().to_owned();
    if text.is_empty() {
        return Err(bad("fileengine.invalid_input", "say what the issue is"));
    }
    let (id, link, _) = linked(app, p.document_id)?;
    if app.active() != Some(id) {
        app.activate(id)?;
    }
    let (view, vp) = crate::bcf_topics::capture_view(app, &p.view_id)?;
    let snapshot = crate::bcf_topics::snapshot_png(app, view);
    let mut viewpoint = bcf::to_json(&turned(vp.clone()));
    if let Some(png) = snapshot {
        viewpoint["snapshot"] = json!({ "snapshot_type": "png", "snapshot_data": format!("data:image/png;base64,{png}") });
    }
    let anchor = json!({
        "kind": "model-viewpoint",
        "schema": VIEWER_ANCHOR,
        "viewpoint": viewpoint,
        "object_refs": vp.selection.iter().map(|g| json!({ "id": g })).collect::<Vec<_>>(),
        "model_source": { "kind": "model" },
    });
    let title = p.title.filter(|t| !t.trim().is_empty()).unwrap_or_default();
    let body = json!({ "title": title, "body": text, "version": link.version, "anchor": anchor });
    let t = app.fileengine.with(&link.connection_id, |c| c.open_thread(&link.uid, &body))?;
    let tid = t["id"].as_str().unwrap_or_default().to_owned();
    refreshed(app, Some(id), &tid)
});
