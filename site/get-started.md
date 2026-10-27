# Get started

[← Home](index.md)

> Reviera is pre-release software. Expect rough edges, and keep copies of models you care
> about.

## Install a package

The current packages are pre-release 3 (`0.1.0-pre.3`), on the
[GitHub releases page](https://github.com/Rational-Boxes/NoDesk_Reviera/releases).

| Platform | Package | Status |
|---|---|---|
| Linux | `BIM-Reviera-<version>-x86_64.flatpak` | Built and tested on Fedora 43 |
| Windows | `BIM-Reviera-<version>-windows-x64-setup.exe`, or the portable ZIP | Tested on Windows 11 |
| macOS | `.dmg` | Not yet tested on a Mac |

**Linux:**

```sh
flatpak install --user BIM-Reviera-<version>-x86_64.flatpak
```

**Windows:** run the installer, or unzip the portable build and start `reviera.exe`.

Every package includes the sample models and the sample Component Library.

## Your first model

1. Open **Help › Reviera Help** (F1). The Samples section opens each sample model as a fresh
   copy, grouped by ribbon tab: Architecture, Structure, Steel, Concrete, Systems, Analyze,
   Sustainability, Massing & Site, Annotate, View, Manage and Modify.
2. Try the **Manor** for architecture, or the **Steel-framed office** for structure. Each sample
   has a walkthrough showing how to build it by hand.
3. Or start from **File › New**, set your units and building code in **Manage › Settings ›
   Model Properties**, and draw some walls.

## Build from source

Fedora is the development platform today.

```sh
sudo dnf install rustup cmake ninja-build gcc-c++ opencascade-devel boost-devel eigen3-devel \
    libxml2-devel json-devel perl-FindBin perl-IPC-Cmd perl-Time-Piece webkit2gtk4.1-devel
rustup-init

git clone https://github.com/Rational-Boxes/NoDesk_Reviera.git
cd NoDesk_Reviera
tools/build-ifcopenshell.sh      # IfcOpenShell into third_party/ (about 15 minutes)

cargo build
cargo run -- gui model.ifc
```

## Script it

Run a Lua script against the open model from **Scripts › Run Script…**, or from the shell:

```lua
local l1 = reviera.level.list()[1].id
local w = reviera.wall.create{
  curve = { line = { start = { 0, 0 }, ["end"] = { 10, 0 } } },
  base_level = l1, top_constraint = { unconnected = 3.0 },
}
reviera.door.place{ host_wall_id = w, point_along = 7.5 }
reviera.window.place{ host_wall_id = w, point_along = 2.5 }
```

The scripts behind every sample model are in `test-data/samples/`.

## Connect an AI agent

Reviera has a built-in MCP server. For example, with Claude Code:

```sh
claude mcp add reviera -- /path/to/reviera --mcp-stdio --permission model_edit /path/to/model.ifc
```

The permission levels are `read_only` (the default), `model_edit` and `full_scripting`.

## Get involved

The source, the specification and the issue tracker are on
[GitHub](https://github.com/Rational-Boxes/NoDesk_Reviera). Reviera is licensed under the
GNU GPL v3.0 or later. Its open data formats, protocols and client libraries are MIT-licensed.

---

[What Reviera does →](features.md) · [See it in action →](screenshots.md)
