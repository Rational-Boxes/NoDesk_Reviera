// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! `steel_nc.export` (STR-STL-010): one DSTV NC1 file per steel part mark, written by the MIT
//! `reviera-dstv` crate from `reviera_core::steel_numbering::nc_pieces`.

use std::path::PathBuf;

use reviera_core::steel_numbering::{NcParams, Options};
use reviera_core::{Caller, CommandMeta, Effect, Error, HostCommand, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::App;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportParams {
    /// The folder the files go in (made if missing).
    pub folder: PathBuf,
    /// Only `dstv` (NC1).
    #[serde(default = "dstv")]
    pub format: String,
    #[serde(default)]
    pub order: Option<String>,
    #[serde(default)]
    pub drawing: Option<String>,
    #[serde(default)]
    pub options: Option<Options>,
    /// Replace files already there.
    #[serde(default)]
    pub overwrite: bool,
}

fn dstv() -> String {
    "dstv".into()
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Written {
    pub mark: String,
    pub quantity: u32,
    pub path: PathBuf,
}

pub(crate) struct ExportNc;

impl HostCommand for ExportNc {
    type Host = App;
    type Params = ExportParams;
    type Output = Vec<Written>;
    const META: CommandMeta = CommandMeta {
        name: "steel_nc.export",
        version: 1,
        summary: "Export steel parts as DSTV NC1 files",
        description: "Writes one DSTV NC1 file (`<mark>.nc1`) per steel part mark to `folder` (STR-STL-010): \
                      members with their profile, length and holes on the web and flanges, plates with their \
                      outline and holes, each with the quantity of parts carrying the mark. Marks are worked out \
                      as `steel_numbering.run` would (with `options`). `order` and `drawing` go in each header. \
                      Returns the files written.",
        effects: &[Effect::FileWrite],
        transaction_name: "",
    };

    fn run(&self, app: &mut App, _: &Caller, p: ExportParams) -> Result<Vec<Written>> {
        if p.format != "dstv" {
            return Err(Error::new("steel_nc.format", format!("no NC format `{}` (only dstv)", p.format)));
        }
        let doc = app
            .active()
            .ok_or_else(|| Error::new("document.none", "no document is open"))?;
        let store = app.get(doc)?.doc.store();
        let pieces = reviera_core::steel_numbering::nc_pieces(
            store,
            &NcParams { order: p.order.clone(), drawing: p.drawing.clone(), options: p.options.clone() },
        );
        if pieces.is_empty() {
            return Err(Error::new("steel_nc.nothing", "the model has no steel parts to export"));
        }
        let fail = |e: std::io::Error| Error::new("export.write_failed", format!("{}: {e}", p.folder.display()));
        std::fs::create_dir_all(&p.folder).map_err(fail)?;
        let mut out = Vec::new();
        for piece in pieces {
            let safe: String = piece.piece.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
            let path = p.folder.join(format!("{safe}.nc1"));
            if path.exists() && !p.overwrite {
                return Err(Error::new("file.exists", format!("{} exists; pass overwrite to replace it", path.display())));
            }
            std::fs::write(&path, reviera_dstv::write(&piece)).map_err(fail)?;
            out.push(Written { mark: piece.piece.clone(), quantity: piece.quantity, path });
        }
        Ok(out)
    }
}
