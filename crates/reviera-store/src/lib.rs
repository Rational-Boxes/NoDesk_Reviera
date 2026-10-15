// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Model stores (TECH_STACK §3.7). Local mode keeps the project in one `.ifc` file with embedded
//! RXD; BIMserver mode will add a server-backed store behind the same operations.

pub mod commands;
mod curtains;
pub mod fragments;
mod geometry;
mod grids;
mod local;
mod materials;
mod mesh_objects;
pub mod params;
mod path_arrays;
mod psets;
mod spatial;

pub use local::{LocalProject, OpenReport, SaveReport};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Ifc(#[from] reviera_ifc::IfcError),
    #[error(transparent)]
    Rxd(#[from] reviera_rxd::RxdError),
    #[error(transparent)]
    Core(#[from] reviera_core::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Unsupported(String),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;
