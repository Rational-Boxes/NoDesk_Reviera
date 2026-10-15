// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Groups (spec Ch 3 §3.13, ARC-GRP-001 … 003): reusable sets of elements whose instances stay
//! identical.
//!
//! - A **group type** (`rxd:group_types` on the project) keeps a *template*: its members'
//!   element data as they were in the instance it was made or last edited from, with that
//!   instance's origin and rotation, each member under a stable key.
//! - A **group instance** is an `IfcGroup` with `recipe.group`: its type, origin, rotation, its
//!   members by key (GlobalIds) and the keys excluded from it. The members are real elements,
//!   tagged `rxd:group_member` with the instance's GlobalId.
//! - Placing an instance makes the members from the template: references between members (a
//!   door's host wall, a wall's joins) are pointed at the new members, then they are moved and
//!   turned into place with the ordinary Move and Rotate.
//! - Members are read-only outside the group editor: `group.edit` opens it on one instance,
//!   `group.add_members` and `group.remove_members` change its members, `group.finish` takes
//!   the template from it and remakes every other instance, and `group.cancel` remakes the
//!   edited instance from the template as it was.
//! - Excluding a member removes it from one instance only; restoring makes it again.
//!
//! Not built yet: detail and attached detail groups, nested groups, member GlobalIds kept by key
//! when instances are remade, rehosting members whose host is outside the group, Save Group and
//! Load as Group, converting to a link, the Origin Level Offset.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::{Element, NewElement};
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef, GlobalId};
use crate::store::Store;
use crate::txn::Transaction;
use crate::walls::RECIPE_KEY;

pub const TYPES_KEY: &str = "rxd:group_types";
pub const RECIPE: &str = "recipe.group";
/// On a member: the GlobalId of its instance.
pub const MEMBER_KEY: &str = "rxd:group_member";
/// On the project: the instance open in the group editor, if any.
pub const EDIT_KEY: &str = "rxd:group_edit";

/// One member of a template: its key and element data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TemplateMember {
    pub key: u32,
    pub class: String,
    #[serde(default)]
    pub name: Option<String>,
    /// The element's parameters as they were (its GlobalId under `guid`).
    pub params: BTreeMap<String, Value>,
    pub guid: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GroupType {
    pub id: String,
    pub name: String,
    /// The template's frame: the origin and rotation of the instance it was taken from.
    pub origin: [f64; 3],
    pub rotation: f64,
    pub template: Vec<TemplateMember>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Member {
    pub key: u32,
    pub guid: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GroupRecipe {
    pub kind: String,
    pub group_type: String,
    pub origin: [f64; 3],
    #[serde(default)]
    pub rotation: f64,
    pub members: Vec<Member>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<u32>,
    /// Members removed in the open editor (GlobalIds): Cancel takes them back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
}

pub fn recipe(e: &Element) -> Option<GroupRecipe> {
    let v = e.param(RECIPE_KEY)?;
    (v.get("kind")?.as_str()? == RECIPE).then(|| serde_json::from_value(v.clone()).ok())?
}

fn project(store: &Store) -> Option<&Element> {
    store.iter().find(|e| e.class == "IfcProject")
}

pub fn types(store: &Store) -> Vec<GroupType> {
    project(store)
        .and_then(|p| p.param(TYPES_KEY))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn group_type(store: &Store, r: &str) -> Result<GroupType> {
    types(store)
        .into_iter()
        .find(|t| t.id == r || t.name == r)
        .ok_or_else(|| Error::new("group.no_type", format!("no group type {r}")))
}

fn write_types(txn: &mut Transaction<'_>, list: &[GroupType]) -> Result<()> {
    set_project(
        txn,
        TYPES_KEY,
        serde_json::to_value(list).expect("serialises"),
    )
}

fn set_project(txn: &mut Transaction<'_>, key: &str, v: Value) -> Result<()> {
    let p = project(txn.store())
        .map(Element::id)
        .ok_or_else(|| Error::new("project.missing", "the model has no project"))?;
    txn.modify(p, |e| {
        if v.is_null() {
            e.params.remove(key);
        } else {
            e.params.insert(key.into(), v);
        }
    })
}

/// The instance open in the group editor.
pub fn editing(store: &Store) -> Option<String> {
    project(store)?.param(EDIT_KEY)?.as_str().map(str::to_owned)
}

/// The instance an element belongs to, if it is a group member.
pub fn instance_of(e: &Element) -> Option<String> {
    e.param(MEMBER_KEY)?.as_str().map(str::to_owned)
}

/// Refuses changing a group member outside the editor (ARC-MOD-000).
pub fn check_editable(store: &Store, e: &Element) -> Result<()> {
    let Some(g) = instance_of(e) else {
        return Ok(());
    };
    match editing(store) {
        Some(open) if open == g || open == "*" => Ok(()),
        _ => Err(Error::new(
            "element.group_member",
            format!(
                "element {} is in a group; edit the group (Edit Group) to change it",
                e.id()
            ),
        )),
    }
}

fn find(store: &Store, guid: &str) -> Option<ElementId> {
    store.find_by_guid(&GlobalId::parse(guid)?)
}

fn instance(store: &Store, r: &ElementRef) -> Result<(ElementId, GroupRecipe)> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such group"))?;
    let g = store
        .get(id)
        .and_then(recipe)
        .ok_or_else(|| Error::new("group.not_group", format!("element {id} is not a group")))?;
    Ok((id, g))
}

fn write_instance(txn: &mut Transaction<'_>, id: ElementId, g: &GroupRecipe) -> Result<()> {
    let v = serde_json::to_value(g).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), v);
    })
}

/// Replaces member GlobalIds in a value (references between members).
fn remap(v: &Value, map: &BTreeMap<String, String>) -> Value {
    match v {
        Value::String(s) => Value::String(map.get(s).cloned().unwrap_or_else(|| s.clone())),
        Value::Array(a) => Value::Array(a.iter().map(|x| remap(x, map)).collect()),
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, x)| (k.clone(), remap(x, map))).collect())
        }
        other => other.clone(),
    }
}

/// Runs `f` with the group editor open to every group (internal moves of members).
fn internally<T>(
    txn: &mut Transaction<'_>,
    f: impl FnOnce(&mut Transaction<'_>) -> Result<T>,
) -> Result<T> {
    let before = project(txn.store())
        .and_then(|p| p.param(EDIT_KEY))
        .cloned();
    set_project(txn, EDIT_KEY, Value::from("*"))?;
    let out = f(txn);
    set_project(txn, EDIT_KEY, before.unwrap_or(Value::Null))?;
    out
}

/// Makes members from a template at an origin and rotation; their GlobalIds by key.
fn make_members(
    txn: &mut Transaction<'_>,
    t: &GroupType,
    keys: &[u32],
    origin: [f64; 3],
    rotation: f64,
    instance_guid: &str,
    existing: &[Member],
) -> Result<Vec<Member>> {
    let chosen: Vec<&TemplateMember> = t
        .template
        .iter()
        .filter(|m| keys.contains(&m.key))
        .collect();
    // References to members: new ones to their new GlobalIds, others to this instance's.
    let mut fresh: BTreeMap<String, String> = t
        .template
        .iter()
        .filter_map(|m| {
            let mine = existing.iter().find(|x| x.key == m.key)?;
            Some((m.guid.clone(), mine.guid.clone()))
        })
        .collect();
    for m in &chosen {
        fresh.insert(m.guid.clone(), GlobalId::new().to_string());
    }
    let mut made = Vec::new();
    for m in &chosen {
        let guid = GlobalId::parse(&fresh[&m.guid]).expect("generated");
        let mut n = NewElement::new(m.class.clone()).guid(guid.clone());
        n.name = m.name.clone();
        for (k, v) in &m.params {
            if k == "rxd:identity" || k == MEMBER_KEY {
                continue;
            }
            n.params.insert(k.clone(), remap(v, &fresh));
        }
        n.params
            .insert(MEMBER_KEY.into(), Value::from(instance_guid));
        let id = txn.create(n)?;
        made.push((m.key, id, guid.to_string()));
    }
    // Into place: turn by the difference of rotations about the template's origin, then move.
    // Doors and windows follow their walls.
    let ids: Vec<ElementRef> = made
        .iter()
        .filter(|(_, id, _)| {
            txn.store()
                .get(*id)
                .is_none_or(|e| crate::inserts::insert_recipe(e).is_none())
        })
        .map(|(_, id, _)| ElementRef::Id(*id))
        .collect();
    if !ids.is_empty() {
        internally(txn, |txn| {
            let angle = rotation - t.rotation;
            if angle.abs() > 1e-12 {
                crate::modify::transform(
                    txn,
                    &ids,
                    crate::modify::Xform::Rotate {
                        center: [t.origin[0], t.origin[1]],
                        angle,
                    },
                    false,
                )?;
            }
            let d = [
                origin[0] - t.origin[0],
                origin[1] - t.origin[1],
                origin[2] - t.origin[2],
            ];
            if d.iter().any(|v| v.abs() > 1e-12) {
                crate::modify::transform(txn, &ids, crate::modify::Xform::Translate(d), false)?;
            }
            Ok(())
        })?;
    }
    Ok(made
        .into_iter()
        .map(|(key, _, guid)| Member { key, guid })
        .collect())
}

/// A template from elements as they are now.
fn snapshot(store: &Store, members: &[(u32, ElementId)]) -> Vec<TemplateMember> {
    members
        .iter()
        .filter_map(|(key, id)| {
            let e = store.get(*id)?;
            Some(TemplateMember {
                key: *key,
                class: e.class.clone(),
                name: e.name.clone(),
                params: e
                    .params
                    .iter()
                    .filter(|(k, _)| k.as_str() != "rxd:identity" && k.as_str() != MEMBER_KEY)
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                guid: e.guid().to_string(),
            })
        })
        .collect()
}

/// Elements that come along with a member set: doors and windows of member walls.
fn with_hosted(store: &Store, ids: &[ElementId]) -> Vec<ElementId> {
    let mut out: Vec<ElementId> = ids.to_vec();
    for id in ids {
        if let Some(e) = store.get(*id) {
            for i in crate::inserts::hosted_by(store, e.guid().as_str()) {
                if !out.contains(&i) {
                    out.push(i);
                }
            }
        }
    }
    out
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    pub element_ids: Vec<ElementRef>,
    pub name: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Created {
    pub group_type_id: String,
    pub instance_id: ElementId,
}

/// `group.create`: the elements become the first instance of a new group type, its origin the
/// middle of their extent in plan.
pub(crate) fn create(txn: &mut Transaction<'_>, p: CreateParams) -> Result<Created> {
    let store = txn.store().clone();
    let name = p.name.trim().to_owned();
    let mut list = types(&store);
    if name.is_empty() || list.iter().any(|t| t.name == name) {
        return Err(Error::new(
            "group.name",
            "a group type needs a name of its own",
        ));
    }
    let mut ids = Vec::new();
    for r in &p.element_ids {
        let id = store
            .resolve(r)
            .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?;
        let e = store.get(id).expect("resolved");
        if instance_of(e).is_some() || recipe(e).is_some() {
            return Err(Error::new(
                "group.nested",
                "groups in groups are not available yet",
            ));
        }
        if crate::views::is_view(e)
            || e.class == "IfcBuildingStorey"
            || crate::categories::is_type(e)
        {
            return Err(Error::new(
                "group.member",
                format!("element {id} cannot be grouped"),
            ));
        }
        ids.push(id);
    }
    let ids = with_hosted(&store, &ids);
    if ids.is_empty() {
        return Err(Error::new("group.empty", "select elements to group"));
    }
    // The origin: the middle of the members' bodies in plan, at their lowest point.
    let bodies = crate::bodies::all(&store);
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for b in bodies.iter().filter(|b| ids.contains(&b.element)) {
        if let Some((a, c)) = b.bounds() {
            for k in 0..3 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(c[k]);
            }
        }
    }
    let origin = if lo[0] <= hi[0] {
        [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, lo[2]]
    } else {
        [0.0; 3]
    };
    let keyed: Vec<(u32, ElementId)> = ids
        .iter()
        .enumerate()
        .map(|(k, id)| (k as u32, *id))
        .collect();
    let type_id = GlobalId::new().to_string();
    list.push(GroupType {
        id: type_id.clone(),
        name: name.clone(),
        origin,
        rotation: 0.0,
        template: snapshot(&store, &keyed),
    });
    write_types(txn, &list)?;
    let guid = GlobalId::new();
    let g = GroupRecipe {
        kind: RECIPE.into(),
        group_type: type_id.clone(),
        origin,
        rotation: 0.0,
        members: keyed
            .iter()
            .map(|(k, id)| Member {
                key: *k,
                guid: store.get(*id).expect("member").guid().to_string(),
            })
            .collect(),
        excluded: Vec::new(),
        removed: Vec::new(),
    };
    let instance = txn.create(
        NewElement::new("IfcGroup")
            .guid(guid.clone())
            .name(name.as_str())
            .param("@ObjectType", name.as_str())
            .param(RECIPE_KEY, serde_json::to_value(&g).expect("serialises")),
    )?;
    for (_, id) in &keyed {
        txn.modify(*id, |e| {
            e.params
                .insert(MEMBER_KEY.into(), Value::from(guid.as_str()));
        })?;
    }
    Ok(Created {
        group_type_id: type_id,
        instance_id: instance,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    /// The group type's key or name.
    pub group_type_id: String,
    /// Where the origin goes (plan; z optional).
    pub point: Vec<f64>,
    /// Counter-clockwise (radians or "90°").
    #[serde(default)]
    #[schemars(extend("x-reviera-data-type" = "angle"))]
    pub rotation: f64,
}

/// `group.place`: a new instance of a group type.
pub(crate) fn place(txn: &mut Transaction<'_>, p: PlaceParams) -> Result<ElementId> {
    let store = txn.store().clone();
    let t = group_type(&store, &p.group_type_id)?;
    let origin = [
        *p.point.first().unwrap_or(&0.0),
        *p.point.get(1).unwrap_or(&0.0),
        *p.point.get(2).unwrap_or(&t.origin[2]),
    ];
    let guid = GlobalId::new();
    let keys: Vec<u32> = t.template.iter().map(|m| m.key).collect();
    let members = make_members(txn, &t, &keys, origin, p.rotation, guid.as_str(), &[])?;
    let g = GroupRecipe {
        kind: RECIPE.into(),
        group_type: t.id.clone(),
        origin,
        rotation: p.rotation,
        members,
        excluded: Vec::new(),
        removed: Vec::new(),
    };
    txn.create(
        NewElement::new("IfcGroup")
            .guid(guid)
            .name(t.name.as_str())
            .param("@ObjectType", t.name.as_str())
            .param(RECIPE_KEY, serde_json::to_value(&g).expect("serialises")),
    )
}

/// Deletes an instance's members.
fn delete_members(txn: &mut Transaction<'_>, g: &GroupRecipe) -> Result<()> {
    let store = txn.store().clone();
    for m in &g.members {
        if let Some(id) = find(&store, &m.guid)
            && txn.store().get(id).is_some()
        {
            txn.delete(id)?;
        }
    }
    Ok(())
}

/// Remakes an instance from its type's template (keeping its exclusions).
fn remake(txn: &mut Transaction<'_>, id: ElementId, t: &GroupType) -> Result<()> {
    let store = txn.store().clone();
    let mut g = recipe(store.get(id).expect("exists")).expect("a group");
    delete_members(txn, &g)?;
    let keys: Vec<u32> = t
        .template
        .iter()
        .map(|m| m.key)
        .filter(|k| !g.excluded.contains(k))
        .collect();
    let guid = store.get(id).expect("exists").guid().to_string();
    g.members = make_members(txn, t, &keys, g.origin, g.rotation, &guid, &[])?;
    g.group_type = t.id.clone();
    write_instance(txn, id, &g)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstancesParams {
    pub instance_ids: Vec<ElementRef>,
}

/// `group.ungroup`: the members become independent; their ids.
pub(crate) fn ungroup(txn: &mut Transaction<'_>, p: InstancesParams) -> Result<Vec<ElementId>> {
    let mut out = Vec::new();
    for r in &p.instance_ids {
        let store = txn.store().clone();
        let (id, g) = instance(&store, r)?;
        for m in &g.members {
            if let Some(mid) = find(&store, &m.guid) {
                txn.modify(mid, |e| {
                    e.params.remove(MEMBER_KEY);
                })?;
                out.push(mid);
            }
        }
        txn.delete(id)?;
    }
    Ok(out)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SwapParams {
    pub instance_ids: Vec<ElementRef>,
    pub group_type_id: String,
}

/// `group.swap_type`: the instances become another type, at their origins.
pub(crate) fn swap_type(txn: &mut Transaction<'_>, p: SwapParams) -> Result<()> {
    let store = txn.store().clone();
    let t = group_type(&store, &p.group_type_id)?;
    for r in &p.instance_ids {
        let (id, mut g) = instance(txn.store(), r)?;
        g.excluded.clear();
        write_instance(txn, id, &g)?;
        remake(txn, id, &t)?;
        txn.modify(id, |e| {
            e.name = Some(t.name.clone());
            e.params
                .insert("@ObjectType".into(), Value::from(t.name.as_str()));
        })?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeNameParams {
    pub group_type_id: String,
    pub name: String,
}

/// `group.duplicate_type`: its key.
pub(crate) fn duplicate_type(txn: &mut Transaction<'_>, p: TypeNameParams) -> Result<String> {
    let mut list = types(txn.store());
    let mut t = group_type(txn.store(), &p.group_type_id)?;
    if list.iter().any(|x| x.name == p.name.trim()) {
        return Err(Error::new("group.name", "that name is taken"));
    }
    t.id = GlobalId::new().to_string();
    t.name = p.name.trim().to_owned();
    let id = t.id.clone();
    list.push(t);
    write_types(txn, &list)?;
    Ok(id)
}

/// `group.rename_type`.
pub(crate) fn rename_type(txn: &mut Transaction<'_>, p: TypeNameParams) -> Result<()> {
    let mut list = types(txn.store());
    let t = group_type(txn.store(), &p.group_type_id)?;
    if list.iter().any(|x| x.name == p.name.trim() && x.id != t.id) {
        return Err(Error::new("group.name", "that name is taken"));
    }
    for x in list.iter_mut().filter(|x| x.id == t.id) {
        x.name = p.name.trim().to_owned();
    }
    write_types(txn, &list)?;
    let store = txn.store().clone();
    for e in store
        .iter()
        .filter(|e| recipe(e).is_some_and(|g| g.group_type == t.id))
    {
        txn.modify(e.id(), |el| {
            el.name = Some(p.name.trim().to_owned());
            el.params
                .insert("@ObjectType".into(), Value::from(p.name.trim()));
        })?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstanceParams {
    pub instance_id: ElementRef,
}

/// `group.edit`: opens the group editor on an instance; its members can be changed.
pub(crate) fn edit(txn: &mut Transaction<'_>, p: InstanceParams) -> Result<()> {
    let store = txn.store().clone();
    if editing(&store).is_some() {
        return Err(Error::new(
            "group.editing",
            "a group is already being edited; finish or cancel it first",
        ));
    }
    let (id, _) = instance(&store, &p.instance_id)?;
    let guid = store.get(id).expect("resolved").guid().to_string();
    set_project(txn, EDIT_KEY, Value::from(guid))
}

fn open_instance(store: &Store) -> Result<(ElementId, GroupRecipe)> {
    let guid = editing(store)
        .ok_or_else(|| Error::new("group.not_editing", "no group is being edited"))?;
    let id = find(store, &guid)
        .ok_or_else(|| Error::new("group.not_editing", "the edited group is gone"))?;
    Ok((id, recipe(store.get(id).expect("found")).expect("a group")))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MembersParams {
    pub element_ids: Vec<ElementRef>,
}

/// `group.add_members`: elements join the group being edited.
pub(crate) fn add_members(txn: &mut Transaction<'_>, p: MembersParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut g) = open_instance(&store)?;
    let guid = store.get(id).expect("found").guid().to_string();
    let mut next = g.members.iter().map(|m| m.key + 1).max().unwrap_or(0);
    let t = group_type(&store, &g.group_type)?;
    next = next.max(t.template.iter().map(|m| m.key + 1).max().unwrap_or(0));
    let mut ids = Vec::new();
    for r in &p.element_ids {
        ids.push(
            store
                .resolve(r)
                .ok_or_else(|| Error::new("element.not_found", format!("no element {r:?}")))?,
        );
    }
    for mid in with_hosted(&store, &ids) {
        let e = store.get(mid).expect("resolved");
        if instance_of(e).is_some() || recipe(e).is_some() {
            continue;
        }
        txn.modify(mid, |el| {
            el.params
                .insert(MEMBER_KEY.into(), Value::from(guid.as_str()));
        })?;
        g.members.push(Member {
            key: next,
            guid: e.guid().to_string(),
        });
        next += 1;
    }
    write_instance(txn, id, &g)
}

/// `group.remove_members`: members of the group being edited leave it (they stay as elements).
pub(crate) fn remove_members(txn: &mut Transaction<'_>, p: MembersParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut g) = open_instance(&store)?;
    for r in &p.element_ids {
        let Some(mid) = store.resolve(r) else {
            continue;
        };
        let guid = store.get(mid).expect("resolved").guid().to_string();
        if g.members.iter().any(|m| m.guid == guid) {
            g.members.retain(|m| m.guid != guid);
            g.removed.push(guid.clone());
            txn.modify(mid, |e| {
                e.params.remove(MEMBER_KEY);
            })?;
        }
    }
    write_instance(txn, id, &g)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

/// `group.finish`: the edited instance becomes the template; every other instance of the type
/// is remade from it.
pub(crate) fn finish(txn: &mut Transaction<'_>, _: NoParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut g) = open_instance(&store)?;
    if !g.removed.is_empty() {
        g.removed.clear();
        write_instance(txn, id, &g)?;
    }
    let mut list = types(&store);
    let members: Vec<(u32, ElementId)> = g
        .members
        .iter()
        .filter_map(|m| Some((m.key, find(&store, &m.guid)?)))
        .collect();
    let mut t = group_type(&store, &g.group_type)?;
    // Members excluded from this instance stay in the template as they were.
    let kept: Vec<TemplateMember> = t
        .template
        .iter()
        .filter(|m| g.excluded.contains(&m.key))
        .cloned()
        .collect();
    t.template = snapshot(&store, &members);
    t.template.extend(kept);
    t.origin = g.origin;
    t.rotation = g.rotation;
    for x in list.iter_mut().filter(|x| x.id == t.id) {
        *x = t.clone();
    }
    write_types(txn, &list)?;
    set_project(txn, EDIT_KEY, Value::Null)?;
    let others: Vec<ElementId> = store
        .iter()
        .filter(|e| e.id() != id && recipe(e).is_some_and(|x| x.group_type == t.id))
        .map(Element::id)
        .collect();
    for o in others {
        remake(txn, o, &t)?;
    }
    Ok(())
}

/// `group.cancel`: the edited instance is remade from the template; nothing else changes.
pub(crate) fn cancel(txn: &mut Transaction<'_>, _: NoParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut g) = open_instance(&store)?;
    let t = group_type(&store, &g.group_type)?;
    // Elements added in the editor go back to being independent; members removed in it (now
    // independent) go, as the instance is remade with them.
    let keys: Vec<u32> = t.template.iter().map(|m| m.key).collect();
    for m in g.members.iter().filter(|m| !keys.contains(&m.key)) {
        if let Some(mid) = find(&store, &m.guid) {
            txn.modify(mid, |e| {
                e.params.remove(MEMBER_KEY);
            })?;
        }
    }
    for guid in &g.removed {
        if let Some(mid) = find(&store, guid) {
            txn.delete(mid)?;
        }
    }
    g.members.retain(|m| keys.contains(&m.key));
    g.removed.clear();
    write_instance(txn, id, &g)?;
    set_project(txn, EDIT_KEY, Value::Null)?;
    remake(txn, id, &t)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemberParams {
    pub instance_id: ElementRef,
    /// The member: its element (ID or GlobalId) …
    pub element_id: Option<ElementRef>,
    /// … or its key.
    pub member_key: Option<u32>,
}

fn member_key(store: &Store, g: &GroupRecipe, p: &MemberParams) -> Result<u32> {
    if let Some(k) = p.member_key {
        return Ok(k);
    }
    let r = p
        .element_id
        .as_ref()
        .ok_or_else(|| Error::new("group.member", "give the member's element or key"))?;
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such element"))?;
    let guid = store.get(id).expect("resolved").guid().to_string();
    g.members
        .iter()
        .find(|m| m.guid == guid)
        .map(|m| m.key)
        .ok_or_else(|| Error::new("group.member", "the element is not a member of the group"))
}

/// `group.exclude_member`: the member is gone from this instance only.
pub(crate) fn exclude_member(txn: &mut Transaction<'_>, p: MemberParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut g) = instance(&store, &p.instance_id)?;
    let key = member_key(&store, &g, &p)?;
    if let Some(m) = g.members.iter().find(|m| m.key == key)
        && let Some(mid) = find(&store, &m.guid)
    {
        txn.delete(mid)?;
    }
    g.members.retain(|m| m.key != key);
    if !g.excluded.contains(&key) {
        g.excluded.push(key);
    }
    write_instance(txn, id, &g)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreParams {
    pub instance_id: ElementRef,
    /// One member's key; omitted, every excluded member.
    pub member_key: Option<u32>,
}

/// `group.restore_member`: excluded members come back.
pub(crate) fn restore_member(txn: &mut Transaction<'_>, p: RestoreParams) -> Result<()> {
    let store = txn.store().clone();
    let (id, mut g) = instance(&store, &p.instance_id)?;
    let t = group_type(&store, &g.group_type)?;
    let keys: Vec<u32> = g
        .excluded
        .iter()
        .copied()
        .filter(|k| p.member_key.is_none_or(|m| m == *k))
        .collect();
    let guid = store.get(id).expect("resolved").guid().to_string();
    let made = make_members(txn, &t, &keys, g.origin, g.rotation, &guid, &g.members)?;
    g.excluded.retain(|k| !keys.contains(k));
    g.members.extend(made);
    write_instance(txn, id, &g)
}

/// `group.move_member_to_project`: the member leaves this instance as an independent element.
pub(crate) fn move_member_to_project(
    txn: &mut Transaction<'_>,
    p: MemberParams,
) -> Result<ElementId> {
    let store = txn.store().clone();
    let (id, mut g) = instance(&store, &p.instance_id)?;
    let key = member_key(&store, &g, &p)?;
    let m = g
        .members
        .iter()
        .find(|m| m.key == key)
        .cloned()
        .ok_or_else(|| Error::new("group.member", "the member is excluded"))?;
    let mid =
        find(&store, &m.guid).ok_or_else(|| Error::new("group.member", "the member is gone"))?;
    txn.modify(mid, |e| {
        e.params.remove(MEMBER_KEY);
    })?;
    g.members.retain(|x| x.key != key);
    g.excluded.push(key);
    write_instance(txn, id, &g)?;
    Ok(mid)
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct TypeInfo {
    pub id: String,
    pub name: String,
    pub members: usize,
    pub instances: Vec<ElementId>,
}

/// `group.list`.
pub(crate) fn list(store: &Store, _: NoParams) -> Result<Vec<TypeInfo>> {
    Ok(types(store)
        .into_iter()
        .map(|t| TypeInfo {
            members: t.template.len(),
            instances: store
                .iter()
                .filter(|e| recipe(e).is_some_and(|g| g.group_type == t.id))
                .map(Element::id)
                .collect(),
            id: t.id,
            name: t.name,
        })
        .collect())
}
