// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Property-set commands (spec Ch 2 §2.3.3): `pset.set` and `pset.remove`. They edit element
//! parameters; saving writes the values to IFC property sets.

use reviera_core::{
    Command, CommandContext, CommandMeta, CommandRegistry, Effect, ElementId, ElementRef, Error,
    Result,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::params::{category, infer_type, is_writable};

pub fn register(registry: &mut CommandRegistry) {
    registry.add(PsetSet).expect("pset.set is valid");
    registry.add(PsetRemove).expect("pset.remove is valid");
}

fn resolve(cx: &CommandContext<'_>, r: &ElementRef) -> Result<ElementId> {
    let store = cx.store();
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such element"))?;
    let class = &store.get(id).expect("resolved").class;
    if !class.starts_with("Ifc") {
        return Err(Error::new(
            "pset.not_ifc",
            format!("{class} elements have no IFC property sets"),
        ));
    }
    Ok(id)
}

fn check_set_name(name: &str) -> Result<()> {
    if name.starts_with("Reviera_") {
        return Err(Error::new(
            "pset.reserved",
            "`Reviera_*` property sets hold Reviera's own data",
        ));
    }
    if name.is_empty() || name.contains('.') || name.starts_with('@') {
        return Err(Error::new(
            "pset.invalid_name",
            "property set names must be non-empty, without `.` or a leading `@`",
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetParams {
    pub element_id: ElementRef,
    pub pset_name: String,
    pub property_name: String,
    /// Text, number, boolean or null.
    pub value: Value,
    /// IFC measure type, for example `IfcLabel` or `IfcLengthMeasure`. Defaults to the property's
    /// current type, else one inferred from the value.
    pub ifc_type: Option<String>,
}

struct PsetSet;

impl Command for PsetSet {
    type Params = SetParams;
    type Output = ();
    const META: CommandMeta = CommandMeta {
        name: "pset.set",
        version: 1,
        summary: "Set a property value",
        description: "Sets a property in an element's property set; it is written to IFC on save. \
                      Quantity sets are computed and cannot be set.",
        effects: &[Effect::Edit],
        transaction_name: "Modify property",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: SetParams) -> Result<()> {
        let id = resolve(cx, &p.element_id)?;
        check_set_name(&p.pset_name)?;
        let key = format!("{}.{}", p.pset_name, p.property_name);
        let current = cx.store().get(id).and_then(|e| e.param(&key)).cloned();
        if current.as_ref().is_some_and(|c| !is_writable(c)) {
            return Err(Error::new(
                "pset.read_only",
                format!("`{key}` is a quantity or a property kind that cannot be set"),
            ));
        }
        if !matches!(
            p.value,
            Value::Null | Value::String(_) | Value::Number(_) | Value::Bool(_)
        ) {
            return Err(Error::new(
                "args.invalid",
                "value must be text, a number, a boolean or null",
            ));
        }
        let ifc_type = p
            .ifc_type
            .clone()
            .or_else(|| {
                current
                    .as_ref()
                    .and_then(|c| c.get("type"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| infer_type(&p.value).map(str::to_owned))
            .ifc_type_or_label();
        if !ifc_type.starts_with("Ifc") {
            return Err(Error::new(
                "args.invalid",
                format!("`{ifc_type}` is not an IFC type"),
            ));
        }
        let fits = match (category(&ifc_type), &p.value) {
            (_, Value::Null) => true,
            ("string", Value::String(_))
            | ("boolean", Value::Bool(_))
            | ("number", Value::Number(_)) => true,
            ("integer", Value::Number(n)) => n.is_i64(),
            _ => false,
        };
        if !fits {
            return Err(Error::new(
                "args.invalid",
                format!("{} does not fit {ifc_type}", p.value),
            ));
        }
        let value = json!({ "type": ifc_type, "value": p.value });
        cx.edit()?.modify(id, |e| {
            e.params.insert(key, value);
        })
    }
}

trait OrLabel {
    fn ifc_type_or_label(self) -> String;
}

impl OrLabel for Option<String> {
    /// A null value with no known type is stored as a text property.
    fn ifc_type_or_label(self) -> String {
        self.unwrap_or_else(|| "IfcLabel".into())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveParams {
    pub element_id: ElementRef,
    pub pset_name: String,
    /// Omit to remove the whole property set.
    pub property_name: Option<String>,
}

struct PsetRemove;

impl Command for PsetRemove {
    type Params = RemoveParams;
    type Output = Vec<String>;
    const META: CommandMeta = CommandMeta {
        name: "pset.remove",
        version: 1,
        summary: "Remove a property or property set",
        description: "Removes one property, or every property of the set; returns the removed keys.",
        effects: &[Effect::Edit],
        transaction_name: "Remove property",
    };

    fn run(&self, cx: &mut CommandContext<'_>, p: RemoveParams) -> Result<Vec<String>> {
        let id = resolve(cx, &p.element_id)?;
        check_set_name(&p.pset_name)?;
        let prefix = format!("{}.", p.pset_name);
        let element = cx.store().get(id).expect("resolved");
        let keys: Vec<String> = element
            .params
            .iter()
            .filter(|(k, _)| match &p.property_name {
                Some(prop) => **k == format!("{prefix}{prop}"),
                None => k.starts_with(&prefix),
            })
            .map(|(k, _)| k.clone())
            .collect();
        if keys.is_empty() {
            return Err(Error::new("pset.not_found", "no such property"));
        }
        if keys.iter().any(|k| !is_writable(&element.params[k])) {
            return Err(Error::new(
                "pset.read_only",
                "quantities and read-only properties cannot be removed",
            ));
        }
        cx.edit()?.modify(id, |e| {
            for k in &keys {
                e.params.remove(k);
            }
        })?;
        Ok(keys)
    }
}
