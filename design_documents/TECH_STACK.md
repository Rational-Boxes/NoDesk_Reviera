# Reviera — Re-implementation Technology Stack

**Status:** Decided by project owner, 2026-10-12
**Scope:** Technology choices for the clean-room re-implementation. Integration notes and risks are listed for planning.

## 1. Decisions

| Layer | Choice | Role |
|-------|--------|------|
| File format / data model | **Native IFC** via **IfcOpenShell** | IFC is the authoring format and source of truth. There is no proprietary project format. |
| Project licence | **GPL-3.0-or-later** | Covers the whole application; all dependencies and distributed add-ins must be compatible with GPL-3.0 |
| Core application | **Rust**, multi-core | Element database, parametric regeneration, commands, undo, and all application logic |
| Geometry kernel | **OpenCASCADE (OCCT)** | B-rep solids, booleans (joins, cuts, voids), sweeps, lofts, sections, tessellation |
| UI toolkit | **wxWidgets** | Cross-platform native UI: ribbon, dockable palettes, dialogs, view frames |
| Scripting / automation | **Lua with JIT (LuaJIT)** | User scripts, macros, add-ins (the counterpart to Revit macros, API add-ins and Dynamo scripting) |
| AI integration | **MCP server** | Exposes every application operation to AI agents |
| Crash safety | **SQLite live model store** (`rusqlite`, WAL) | Near real-time on-disk mirror of the model, updated per committed transaction; recovery after a crash (`spec/02b_live_model_store.md`) |
| Collaboration | **Open Source BIMserver client** (opensourcebim.org) | Optional shared model server. Reviera runs in **Local** or **BIMserver** mode (§3.7). |

## 2. Architecture Sketch

```
 ┌──────────────────────── wxWidgets UI (main thread) ─────────────────────────┐
 │ Ribbon · QAT · Properties · Project Browser · View windows · Dialogs        │
 └───────────────┬─────────────────────────────────────────────────────────────┘
                 │ Commands (single registry)
 ┌───────────────▼─────────────────────────────────────────────────────────────┐
 │                         Rust core (multi-core)                              │
 │  Command registry ── Transaction/Undo ── Element DB ── Regeneration graph    │
 │        ▲     ▲                              │                 │ (parallel)  │
 │        │     │                              ▼                 ▼             │
 │   LuaJIT   MCP server               IfcOpenShell (IFC)   OCCT geometry      │
 │   (scripts) (AI agents)             read/write/schema    solids/booleans    │
 │                                            │                                │
 │                         Model Store abstraction (§3.7)                      │
 │                    ┌──────────────┴───────────────┐                         │
 │              LocalStore (.ifc file)      BimServerStore (JSON API client)   │
 └─────────────────────────────────────────────────────────────────────────────┘
```

**Key principle: a single command registry.** Every operation is a typed command with a schema covering name, parameters, result and undo behaviour. These commands are registered once and appear automatically to:
1. the UI (ribbon buttons, shortcuts, context menus)
2. Lua (`reviera.<command>(...)`)
3. MCP (one tool per command, with JSON Schema taken from the command definition)

This is how the requirement that *all* operations are exposed to AI agents is met by construction, not maintained by hand.

## 3. Integration Notes

### 3.1 IfcOpenShell (native IFC)
- The model is held in memory as IFC (target schema **IFC4X3_ADD2**; read support for IFC2X3 and IFC4). Saving writes `.ifc` directly, with `.ifczip` as an option.
- Revit concepts map onto IFC as follows:

| Revit concept | IFC equivalent |
|---------------|----------------|
| Category | IFC entity class (`IfcWall`, `IfcDoor`, …) plus `PredefinedType` |
| Family / type | `IfcTypeObject` subtypes (`IfcWallType`, …) via `IfcRelDefinesByType` |
| Instance | Occurrence entity |
| Parameters | Property sets (`Pset_*`) and quantity sets (`Qto_*`) |
| Levels | `IfcBuildingStorey` |
| Grids | `IfcGrid` |
| Hosting | `IfcRelVoidsElement` / `IfcRelFillsElement` |
| Materials and layers | `IfcMaterialLayerSetUsage` |
| Phasing | `IfcRelSequence` / status properties |
| Worksets | Not native to IFC |

- Parametric intent and documentation can't be fully represented in standard IFC entities: constraints, family formulas, sketches, views, sheets and annotation. They are held in **Reviera Extension Data (RXD)**, a bespoke, versioned JSON format. By default it is **embedded in the IFC file as standard `Reviera_*` property sets** (IfcText payloads), so the file stays schema-valid and maximally compatible. An external `.reviera` sidecar is an alternative carrier. Full definition: `spec/02a_extension_data_format.md`.
- IfcOpenShell is C++ (with Python bindings). It will be reached from Rust through a C/C++ FFI layer (`cxx`/`autocxx` or a thin C shim).
- IfcOpenShell is LGPL-3.0; because Reviera is GPL-3.0, it may be linked statically (§4.2).
- **Verified 2026-10-12:** IfcOpenShell 0.8.5 builds against OCCT 7.9.3 (Fedora 43) and is linked statically into `reviera-ifc` through a `cxx` shim (`tools/build-ifcopenshell.sh`). The FZK-Haus and Institute sample models round-trip without loss and tessellate. IfcOpenShell builds schema tables lazily and not thread-safely, so the shim builds them once at start-up.

### 3.2 OpenCASCADE
- IfcOpenShell already uses OCCT as its geometry backend, so it fits naturally: IFC geometry goes to OCCT shapes, and both share one kernel.
- OCCT is C++ under **LGPL-2.1 with the OCCT exception**. Access will be through an FFI binding layer; the existing `opencascade-rs` crates are a possible starting point, but may need extending.
- OCCT data structures are not uniformly thread-safe. Parallel regeneration should give each worker its own shapes and avoid sharing mutable `TopoDS` state across threads. Boolean operations support internal parallelism (`SetRunParallel`).
- Rendering: tessellate in OCCT and render with a custom **`wgpu`** pipeline inside a wx-hosted native window, instead of OCCT's own AIS viewer (decided 2026-10-12; see §5.2). Exact 2D drawing output still comes from OCCT's hidden-line algorithms (spec Ch 5 §5.12).
- **Pluggable presentation (owner directive, 2026-10-12).** All 3D presentation goes through a render backend interface fed by a render-agnostic *Render Scene* (snapshot plus per-transaction deltas). The `wgpu` viewport is the built-in backend. Other backends can be:
  - in-process plug-ins, which must be GPL-compatible
  - external programs speaking the open **Reviera Scene Protocol** (glTF 2.0 snapshot with a `REVIERA_scene` extension, plus streamed deltas). Blender, game engines and others connect this way.
  - an **internal translation layer to Godot** (MIT) and similar engines, for desktop walkthroughs and full VR/AR through OpenXR.

  Drawings, printing and export never depend on the chosen backend. See spec Ch 5 DOC-RBE-001…002 and Ch 7 (advanced features).

### 3.3 Rust core (multi-core)
- **Live model store.** Every committed transaction's change set is passed to a dedicated writer thread. That thread group-commits it, every 250 ms, into a local SQLite database (WAL mode) holding the materialised IFC entities, the decoded RXD and the transaction journal. If Reviera crashes, the model and its undo history are restored from this database. The `.ifc` file stays the canonical saved format. See `spec/02b_live_model_store.md`.
- Element database: entity storage keyed by stable IDs (IFC GlobalId, plus an internal integer ID).
- Regeneration: a dependency graph of elements, constraints and joins. Invalidated subgraphs are recomputed in parallel with `rayon`. Results are committed atomically per transaction.
- Transactions: every command runs inside a transaction that records undo. Groups of transactions support multi-step tools (the equivalent of Revit's TransactionGroup).
- Concurrency model: the UI thread owns the event loop. The core runs on a worker pool. Lua and MCP requests are serialised through the command queue, so one transaction runs at a time; geometry is computed in parallel inside each transaction.

### 3.4 wxWidgets
- wxWidgets is C++. Reviera uses the **`wxDragon`** Rust bindings (MIT OR Apache-2.0), decided 2026-10-12 (§5.1).
- **Upstream-first extension policy.** Where `wxDragon` lacks a widget, event or platform hook that Reviera needs (for example `wxRibbonBar` art providers, AUI perspective details, native child windows for `wgpu`), the work is done in a fork kept as **clean, self-contained patches against upstream**: one feature per change, following upstream's code style, tests and examples, with no Reviera-specific code. Each patch is offered upstream as a pull request. Patches are contributed under upstream's licence (MIT OR Apache-2.0), so they can be merged as-is. Reviera pins the fork only until the change is released upstream, and the build records which patches are outstanding.
- Revit-style UI elements and how they map to wxWidgets:

| Revit-style element | wxWidgets approach |
|---------------------|--------------------|
| Ribbon (tabs, panels, split buttons, contextual tabs) | `wxRibbonBar`; contextual "Modify \| X" tabs need custom handling |
| Dockable Properties / Project Browser palettes | `wxAuiManager` |
| Multiple view windows and tabs | `wxAuiNotebook` |
| 3D viewports | Native child window handle for the GPU renderer |
| Dark theme | wxWidgets 3.3 dark mode support on Windows |

- Licence: wxWindows Library Licence (LGPL with a static-linking exception), which is permissive enough for commercial use.

### 3.5 LuaJIT scripting
- Embedded with the `mlua` crate using the `luajit` feature. The language level is Lua 5.1 plus LuaJIT extensions, so Lua 5.3/5.4 features such as integers and `goto` are not all available.
- LuaJIT states are single-threaded. The plan is one main scripting state, plus optional isolated states per worker for pure computation tasks such as batch geometry evaluation.
- The API surface is generated from the command registry. Read-only element queries (the counterpart to FilteredElementCollector) are exposed directly; writes go through commands and transactions.
- Use cases include macros, add-ins (a Lua package plus a manifest, the counterpart to `.addin`), batch automation, and visual-programming nodes (a possible future Dynamo-equivalent built on Lua).
- Sandboxing: limit `io`, `os` and `ffi` per trust level, because LuaJIT's FFI gives native memory access.
- LuaJIT's upstream is a rolling release with limited ARM64 macOS JIT maturity; this should be validated per target platform.

### 3.6 MCP server
- Built into the application with the official Rust MCP SDK (`rmcp`). Transports: stdio (local agents) and streamable HTTP (remote, auth required).
- **Tools:** one per registered command, with JSON Schemas generated from the command definitions.
- **Resources:** model elements, types, views, sheets, schedules and the parameter catalogue, exposed as readable resources (for example `reviera://element/{id}`).
- **Extras:** an `execute_lua` tool for compound operations, gated by permission. Undo/redo tools. Optional "preview/confirm" mode for destructive operations.
- **Safety:** each agent session gets its own transaction boundary, operations are attributed in undo history, and permission levels are read-only, model-edit and full-scripting.

### 3.7 Operating modes: Local and BIMserver client

Reviera can be used either on its own or as a client of the **Open Source BIMserver** (github.com/opensourceBIM/BIMserver). BIMserver is an IFC model server written in Java and licensed under AGPL-3.0. The latest release seen was 1.5.188 (2024-07-31). BIMserver stores IFC as objects, not files, and provides revisions, check-in/check-out, server-side queries and change notifications.

Both modes sit behind one **`ModelStore` trait** in the Rust core. Editing, regeneration, Lua and MCP work the same way in either mode. Only open, save, sync and history behave differently.

| Capability | Local mode | BIMserver mode |
|------------|------------|----------------|
| Storage | `.ifc` / `.ifczip` file on disk | BIMserver project (and sub-projects); IFC held as objects on the server |
| Open | File → Open | Login, then pick a project from a server browser; downloads the chosen revision (latest by default) |
| Save | Writes the file | Saves to a local working copy and cache (works offline) |
| Publish | — | **Check in**: uploads a new revision with a comment |
| Get latest | — | Downloads the newest revision and merges it into the working copy by GlobalId |
| History | Optional local backups | Server revision list, compare revisions, download any revision |
| Multi-user | No | Yes. Several clients use one project; notifications announce new revisions. |
| Revit equivalent | Standalone `.rvt` | Workshared central model (Revit Server / ACC cloud worksharing) |

**Moving between modes:** *Publish to BIMserver* creates or chooses a project and checks in the local file as the first revision. *Export local copy* writes the current working copy out as a standalone `.ifc`.

#### 3.7.1 BIMserver client implementation
- **Transport:** the BIMserver JSON API. Every call is an HTTP `POST` to `https://<host>[:port]/<context>/json` with a body of the form `{"token": "...", "request": {"interface": "...", "method": "...", "parameters": {...}}}`. Several calls can be batched in a `requests` array. A token comes from `AuthInterface.login` (or `autologin`).
- **Interfaces used:**
  - `AuthInterface`: login and logout
  - `ServiceInterface`: projects, revisions, deserializer and serializer lookup, check-in, download
  - `LowLevelInterface`: object-level transactions, for fine-grained updates without uploading a whole file (to verify)
  - Notification interfaces: new-revision events over WebSocket, so Reviera can say "model updated by X"
- **Check-in:** serialise the working copy with IfcOpenShell, look up the IFC deserializer for the project's schema, then call `ServiceInterface.checkin` (or its sync variant). File data travels base64-encoded in JSON; large models need the binary/streamed upload endpoint.
- **Download:** call `ServiceInterface.download` with the IFC serializer's OID. It returns a topic ID; Reviera then fetches the data and parses it with IfcOpenShell.
- **Rust client:** a `reqwest` + `serde` client generated from BIMserver's service metadata (method and type definitions can be queried from the server), running asynchronously on `tokio` so the UI never blocks.
- **Licensing:** BIMserver is AGPL-3.0. Reviera (GPL-3.0) talks to it only over the network and does not include its code, so no licence obligations transfer. GPL-3.0 §13 also explicitly allows combining GPL-3.0 and AGPL-3.0 code if that is ever needed.

#### 3.7.2 Mapping Revit collaboration concepts

| Revit concept | Reviera in BIMserver mode |
|---------------|---------------------------|
| Central model | BIMserver project |
| Local copy | Local working copy and cache (stored as IFC) |
| Synchronize with Central | **Sync**: get latest, merge, then check in |
| Reload Latest | Get latest |
| Element borrowing / ownership | Not native to BIMserver. Advisory locks are published as a small JSON document in BIMserver **Extended Data** on the latest revision (schema `org.reviera.ownership/1.0`), so they change without new revisions. Conflicts are detected at merge time by GlobalId. |
| Worksets | RXD document `project/worksets`, plus a membership entry in each element's `Reviera_Element` data (optional `IfcGroup` export). BIMserver sub-projects can also split models by discipline. |
| Linked models | Other BIMserver projects or revisions referenced as links |
| Model history | Server revision list |

#### 3.7.3 Consequences for other decisions
- **IFC schema.** BIMserver's mature support covers IFC2X3 TC1 and IFC4; support for IFC4X3 must be confirmed against the deployed server. The schema policy is therefore: **IFC4 (ADD2 TC1) is the default project schema**, which works in both modes. IFC4X3 is available in Local mode, and in BIMserver mode only when the server advertises a matching deserializer.
- **Storing Reviera-specific data.** RXD is embedded as property sets: element data on the element, project documents on IfcProject. It therefore travels inside every BIMserver check-in as ordinary IFC objects. Only live ownership/lock state uses BIMserver Extended Data (`addExtendedDataToRevision` and related calls, confirmed in the public BIMserver source). See `spec/02a_extension_data_format.md` §8.
- **MCP and Lua:** BIMserver operations (login, list projects, check in, get latest, revisions) are ordinary commands, so they are automatically available to AI agents and scripts. Credentials are never exposed through MCP resources.

## 4. Licensing

**Reviera is licensed under the GNU General Public License, version 3 or (at your option) any later version (SPDX: `GPL-3.0-or-later`).** The full text is in `/LICENSE`. Every source file starts with:

```
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) <year> Reviera contributors
```

### 4.0 Licensing exceptions inside the repository

| Path | Licence | Reason |
|------|---------|--------|
| `schemas/rxd/` (RXD JSON Schemas, examples, format description) | **MIT** | So any tool can adopt the RXD format (owner decision, 2026-10-12; `spec/02a_extension_data_format.md` §12) |
| `schemas/rsp/`, `clients/rsp/` (Reviera Scene Protocol schemas, `REVIERA_scene` glTF extension, reference client libraries) | **MIT** | So any renderer, engine or Blender add-on can implement a bridge (spec Ch 5 ADV-RSP-001) |
| Everything else | GPL-3.0-or-later | Project licence |

### 4.1 Dependency compatibility

| Component | Licence | Compatible with GPL-3.0? | Notes |
|-----------|---------|--------------------------|-------|
| IfcOpenShell | LGPL-3.0 | Yes | LGPL-3.0 code can be combined into a GPL-3.0 work. Static linking is fine. |
| OpenCASCADE | LGPL-2.1 + OCCT exception | Yes | LGPL-2.1 allows relicensing to GPL v2 or later, which includes GPL-3.0. KiCad (GPL-3.0) is an existing precedent for using OCCT. |
| wxWidgets | wxWindows Library Licence | Yes | — |
| LuaJIT | MIT | Yes | — |
| mlua, rayon | MIT / Apache-2.0 | Yes | — |
| Open Source BIMserver | AGPL-3.0 | Yes (network peer) | Separate server process; Reviera includes no BIMserver code. |
| reqwest, serde, tokio | MIT / Apache-2.0 | Yes | — |
| SQLite (bundled) / rusqlite / zstd | Public domain / MIT / MIT-BSD | Yes | Live model store |
| SQLCipher / OpenSSL 3 (vendored) / keyring | BSD-3-style / Apache-2.0 / MIT-Apache-2.0 | Yes (OpenSSL 3's Apache-2.0 requires GPL-**3**) | Encryption at rest for the live store |
| rmcp (MCP SDK) | Apache-2.0 | Yes | Apache-2.0 works with GPL-**3**, but not with GPL-2-only. |
| wxDragon | MIT OR Apache-2.0 | Yes | Our patches are contributed under the same licence so upstream can take them |
| wgpu | MIT OR Apache-2.0 | Yes | — |
| Godot Engine (translation layer, ADV-XR-001) | MIT | Yes | Bundled, either embedded or as a supervised child process |
| OpenXR loader | Apache-2.0 | Yes (GPL-3) | VR/AR runtime access |
| External render bridges (Blender add-on, third-party engines) | Their own | n/a (separate programs) | They talk only over the public scene protocol; nothing of theirs is bundled or loaded in-process |

### 4.2 Implications

- **Linking:** because the whole program is GPL-3.0, the dynamic-linking requirement that came from the LGPL libraries no longer applies. Static linking of IfcOpenShell and OCCT is allowed.
- **Dependency rule:** every new dependency must be GPL-3.0 compatible. Allowed: MIT, BSD, Apache-2.0, MPL-2.0, ISC, Zlib, LGPL-2.1+/3, GPL-3. Not allowed: GPL-2.0-only, SSPL, proprietary, or "non-commercial" licences. CI should enforce this with `cargo-deny` and a licence allow-list.
- **Add-ins must be GPL-compatible (policy decided).** Lua add-ins run inside Reviera's process and call its internal API, so they are treated as extensions of Reviera.
  - Every add-in that is **distributed** (shared publicly, sold or given to others) must be licensed under GPL-3.0-or-later or a licence that is compatible with GPL-3.0 (for example MIT, BSD, Apache-2.0, LGPL-3.0, MPL-2.0).
  - The add-in manifest must include a `license` field holding an SPDX expression. The add-in loader checks it against the GPL-compatible allow-list. If the field is missing or the licence is not compatible, the add-in is **not loaded by default**. Users can switch on a clearly labelled override that loads the add-in anyway: the GPL cannot restrict private use, and the loader is GPL code that users can change.
  - Any official Reviera add-in registry will only accept add-ins with a compatible SPDX licence and published source code.
  - Users' own private scripts and macros that are never distributed carry no obligations.
  - MCP clients and AI agents are separate programs talking over a protocol, so they are not add-ins and this policy does not apply to them.
- **MCP clients:** AI agents talk to the MCP server across a process/network boundary, so they are not derivative works and need no licence obligation.
- **Bundled data sets.** Data shipped with Reviera (weather, EPD/emission factors, textures, libraries) needs its own licence check, separate from code. Known limits from the gap-fill research (spec Ch 4 ANA-CARB-001): the ICE embodied-carbon database's licence forbids embedding its data in software, so users import it themselves; ÖKOBAUDAT is free to use but its terms exclude some uses, so it is not bundled either. Bundled sets are tracked in the data licence list (spec Q-DM-09).
- **IFC files:** models created by users are their own data and are not covered by the GPL.
- **Distribution:** every binary release must ship the GPL text, the complete corresponding source code (or a written offer for it), and the licence notices of third-party components.
- **Clean-room tie-in:** GPL source releases make the code public. That means the public-source provenance in `RESEARCH_LOG.md` matters even more, because anyone will be able to inspect the code against Autodesk's IP.

## 5. Open Decisions

1. ~~**Rust ↔ wxWidgets binding**~~: **resolved** as `wxDragon`. Extensions are kept as clean patches offered upstream (§3.4).
2. ~~**Viewport renderer**~~: **resolved** as a custom `wgpu` renderer using OCCT tessellation (§3.2).
3. ~~**Encoding parametric and documentation data in IFC**~~: **resolved**. The bespoke RXD JSON format is embedded in IFC as `Reviera_*` property sets by default, with an optional external sidecar. The priority is compatibility (`spec/02a_extension_data_format.md`).
4. ~~**Target platforms and minimum versions**~~: **resolved** (owner, 2026-10-12); see §5.4 below.
5. ~~**GPL variant**~~: **resolved** as GPL-3.0-or-later.
6. ~~**Plugin/add-in licensing policy**~~: **resolved**; add-ins must be GPL-compatible (§4.2).
7. ~~**Minimum supported BIMserver version**~~: **resolved** (owner, 2026-10-12). Reviera targets the **latest stable BIMserver release**, currently **1.5.188** (July 2024). When a newer stable release appears, it becomes the target after the compatibility suite passes against it; older releases are not supported. At connect time the client reads the server version and service metadata, and refuses with a clear message if the server is older than the target or a required method is missing. *Still to verify on 1.5.188:* IFC4X3 deserialiser and serialiser availability (§3.7.3), and the methods marked "(verify)" in spec Ch 6 §6.3.
8. ~~**Element ownership model**~~: **resolved** (owner, 2026-10-12). Defer to what BIMserver supports: stock BIMserver has no locking, so ownership is **advisory** (live Extended Data, enforced by cooperating Reviera clients and the merge rule). Reviera does not build or ship an enforcement plugin. If the server ever offers enforcement, it is detected through `bimserver.capabilities` and used (spec Ch 6 COL-OWN-004, deferred to P3).

### 5.4 Target platforms (resolved 2026-10-12)

The owner's directive is to reach older Windows and macOS installations, while Linux users are expected to keep current. The owner first asked for Windows 7, accepting a later baseline if Windows 7 meant a lot of extra work; it does (see the note below), so the Windows baseline is Windows 10.

| Platform | Minimum | Architectures | Packaging |
|----------|---------|---------------|-----------|
| Windows | **Windows 10** (any supported build) | x86_64 (arm64 later) | Installer and portable ZIP |
| macOS | **macOS 10.12 Sierra** (2016, about ten years back) on Intel; macOS 11 on Apple silicon, the first release for it | x86_64, arm64 (universal binary) | Signed, notarised `.dmg` |
| Linux | Current distribution releases | x86_64 (arm64 later) | **Flatpak** on Flathub, bundling wxWidgets/GTK, OCCT, IfcOpenShell, SQLCipher and LuaJIT so that slightly older distributions also work; a tarball for advanced users |

**Feasibility notes (to verify before M1).**
- **Why not Windows 7 or 8.1.** Rust 1.78 made Windows 10 the minimum for its main Windows targets. Windows 7 would need the tier-3 targets `x86_64-win7-windows-msvc`/`-gnu`: no prebuilt standard library (a nightly `-Z build-std` toolchain), no upstream CI guarantee, and every dependency audited for Windows 8+ APIs. Windows 7 also has no Direct3D 12, so `wgpu` would be limited to Vulkan or OpenGL. Together this is a permanent extra build, test and support track, so Windows 7 was dropped under the owner's fallback. It can be revisited as a community-maintained build if there is demand.
- **Rendering on Windows 10.** `wgpu` uses Direct3D 12 by default, with Vulkan as an alternative and OpenGL as a last resort for old drivers. The viewport's feature tiers (Ch 5 §5.12) must degrade gracefully on the GL path.
- **macOS 10.12.** Rust's minimum macOS deployment target is 10.12, which matches. Metal is available on 10.12, but the Metal feature set required by `wgpu`, and the minimums of the current wxWidgets release, OCCT and the code-signing toolchain, must be checked. Where a dependency needs a newer macOS, the oldest compatible release of that dependency is pinned for the legacy build, or the minimum is raised with the owner's agreement.
- **Secrets.** The OS keyring (`keyring` crate) is available on all three: Windows Credential Manager, macOS Keychain, and the Secret Service portal inside Flatpak.
- **Spec impact.** Features that need newer OS facilities (Windows 11 dark title bars, macOS 11+ SF Symbols and similar) are optional enhancements, detected at run time.

*Versions and binding crates named here should be checked against current releases before implementation starts.*
