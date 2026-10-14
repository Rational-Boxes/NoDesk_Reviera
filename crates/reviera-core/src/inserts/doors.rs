// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! How a door opens and what its leaves are (ARC-DOOR-002): single or double swing, revolving,
//! or bi-parting sliding; solid or glass storefront leaves; and a powered operator (automatic).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::shape::names;

/// A door's operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// One leaf hinged at a jamb.
    #[default]
    SingleSwing,
    /// Two leaves hinged at both jambs, meeting in the middle.
    DoubleSwing,
    /// Four wings turning in a round drum (a revolving door): its width is the drum's
    /// diameter.
    Revolving,
    /// Two leaves sliding apart along the wall (bi-parting), usually automatic.
    Sliding,
}

names!(Operation,
    SingleSwing => "single_swing", "Single Swing";
    DoubleSwing => "double_swing", "Double Swing";
    Revolving => "revolving", "Revolving";
    Sliding => "sliding", "Sliding (Bi-parting)";
);

impl Operation {
    /// `IfcDoorTypeOperationEnum` (IFC4).
    pub fn ifc(self, hinge_left: bool) -> &'static str {
        match self {
            Operation::SingleSwing if hinge_left => "SINGLE_SWING_LEFT",
            Operation::SingleSwing => "SINGLE_SWING_RIGHT",
            Operation::DoubleSwing => "DOUBLE_DOOR_SINGLE_SWING",
            Operation::Revolving => "REVOLVING",
            Operation::Sliding => "DOUBLE_DOOR_SLIDING",
        }
    }

    /// Whether its leaves swing (and sweep a swing volume).
    pub fn swings(self) -> bool {
        matches!(self, Operation::SingleSwing | Operation::DoubleSwing)
    }
}

/// What a door's leaves are made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    /// A solid leaf.
    #[default]
    Solid,
    /// A storefront leaf: narrow stiles and rails (a deep bottom rail) round full-height glass.
    Glass,
}

names!(Panel,
    Solid => "solid", "Solid";
    Glass => "glass", "Glass (Storefront)";
);

/// Storefront stiles and top rail, and bottom rail (metres).
pub const STILE: f64 = 0.1;
pub const BOTTOM_RAIL: f64 = 0.25;
/// The operator housing of an automatic door: its height and depth off the wall face.
pub const OPERATOR_HEIGHT: f64 = 0.15;
pub const OPERATOR_DEPTH: f64 = 0.15;
