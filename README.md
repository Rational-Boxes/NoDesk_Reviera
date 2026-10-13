# NoDesk Reviera

**Open-source BIM authoring on native IFC: build anywhere, even from the beach.**

![A laptop showing a building model on a beach table, next to a "Beach Life" mug](design_documents/beach_life.jpg)

Reviera is a parametric Building Information Modeling (BIM) application for architecture,
structure and MEP. IFC is its native file format, so there is no proprietary container. Every
operation can be driven by Lua scripts and by AI agents through the Model Context Protocol (MCP).

> **Status: early development.** The specification is complete as a draft. The application
> core is being built; there is no graphical interface yet.

## What works today

- **Native IFC** through IfcOpenShell and OpenCASCADE: open, edit, save and tessellate IFC2X3,
  IFC4 and IFC4X3_ADD2 models, with lossless round trips of files from other tools.
- **Element identity** that survives save and reopen. Reviera's own data is embedded in the IFC
  file as standard `Reviera_*` property sets (RXD), so the file stays valid for every IFC tool.
- **Transactions, undo and redo**, with one command registry. Each command has a JSON Schema, a
  permission level and attribution (user, script or agent).
- **Crash recovery.** Every change is mirrored to an encrypted SQLite live store (SQLCipher, with
  the key kept in the OS keyring). After a crash, unsaved work and its undo history come back.
- **MCP server** for AI agents: every command is a tool, with toolsets, preview and confirm for
  destructive changes, and resources for documents and elements.

## Building

Linux (Fedora) is the development platform today; Windows 10+ and macOS are targets.

```sh
# Toolchain and libraries (Fedora)
sudo dnf install rustup cmake ninja-build gcc-c++ opencascade-devel boost-devel eigen3-devel \
    libxml2-devel json-devel perl-FindBin perl-IPC-Cmd perl-Time-Piece
rustup-init

# IfcOpenShell 0.8.5, built into third_party/ (about 15 minutes)
tools/build-ifcopenshell.sh

cargo build
cargo test --workspace
```

## Trying it

```sh
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
