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
