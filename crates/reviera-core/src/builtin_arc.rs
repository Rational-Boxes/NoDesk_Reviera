// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Architecture commands added for the first pre-release: wall openings and shafts
//! (ARC-OPEN-003, ARC-OPEN-004), and the tools after them. Registered with the core commands
//! by [`crate::builtin::register`].

use crate::command::{Command, CommandContext, CommandMeta, CommandRegistry, Effect};
use crate::error::Result;
use crate::id::ElementId;

/// A command that edits the model through one function of a transaction.
macro_rules! edit_command {
    ($ty:ident, $name:literal, $summary:literal, $desc:literal, $txn:literal, $params:ty, $out:ty, $f:expr) => {
        #[derive(Debug)]
        struct $ty;

        impl Command for $ty {
            type Params = $params;
            type Output = $out;
            const META: CommandMeta = CommandMeta {
                name: $name,
                version: 1,
                summary: $summary,
                description: $desc,
                effects: &[Effect::Edit],
                transaction_name: $txn,
            };

            fn run(&self, cx: &mut CommandContext<'_>, p: $params) -> Result<$out> {
                let f: fn(&mut crate::txn::Transaction<'_>, $params) -> Result<$out> = $f;
                f(cx.edit()?, p)
            }
        }
    };
}

pub(crate) fn register(registry: &mut CommandRegistry) {
    let results = [
        registry.add(OpeningCreateWall),
        registry.add(OpeningEditWall),
        registry.add(OpeningCreateShaft),
        registry.add(OpeningEditShaft),
        registry.add(ColumnPlace),
        registry.add(ColumnSet),
        registry.add(ColumnAttach),
        registry.add(ColumnDetach),
        registry.add(ElementArrayLinear),
        registry.add(ElementArrayRadial),
        registry.add(CompoundGet),
        registry.add(CompoundSetLayers),
        registry.add(CurtainCreate),
        registry.add(CurtainSet),
    ];
    for r in results {
        r.expect("architecture commands are valid");
    }
}

edit_command!(
    OpeningCreateWall,
    "opening.create_wall",
    "Cut a rectangular opening through a wall",
    "Cuts a plain rectangular opening through a Reviera wall, straight or curved (ARC-WALL-012, \
     ARC-OPEN-003): from `u0` to `u1` along its location curve from its start, from its base level (the \
     wall's by default) and offset to a top level and offset or a height (2.1 m by default). It goes \
     through the whole thickness, square to the curve; it follows its levels, slides along the wall, \
     moves and is copied with the wall, and goes with it. It may not overlap the wall's doors, windows or \
     other openings. Written as an IfcOpeningElement voiding the wall. Returns the opening.",
    "Wall Opening",
    crate::wall_openings::CreateParams,
    ElementId,
    crate::wall_openings::create
);

edit_command!(
    OpeningEditWall,
    "opening.edit_wall",
    "Change a wall opening",
    "Changes a wall opening's span along its wall (`u0`, `u1`) or its extent (base level and offset, top \
     constraint and offset); omitted values are kept.",
    "Edit Wall Opening",
    crate::wall_openings::EditParams,
    (),
    crate::wall_openings::edit
);

edit_command!(
    OpeningCreateShaft,
    "opening.create_shaft",
    "Make a shaft through several levels",
    "Makes a shaft (ARC-OPEN-004): closed `loops` in plan (corners), or sketch `curves`, from a base level \
     and offset to a top level and offset or a height. It cuts every Reviera floor, ceiling and footprint \
     roof its extent reaches into, plumb, including ones added later; walls are not cut. In IFC it is an \
     IfcBuildingElementProxy (PROVISIONFORVOID) with each host voided by its own IfcOpeningElement. \
     Returns the shaft.",
    "Shaft Opening",
    crate::slab_openings::CreateShaftParams,
    ElementId,
    crate::slab_openings::create_shaft
);

edit_command!(
    OpeningEditShaft,
    "opening.edit_shaft",
    "Change a shaft",
    "Changes a shaft's loops or extent (base level and offset, top constraint and offset); omitted values \
     are kept.",
    "Edit Shaft",
    crate::slab_openings::EditShaftParams,
    (),
    crate::slab_openings::edit_shaft
);

edit_command!(
    ColumnPlace,
    "column.place_architectural",
    "Place an architectural column",
    "Places a non-structural column (ARC-COL-001) of a column type (the default 400 × 400 mm type when \
     omitted) centred at `point`, rotated by `rotation`, from a base level and offset to a top level \
     and offset or a height. Written as an IfcColumn (Pset_ColumnCommon.LoadBearing false). Returns \
     the column.",
    "Column",
    crate::columns::PlaceParams,
    ElementId,
    crate::columns::place
);

edit_command!(
    ColumnSet,
    "column.set",
    "Change columns",
    "Changes columns' centre, rotation, base level and offset, or top constraint and offset; omitted \
     values are kept.",
    "Edit Column",
    crate::columns::SetParams,
    (),
    crate::columns::set
);

edit_command!(
    ColumnAttach,
    "column.attach",
    "Attach columns' tops",
    "Attaches columns' tops to a Reviera roof, floor or ceiling above them (ARC-COL-001): the top \
     follows the target's underside over the column's section at its lowest point (`minimum`), middle \
     (`midline`) or highest point (`maximum`, the default), plus `offset`; with `maximum` and \
     `midline` the top is cut by the underside.",
    "Attach Column",
    crate::columns::AttachParams,
    (),
    crate::columns::attach
);

edit_command!(
    ColumnDetach,
    "column.detach",
    "Detach columns' tops",
    "Detaches columns' tops: they go back to their top constraint.",
    "Detach Column",
    crate::columns::DetachParams,
    (),
    crate::columns::detach
);

edit_command!(
    ElementArrayLinear,
    "element.array_linear",
    "Array elements in a row",
    "Copies elements into a linear array (ARC-MOD-026): `count` members including the original, the \
     `vector` reaching the second member (`move_to: second`) or the last (`move_to: last`). The members \
     are independent copies (Group and Associate is not available yet). Returns the copies, member by \
     member.",
    "Array",
    crate::modify::ArrayLinearParams,
    Vec<Vec<ElementId>>,
    crate::modify::array_linear
);

edit_command!(
    ElementArrayRadial,
    "element.array_radial",
    "Array elements round a centre",
    "Copies elements into a radial array (ARC-MOD-026) round `center`: `count` members including the \
     original, `angle` reaching the second member (`second`) or the last (`last`; a full turn spaces \
     them evenly without doubling the original). Doors and windows cannot be rotated off their wall. \
     Returns the copies, member by member.",
    "Radial Array",
    crate::modify::ArrayRadialParams,
    Vec<Vec<ElementId>>,
    crate::modify::array_radial
);

#[derive(Debug)]
struct CompoundGet;

impl Command for CompoundGet {
    type Params = crate::compound::GetParams;
    type Output = crate::compound::CompoundReport;
    const META: CommandMeta = CommandMeta {
        name: "compound.get",
        version: 1,
        summary: "A wall type's layers",
        description: "A wall type's compound structure (ARC-CMPD-001): its layers, exterior first (function, \
                      priority, material, thickness, wraps), its core and its total thickness. A type without \
                      layers of its own reads as one Structure layer of its width.",
        effects: &[Effect::Read],
        transaction_name: "",
    };

    fn run(
        &self,
        cx: &mut CommandContext<'_>,
        p: crate::compound::GetParams,
    ) -> Result<crate::compound::CompoundReport> {
        crate::compound::get(cx.store(), p)
    }
}

edit_command!(
    CompoundSetLayers,
    "compound.set_layers",
    "Set a wall type's layers",
    "Gives a wall type its layers (ARC-CMPD-001), exterior first: each a `function` (structure, substrate, \
     thermal_air, finish1, finish2, membrane), an optional `priority` (1 highest … 9; the function's \
     default), a `material` name, a `thickness` (accepts units), and `wraps`, `structural_material` and \
     `variable` flags; and the core `core_start..core_end` (by default the Structure layers). They are \
     validated (thicknesses, priorities not rising away from the core, one variable and one structural \
     layer at most). The type's width becomes their total and its walls regenerate; IFC gets an \
     IfcMaterialLayerSet.",
    "Edit Assembly",
    crate::compound::SetLayersParams,
    (),
    crate::compound::set_layers
);

edit_command!(
    CurtainCreate,
    "curtain.create",
    "Make a curtain wall",
    "Makes a curtain wall (ARC-CW-001/002) along a straight base line, from a base level and offset to a \
     top level and offset or a height, of a curtain wall type (the default Storefront when omitted: \
     vertical grid lines at most 1.5 m apart, a transom at 2.4 m, 50 × 150 mm mullions, glazed panels). \
     Drawn along a basic wall, it cuts its opening through it (ARC-WALL-013). IFC: an IfcCurtainWall \
     aggregating IfcPlate panels and IfcMember mullions. Returns the curtain wall.",
    "Curtain Wall",
    crate::curtain::CreateParams,
    ElementId,
    crate::curtain::create
);

edit_command!(
    CurtainSet,
    "curtain.set",
    "Change curtain walls",
    "Changes curtain walls' base line, base and top, and the justification (`beginning`, `center`, \
     `end`) and offset of their vertical (`u_*`) and horizontal (`v_*`) grid layouts; omitted values are \
     kept. The type's layout rules, mullions and panels are type parameters (`type.set_param`).",
    "Edit Curtain Wall",
    crate::curtain::SetParams,
    (),
    crate::curtain::set
);
