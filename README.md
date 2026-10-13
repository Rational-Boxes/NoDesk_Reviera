# NoDesk Reviera

**Open-source BIM authoring on native IFC: build anywhere, even from the beach.**

![A laptop showing a building model on a beach table, next to a "Beach Life" mug](design_documents/beach_life.jpg)

Reviera is a parametric Building Information Modeling (BIM) application for architecture,
structure and MEP. IFC is its native file format, so there is no proprietary container. Every
operation can be driven by Lua scripts and by AI agents through the Model Context Protocol (MCP).

> **Status: early development.** The specification is complete as a draft. The application
> core is being built, and the first window shows models in an interactive 3D viewport.

## What works today

- **Native IFC** through IfcOpenShell and OpenCASCADE: open, edit, save and tessellate IFC2X3,
  IFC4 and IFC4X3_ADD2 models, with lossless round trips of files from other tools.
- **Element identity** that survives save and reopen. Reviera's own data is embedded in the IFC
  file as standard `Reviera_*` property sets (RXD), so the file stays valid for every IFC tool.
- **Transactions, undo and redo**, with one command registry. Each command has a JSON Schema, a
  permission level and attribution (user, script or agent).
- **Crash recovery.** Every change is mirrored to an encrypted SQLite live store (SQLCipher, with
  the key kept in the OS keyring). After a crash, unsaved work and its undo history come back.
- **Tabbed views**: several models open side by side, each view in its own tab with a `wgpu`
  3D viewport (Window › New 3D View, Tile Views, Tab Views, Ctrl+Tab); a dockable Properties
  palette. In the views:
  click to select, drag with the left mouse button to orbit, with the middle or right button
  to pan, and use the wheel to zoom.
- **Walls**: *Architecture > Wall* draws chains of straight walls on a level, snapping to wall ends,
  midpoints, wall lines and 15° angles; type a length (`4.5 m`, `12' 6"`) and press Enter, and
  Spacebar flips the exterior side. Walls appear at once,
  regenerated from their recipe (`wall.create`, `wall.set_location`). Walls that meet join at
  mitred L corners and T junctions (`wall.set_join_allowed` turns a join off). Saving writes standard IFC
  (`IfcWall` with an `Axis` and a `SweptSolid` body, typed, contained in its storey and
  connected to its neighbours with `IfcRelConnectsPathElements`), so other
  tools see them, and Reviera reopens them as editable walls.
- **Units and parameters**: SI storage with project display units (metric and imperial,
  feet-and-inches), built-in, project and shared parameters routed to types or instances.
- **MCP server** for AI agents: every command is a tool, with toolsets, preview and confirm for
  destructive changes, and resources for documents and elements.

## Building

Linux (Fedora) is the development platform today; Windows 10+ and macOS are targets.

```sh
# Toolchain and libraries (Fedora)
sudo dnf install rustup cmake ninja-build gcc-c++ opencascade-devel boost-devel eigen3-devel \
    libxml2-devel json-devel perl-FindBin perl-IPC-Cmd perl-Time-Piece
rustup-init

# The first build also downloads and compiles wxWidgets 3.3.3 (through wxDragon).
# IfcOpenShell 0.8.5, built into third_party/ (about 15 minutes)
tools/build-ifcopenshell.sh

cargo build
cargo test --workspace
```

## Trying it

```sh
cargo run -- gui model.ifc             # the main window with the 3D viewport
cargo run -- model.ifc                 # open a file and print a summary
cargo run -- shell model.ifc           # run commands: `:commands`, `file.save`, `edit.undo {"steps": 2}`
cargo run                              # list unsaved work left by a crash
```

To connect an AI agent over MCP, for example Claude Code:

```sh
claude mcp add reviera -- /path/to/reviera --mcp-stdio --permission model_edit /path/to/model.ifc
```

Permission levels are `read_only` (the default), `model_edit` and `full_scripting`.

## Repository layout

| Path | Contents |
|---|---|
| `apps/reviera` | The `reviera` program |
| `crates/reviera-core` | Element store, transactions, undo, command registry |
| `crates/reviera-ifc` | IfcOpenShell bridge (C++ shim through `cxx`) |
| `crates/reviera-rxd` | Reviera Extension Data encoding |
| `crates/reviera-store` | Local `.ifc` projects with embedded RXD |
| `crates/reviera-livestore` | Encrypted live store and crash recovery |
| `crates/reviera-app` | Application session and file, live-store and recovery commands |
| `crates/reviera-mcp` | MCP server |
| `crates/…` | Rendering, Godot, UI, Lua, BIMserver and OpenCDE crates (placeholders) |
| `design_documents/` | The specification (`spec/`), technology stack, research log and source register |
| `schemas/`, `clients/rsp/` | Open data formats and protocol clients (MIT) |
| `test-data/ifc/` | Sample IFC models for tests (see their README for provenance) |

The specification is in `design_documents/spec/`, collected into
`design_documents/Revit_Application_Specification.md`.

## Licence

Reviera is licensed under the **GNU General Public License v3.0 or later** (see `LICENSE`). The
RXD schemas (`schemas/rxd/`), the Reviera Scene Protocol (`schemas/rsp/`) and its clients
(`clients/rsp/`) are MIT-licensed, so any tool can adopt them.

## Clean-room statement

Reviera's functional reference is a commercial BIM product, described only from publicly
available sources (online help, public blogs, tutorials, forums and open-source repositories),
all listed in `design_documents/SOURCE_REGISTER.md`. No proprietary, confidential or leaked
material, vendor code, binaries or file formats were used. Autodesk and Revit are trademarks of
Autodesk, Inc. Reviera is not affiliated with or endorsed by Autodesk.
