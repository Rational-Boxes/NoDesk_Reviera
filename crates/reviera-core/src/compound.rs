// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Compound structures (ARC-CMPD-001): the layers of a wall type.
//!
//! A layered wall type keeps its layers in its `recipe.wall_type` (`layers`, exterior first, and
//! `core`, the layer range of the core). Its width is the sum of its layers' thicknesses, so a
//! wall of the type is as thick as its layers; the wall's location line stays the wall's
//! centreline. Each layer has a function, a priority (1 the highest; membranes 0), a material
//! (by name until the material library exists), a thickness and whether it wraps.
//!
//! Validation follows the specification: non-membrane layers are at least 3.2 mm thick and
//! membranes have none; core layers have thickness; on each side of the core, walking out from
//! it, priorities never get stronger (their numbers never fall), membranes aside; at most one
//! layer is variable and at most one carries the structural material, in the core.
//!
//! In IFC the type has an `IfcMaterialLayerSet` of `IfcMaterialLayer`s (Name = the function,
//! Category, Priority = 100 − 10·(p − 1), IsVentilated), and each wall an
//! `IfcMaterialLayerSetUsage` along its axis.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::element::Element;
use crate::error::{Error, Result};
use crate::id::{ElementId, ElementRef};
use crate::store::Store;
use crate::walls::{self, RECIPE_KEY};

/// The thinnest layer that is not a membrane (1/8", §3.0).
pub const MIN_LAYER: f64 = 0.0032;

/// What a layer does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Function {
    Structure,
    Substrate,
    ThermalAir,
    Finish1,
    Finish2,
    Membrane,
}

impl Function {
    pub const ALL: [Function; 6] = [
        Function::Structure,
        Function::Substrate,
        Function::ThermalAir,
        Function::Finish1,
        Function::Finish2,
        Function::Membrane,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Function::Structure => "Structure [1]",
            Function::Substrate => "Substrate [2]",
            Function::ThermalAir => "Thermal/Air [3]",
            Function::Finish1 => "Finish 1 [4]",
            Function::Finish2 => "Finish 2 [5]",
            Function::Membrane => "Membrane Layer",
        }
    }

    /// The function's name in IFC (`IfcMaterialLayer.Name`).
    pub fn name(self) -> &'static str {
        match self {
            Function::Structure => "Structure",
            Function::Substrate => "Substrate",
            Function::ThermalAir => "Thermal/Air",
            Function::Finish1 => "Finish 1",
            Function::Finish2 => "Finish 2",
            Function::Membrane => "Membrane",
        }
    }

    /// `IfcMaterialLayer.Category`.
    pub fn category(self) -> &'static str {
        match self {
            Function::Structure => "LoadBearing",
            Function::Substrate => "Substrate",
            Function::ThermalAir => "Insulation",
            Function::Finish1 | Function::Finish2 => "Finish",
            Function::Membrane => "Membrane",
        }
    }

    /// The priority a new layer of this function gets.
    pub fn default_priority(self) -> u8 {
        match self {
            Function::Structure => 1,
            Function::Substrate => 2,
            Function::ThermalAir => 3,
            Function::Finish1 => 4,
            Function::Finish2 => 5,
            Function::Membrane => 0,
        }
    }

    /// From its label, name or snake_case key.
    pub fn parse(s: &str) -> Option<Function> {
        let s = s.trim();
        Function::ALL.into_iter().find(|f| {
            f.label().eq_ignore_ascii_case(s)
                || f.name().eq_ignore_ascii_case(s)
                || serde_json::to_value(f)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    == Some(s.to_owned())
        })
    }
}

/// One layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Layer {
    pub function: Function,
    /// 1 (highest) to 9; omitted, the function's default. Membranes are 0.
    #[serde(default)]
    pub priority: Option<u8>,
    /// The material's name.
    #[serde(default)]
    pub material: String,
    /// Metres (accepts units in commands).
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub thickness: f64,
    #[serde(default)]
    pub wraps: bool,
    #[serde(default)]
    pub structural_material: bool,
    #[serde(default)]
    pub variable: bool,
}

impl Layer {
    pub fn priority(&self) -> u8 {
        if self.function == Function::Membrane {
            0
        } else {
            self.priority.unwrap_or(self.function.default_priority())
        }
    }
}

/// A compound structure: the layers, exterior first, and the core's layer range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Compound {
    pub layers: Vec<Layer>,
    /// The core: layers `core[0]..core[1]` (may be empty).
    pub core: [usize; 2],
}

impl Compound {
    pub fn thickness(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness).sum()
    }

    /// The offsets of the boundaries between layers from the exterior face, interior faces
    /// included: `n + 1` values from 0 to the thickness.
    pub fn boundaries(&self) -> Vec<f64> {
        let mut out = vec![0.0];
        let mut d = 0.0;
        for l in &self.layers {
            d += l.thickness;
            out.push(d);
        }
        out
    }

    /// A single layer of `width` (a type without layers of its own).
    pub fn single(width: f64) -> Compound {
        Compound {
            layers: vec![Layer {
                function: Function::Structure,
                priority: None,
                material: String::new(),
                thickness: width,
                wraps: false,
                structural_material: true,
                variable: false,
            }],
            core: [0, 1],
        }
    }

    /// Checks the rules of ARC-CMPD-001.
    pub fn validate(&self) -> Result<()> {
        let bad = |msg: String| Err(Error::new("compound.invalid", msg));
        let n = self.layers.len();
        if n == 0 {
            return bad("a compound structure needs at least one layer".into());
        }
        let [cs, ce] = self.core;
        if cs > ce || ce > n {
            return bad(format!("the core {cs}..{ce} is outside the {n} layers"));
        }
        let name = |i: usize| format!("layer {} ({})", i + 1, self.layers[i].function.name());
        for (i, l) in self.layers.iter().enumerate() {
            if !l.thickness.is_finite() {
                return bad(format!("{} has no thickness", name(i)));
            }
            if l.function == Function::Membrane {
                if l.thickness != 0.0 {
                    return bad(format!(
                        "{} is a membrane, so its thickness must be 0",
                        name(i)
                    ));
                }
            } else if l.thickness < MIN_LAYER - 1e-9 {
                return bad(format!("{} must be at least 3.2 mm thick", name(i)));
            }
            if (cs..ce).contains(&i) && l.thickness <= 0.0 {
                return bad(format!(
                    "{} is in the core, so it needs a thickness",
                    name(i)
                ));
            }
            if l.function != Function::Membrane && !(1..=9).contains(&l.priority()) {
                return bad(format!("{}: priorities run from 1 to 9", name(i)));
            }
        }
        // Walking out from the core, priority numbers never fall (membranes aside).
        let check = |order: &mut dyn Iterator<Item = usize>| -> Result<()> {
            let mut last: Option<usize> = None;
            for i in order {
                let l = &self.layers[i];
                if l.function == Function::Membrane {
                    continue;
                }
                if let Some(j) = last
                    && l.priority() < self.layers[j].priority()
                {
                    return bad(format!(
                        "{} (priority {}) is outside {} (priority {}): priorities must not rise away from the core",
                        name(i),
                        l.priority(),
                        name(j),
                        self.layers[j].priority()
                    ));
                }
                last = Some(i);
            }
            Ok(())
        };
        check(&mut (0..cs).rev())?;
        check(&mut (ce..n))?;
        if self.layers.iter().filter(|l| l.variable).count() > 1 {
            return bad("at most one layer can be variable".into());
        }
        let structural: Vec<usize> = (0..n)
            .filter(|i| self.layers[*i].structural_material)
            .collect();
        if structural.len() > 1 {
            return bad("at most one layer can carry the structural material".into());
        }
        if let Some(&i) = structural.first()
            && !(cs..ce).contains(&i)
        {
            return bad(format!(
                "{} carries the structural material, so it must be in the core",
                name(i)
            ));
        }
        if self.thickness() <= 0.0 {
            return bad("the layers must have some thickness".into());
        }
        Ok(())
    }
}

/// A wall type's compound structure: its own layers, else one layer of its width.
pub fn of_wall_type(t: &Element) -> Option<Compound> {
    let v = t.param(RECIPE_KEY)?;
    if v.get("kind")?.as_str()? != walls::WALL_TYPE_RECIPE {
        return None;
    }
    if let Some(c) = v
        .get("compound")
        .and_then(|c| serde_json::from_value(c.clone()).ok())
    {
        return Some(c);
    }
    Some(Compound::single(v.get("width")?.as_f64()?))
}

/// Whether a wall type has layers of its own.
pub fn is_layered(t: &Element) -> bool {
    t.param(RECIPE_KEY)
        .and_then(|v| v.get("compound"))
        .is_some_and(|c| !c.is_null())
}

fn wall_type(store: &Store, r: &ElementRef) -> Result<ElementId> {
    let id = store
        .resolve(r)
        .ok_or_else(|| Error::new("element.not_found", "no such wall type"))?;
    if walls::type_width(store.get(id).expect("resolved")).is_none() {
        return Err(Error::new(
            "compound.not_wall_type",
            format!("element {id} is not a Reviera wall type"),
        ));
    }
    Ok(id)
}

// ---- Commands ----

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetParams {
    pub type_id: ElementRef,
}

/// A type's layers, its core and its total thickness (`compound.get`).
#[derive(Debug, Serialize, JsonSchema)]
pub struct CompoundReport {
    pub layers: Vec<Layer>,
    pub core: [usize; 2],
    pub thickness: f64,
    /// Whether the type has layers of its own (else one layer of its width).
    pub layered: bool,
}

pub(crate) fn get(store: &Store, p: GetParams) -> Result<CompoundReport> {
    let id = wall_type(store, &p.type_id)?;
    let t = store.get(id).expect("resolved");
    let c = of_wall_type(t).expect("a wall type");
    Ok(CompoundReport {
        thickness: c.thickness(),
        layered: is_layered(t),
        layers: c.layers,
        core: c.core,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLayersParams {
    pub type_id: ElementRef,
    /// Exterior first.
    pub layers: Vec<LayerArg>,
    /// The core: layers `core_start..core_end`; omitted, the Structure layers' run.
    pub core_start: Option<usize>,
    pub core_end: Option<usize>,
}

/// A layer argument: as [`Layer`], with the thickness accepting units.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayerArg {
    pub function: String,
    pub priority: Option<u8>,
    #[serde(default)]
    pub material: String,
    #[schemars(extend("x-reviera-data-type" = "length"))]
    pub thickness: Value,
    #[serde(default)]
    pub wraps: bool,
    #[serde(default)]
    pub structural_material: bool,
    #[serde(default)]
    pub variable: bool,
}

/// Gives a wall type its layers (`compound.set_layers`): its width becomes their total, and
/// every wall of the type regenerates.
pub(crate) fn set_layers(txn: &mut crate::txn::Transaction<'_>, p: SetLayersParams) -> Result<()> {
    let store = txn.store().clone();
    let id = wall_type(&store, &p.type_id)?;
    if crate::walls::stacked::of_type(store.get(id).expect("resolved")).is_some() {
        return Err(Error::new(
            "compound.stacked",
            "a stacked wall type has rows, not layers (stackedwall.set_rows)",
        ));
    }
    let settings = crate::units::settings_of(&store);
    let mut layers = Vec::new();
    for (i, a) in p.layers.iter().enumerate() {
        let function = Function::parse(&a.function).ok_or_else(|| {
            Error::new(
                "compound.bad_function",
                format!(
                    "layer {}: `{}` is not a function (structure, substrate, thermal_air, finish1, finish2, membrane)",
                    i + 1,
                    a.function
                ),
            )
        })?;
        let thickness = match &a.thickness {
            Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
            Value::String(s) => crate::units::parse(s, "length", &settings)?,
            _ => f64::NAN,
        };
        layers.push(Layer {
            function,
            priority: a.priority,
            material: a.material.trim().to_owned(),
            thickness,
            wraps: a.wraps,
            structural_material: a.structural_material,
            variable: a.variable,
        });
    }
    let structure: Vec<usize> = (0..layers.len())
        .filter(|i| layers[*i].function == Function::Structure)
        .collect();
    let core = [
        p.core_start
            .unwrap_or_else(|| structure.first().copied().unwrap_or(0)),
        p.core_end
            .unwrap_or_else(|| structure.last().map_or(0, |i| i + 1)),
    ];
    let c = Compound { layers, core };
    c.validate()?;
    let mut recipe = store
        .get(id)
        .and_then(|t| t.param(RECIPE_KEY))
        .cloned()
        .expect("a wall type recipe");
    recipe["width"] = c.thickness().into();
    recipe["compound"] = serde_json::to_value(&c).expect("serialises");
    txn.modify(id, |e| {
        e.params.insert(RECIPE_KEY.into(), recipe);
    })?;
    crate::walls::stacked::refresh_widths(txn)?;
    // Every wall of the type must still regenerate (and its doors and windows fit).
    let store = txn.store().clone();
    let guid = store.get(id).expect("exists").guid().to_string();
    for w in store.iter() {
        if walls::wall_recipe(w).is_some()
            && w.param("@type").and_then(Value::as_str) == Some(guid.as_str())
        {
            walls::regenerate(&store, w)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(f: Function, t: f64) -> Layer {
        Layer {
            function: f,
            priority: None,
            material: String::new(),
            thickness: t,
            wraps: false,
            structural_material: false,
            variable: false,
        }
    }

    #[test]
    fn validation_follows_the_rules() {
        let mut c = Compound {
            layers: vec![
                layer(Function::Finish1, 0.02),
                layer(Function::ThermalAir, 0.1),
                layer(Function::Membrane, 0.0),
                layer(Function::Structure, 0.2),
                layer(Function::Finish2, 0.013),
            ],
            core: [3, 4],
        };
        c.validate().unwrap();
        assert!((c.thickness() - 0.333).abs() < 1e-9);
        assert_eq!(c.boundaries().len(), 6);
        // A Finish at priority 1 outside a priority-3 layer.
        c.layers[0].priority = Some(1);
        let e = c.validate().unwrap_err();
        assert!(
            e.message.contains("layer 1") && e.message.contains("layer 2"),
            "{e:?}"
        );
        c.layers[0].priority = None;
        // A membrane with thickness, a thin layer.
        c.layers[2].thickness = 0.01;
        assert!(c.validate().is_err());
        c.layers[2].thickness = 0.0;
        c.layers[4].thickness = 0.001;
        assert!(c.validate().is_err());
        c.layers[4].thickness = 0.013;
        // An empty core is allowed.
        c.core = [2, 2];
        c.validate().unwrap();
        assert_eq!(Function::parse("Thermal/Air"), Some(Function::ThermalAir));
        assert_eq!(Function::parse("finish1"), Some(Function::Finish1));
    }
}
