// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `docs.*`: materials documented on FileEngine (spec Ch 6, COL-FE-006 and COL-FE-007;
//! `design_documents/FILEENGINE_INTEGRATION.md` §7 and §7c).
//!
//! - A project material links to one **documentation folder** (`docs.link`), whose files carry a
//!   `doc.type` (EPD, HPD, specification, …) in their FileEngine metadata (`docs.list`,
//!   `docs.set_doc_type`).
//! - `docs.extract` asks the conversion service's headless extraction (`POST /csai/v1/extract`)
//!   for a **field set** (`schemas/extract/<set>.json`, MIT) from the folder's documents of that
//!   set's `doc.type`, or from files named. Each value comes back with the quote it was read from,
//!   or as not found. The run is kept for this session; nothing is written.
//! - `docs.apply_extracted` writes the fields the user accepts onto the material in one undo step
//!   (`docs.set_values`), each with its source, optionally into the folder's metadata too, and an
//!   EPD's GWP as the material's embodied-carbon factor.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use reviera_core::product_docs::{Link, Source as DocSource, Taken};
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::App;

/// How long one extraction may take: the service reads each document whole.
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// The field sets Reviera ships, by id.
const FIELD_SETS: &[(&str, &str)] = &[("epd", include_str!("../../../schemas/extract/epd.json")), ("hpd", include_str!("../../../schemas/extract/hpd.json"))];

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

// ---- Field sets ----

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Field {
    pub field: String,
    pub label: String,
    /// The IFC property it is written as.
    pub property: String,
    /// The folder metadata key it may fill.
    pub folder_key: Option<String>,
    /// string, number, boolean.
    pub kind: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FieldSet {
    pub id: String,
    pub title: String,
    pub description: String,
    /// The IFC property set its values are written as.
    pub pset: String,
    /// The `doc.type` of the documents it reads.
    pub doc_type: String,
    pub fields: Vec<Field>,
    #[serde(skip)]
    instructions: String,
    #[serde(skip)]
    schema: Value,
}

/// The schema without Reviera's own annotations, as the service is sent it.
fn plain(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(m.iter().filter(|(k, _)| !k.starts_with("x-") && !matches!(k.as_str(), "$id" | "$schema")).map(|(k, v)| (k.clone(), plain(v))).collect()),
        other => other.clone(),
    }
}

fn parse_set(id: &str, text: &str) -> FieldSet {
    let v: Value = serde_json::from_str(text).expect("the shipped field sets are JSON");
    let x = &v["x-reviera"];
    let s = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    let fields = v["properties"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, f)| Field {
            field: k.clone(),
            label: f["x-reviera"]["label"].as_str().unwrap_or(k).to_owned(),
            property: f["x-reviera"]["property"].as_str().unwrap_or(k).to_owned(),
            folder_key: f["x-reviera"]["folder_key"].as_str().map(str::to_owned),
            kind: f["type"].as_str().unwrap_or("string").to_owned(),
            description: s(&f["description"]),
        })
        .collect();
    FieldSet { id: id.to_owned(), title: s(&v["title"]), description: s(&v["description"]), pset: s(&x["pset"]), doc_type: s(&x["doc_type"]), fields, instructions: s(&x["instructions"]), schema: plain(&v) }
}

pub fn field_sets() -> Vec<FieldSet> {
    FIELD_SETS.iter().map(|(id, t)| parse_set(id, t)).collect()
}

fn field_set(id: &str) -> Result<FieldSet> {
    let id = id.trim().to_lowercase();
    field_sets().into_iter().find(|s| s.id == id).ok_or_else(|| bad("docs.no_field_set", format!("no field set {id:?}; docs.field_sets lists them ({})", FIELD_SETS.iter().map(|s| s.0).collect::<Vec<_>>().join(", "))))
}

host!(DocsFieldSets, "docs.field_sets", "List the product data field sets",
    "The field sets docs.extract can read from documents (COL-FE-007): each with its fields (label, the IFC \
     property it fills, the FileEngine folder metadata key it may fill, its type), the property set its values \
     are written as and the doc.type of the documents it reads. Shipped as JSON Schemas in schemas/extract/.",
    [Read], reviera_core::embodied_carbon::Empty, Vec<FieldSet>, |app, caller, p| {
    Ok(field_sets())
});

// ---- Links and documents (COL-FE-006) ----

fn link_of(app: &App, material: &str) -> Result<Link> {
    let id = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
    let d = reviera_core::product_docs::data(app.get(id)?.doc.store());
    d.materials.get(material.trim()).and_then(|m| m.link.clone()).ok_or_else(|| bad("docs.not_linked", format!("{material:?} is not linked to a documentation folder; docs.link links it")))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkParams {
    /// The material, as its layers name it (docs.materials lists them).
    pub material: String,
    pub connection_id: String,
    pub folder_uid: String,
}

host!(DocsLink, "docs.link", "Link a material to its documentation folder",
    "Links a project material to one FileEngine folder of its product documents (COL-FE-006), by the folder's \
     uid, which survives renames and moves. The model keeps the connection, the server's address and the \
     folder's name too, and exports the link as an IfcDocumentReference on the IfcMaterial. One undo step. \
     Returns the link.",
    [Network, Edit], LinkParams, Link, |app, caller, p| {
    let url = app.fileengine.profile_url(&p.connection_id)?;
    let folder = app.fileengine.with(&p.connection_id, |c| c.stat(&p.folder_uid))?;
    if !folder.is_dir() {
        return Err(bad("docs.invalid_input", format!("{} is a file, not a folder", folder.name)));
    }
    let link = Link { connection_id: p.connection_id, url, folder_uid: folder.uid, folder_name: folder.name };
    app.execute("docs.set_link", json!({ "material": p.material, "link": link }), caller)?;
    Ok(link)
});

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocumentOut {
    pub uid: String,
    pub name: String,
    /// The `doc.type` metadata (EPD, HPD, specification, certification, …), if set.
    pub doc_type: Option<String>,
    pub version: String,
    pub size: u64,
    pub modified_by: String,
    /// Unix seconds.
    pub modified_at: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// A linked material: its folder.
    pub material: Option<String>,
    /// Or a folder on a connection.
    pub connection_id: Option<String>,
    pub folder_uid: Option<String>,
}

fn folder_of(app: &App, material: Option<&str>, connection_id: Option<&str>, folder_uid: Option<&str>) -> Result<(String, String)> {
    match (material, connection_id, folder_uid) {
        (Some(m), None, None) => link_of(app, m).map(|l| (l.connection_id, l.folder_uid)),
        (None, Some(c), Some(f)) => Ok((c.to_owned(), f.to_owned())),
        _ => Err(bad("docs.invalid_input", "give a linked material, or a connection_id and a folder_uid")),
    }
}

fn documents(app: &mut App, conn: &str, folder: &str) -> Result<Vec<DocumentOut>> {
    let mut out = app.fileengine.with(conn, |c| {
        let mut out = Vec::new();
        for e in c.list(folder, false)?.into_iter().filter(|e| !e.deleted && !e.is_dir()) {
            let meta = c.metadata(&e.uid).unwrap_or_default();
            let stat = c.stat(&e.uid)?;
            out.push(DocumentOut { doc_type: meta.get("doc.type").cloned(), version: stat.version, uid: e.uid, name: e.name, size: e.size, modified_by: e.modified_by, modified_at: e.modified_at });
        }
        Ok(out)
    })?;
    out.sort_by(|a, b| a.doc_type.cmp(&b.doc_type).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

host!(DocsList, "docs.list", "List a material's documents",
    "The documents in a linked material's folder, or in any folder (`connection_id`, `folder_uid`) (COL-FE-006): \
     each with its doc.type metadata (EPD, HPD, specification, certification, installation, O&M, warranty, \
     SDS, drawing, other), its newest version, size and who changed it last.",
    [Network, Read], ListParams, Vec<DocumentOut>, |app, caller, p| {
    let (conn, folder) = folder_of(app, p.material.as_deref(), p.connection_id.as_deref(), p.folder_uid.as_deref())?;
    documents(app, &conn, &folder)
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetDocTypeParams {
    pub connection_id: String,
    pub file_uid: String,
    /// EPD, HPD, specification, certification, installation, O&M, warranty, SDS, drawing or other.
    pub doc_type: String,
}

host!(DocsSetDocType, "docs.set_doc_type", "Set a document's type",
    "Sets a FileEngine document's doc.type metadata (COL-FE-006), which docs.extract uses to choose the \
     documents a field set reads (an EPD's for `epd`, an HPD's for `hpd`).",
    [Network], SetDocTypeParams, (), |app, caller, p| {
    let t = p.doc_type.trim();
    if t.is_empty() {
        return Err(bad("docs.invalid_input", "name the document's type (EPD, HPD, specification, …)"));
    }
    app.fileengine.with(&p.connection_id, |c| c.set_metadata(&p.file_uid, "doc.type", t))
});

// ---- Extraction (COL-FE-007) ----

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FieldValue {
    pub field: String,
    pub label: String,
    pub found: bool,
    pub value: Value,
    /// The unit as printed.
    pub unit: Option<String>,
    /// The words the value was read from, and where they sit in the document's text.
    pub quote: Option<String>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DocumentResult {
    pub file_uid: String,
    pub name: String,
    pub version: String,
    pub cached: bool,
    /// Whether the answer matched the field set.
    pub valid: bool,
    pub fields: Vec<FieldValue>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Skipped {
    pub file_uid: String,
    pub name: Option<String>,
    /// Why: no text, not found or not readable, budget, …
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Run {
    pub run_id: String,
    pub set: String,
    pub connection_id: String,
    /// The material it was run for, if any.
    pub material: Option<String>,
    pub documents: Vec<DocumentResult>,
    pub skipped: Vec<Skipped>,
    /// The service's counts: found, not found, dropped for want of a quote, cached, tokens.
    pub summary: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractParams {
    /// The field set: `epd` or `hpd` (docs.field_sets).
    pub set: String,
    /// A linked material: its folder's documents of the set's doc.type.
    pub material: Option<String>,
    /// Or these documents, on `connection_id`.
    pub connection_id: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    /// `use` (default) answers again from the service's cache; `refresh` reads the documents again.
    #[serde(default)]
    pub cache: Option<String>,
}

fn to_run(v: &Value, set: &FieldSet, conn: &str, material: Option<String>) -> Run {
    let s = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    let documents = v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|it| it["skipped"].is_null())
        .map(|it| DocumentResult {
            file_uid: s(&it["file_uid"]),
            name: s(&it["name"]),
            version: s(&it["version"]),
            cached: it["cached"] == true,
            valid: it["valid"] != false,
            fields: set
                .fields
                .iter()
                .map(|f| {
                    let d = &it["data"][&f.field];
                    let found = d["found"] == true && !d["value"].is_null();
                    FieldValue {
                        field: f.field.clone(),
                        label: f.label.clone(),
                        found,
                        value: if found { d["value"].clone() } else { Value::Null },
                        unit: d["unit"].as_str().filter(|u| !u.is_empty()).map(str::to_owned),
                        quote: d["evidence"]["quote"].as_str().map(str::to_owned),
                        offset: d["evidence"]["offset"].as_u64(),
                    }
                })
                .collect(),
        })
        .collect();
    let mut skipped: Vec<Skipped> = v["plan"]["skipped"].as_array().into_iter().flatten().map(|k| Skipped { file_uid: s(&k["file_uid"]), name: k["name"].as_str().map(str::to_owned), reason: s(&k["reason"]) }).collect();
    skipped.extend(v["items"].as_array().into_iter().flatten().filter(|it| !it["skipped"].is_null()).map(|it| Skipped { file_uid: s(&it["file_uid"]), name: it["name"].as_str().map(str::to_owned), reason: it["skipped"].as_str().map_or_else(|| it["skipped"].to_string(), str::to_owned) }));
    Run { run_id: s(&v["run_id"]), set: set.id.clone(), connection_id: conn.to_owned(), material, documents, skipped, summary: v["summary"].clone() }
}

host!(DocsExtract, "docs.extract", "Read product data from documents",
    "Reads a field set's values (docs.field_sets: `epd`, `hpd`) out of documents with the conversion service's \
     headless extraction (COL-FE-007): a linked material's folder documents whose doc.type is the set's, or the \
     `files` named on `connection_id`. Each value comes with the quote it was read from, or as not found: the \
     service never guesses. Nothing is written; docs.apply_extracted writes the values the user accepts. The \
     run is kept for this session. Reading takes about half a minute a document; asking again is answered \
     from the service's cache unless `cache` is `refresh`.",
    [Network, Read], ExtractParams, Run, |app, caller, p| {
    let set = field_set(&p.set)?;
    let cache = p.cache.as_deref().unwrap_or("use");
    if !matches!(cache, "use" | "refresh") {
        return Err(bad("docs.invalid_input", "cache is use or refresh"));
    }
    let (conn, files, material) = match (&p.material, &p.connection_id, p.files.is_empty()) {
        (Some(m), None, true) => {
            let link = link_of(app, m)?;
            let docs = documents(app, &link.connection_id, &link.folder_uid)?;
            let files: Vec<String> = docs.iter().filter(|d| d.doc_type.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(&set.doc_type))).map(|d| d.uid.clone()).collect();
            if files.is_empty() {
                return Err(bad("docs.no_documents", format!("no document in {:?} has doc.type {}; docs.list shows their types and docs.set_doc_type sets one", link.folder_name, set.doc_type)));
            }
            (link.connection_id, files, Some(m.trim().to_owned()))
        }
        (None, Some(c), false) => (c.clone(), p.files.clone(), None),
        _ => return Err(bad("docs.invalid_input", "give a linked material, or a connection_id and files")),
    };
    let body = json!({ "files": files, "schema": set.schema, "instructions": set.instructions, "cache": cache });
    let v = app.fileengine.with(&conn, |c| c.extract(&body, EXTRACT_TIMEOUT))?;
    let run = to_run(&v, &set, &conn, material);
    app.fileengine.keep_run(run.clone());
    Ok(run)
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyParams {
    pub run_id: String,
    /// The material to write onto (default: the run's).
    pub material: Option<String>,
    /// The fields accepted (default: every field found).
    pub fields: Option<Vec<String>>,
    /// The document to take them from, when the run read several that disagree.
    pub file_uid: Option<String>,
    /// Also write them into the folder's metadata under each field's key (product.*, epd.*, hpd.*).
    #[serde(default)]
    pub to_folder: bool,
    /// For an EPD: make its GWP A1-A3 the material's embodied-carbon factor (needs the GWP, the
    /// declared unit and the EPD number among the values).
    #[serde(default)]
    pub carbon: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Applied {
    pub material: String,
    pub written: Vec<String>,
    /// The carbon factor made, its value and unit (kg CO2e per m2, m3, kg or piece).
    pub factor_ref: Option<String>,
    pub factor_per_unit: Option<f64>,
    pub factor_unit: Option<reviera_core::embodied_carbon::Unit>,
    /// The folder metadata keys written.
    pub folder_keys: Vec<String>,
}

host!(DocsApplyExtracted, "docs.apply_extracted", "Write extracted product data onto a material",
    "Writes the accepted fields of a docs.extract run onto a material (COL-FE-007), in one undo step: each \
     value with its source (document, version, run), exported as the field set's IfcMaterialProperties. With \
     `to_folder`, also into the documentation folder's metadata; with `carbon`, an EPD's GWP A1-A3 becomes the \
     material's embodied-carbon factor in the Product EPDs library, converted to per m2, m3, kg or piece from \
     the declared unit (1,000 ft2 is 92.903 m2). When the run read several documents that disagree on a \
     field, name the one to take with `file_uid`.",
    [Edit, Network], ApplyParams, Applied, |app, caller, p| {
    let run = app.fileengine.run(&p.run_id).ok_or_else(|| bad("docs.no_run", format!("no extraction run {:?} in this session; docs.extract makes one", p.run_id)))?;
    let set = field_set(&run.set)?;
    let material = p.material.clone().or_else(|| run.material.clone()).ok_or_else(|| bad("docs.invalid_input", "name the material to write onto"))?;
    let wanted: Vec<String> = match &p.fields {
        Some(f) => f.clone(),
        None => set.fields.iter().filter(|f| run.documents.iter().any(|d| d.fields.iter().any(|v| v.field == f.field && v.found))).map(|f| f.field.clone()).collect(),
    };
    if wanted.is_empty() {
        return Err(bad("docs.nothing_found", "the run found none of the fields"));
    }
    let mut values: BTreeMap<String, Option<Taken>> = BTreeMap::new();
    let mut plain: HashMap<String, Value> = HashMap::new();
    for field in &wanted {
        let f = set.fields.iter().find(|f| &f.field == field).ok_or_else(|| bad("docs.invalid_input", format!("{field:?} is not a field of {}", set.id)))?;
        let hits: Vec<(&DocumentResult, &FieldValue)> = run.documents.iter().filter(|d| p.file_uid.as_ref().is_none_or(|u| *u == d.file_uid)).filter_map(|d| d.fields.iter().find(|v| v.field == *field && v.found).map(|v| (d, v))).collect();
        let Some((d, v)) = hits.first() else {
            return Err(bad("docs.not_found", format!("{} was not found in {}", f.label, if p.file_uid.is_some() { "that document" } else { "the documents" })));
        };
        if let Some((other, _)) = hits.iter().find(|(_, w)| w.value != v.value) {
            return Err(bad("docs.ambiguous", format!("{} reads {} in {:?} but {} in {:?}; choose one with file_uid", f.label, v.value, d.name, hits.iter().find(|(o, _)| o.file_uid == other.file_uid).map_or(String::new(), |(_, w)| w.value.to_string()), other.name)));
        }
        plain.insert(field.clone(), v.value.clone());
        values.insert(
            field.clone(),
            Some(Taken { value: v.value.clone(), unit: v.unit.clone(), label: f.label.clone(), property: f.property.clone(), source: DocSource { file_uid: d.file_uid.clone(), file_name: d.name.clone(), version: d.version.clone(), run_id: run.run_id.clone(), cached: d.cached } }),
        );
    }
    let carbon = if p.carbon {
        if set.id != "epd" {
            return Err(bad("docs.invalid_input", "only an EPD's values make a carbon factor"));
        }
        let get = |k: &str| plain.get(k).cloned().ok_or_else(|| bad("docs.invalid_input", format!("a carbon factor needs the EPD's {k} among the values")));
        let gwp = get("gwp_a1a3")?.as_f64().ok_or_else(|| bad("docs.invalid_input", "the GWP is not a number"))?;
        let s = |v: Option<Value>| v.and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        Some(json!({ "gwp_a1a3": gwp, "declared_unit": s(Some(get("declared_unit")?)), "epd_number": s(Some(get("epd_number")?)), "program_operator": s(plain.get("program_operator").cloned()), "product": s(plain.get("product").cloned()), "valid_until": plain.get("valid_until").and_then(Value::as_str) }))
    } else {
        None
    };
    let out = app.execute("docs.set_values", json!({ "material": material, "set": set.id, "pset": set.pset, "values": values, "carbon": carbon }), caller)?;
    let o = &out.result;
    let mut folder_keys = Vec::new();
    if p.to_folder {
        let link = link_of(app, &material)?;
        let pairs: Vec<(String, String)> = wanted
            .iter()
            .filter_map(|field| {
                let key = set.fields.iter().find(|f| &f.field == field)?.folder_key.clone()?;
                let v = plain.get(field)?;
                Some((key, v.as_str().map_or_else(|| v.to_string(), str::to_owned)))
            })
            .collect();
        app.fileengine.with(&link.connection_id, |c| {
            for (k, v) in &pairs {
                c.set_metadata(&link.folder_uid, k, v)?;
            }
            Ok(())
        })?;
        folder_keys = pairs.into_iter().map(|(k, _)| k).collect();
    }
    Ok(Applied {
        material,
        written: serde_json::from_value(o["written"].clone()).unwrap_or_default(),
        factor_ref: o["factor_ref"].as_str().map(str::to_owned),
        factor_per_unit: o["factor_per_unit"].as_f64(),
        factor_unit: serde_json::from_value(o["factor_unit"].clone()).ok(),
        folder_keys,
    })
});

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UsageParams {
    pub connection_id: String,
}

host!(DocsUsage, "docs.usage", "Product data extraction usage",
    "The connection's key's usage of the conversion service (COL-FE-007): `state` (ok, warning, limited), the \
     limits (requests per minute, tokens per day, concurrent runs, files per run) with what is used and when \
     each resets, and a message to show.",
    [Network, Read], UsageParams, Value, |app, caller, p| {
    app.fileengine.with(&p.connection_id, |c| c.usage())
});

/// `reviera.products`, the documented materials and their values, for the model's metadata.
pub(crate) fn products_metadata(store: &reviera_core::Store) -> Option<String> {
    let d = reviera_core::product_docs::data(store);
    if d.materials.is_empty() {
        return None;
    }
    let list: Vec<Value> = d
        .materials
        .iter()
        .map(|(name, m)| {
            let values: BTreeMap<String, BTreeMap<String, &Value>> = m.values.iter().map(|(set, f)| (set.clone(), f.iter().map(|(k, t)| (k.clone(), &t.value)).collect())).collect();
            json!({ "material": name, "folder_uid": m.link.as_ref().map(|l| &l.folder_uid), "values": values })
        })
        .collect();
    Some(serde_json::to_string(&list).expect("serialises"))
}

// ---- The report (COL-FE-008) ----

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Today (UTC) as YYYY-MM-DD.
fn today() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let z = t.div_euclid(86_400) + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

/// The date a year after `day` (YYYY-MM-DD), for "expiring soon".
fn a_year_on(day: &str) -> String {
    match day.get(..4).and_then(|y| y.parse::<i32>().ok()) {
        Some(y) => format!("{}{}", y + 1, &day[4..]),
        None => day.to_owned(),
    }
}

/// `expired`, `expires within a year` or empty, for a validity date.
fn validity(until: Option<&str>, today: &str) -> &'static str {
    match until.filter(|d| d.len() == 10) {
        Some(d) if d < today => "expired",
        Some(d) if d < a_year_on(today).as_str() => "expires within a year",
        _ => "",
    }
}

/// A FileEngine version (`20261030_230012.883`) as `2026-10-30 23:00:12 UTC`.
fn version_stamp(v: &str) -> String {
    let b = v.as_bytes();
    if b.len() >= 15 && b[8] == b'_' && v.is_char_boundary(15) {
        format!("{}-{}-{} {}:{}:{} UTC", &v[0..4], &v[4..6], &v[6..8], &v[9..11], &v[11..13], &v[13..15])
    } else {
        v.to_owned()
    }
}

fn shown(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), |f| if f.fract() == 0.0 && f.abs() < 1e15 { format!("{f:.0}") } else { format!("{f}") }),
        Value::Bool(b) => if *b { "yes".into() } else { "no".into() },
        _ => String::new(),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportParams {
    /// The HTML file to write.
    pub path: std::path::PathBuf,
    /// The embodied carbon run to include (default: the latest, if any).
    pub carbon_run_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReportOut {
    pub path: std::path::PathBuf,
    pub materials: usize,
    pub documented: usize,
    /// Warnings: expired or expiring documents, materials in use without documentation.
    pub warnings: Vec<String>,
}

/// The report's HTML and its warnings.
pub(crate) fn report_html(app: &mut App, caller: &Caller, carbon_run_id: Option<&str>) -> Result<(String, usize, usize, Vec<String>)> {
    let id = app.active().ok_or_else(|| bad("document.none", "no document is open"))?;
    let store = app.get(id)?.doc.store();
    let project = store.iter().find(|e| e.class == "IfcProject").and_then(|p| p.name.clone()).unwrap_or_default();
    let data = reviera_core::product_docs::data(store);
    let materials = run_materials(app, caller)?;
    let carbon = app.execute("embodied_carbon.results", json!({ "run_id": carbon_run_id }), caller).ok().map(|o| o.result);
    let today = today();
    let mut warnings = Vec::new();
    let mut h = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Material documentation and embodied carbon</title><style>\
         body{font-family:sans-serif;margin:2em;max-width:72em}table{border-collapse:collapse;margin:.5em 0 1.5em}\
         td,th{border:1px solid #bbb;padding:.25em .6em;text-align:left;vertical-align:top}th{background:#eee}\
         td.n{text-align:right}.warn{color:#a40}.muted{color:#666;font-size:90%}</style></head><body>",
    );
    h.push_str(&format!("<h1>Material documentation and embodied carbon</h1><p class=\"muted\">{}Written by BIM Reviera {} on {today} (UTC).</p>", if project.is_empty() { String::new() } else { format!("Project: <b>{}</b>. ", esc(&project)) }, env!("CARGO_PKG_VERSION")));

    // Summary: one row per material.
    h.push_str("<h2>Materials</h2><table><tr><th>Material</th><th>In use</th><th>Documentation folder</th><th>EPD</th><th>GWP A1–A3</th><th>HPD</th></tr>");
    let mut documented = 0;
    for m in &materials {
        let md = data.materials.get(&m.material);
        let epd = md.and_then(|d| d.values.get("epd"));
        let hpd = md.and_then(|d| d.values.get("hpd"));
        let get = |set: Option<&BTreeMap<String, Taken>>, k: &str| set.and_then(|s| s.get(k)).map(|t| shown(&t.value));
        if md.is_some_and(|d| d.link.is_some() || !d.values.is_empty()) {
            documented += 1;
        } else if m.in_use {
            warnings.push(format!("{}: in use, with no product documentation", m.material));
        }
        let mut cell = |set: &str, number: Option<String>, until: Option<String>| -> String {
            let Some(n) = number else { return "<span class=\"muted\">none</span>".into() };
            let state = validity(until.as_deref(), &today);
            if !state.is_empty() {
                warnings.push(format!("{}: the {set} {n} {state} ({})", m.material, until.clone().unwrap_or_default()));
            }
            format!("{}<br><span class=\"muted\">valid until {}</span>{}", esc(&n), esc(until.as_deref().unwrap_or("?")), if state.is_empty() { String::new() } else { format!(" <span class=\"warn\">{state}</span>") })
        };
        let epd_cell = cell("EPD", get(epd, "epd_number"), get(epd, "valid_until"));
        let hpd_cell = cell("HPD", get(hpd, "hpd_id"), get(hpd, "valid_until"));
        let gwp = match (epd.and_then(|s| s.get("gwp_a1a3")), get(epd, "declared_unit")) {
            (Some(t), unit) => format!("{} {} per {}", esc(&shown(&t.value)), esc(t.unit.as_deref().unwrap_or("kg CO2e")), esc(&unit.unwrap_or_else(|| "declared unit".into()))),
            _ => String::new(),
        };
        let folder = md.and_then(|d| d.link.as_ref()).map_or_else(String::new, |l| esc(&l.folder_name));
        h.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{folder}</td><td>{epd_cell}</td><td>{gwp}</td><td>{hpd_cell}</td></tr>", esc(&m.material), if m.in_use { "yes" } else { "no" }));
    }
    h.push_str("</table>");

    // Each documented material's values, with where they came from.
    for (name, md) in &data.materials {
        h.push_str(&format!("<h2>{}</h2>", esc(name)));
        if let Some(l) = &md.link {
            h.push_str(&format!("<p>Documentation folder: <b>{}</b> <span class=\"muted\">({}, folder {})</span></p>", esc(&l.folder_name), esc(&l.url), esc(&l.folder_uid)));
        }
        for (set, values) in &md.values {
            h.push_str(&format!("<h3>{}</h3><table><tr><th>Value</th><th></th><th>Unit</th><th>From</th></tr>", esc(&set.to_uppercase())));
            for t in values.values() {
                h.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{} <span class=\"muted\">version of {}{}</span></td></tr>",
                    esc(&t.label),
                    esc(&shown(&t.value)),
                    esc(t.unit.as_deref().unwrap_or("")),
                    esc(if t.source.file_name.is_empty() { &t.source.file_uid } else { &t.source.file_name }),
                    esc(&version_stamp(&t.source.version)),
                    if t.source.run_id.is_empty() { String::new() } else { format!(", read in run {}", esc(&t.source.run_id)) }
                ));
            }
            h.push_str("</table>");
        }
    }

    // Embodied carbon.
    h.push_str("<h2>Embodied carbon</h2>");
    match carbon.as_ref().filter(|c| c["run"].is_object()) {
        Some(c) => {
            let r = &c["run"];
            let f = |v: &Value| format!("{:.1}", v.as_f64().unwrap_or(0.0));
            h.push_str(&format!(
                "<p>Run {}{}: A1–A3 <b>{} kg CO₂e</b>, A–C total {} kg CO₂e; module D {} and biogenic {} reported apart.</p>",
                esc(r["id"].as_str().unwrap_or("")),
                if c["stale"] == true { " <span class=\"warn\">(out of date: the model or factors changed since)</span>" } else { "" },
                f(&r["totals"]["a1_a3"]),
                f(&r["totals"]["total"]),
                f(&r["totals"]["d"]),
                f(&r["totals"]["biogenic"])
            ));
            h.push_str("<table><tr><th>Material</th><th>Factor</th><th>Volume (m³)</th><th>A1–A3 (kg CO₂e)</th><th>A–C total</th></tr>");
            for l in r["by_material"].as_array().into_iter().flatten() {
                let key = l["key"].as_str().unwrap_or("");
                let factor = r["factors"][key]["name"].as_str().unwrap_or("");
                h.push_str(&format!("<tr><td>{}</td><td>{}</td><td class=\"n\">{:.3}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>", esc(key), esc(factor), l["volume"].as_f64().unwrap_or(0.0), f(&l["gwp"]["a1_a3"]), f(&l["gwp"]["total"])));
            }
            h.push_str("</table>");
            for x in r["excluded"].as_array().into_iter().flatten() {
                warnings.push(format!("{}: left out of the carbon run ({})", x["material"].as_str().unwrap_or(""), x["reason"].as_str().unwrap_or("")));
            }
        }
        None => h.push_str("<p class=\"muted\">No embodied carbon run yet (Sustainability › Embodied Carbon).</p>"),
    }

    h.push_str("<h2>To check</h2>");
    if warnings.is_empty() {
        h.push_str("<p>Nothing: every material in use is documented and every document is in date.</p>");
    } else {
        h.push_str("<ul>");
        for w in &warnings {
            h.push_str(&format!("<li class=\"warn\">{}</li>", esc(w)));
        }
        h.push_str("</ul>");
    }
    h.push_str("<p class=\"muted\">Values were read from the documents named, by FileEngine's conversion service, and accepted in Reviera; each one's quote is kept with its run on the server.</p></body></html>");
    Ok((h, materials.len(), documented, warnings))
}

fn run_materials(app: &mut App, caller: &Caller) -> Result<Vec<reviera_core::product_docs::MaterialOut>> {
    let v = app.execute("docs.materials", json!({}), caller)?.result;
    Ok(v.as_array().into_iter().flatten().map(|m| reviera_core::product_docs::MaterialOut { material: m["material"].as_str().unwrap_or("").to_owned(), in_use: m["in_use"] == true, link: None, values: BTreeMap::new() }).collect())
}

host!(DocsReport, "docs.report", "Write the material documentation report",
    "Writes an HTML report of the project's materials (COL-FE-008): each with its documentation folder, its EPD \
     (number, validity, GWP A1-A3 per declared unit) and HPD (identifier, validity), every value taken from \
     documents with the document and version it came from, the embodied carbon run (`carbon_run_id`, else the \
     latest) by material, and what to check: expired documents or ones expiring within a year, materials in use \
     with no documentation, materials left out of the carbon run. fileengine.publish puts it on FileEngine.",
    [Read, FileWrite], ReportParams, ReportOut, |app, caller, p| {
    let (html, materials, documented, warnings) = report_html(app, caller, p.carbon_run_id.as_deref())?;
    std::fs::write(&p.path, html).map_err(|e| bad("docs.write", format!("{}: {e}", p.path.display())))?;
    Ok(ReportOut { path: p.path, materials, documented, warnings })
});
