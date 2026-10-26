// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The XML of a BCF archive (COL-BCF-001), for `bcf.export` and `bcf.import`: a topic's
//! `markup.bcf` in BCF 3.0 or 2.1, `bcf.version` and 3.0's `extensions.xml`. A topic's
//! viewpoints are `<guid>.bcfv` files beside its markup ([`crate::bcf::to_xml`]); the
//! application zips them.
//!
//! BCF 3.0 keeps a topic's comments and viewpoints inside its `Topic`, labels as `Labels/Label`
//! and links as `ReferenceLinks/ReferenceLink`; 2.1 puts comments and viewpoints beside the
//! topic in the `Markup`, repeats `Labels` and `ReferenceLink`, and nests a comment's text in an
//! element of the same name. Both are read.

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::{Error, Result};
use crate::issues::{Comment, Lists, Topic};

/// The BCF version written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V2_1,
    V3_0,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "2.1" => Some(Self::V2_1),
            "3.0" | "3" => Some(Self::V3_0),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::V2_1 => "2.1",
            Self::V3_0 => "3.0",
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn el(tag: &str, v: &Option<String>) -> String {
    v.as_ref().filter(|v| !v.is_empty()).map_or(String::new(), |v| format!("<{tag}>{}</{tag}>", esc(v)))
}

/// `bcf.version`.
pub fn version_xml(v: Version) -> String {
    match v {
        Version::V2_1 => "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Version VersionId=\"2.1\"><DetailedVersion>2.1</DetailedVersion></Version>\n".into(),
        Version::V3_0 => "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Version VersionId=\"3.0\"/>\n".into(),
    }
}

/// BCF 3.0's `extensions.xml`: the project's lists.
pub fn extensions_xml(l: &Lists) -> String {
    let list = |outer: &str, inner: &str, items: &[String]| {
        if items.is_empty() {
            return String::new();
        }
        let mut s = format!("<{outer}>");
        for i in items {
            s.push_str(&format!("<{inner}>{}</{inner}>", esc(i)));
        }
        s.push_str(&format!("</{outer}>"));
        s
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Extensions>{}{}{}{}{}{}</Extensions>\n",
        list("TopicTypes", "TopicType", &l.topic_types),
        list("TopicStatuses", "TopicStatus", &l.statuses),
        list("Priorities", "Priority", &l.priorities),
        list("TopicLabels", "TopicLabel", &l.labels),
        list("Users", "User", &l.users),
        list("Stages", "Stage", &l.stages),
    )
}

/// A topic's `markup.bcf`; its viewpoints are `<guid>.bcfv`, their snapshots kept by name.
pub fn markup_xml(t: &Topic, v: Version) -> String {
    let mut topic = format!("<Topic Guid=\"{}\"", esc(&t.guid));
    if let Some(x) = &t.topic_type {
        topic.push_str(&format!(" TopicType=\"{}\"", esc(x)));
    }
    if let Some(x) = &t.status {
        topic.push_str(&format!(" TopicStatus=\"{}\"", esc(x)));
    }
    topic.push('>');
    let links: String = t.reference_links.iter().map(|l| format!("<ReferenceLink>{}</ReferenceLink>", esc(l))).collect();
    let labels: String = match v {
        Version::V3_0 if !t.labels.is_empty() => format!("<Labels>{}</Labels>", t.labels.iter().map(|l| format!("<Label>{}</Label>", esc(l))).collect::<String>()),
        Version::V3_0 => String::new(),
        Version::V2_1 => t.labels.iter().map(|l| format!("<Labels>{}</Labels>", esc(l))).collect(),
    };
    if v == Version::V3_0 && !links.is_empty() {
        topic.push_str(&format!("<ReferenceLinks>{links}</ReferenceLinks>"));
    } else if v == Version::V2_1 {
        topic.push_str(&links);
    }
    topic.push_str(&format!("<Title>{}</Title>", esc(&t.title)));
    topic.push_str(&el("Priority", &t.priority));
    topic.push_str(&labels);
    topic.push_str(&format!("<CreationDate>{}</CreationDate><CreationAuthor>{}</CreationAuthor>", esc(&t.creation_date), esc(&t.creation_author)));
    topic.push_str(&el("ModifiedDate", &t.modified_date));
    topic.push_str(&el("ModifiedAuthor", &t.modified_author));
    topic.push_str(&el("DueDate", &t.due_date));
    topic.push_str(&el("AssignedTo", &t.assigned_to));
    topic.push_str(&el("Stage", &t.stage));
    topic.push_str(&el("Description", &t.description));
    let related: String = t.related_topics.iter().map(|g| format!("<RelatedTopic Guid=\"{}\"/>", esc(g))).collect();
    if !related.is_empty() {
        topic.push_str(&match v {
            Version::V3_0 => format!("<RelatedTopics>{related}</RelatedTopics>"),
            Version::V2_1 => related,
        });
    }
    let comment = |c: &Comment| {
        let mut s = format!("<Comment Guid=\"{}\"><Date>{}</Date><Author>{}</Author><Comment>{}</Comment>", esc(&c.guid), esc(&c.date), esc(&c.author), esc(&c.text));
        if let Some(g) = &c.viewpoint {
            s.push_str(&format!("<Viewpoint Guid=\"{}\"/>", esc(g)));
        }
        s.push_str(&el("ModifiedDate", &c.modified_date));
        s.push_str(&el("ModifiedAuthor", &c.modified_author));
        s.push_str("</Comment>");
        s
    };
    let viewpoint = |tag: &str, g: &str, snapshot: &Option<String>| format!("<{tag} Guid=\"{0}\"><Viewpoint>{0}.bcfv</Viewpoint>{1}</{tag}>", esc(g), el("Snapshot", snapshot));
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Markup>");
    match v {
        Version::V3_0 => {
            s.push_str(&topic);
            if !t.comments.is_empty() {
                s.push_str("<Comments>");
                for c in &t.comments {
                    s.push_str(&comment(c));
                }
                s.push_str("</Comments>");
            }
            if !t.viewpoints.is_empty() {
                s.push_str("<Viewpoints>");
                for w in &t.viewpoints {
                    s.push_str(&viewpoint("ViewPoint", &w.guid, &w.snapshot));
                }
                s.push_str("</Viewpoints>");
            }
            s.push_str("</Topic>");
        }
        Version::V2_1 => {
            s.push_str(&topic);
            s.push_str("</Topic>");
            for c in &t.comments {
                s.push_str(&comment(c));
            }
            for w in &t.viewpoints {
                s.push_str(&viewpoint("Viewpoints", &w.guid, &w.snapshot));
            }
        }
    }
    s.push_str("</Markup>\n");
    s
}

/// A viewpoint a markup lists: its GUID, its file and its snapshot's.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewpointEntry {
    pub guid: String,
    pub file: Option<String>,
    pub snapshot: Option<String>,
}

/// Reads a `markup.bcf` (BCF 2.1 or 3.0): the topic (without its viewpoints) and the viewpoints
/// it lists, for the caller to read from their files.
pub fn read_markup(text: &str) -> Result<(Topic, Vec<ViewpointEntry>)> {
    let bad = |m: String| Error::new("bcf.parse", m);
    // Text is trimmed as a whole, not around each entity (`beam &amp; column` keeps its spaces).
    let mut r = Reader::from_str(text);
    let mut t = Topic::default();
    let mut entries: Vec<ViewpointEntry> = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut comment: Option<Comment> = None;
    let mut saw_topic = false;
    loop {
        let ev = r.read_event().map_err(|e| bad(format!("at byte {}: {e}", r.buffer_position())))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = e.local_name().as_ref().to_owned();
                let attr = |k: &str| {
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == k)
                        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::default()).ok().map(|v| v.into_owned()))
                };
                let parent = path.last().map(String::as_str).unwrap_or("");
                match (name.as_str(), parent) {
                    ("Topic", _) => {
                        saw_topic = true;
                        t.guid = attr("Guid").unwrap_or_default();
                        t.topic_type = attr("TopicType");
                        t.status = attr("TopicStatus");
                    }
                    // A comment (not the text inside a 2.1 comment).
                    ("Comment", p) if p != "Comment" => {
                        comment = Some(Comment { guid: attr("Guid").unwrap_or_default(), date: String::new(), author: String::new(), text: String::new(), viewpoint: None, modified_date: None, modified_author: None });
                    }
                    ("Viewpoint", "Comment") => {
                        if let Some(c) = comment.as_mut() {
                            c.viewpoint = attr("Guid");
                        }
                    }
                    ("ViewPoint", _) | ("Viewpoints", "Markup") => entries.push(ViewpointEntry { guid: attr("Guid").unwrap_or_default(), file: None, snapshot: None }),
                    ("RelatedTopic", _) => {
                        if let Some(g) = attr("Guid") {
                            t.related_topics.push(g);
                        }
                    }
                    _ => {}
                }
                buf.clear();
                if matches!(ev, Event::Start(_)) {
                    path.push(name);
                }
            }
            Event::Text(x) => buf.push_str(&x.xml10_content()),
            Event::GeneralRef(g) => {
                let c = match &*g {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    n => n.strip_prefix("#x").and_then(|h| u32::from_str_radix(h, 16).ok()).or_else(|| n.strip_prefix('#').and_then(|d| d.parse().ok())).and_then(char::from_u32),
                };
                if let Some(c) = c {
                    buf.push(c);
                }
            }
            Event::End(_) => {
                let name = path.pop().unwrap_or_default();
                let parent = path.last().map(String::as_str).unwrap_or("");
                let text = std::mem::take(&mut buf).trim().to_owned();
                let value = (!text.is_empty()).then(|| text.clone());
                if let Some(c) = comment.as_mut()
                    && (parent == "Comment")
                {
                    match name.as_str() {
                        "Date" => c.date = text,
                        "Author" => c.author = text,
                        "Comment" => c.text = text,
                        "ModifiedDate" => c.modified_date = value,
                        "ModifiedAuthor" => c.modified_author = value,
                        _ => {}
                    }
                    continue;
                }
                if name == "Comment" && parent != "Comment" {
                    if let Some(c) = comment.take() {
                        t.comments.push(c);
                    }
                    continue;
                }
                if matches!(parent, "ViewPoint" | "Viewpoints")
                    && let Some(e) = entries.last_mut()
                {
                    match name.as_str() {
                        "Viewpoint" => e.file = value,
                        "Snapshot" => e.snapshot = value,
                        _ => {}
                    }
                    continue;
                }
                match (name.as_str(), parent) {
                    ("Title", "Topic") => t.title = text,
                    ("Priority", "Topic") => t.priority = value,
                    ("Labels", "Topic") | ("Label", "Labels") => {
                        if let Some(v) = value {
                            t.labels.push(v);
                        }
                    }
                    ("ReferenceLink", _) => {
                        if let Some(v) = value {
                            t.reference_links.push(v);
                        }
                    }
                    ("CreationDate", "Topic") => t.creation_date = text,
                    ("CreationAuthor", "Topic") => t.creation_author = text,
                    ("ModifiedDate", "Topic") => t.modified_date = value,
                    ("ModifiedAuthor", "Topic") => t.modified_author = value,
                    ("DueDate", "Topic") => t.due_date = value,
                    ("AssignedTo", "Topic") => t.assigned_to = value,
                    ("Stage", "Topic") => t.stage = value,
                    ("Description", "Topic") => t.description = value,
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_topic {
        return Err(bad("a markup with no Topic".into()));
    }
    Ok((t, entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issues::Comment;

    fn topic() -> Topic {
        Topic {
            guid: "8dc86298-9737-40b4-a448-98a9e953293a".into(),
            title: "Duct clashes with beam & column".into(),
            description: Some("Level 2, grid B/3 <east>".into()),
            topic_type: Some("Clash".into()),
            status: Some("Open".into()),
            priority: Some("High".into()),
            labels: vec!["MEP".into(), "Structure".into()],
            assigned_to: Some("mep@example.com".into()),
            due_date: Some("2026-11-01".into()),
            stage: Some("Design".into()),
            creation_date: "2026-10-26T10:00:00Z".into(),
            creation_author: "arch@example.com".into(),
            modified_date: Some("2026-10-26T11:00:00Z".into()),
            modified_author: Some("mep@example.com".into()),
            related_topics: vec!["3f0e0a0c-2b8d-4c41-9e9d-4b9f6f5a7a11".into()],
            reference_links: vec!["https://example.com/rfi/12".into()],
            comments: vec![Comment {
                guid: "c1".into(),
                date: "2026-10-26T10:30:00Z".into(),
                author: "mep@example.com".into(),
                text: "Will drop the duct".into(),
                viewpoint: Some("v1".into()),
                modified_date: None,
                modified_author: None,
            }],
            viewpoints: vec![crate::issues::TopicViewpoint { guid: "v1".into(), viewpoint: crate::bcf::Viewpoint::default(), snapshot: Some("v1.png".into()) }],
        }
    }

    #[test]
    fn markup_round_trips_in_both_versions() {
        for v in [Version::V3_0, Version::V2_1] {
            let t = topic();
            let (back, entries) = read_markup(&markup_xml(&t, v)).unwrap();
            assert_eq!(back, Topic { viewpoints: Vec::new(), ..t.clone() }, "{v:?}");
            assert_eq!(entries, [ViewpointEntry { guid: "v1".into(), file: Some("v1.bcfv".into()), snapshot: Some("v1.png".into()) }], "{v:?}");
        }
    }

    /// A 2.1 markup as other tools write it.
    #[test]
    fn reads_a_bcf_2_1_markup() {
        let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<Markup xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Header><File IfcProject="0M6o7Znnv7hxsbWgeu7oQq" isExternal="true"><Filename>model.ifc</Filename></File></Header>
  <Topic Guid="a5ae7b4c-1f6b-4b07-9e1a-97e0f8a0f2a1" TopicType="Issue" TopicStatus="Active">
    <Title>Missing fire damper</Title>
    <Labels>Fire</Labels>
    <CreationDate>2025-05-01T09:00:00+00:00</CreationDate>
    <CreationAuthor>someone@example.com</CreationAuthor>
  </Topic>
  <Comment Guid="b1"><Date>2025-05-01T09:05:00+00:00</Date><Author>someone@example.com</Author><Comment>Please add one</Comment><Viewpoint Guid="e1"/></Comment>
  <Viewpoints Guid="e1"><Viewpoint>viewpoint.bcfv</Viewpoint><Snapshot>snapshot.png</Snapshot></Viewpoints>
</Markup>"#;
        let (t, entries) = read_markup(text).unwrap();
        assert_eq!((t.title.as_str(), t.status.as_deref(), t.labels.as_slice()), ("Missing fire damper", Some("Active"), &["Fire".to_owned()][..]));
        assert_eq!(t.comments.len(), 1);
        assert_eq!((t.comments[0].text.as_str(), t.comments[0].viewpoint.as_deref()), ("Please add one", Some("e1")));
        assert_eq!(entries[0].file.as_deref(), Some("viewpoint.bcfv"));
    }
}
