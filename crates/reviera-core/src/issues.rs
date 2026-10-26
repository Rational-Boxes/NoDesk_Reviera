// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Issues (COL-BCF-001): BCF topics kept with the model, one RXD object per topic
//! (`rxd:coordination/issues/<topic guid>`), so they are saved, undone and merged like the rest of
//! the model. A topic has BCF's fields, its comments and its viewpoints ([`crate::bcf::Viewpoint`]).
//!
//! The commands here edit topics as data; capturing a viewpoint from a view, showing one, and
//! reading and writing `.bcf` archives need the window's state and files, and are the
//! application's (`bcf.create_topic`, `bcf.add_viewpoint`, `bcf.apply_viewpoint`, `bcf.import`,
//! `bcf.export`). Topics from a file are merged by GUID ([`merge`]): the newer modification wins,
//! comments and viewpoints by their GUIDs.
//!
//! A viewpoint's snapshot (a PNG) is kept beside its topic as its own RXD object
//! (`rxd:coordination/snapshots/<viewpoint guid>`), written in the same step as the topic or
//! viewpoint; the viewpoint keeps the snapshot's file name.
//!
//! Not built yet: the project's lists as edited extensions (the defaults and the values in use are
//! exported), document references, the docked Issues pane, BCF API sync (COL-BCF-002).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bcf::Viewpoint;
use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::ElementId;
use crate::store::Store;
use crate::txn::Transaction;

pub const CLASS_PREFIX: &str = "rxd:coordination/issues/";
pub const TOPIC_KEY: &str = "rxd:topic";
/// A viewpoint's snapshot: a PNG kept with the model, one RXD object per viewpoint GUID.
pub const SNAPSHOT_PREFIX: &str = "rxd:coordination/snapshots/";
pub const SNAPSHOT_KEY: &str = "rxd:snapshot";
/// The largest snapshot kept (bytes of PNG).
pub const MAX_SNAPSHOT: usize = 4 << 20;

/// The lists offered for new topics (BCF extensions); values in use are added when exported.
pub const TYPES: &[&str] = &["Issue", "Clash", "Request", "Question", "Remark"];
pub const STATUSES: &[&str] = &["Open", "In Progress", "Resolved", "Closed"];
pub const PRIORITIES: &[&str] = &["High", "Normal", "Low"];

fn bad(code: &'static str, msg: impl Into<String>) -> Error {
    Error::new(code, msg)
}

/// A comment on a topic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Comment {
    pub guid: String,
    pub date: String,
    pub author: String,
    pub text: String,
    /// The topic viewpoint it refers to (its GUID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_author: Option<String>,
}

/// A viewpoint of a topic, with the name its snapshot had in the file it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TopicViewpoint {
    pub guid: String,
    pub viewpoint: Viewpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}

/// A BCF topic.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Topic {
    pub guid: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_to: Option<String>,
    /// An ISO 8601 date or date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    pub creation_date: String,
    pub creation_author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_author: Option<String>,
    /// Other topics' GUIDs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related_topics: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reference_links: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<Comment>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub viewpoints: Vec<TopicViewpoint>,
}

impl Topic {
    /// When it last changed: its modification, else its creation.
    pub fn changed(&self) -> &str {
        self.modified_date.as_deref().unwrap_or(&self.creation_date)
    }
}

/// The fields `bcf.create_topic` and `bcf.update_topic` set; a field left out is kept (an empty
/// text clears it).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TopicFields {
    pub title: Option<String>,
    pub description: Option<String>,
    pub topic_type: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub labels: Option<Vec<String>>,
    pub assigned_to: Option<String>,
    pub due_date: Option<String>,
    pub stage: Option<String>,
    pub related_topics: Option<Vec<String>>,
    pub reference_links: Option<Vec<String>>,
}

impl TopicFields {
    /// Sets these fields on `t`.
    pub fn apply_to(self, t: &mut Topic) {
        let text = |v: String| (!v.trim().is_empty()).then_some(v);
        if let Some(v) = self.title {
            t.title = v;
        }
        for (field, value) in [
            (&mut t.description, self.description),
            (&mut t.topic_type, self.topic_type),
            (&mut t.status, self.status),
            (&mut t.priority, self.priority),
            (&mut t.assigned_to, self.assigned_to),
            (&mut t.due_date, self.due_date),
            (&mut t.stage, self.stage),
        ] {
            if let Some(v) = value {
                *field = text(v);
            }
        }
        if let Some(v) = self.labels {
            t.labels = v;
        }
        if let Some(v) = self.related_topics {
            t.related_topics = v;
        }
        if let Some(v) = self.reference_links {
            t.reference_links = v;
        }
    }
}

/// Now, as BCF writes dates: ISO 8601 in UTC to the second.
pub fn now() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// The author when none is given: the system's user name.
pub fn default_author() -> String {
    ["USER", "USERNAME", "LOGNAME"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).unwrap_or_else(|| "Reviera user".to_owned())
}

pub fn new_guid() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn is_topic(e: &Element) -> bool {
    e.class.starts_with(CLASS_PREFIX)
}

pub fn topic(e: &Element) -> Option<Topic> {
    if !is_topic(e) {
        return None;
    }
    serde_json::from_value(e.param(TOPIC_KEY)?.clone()).ok()
}

/// Every topic, by creation date.
pub fn all(store: &Store) -> Vec<(ElementId, Topic)> {
    let mut v: Vec<(ElementId, Topic)> = store.iter().filter_map(|e| Some((e.id(), topic(e)?))).collect();
    v.sort_by(|a, b| (&a.1.creation_date, &a.1.guid).cmp(&(&b.1.creation_date, &b.1.guid)));
    v
}

/// A topic by its GUID or element id.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TopicRef {
    Id(u64),
    Guid(String),
}

pub fn find(store: &Store, r: &TopicRef) -> Result<(ElementId, Topic)> {
    let found = match r {
        TopicRef::Id(id) => store.get(ElementId(*id)).and_then(|e| Some((e.id(), topic(e)?))),
        TopicRef::Guid(g) => {
            let g = g.to_lowercase();
            all(store).into_iter().find(|(_, t)| t.guid.to_lowercase() == g)
        }
    };
    found.ok_or_else(|| bad("bcf.no_topic", format!("no topic {r:?}")))
}

fn write(txn: &mut Transaction<'_>, id: Option<ElementId>, t: &Topic) -> Result<ElementId> {
    let value = serde_json::to_value(t).expect("a topic serialises");
    match id {
        Some(id) => {
            let name = t.title.clone();
            txn.modify(id, |e| {
                e.name = Some(name);
                e.params.insert(TOPIC_KEY.into(), value);
            })?;
            Ok(id)
        }
        None => txn.create(NewElement::new(format!("{CLASS_PREFIX}{}", t.guid)).name(t.title.clone()).param(TOPIC_KEY, value)),
    }
}

/// Snapshots by viewpoint GUID, as base64 PNG.
pub type Snapshots = std::collections::BTreeMap<String, String>;

fn snapshot_element(store: &Store, viewpoint: &str) -> Option<ElementId> {
    let class = format!("{SNAPSHOT_PREFIX}{}", viewpoint.to_lowercase());
    store.iter().find(|e| e.class == class).map(Element::id)
}

/// A viewpoint's snapshot (PNG bytes), if it has one.
pub fn snapshot(store: &Store, viewpoint: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let e = store.get(snapshot_element(store, viewpoint)?)?;
    base64::engine::general_purpose::STANDARD.decode(e.param(SNAPSHOT_KEY)?.as_str()?).ok()
}

/// Keeps a snapshot for a viewpoint of `t`, naming it on the viewpoint.
fn put_snapshot(txn: &mut Transaction<'_>, t: &mut Topic, viewpoint: &str, png_base64: &str) -> Result<()> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(png_base64.trim()).map_err(|e| bad("bcf.bad_snapshot", format!("not base64: {e}")))?;
    if !bytes.starts_with(b"\x89PNG") && !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(bad("bcf.bad_snapshot", "a snapshot is a PNG or JPEG"));
    }
    if bytes.len() > MAX_SNAPSHOT {
        return Err(bad("bcf.bad_snapshot", format!("a snapshot of {} bytes is larger than {MAX_SNAPSHOT}", bytes.len())));
    }
    let Some(v) = t.viewpoints.iter_mut().find(|v| v.guid.eq_ignore_ascii_case(viewpoint)) else {
        return Err(bad("bcf.no_viewpoint", format!("the topic has no viewpoint {viewpoint}")));
    };
    let ext = if bytes.starts_with(&[0xff, 0xd8]) { "jpg" } else { "png" };
    v.snapshot = Some(format!("{}.{ext}", v.guid));
    let value = Value::String(png_base64.trim().to_owned());
    let store = txn.store().clone();
    match snapshot_element(&store, viewpoint) {
        Some(id) => txn.modify(id, |e| {
            e.params.insert(SNAPSHOT_KEY.into(), value);
        }),
        None => txn.create(NewElement::new(format!("{SNAPSHOT_PREFIX}{}", viewpoint.to_lowercase())).param(SNAPSHOT_KEY, value)).map(|_| ()),
    }
}

fn check(t: &Topic) -> Result<()> {
    if t.title.trim().is_empty() {
        return Err(bad("bcf.no_title", "a topic needs a title"));
    }
    if uuid::Uuid::parse_str(&t.guid).is_err() {
        return Err(bad("bcf.bad_guid", format!("{:?} is not a GUID", t.guid)));
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PutParams {
    pub topic: Topic,
    /// Snapshots of its viewpoints, by viewpoint GUID (base64 PNG or JPEG).
    #[serde(default)]
    pub snapshots: Snapshots,
}

/// `bcf.put_topic`: a whole topic, new or replacing the one with its GUID.
pub(crate) fn put(txn: &mut Transaction<'_>, p: PutParams) -> Result<ElementId> {
    let mut t = p.topic;
    if t.guid.is_empty() {
        t.guid = new_guid();
    }
    if t.creation_date.is_empty() {
        t.creation_date = now();
    }
    if t.creation_author.is_empty() {
        t.creation_author = default_author();
    }
    check(&t)?;
    for (v, png) in &p.snapshots {
        put_snapshot(txn, &mut t, v, png)?;
    }
    let store = txn.store().clone();
    let existing = find(&store, &TopicRef::Guid(t.guid.clone())).ok().map(|(id, _)| id);
    write(txn, existing, &t)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateParams {
    pub topic_id: TopicRef,
    pub fields: TopicFields,
    pub author: Option<String>,
}

/// `bcf.update_topic`.
pub(crate) fn update(txn: &mut Transaction<'_>, p: UpdateParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut t) = find(&store, &p.topic_id)?;
    p.fields.apply_to(&mut t);
    t.modified_date = Some(now());
    t.modified_author = Some(p.author.unwrap_or_else(default_author));
    check(&t)?;
    write(txn, Some(id), &t).map(|_| ())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommentParams {
    pub topic_id: TopicRef,
    pub text: String,
    /// One of the topic's viewpoints (its GUID) the comment refers to.
    pub viewpoint: Option<String>,
    pub author: Option<String>,
}

/// `bcf.add_comment`: returns the comment's GUID.
pub(crate) fn add_comment(txn: &mut Transaction<'_>, p: CommentParams) -> Result<String> {
    if p.text.trim().is_empty() {
        return Err(bad("bcf.empty_comment", "the comment is empty"));
    }
    let store = txn.store().clone();
    let (id, mut t) = find(&store, &p.topic_id)?;
    if let Some(v) = &p.viewpoint
        && !t.viewpoints.iter().any(|w| &w.guid == v)
    {
        return Err(bad("bcf.no_viewpoint", format!("the topic has no viewpoint {v}")));
    }
    let guid = new_guid();
    let author = p.author.unwrap_or_else(default_author);
    let date = now();
    t.comments.push(Comment { guid: guid.clone(), date: date.clone(), author: author.clone(), text: p.text, viewpoint: p.viewpoint, modified_date: None, modified_author: None });
    t.modified_date = Some(date);
    t.modified_author = Some(author);
    write(txn, Some(id), &t)?;
    Ok(guid)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddViewpointParams {
    pub topic_id: TopicRef,
    pub viewpoint: Viewpoint,
    /// Its snapshot (base64 PNG or JPEG).
    pub snapshot: Option<String>,
    pub author: Option<String>,
}

/// `bcf.put_viewpoint`: a viewpoint added to a topic; returns its GUID.
pub(crate) fn add_viewpoint(txn: &mut Transaction<'_>, p: AddViewpointParams) -> Result<String> {
    let store = txn.store().clone();
    let (id, mut t) = find(&store, &p.topic_id)?;
    let mut v = p.viewpoint;
    let guid = v.guid.clone().filter(|g| uuid::Uuid::parse_str(g).is_ok()).unwrap_or_else(new_guid);
    v.guid = Some(guid.clone());
    t.viewpoints.retain(|w| w.guid != guid);
    t.viewpoints.push(TopicViewpoint { guid: guid.clone(), viewpoint: v, snapshot: None });
    if let Some(png) = &p.snapshot {
        put_snapshot(txn, &mut t, &guid, png)?;
    }
    t.modified_date = Some(now());
    t.modified_author = Some(p.author.unwrap_or_else(default_author));
    write(txn, Some(id), &t)?;
    Ok(guid)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    pub topic_ids: Vec<TopicRef>,
}

/// `bcf.delete_topic`.
pub(crate) fn delete(txn: &mut Transaction<'_>, p: DeleteParams) -> Result<usize> {
    let store = txn.store().clone();
    let found: Vec<(ElementId, Topic)> = p.topic_ids.iter().map(|r| find(&store, r)).collect::<Result<_>>()?;
    for (id, t) in &found {
        for v in &t.viewpoints {
            if let Some(s) = snapshot_element(&store, &v.guid) {
                txn.delete(s)?;
            }
        }
        txn.delete(*id)?;
    }
    Ok(found.len())
}

/// One line of `bcf.list`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Summary {
    pub id: ElementId,
    pub guid: String,
    pub title: String,
    pub topic_type: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub labels: Vec<String>,
    pub assigned_to: Option<String>,
    pub due_date: Option<String>,
    pub stage: Option<String>,
    pub modified: String,
    pub comments: usize,
    pub viewpoints: usize,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    pub status: Option<String>,
    pub topic_type: Option<String>,
    pub priority: Option<String>,
    pub assigned_to: Option<String>,
    pub label: Option<String>,
    /// Text in the title, description or comments (any case).
    pub text: Option<String>,
}

/// `bcf.list`.
pub fn list(store: &Store, p: &ListParams) -> Vec<Summary> {
    let same = |want: &Option<String>, have: &Option<String>| want.as_ref().is_none_or(|w| have.as_ref().is_some_and(|h| h.eq_ignore_ascii_case(w)));
    all(store)
        .into_iter()
        .filter(|(_, t)| {
            same(&p.status, &t.status)
                && same(&p.topic_type, &t.topic_type)
                && same(&p.priority, &t.priority)
                && same(&p.assigned_to, &t.assigned_to)
                && p.label.as_ref().is_none_or(|l| t.labels.iter().any(|x| x.eq_ignore_ascii_case(l)))
                && p.text.as_ref().is_none_or(|q| {
                    let q = q.to_lowercase();
                    t.title.to_lowercase().contains(&q)
                        || t.description.as_ref().is_some_and(|d| d.to_lowercase().contains(&q))
                        || t.comments.iter().any(|c| c.text.to_lowercase().contains(&q))
                })
        })
        .map(|(id, t)| Summary {
            id,
            guid: t.guid.clone(),
            title: t.title.clone(),
            modified: t.changed().to_owned(),
            comments: t.comments.len(),
            viewpoints: t.viewpoints.len(),
            topic_type: t.topic_type,
            status: t.status,
            priority: t.priority,
            labels: t.labels,
            assigned_to: t.assigned_to,
            due_date: t.due_date,
            stage: t.stage,
        })
        .collect()
}

/// A topic from a file merged into the one with its GUID (COL-BCF-001): the fields of whichever
/// changed last; comments and viewpoints joined by GUID, a comment taken from the file when it was
/// changed later there. Returns whether anything changed.
pub fn merge(mine: &mut Topic, theirs: Topic) -> bool {
    let before = mine.clone();
    let newer = theirs.changed() > mine.changed();
    let comments = std::mem::take(&mut mine.comments);
    let viewpoints = std::mem::take(&mut mine.viewpoints);
    let (mut their_comments, their_viewpoints) = (theirs.comments.clone(), theirs.viewpoints.clone());
    if newer {
        *mine = Topic { comments: Vec::new(), viewpoints: Vec::new(), ..theirs };
    }
    let changed = |c: &Comment| c.modified_date.clone().unwrap_or_else(|| c.date.clone());
    let mut out: Vec<Comment> = Vec::new();
    for c in comments {
        match their_comments.iter().position(|d| d.guid == c.guid) {
            Some(k) => {
                let d = their_comments.remove(k);
                out.push(if changed(&d) > changed(&c) { d } else { c });
            }
            None => out.push(c),
        }
    }
    out.extend(their_comments);
    out.sort_by(|a, b| a.date.cmp(&b.date));
    mine.comments = out;
    mine.viewpoints = viewpoints;
    for v in their_viewpoints {
        if !mine.viewpoints.iter().any(|w| w.guid == v.guid) {
            mine.viewpoints.push(v);
        }
    }
    *mine != before
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeParams {
    pub topics: Vec<Topic>,
    /// Snapshots of their viewpoints, by viewpoint GUID (base64); kept for viewpoints that have
    /// none yet.
    #[serde(default)]
    pub snapshots: Snapshots,
}

/// What `bcf.merge_topics` did.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct MergeReport {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
}

/// `bcf.merge_topics`: topics from a file, added or merged by GUID, as one step.
pub(crate) fn merge_all(txn: &mut Transaction<'_>, p: MergeParams) -> Result<MergeReport> {
    let mut r = MergeReport::default();
    for t in p.topics {
        check(&t)?;
        let store = txn.store().clone();
        let views: Vec<String> = t.viewpoints.iter().map(|v| v.guid.clone()).collect();
        let (id, mut topic, changed) = match find(&store, &TopicRef::Guid(t.guid.clone())) {
            Ok((id, mut mine)) => {
                let changed = merge(&mut mine, t);
                (Some(id), mine, changed)
            }
            Err(_) => (None, t, true),
        };
        let mut pictures = false;
        for v in views {
            if let Some(png) = p.snapshots.get(&v)
                && snapshot_element(&store, &v).is_none()
            {
                put_snapshot(txn, &mut topic, &v, png)?;
                pictures = true;
            }
        }
        match id {
            None => {
                write(txn, None, &topic)?;
                r.added += 1;
            }
            Some(id) if changed || pictures => {
                write(txn, Some(id), &topic)?;
                r.updated += 1;
            }
            Some(_) => r.unchanged += 1,
        }
    }
    Ok(r)
}

/// The lists the project offers (BCF extensions): the defaults and every value in use.
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct Lists {
    pub topic_types: Vec<String>,
    pub statuses: Vec<String>,
    pub priorities: Vec<String>,
    pub labels: Vec<String>,
    pub users: Vec<String>,
    pub stages: Vec<String>,
}

pub fn lists(store: &Store) -> Lists {
    let mut l = Lists {
        topic_types: TYPES.iter().map(|s| (*s).to_owned()).collect(),
        statuses: STATUSES.iter().map(|s| (*s).to_owned()).collect(),
        priorities: PRIORITIES.iter().map(|s| (*s).to_owned()).collect(),
        ..Lists::default()
    };
    let add = |v: &mut Vec<String>, x: &Option<String>| {
        if let Some(x) = x
            && !v.contains(x)
        {
            v.push(x.clone());
        }
    };
    for (_, t) in all(store) {
        add(&mut l.topic_types, &t.topic_type);
        add(&mut l.statuses, &t.status);
        add(&mut l.priorities, &t.priority);
        add(&mut l.stages, &t.stage);
        add(&mut l.users, &t.assigned_to);
        add(&mut l.users, &Some(t.creation_author.clone()));
        for x in &t.labels {
            add(&mut l.labels, &Some(x.clone()));
        }
    }
    l
}

/// A topic as JSON, for `bcf.get`.
pub fn get(store: &Store, r: &TopicRef) -> Result<Value> {
    let (id, t) = find(store, r)?;
    let mut v = serde_json::to_value(t).expect("a topic serialises");
    v["id"] = serde_json::json!(id);
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topic(guid: &str, modified: Option<&str>) -> Topic {
        Topic { guid: guid.into(), title: "T".into(), creation_date: "2026-01-01T00:00:00Z".into(), creation_author: "a".into(), modified_date: modified.map(Into::into), ..Topic::default() }
    }

    fn comment(guid: &str, date: &str, text: &str) -> Comment {
        Comment { guid: guid.into(), date: date.into(), author: "a".into(), text: text.into(), viewpoint: None, modified_date: None, modified_author: None }
    }

    #[test]
    fn merging_takes_the_newer_fields_and_joins_comments() {
        let mut mine = topic("g", Some("2026-02-01T00:00:00Z"));
        mine.status = Some("Open".into());
        mine.comments = vec![comment("c1", "2026-01-02T00:00:00Z", "mine"), comment("c2", "2026-01-03T00:00:00Z", "only mine")];
        let mut theirs = topic("g", Some("2026-03-01T00:00:00Z"));
        theirs.status = Some("Resolved".into());
        let mut edited = comment("c1", "2026-01-02T00:00:00Z", "edited there");
        edited.modified_date = Some("2026-02-15T00:00:00Z".into());
        theirs.comments = vec![edited, comment("c3", "2026-01-04T00:00:00Z", "theirs")];
        assert!(merge(&mut mine, theirs.clone()));
        assert_eq!(mine.status.as_deref(), Some("Resolved"));
        let texts: Vec<&str> = mine.comments.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["edited there", "only mine", "theirs"]);
        // The same file again changes nothing; an older one keeps my fields.
        assert!(!merge(&mut mine, theirs));
        let mut old = topic("g", Some("2025-12-01T00:00:00Z"));
        old.status = Some("Closed".into());
        merge(&mut mine, old);
        assert_eq!(mine.status.as_deref(), Some("Resolved"));
    }

    #[test]
    fn dates_are_iso_8601_utc() {
        let d = now();
        assert_eq!(d.len(), 20, "{d}");
        assert!(d.starts_with("20") && d.ends_with('Z') && &d[10..11] == "T", "{d}");
    }
}
