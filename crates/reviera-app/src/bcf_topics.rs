// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! BCF topics in the window and in files (COL-BCF-001, COL-BCF-003): `bcf.create_topic` and
//! `bcf.add_viewpoint` capture the 3D view as a viewpoint ([`crate::bcf_files::capture`]);
//! `bcf.apply_viewpoint` shows a topic's viewpoint ([`crate::bcf_files::show`]); `bcf.export`
//! writes topics as a `.bcf` archive (BCF 3.0 or 2.1) and `bcf.import` reads one (either), merging
//! by topic GUID. The topics themselves are model data (`reviera_core::issues`).

use std::io::{Read, Write};

use reviera_core::bcf_archive::{self, Version};
use reviera_core::issues::{self, Topic, TopicFields, TopicRef, TopicViewpoint};
use reviera_core::{Caller, CommandMeta, Effect, ElementId, ElementRef, Error, HostCommand, Result, bcf};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::App;
use crate::bcf_files::{ImportReport, capture, show};

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

fn store(app: &App) -> Result<&reviera_core::Store> {
    let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
    Ok(app.get(doc)?.doc.store())
}

/// A snapshot of a 3D view as it shows now (base64 PNG), when there is a graphics adapter to
/// render it with.
fn snapshot_of(app: &App, view: ElementId) -> Option<String> {
    use base64::Engine;
    let img = app.render_view_image(app.active()?, Some(view), 1024).ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(img.png))
}

/// The 3D view to capture: the one named, else the active view when it is 3D.
fn view_to_capture(app: &App, view: &Option<ElementRef>) -> Result<Option<ElementId>> {
    let doc = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
    let store = app.get(doc)?.doc.store();
    let is_3d = |id: ElementId| store.get(id).and_then(reviera_core::views::view_data).is_some_and(|d| d.kind == reviera_core::views::KIND_3D);
    match view {
        Some(r) => {
            let (id, _) = reviera_core::views::resolve_view(store, r)?;
            if !is_3d(id) {
                return Err(bad("bcf.not_3d", "viewpoints are taken from 3D views"));
            }
            Ok(Some(id))
        }
        None => Ok(app.active_view(doc).filter(|v| is_3d(*v))),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub title: String,
    pub description: Option<String>,
    /// The other fields (type, status, priority, labels, assigned_to, due_date, stage, …).
    #[serde(default)]
    pub fields: TopicFields,
    /// The 3D view to capture as its viewpoint; the active view when it is 3D. Without one, a
    /// viewpoint of only the elements is kept.
    pub view_id: Option<ElementRef>,
    /// The elements it is about (the selection when omitted): its viewpoint's selection.
    pub element_ids: Option<Vec<ElementRef>>,
    pub author: Option<String>,
}

pub(crate) struct BcfCreateTopic;

impl HostCommand for BcfCreateTopic {
    type Host = App;
    type Params = CreateParams;
    type Output = ElementId;
    const META: CommandMeta = CommandMeta {
        name: "bcf.create_topic",
        version: 1,
        summary: "Create a BCF issue from the view",
        description: "Makes a BCF topic (COL-BCF-001) kept with the model: `title`, `description` and `fields` \
                      (topic_type, status, priority, labels, assigned_to, due_date, stage, related_topics, \
                      reference_links), with a viewpoint of the 3D view (`view_id`, or the active view when 3D: \
                      its camera, section box and slices, hidden or isolated elements) whose selection is \
                      `element_ids` (or the selection). Status Open and type Issue unless given; the author is \
                      `author` or the system's user name. Returns the topic's element id.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: CreateParams) -> Result<ElementId> {
        let view = view_to_capture(app, &p.view_id)?;
        let mut vp = match view {
            Some(v) => capture(app, v)?,
            None => bcf::Viewpoint { guid: Some(issues::new_guid()), ..bcf::Viewpoint::default() },
        };
        if let Some(ids) = &p.element_ids {
            let s = store(app)?;
            vp.selection = ids
                .iter()
                .map(|r| s.resolve(r).and_then(|id| s.get(id)).map(|e| e.guid().to_string()).ok_or_else(|| bad("element.not_found", format!("no element {r:?}"))))
                .collect::<Result<_>>()?;
        }
        let mut t = Topic {
            guid: issues::new_guid(),
            title: p.title,
            description: p.description.filter(|d| !d.trim().is_empty()),
            creation_date: issues::now(),
            creation_author: p.author.unwrap_or_else(issues::default_author),
            ..Topic::default()
        };
        p.fields.apply_to(&mut t);
        t.topic_type.get_or_insert_with(|| "Issue".to_owned());
        t.status.get_or_insert_with(|| "Open".to_owned());
        let keep = view.is_some() || !vp.selection.is_empty();
        let mut snapshots = issues::Snapshots::new();
        if keep {
            let guid = vp.guid.clone().unwrap_or_else(issues::new_guid);
            if let Some(png) = view.and_then(|v| snapshot_of(app, v)) {
                snapshots.insert(guid.clone(), png);
            }
            t.viewpoints.push(TopicViewpoint { guid, viewpoint: vp, snapshot: None });
        }
        let out = app.execute("bcf.put_topic", json!({ "topic": t, "snapshots": snapshots }), caller)?;
        serde_json::from_value(out.result).map_err(|e| bad("bcf.put", e.to_string()))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddViewpointParams {
    pub topic_id: TopicRef,
    /// The 3D view to capture; the active view when it is 3D.
    pub view_id: Option<ElementRef>,
    pub author: Option<String>,
}

pub(crate) struct BcfAddViewpoint;

impl HostCommand for BcfAddViewpoint {
    type Host = App;
    type Params = AddViewpointParams;
    type Output = String;
    const META: CommandMeta = CommandMeta {
        name: "bcf.add_viewpoint",
        version: 1,
        summary: "Add the view to a BCF issue",
        description: "Captures a 3D view (`view_id`, or the active view when 3D) as a viewpoint of a topic \
                      (COL-BCF-003): camera, section box and slices, hidden or isolated elements and the \
                      selection. Returns the viewpoint's GUID.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: AddViewpointParams) -> Result<String> {
        let view = view_to_capture(app, &p.view_id)?.ok_or_else(|| bad("bcf.not_3d", "the active view is not a 3D view: give view_id"))?;
        let vp = capture(app, view)?;
        let snapshot = snapshot_of(app, view);
        let topic = serde_json::to_value(&p.topic_id).unwrap_or_default();
        let out = app.execute("bcf.put_viewpoint", json!({ "topic_id": topic, "viewpoint": vp, "snapshot": snapshot, "author": p.author }), caller)?;
        serde_json::from_value(out.result).map_err(|e| bad("bcf.put", e.to_string()))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyParams {
    pub topic_id: TopicRef,
    /// The topic's viewpoint (its GUID); the first when omitted.
    pub viewpoint_id: Option<String>,
    /// The 3D view to show it in; the active one when 3D, else the default 3D view.
    pub target_view_id: Option<ElementRef>,
}

pub(crate) struct BcfApplyViewpoint;

impl HostCommand for BcfApplyViewpoint {
    type Host = App;
    type Params = ApplyParams;
    type Output = ImportReport;
    const META: CommandMeta = CommandMeta {
        name: "bcf.apply_viewpoint",
        version: 1,
        summary: "Show a BCF issue's viewpoint",
        description: "Shows one of a topic's viewpoints (`viewpoint_id`, else its first) in a 3D view \
                      (COL-BCF-003): its camera and cut as one undoable step, then its components hidden or \
                      isolated and selected. Returns what it applied and the GlobalIds not in the model.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: ApplyParams) -> Result<ImportReport> {
        let (_, t) = issues::find(store(app)?, &p.topic_id)?;
        let vp = match &p.viewpoint_id {
            Some(g) => t.viewpoints.iter().find(|v| v.guid.eq_ignore_ascii_case(g)),
            None => t.viewpoints.first(),
        }
        .ok_or_else(|| bad("bcf.no_viewpoint", "the topic has no such viewpoint"))?
        .viewpoint
        .clone();
        show(app, caller, &vp, &p.target_view_id)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    /// The `.bcf` (or `.bcfzip`) file to write.
    pub path: String,
    /// The topics (all when omitted).
    pub topic_ids: Option<Vec<TopicRef>>,
    /// `3.0` (the default) or `2.1`.
    pub version: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportReport {
    pub topics: usize,
    pub comments: usize,
    pub viewpoints: usize,
    pub snapshots: usize,
    pub version: String,
}

pub(crate) struct BcfExport;

impl HostCommand for BcfExport {
    type Host = App;
    type Params = ExportParams;
    type Output = ExportReport;
    const META: CommandMeta = CommandMeta {
        name: "bcf.export",
        version: 1,
        summary: "Export BCF issues",
        description: "Writes topics (all, or `topic_ids`) as a BCF archive at `path` (COL-BCF-001), BCF 3.0 or 2.1: \
                      bcf.version, extensions.xml (3.0: the project's types, statuses, priorities, labels, users \
                      and stages) and per topic a folder with markup.bcf (fields, comments, related topics, \
                      reference links) and its viewpoints as <guid>.bcfv.",
        effects: &[Effect::FileWrite],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: ExportParams) -> Result<ExportReport> {
        let version = match &p.version {
            Some(v) => Version::parse(v).ok_or_else(|| bad("bcf.bad_version", "version is 2.1 or 3.0"))?,
            None => Version::V3_0,
        };
        let s = store(app)?;
        let topics: Vec<Topic> = match &p.topic_ids {
            Some(ids) => ids.iter().map(|r| issues::find(s, r).map(|(_, t)| t)).collect::<Result<_>>()?,
            None => issues::all(s).into_iter().map(|(_, t)| t).collect(),
        };
        let lists = issues::lists(s);
        let file = std::fs::File::create(&p.path).map_err(|e| bad("bcf.write", format!("{}: {e}", p.path)))?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let io = |e: std::io::Error| bad("bcf.write", e.to_string());
        let zerr = |e: zip::result::ZipError| bad("bcf.write", e.to_string());
        zip.start_file("bcf.version", opts).map_err(zerr)?;
        zip.write_all(bcf_archive::version_xml(version).as_bytes()).map_err(io)?;
        if version == Version::V3_0 {
            zip.start_file("extensions.xml", opts).map_err(zerr)?;
            zip.write_all(bcf_archive::extensions_xml(&lists).as_bytes()).map_err(io)?;
        }
        let (mut comments, mut viewpoints, mut snapshots) = (0, 0, 0);
        for t in &topics {
            // Each snapshot kept, beside its viewpoint; a name without a picture is left out.
            let mut t = t.clone();
            let mut pictures = Vec::new();
            for v in &mut t.viewpoints {
                match (issues::snapshot(s, &v.guid), &v.snapshot) {
                    (Some(bytes), name) => {
                        let name = name.clone().unwrap_or_else(|| format!("{}.png", v.guid));
                        v.snapshot = Some(name.clone());
                        pictures.push((name, bytes));
                    }
                    (None, _) => v.snapshot = None,
                }
            }
            let t = &t;
            zip.start_file(format!("{}/markup.bcf", t.guid), opts).map_err(zerr)?;
            zip.write_all(bcf_archive::markup_xml(t, version).as_bytes()).map_err(io)?;
            for (name, bytes) in &pictures {
                zip.start_file(format!("{}/{name}", t.guid), zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).map_err(zerr)?;
                zip.write_all(bytes).map_err(io)?;
                snapshots += 1;
            }
            for v in &t.viewpoints {
                let mut vp = v.viewpoint.clone();
                vp.guid = Some(v.guid.clone());
                zip.start_file(format!("{}/{}.bcfv", t.guid, v.guid), opts).map_err(zerr)?;
                let xml = if version == Version::V2_1 { bcf::to_xml_2_1(&vp) } else { bcf::to_xml(&vp) };
                zip.write_all(xml.as_bytes()).map_err(io)?;
            }
            comments += t.comments.len();
            viewpoints += t.viewpoints.len();
        }
        zip.finish().map_err(zerr)?;
        Ok(ExportReport { topics: topics.len(), comments, viewpoints, snapshots, version: version.id().to_owned() })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportParams {
    /// A `.bcf` or `.bcfzip` archive (BCF 2.1 or 3.0).
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ImportResult {
    pub version: Option<String>,
    pub topics: usize,
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub viewpoints: usize,
    /// Snapshots read and kept with their viewpoints.
    pub snapshots: usize,
    /// Viewpoint files the markup names that are missing or unreadable.
    pub problems: Vec<String>,
}

pub(crate) struct BcfImport;

impl HostCommand for BcfImport {
    type Host = App;
    type Params = ImportParams;
    type Output = ImportResult;
    const META: CommandMeta = CommandMeta {
        name: "bcf.import",
        version: 1,
        summary: "Import BCF issues",
        description: "Reads a BCF archive (.bcf or .bcfzip, BCF 2.1 or 3.0) and adds its topics to the model, or \
                      merges them into those with their GUIDs (COL-BCF-001: the fields of whichever changed \
                      last; comments and viewpoints joined by GUID), as one undoable step. Viewpoints keep \
                      their camera, clipping planes, components and snapshots. Returns the counts and any \
                      viewpoint or snapshot files that could not be read.",
        effects: &[Effect::Edit],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, caller: &Caller, p: ImportParams) -> Result<ImportResult> {
        use base64::Engine;
        let file = std::fs::File::open(&p.path).map_err(|e| bad("bcf.read", format!("{}: {e}", p.path)))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| bad("bcf.read", format!("{}: not a BCF archive: {e}", p.path)))?;
        let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
        let mut bytes = |name: &str| -> Option<Vec<u8>> {
            let mut f = zip.by_name(name).ok()?;
            let mut b = Vec::new();
            f.read_to_end(&mut b).ok()?;
            Some(b)
        };
        let version = bytes("bcf.version").and_then(|v| {
            let v = String::from_utf8(v).ok()?;
            let at = v.find("VersionId=\"")? + 11;
            Some(v[at..].split('"').next()?.to_owned())
        });
        let mut topics = Vec::new();
        let mut pictures = issues::Snapshots::new();
        let (mut viewpoints, mut problems) = (0, Vec::new());
        for markup in names.iter().filter(|n| n.ends_with("/markup.bcf")) {
            let dir = &markup[..markup.len() - "markup.bcf".len()];
            let text = bytes(markup).and_then(|b| String::from_utf8(b).ok()).ok_or_else(|| bad("bcf.read", format!("{markup}: unreadable")))?;
            let (mut t, entries) = bcf_archive::read_markup(&text).map_err(|e| bad("bcf.parse", format!("{markup}: {}", e.message)))?;
            if t.guid.is_empty() {
                t.guid = dir.trim_end_matches('/').to_owned();
            }
            if t.creation_date.is_empty() {
                t.creation_date = issues::now();
            }
            if t.creation_author.is_empty() {
                t.creation_author = "unknown".into();
            }
            for e in entries {
                let file = e.file.clone().unwrap_or_else(|| format!("{}.bcfv", e.guid));
                match bytes(&format!("{dir}{file}")).and_then(|b| String::from_utf8(b).ok()).map(|x| bcf::from_xml(&x)) {
                    Some(Ok(mut vp)) => {
                        let guid = if e.guid.is_empty() { vp.guid.clone().unwrap_or_else(issues::new_guid) } else { e.guid.clone() };
                        vp.guid = Some(guid.clone());
                        if let Some(name) = &e.snapshot {
                            match bytes(&format!("{dir}{name}")) {
                                Some(png) => {
                                    pictures.insert(guid.clone(), base64::engine::general_purpose::STANDARD.encode(png));
                                }
                                None => problems.push(format!("{dir}{name}: missing")),
                            }
                        }
                        t.viewpoints.push(TopicViewpoint { guid, viewpoint: vp, snapshot: None });
                        viewpoints += 1;
                    }
                    Some(Err(err)) => problems.push(format!("{dir}{file}: {}", err.message)),
                    None => problems.push(format!("{dir}{file}: missing")),
                }
            }
            topics.push(t);
        }
        let snapshots = pictures.len();
        let out = app.execute("bcf.merge_topics", json!({ "topics": topics, "snapshots": pictures }), caller)?;
        let r = out.result;
        let n = |k: &str| r[k].as_u64().unwrap_or(0) as usize;
        Ok(ImportResult { version, topics: topics.len(), added: n("added"), updated: n("updated"), unchanged: n("unchanged"), viewpoints, snapshots, problems })
    }
}
