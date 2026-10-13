# Chapter 7 — Advanced Features

**ID prefix:** `ADV` · **Status:** Draft 0.1 · 2026-10-12

## 7.0 Scope and provenance

This chapter collects capabilities that go **beyond the reference product's feature set**: features that are Reviera's own design, mostly from owner directives, built on open standards and open-source engines. It keeps the clean-room boundary visible. Chapters 1–6 describe behaviour traced to public research on the reference product (with Reviera's own architecture underneath). This chapter describes features that have no reference counterpart, or that extend one well beyond it, and cites only owner directives and public documentation of open standards and open-source projects.

New features of this kind are added here, unless they are inseparable from a core chapter's mechanism. In that case they stay in the core chapter and are indexed in §7.2. The full list of requirements with no reference-product research behind them is generated in Appendix A.2a.

**Principles.**
1. **Nothing here is required to author or document a building.** Every feature is optional, behind capability checks, and the core works without it.
2. **Same contracts as the core.** Every feature is driven by registered commands (UI, Lua, MCP), reads the model only through the core's interfaces, and never edits it outside a transaction.
3. **Open standards first.** IFC, BCF, glTF and OpenXR are preferred over vendor SDKs. Engines and tools are bundled only under GPL-compatible licences; everything else attaches through public protocols (Ch 5 DOC-RBE-002).

## 7.1 Immersive and real-time visualisation

This section builds on the pluggable render backend interface in Chapter 5 (DOC-RBE-001 Render Scene and backend contract, DOC-RBE-002 hosting and licensing).

### Reviera Scene Protocol (RSP)
**ID:** ADV-RSP-001 · **Priority:** P2 · **Ref:** owner directive 2026-10-12; Khronos glTF 2.0 specification (public); DOC-OUT-007

**Purpose.** One open, versioned protocol for streaming the Render Scene to any external renderer, so a bridge is written once per engine rather than once per Reviera release.

**Behaviour & rules.**
- **Snapshot** is a binary glTF 2.0 (GLB) document. Reviera information is carried in a vendor extension, `REVIERA_scene`:
  - per node: element GlobalId, category key and sub-category, and phase and design-option state
  - lights: `KHR_lights_punctual`, plus an extension block for IES profiles and light groups
  - sun and sky: date and time, project location, and sky model
  - other materials data: `KHR_materials_*` extensions where they exist (transmission, IOR, emissive strength, clearcoat, sheen, volume, texture transform)
  - collision hints: for each node, whether it is walkable, a blocker or ignored. This is derived from category (floors, stairs and ramps are walkable; walls, columns and furniture block; openings, annotation and spaces are ignored) and can be overridden per category.
- **Deltas** are messages on the same connection: `node.add/update/remove`, `mesh.add/remove`, `material.add/update`, `graphics.update` (visibility, override colour, transparency), `light.update`, `sun.update`, `camera.update`, `section.update`. Each carries a monotonically increasing sequence number. Large buffers (meshes, textures) travel as binary frames, or through shared memory when both sides are on one machine. A bridge that falls behind can ask for a fresh snapshot.
- **Back-channel messages** (bridge → Reviera): `hello` (protocol version, capabilities), `camera.pose`, `pick` (GlobalId), `viewpoint.capture` (camera, image and optional comment, which becomes a BCF viewpoint or a saved view), `walk.path` (recorded path samples), `render.result` (image and metadata for the render gallery), and `log`.
- **Versioning and publication.** RSP is versioned with semantic versioning, negotiated in `hello`, and described by JSON Schemas for the message envelopes plus the `REVIERA_scene` glTF extension schema. These are published under **MIT**, like the RXD schemas (02a §12), at `https://nodeskreviera.org/schemas/rsp/<major>.<minor>/`. A reference client library (Rust, MIT) and a Python client for Blender are part of the protocol deliverables.
- **Offline use.** `render.export_scene` writes the same snapshot to a `.glb` file (with the extension), so engines can also import a static model without a live connection. OpenUSD output is a later option, subject to the licence check in Q-DM-09.

**Commands.** `render.export_scene(path, view_id?, scope?: view|model, detail?: coarse|medium|fine, include_lights?, include_collision_hints?) -> report`; `render.protocol_info() -> {version, schema_urls}` (query).

**Acceptance criteria.**
- [ ] The snapshot of the sample project opens in a generic glTF viewer with correct geometry and materials, ignoring the unknown extension.
- [ ] After 1 000 random edits streamed as deltas, a bridge's scene matches a fresh snapshot (same nodes, transforms, materials and visibility).
- [ ] A bridge speaking an older minor protocol version keeps working; one speaking an unsupported major version is refused with a clear message.

### Simulated walkthrough sessions
**ID:** ADV-WALK-001 · **Priority:** P2 · **Ref:** owner directive 2026-10-12; research/04 §1.5 *Walkthrough* (context); DOC-OUT-012

**Purpose.** Let designers and clients walk through the model interactively, at eye level with realistic movement, in the built-in viewport or a more photo-real backend. Paths and findings come back into the project.

**Workflow.**
1. View tab › Presentation › **Walk Through** (`walkthrough.start`), from a 3D, plan or camera view. The dialog chooses a backend (from those offering `walkthrough`), the start point (current camera, click a point in plan, or a named view) and the mode: *Walk* (gravity and collision) or *Fly*.
2. The session opens in the backend's window (or in the view, for the built-in viewport). Reviera keeps a small *Walkthrough* panel with the position on a plan mini-map, the time-of-day slider and the record and capture buttons.
3. The user walks with WASD or arrow keys and mouse-look, a game controller, or a VR headset where the backend supports OpenXR (full VR and AR: ADV-XR-001).
4. *Record* captures the path. *Save path* turns it into a Reviera walkthrough view (`view.walkthrough`, DOC-OUT-012) that can be edited, animated and exported. *Capture* saves a viewpoint with an optional comment as a saved 3D view or a BCF issue.
5. Model edits made in Reviera during the session appear in the backend after the next sync (ADV-RSP-001).

**Behaviour & rules.**
- **Movement defaults:** eye height 1.60 m (setting 0.5–2.5 m), walking speed 1.4 m/s (run ×2.5), turn rate by mouse sensitivity. Step-up height is 0.20 m, so stairs and ramps can be climbed and kerbs above that block. Gravity is on in *Walk* mode. Collision uses the RSP collision hints and the avatar capsule (radius 0.25 m).
- **Doors:** in *Walk* mode, doors whose family declares an opening (swing or slide) open automatically when the avatar comes within 1.2 m and close behind it. This is presentation only and never changes the model. *Doors open* can be switched off.
- **Lighting:** the time-of-day slider moves the sun (project location and date); light groups can be switched and dimmed for the session; changes are not saved unless the user saves them to the view's render settings.
- **Phases and options:** the session shows the phase and design-option state of the source view, and can switch between design options for comparison if the backend supports live visibility changes.
- **Measuring and info:** a pick (click, or controller trigger) shows the element's category, family, type and mark from Reviera, and can select the element back in Reviera (cross-highlight).
- **Built-in fallback:** the built-in viewport offers the same walkthrough in its Realistic style, so the feature works with no external backend installed.
- **Performance:** external backends receive a coarser detail level by default for models over 2 million triangles; the built-in viewport targets 60 fps at that size on the reference GPU.

**Data (RXD).** Saved paths are walkthrough views (`view.walkthrough`, `views/<viewId>`). Captures are saved 3D views or BCF topics (`coordination/issues`). Per-session settings (eye height, speed, door behaviour) are user preferences and not stored in the model, except when saved into a walkthrough view.

**Commands.** `walkthrough.start(view_id?, backend_id?, start?, mode?: walk|fly) -> session_id`; `walkthrough.stop(session_id) -> ()`; `walkthrough.record(session_id, on) -> ()`; `walkthrough.save_path(session_id, name) -> view_id`; `walkthrough.capture(session_id, comment?, as?: view|bcf) -> id`; `walkthrough.set_time(session_id, datetime) -> ()`; `walkthrough.set_option(session_id, key, value) -> ()`.

**Acceptance criteria.**
- [ ] In *Walk* mode the avatar climbs a stair of 175 mm risers and is stopped by a 250 mm step and by walls.
- [ ] A recorded path saved as a walkthrough view exports as an animation with the same camera positions (within 1 mm and 0.1°).
- [ ] A capture made in an external backend arrives in Reviera as a BCF issue with its viewpoint and image.
- [ ] Moving a wall in Reviera during a session updates the backend within 1 s on the same machine.

### Internal engine translation layer (walkthrough, VR and AR)
**ID:** ADV-XR-001 · **Priority:** P2 (engine adapter and desktop walkthrough), P2 (VR), P3 (AR and multi-user review) · **Ref:** owner directive 2026-10-12 ("a translation layer to Godot and similar … to support walkthrough, full VR/AR"); DOC-RBE-001, ADV-WALK-001

**Purpose.** Ship a first-party bridge inside Reviera between the Render Scene and a full game engine, so that immersive features (walkthrough, room-scale VR, mixed and augmented reality) come from a mature engine rather than being rebuilt in the core. Godot is the first target; the layer is designed so similar engines can be added.

**Architecture.**
- **Engine-neutral core, engine-specific adapters.** The layer has two parts:
  - **Translator** (engine-neutral, Rust): turns the Render Scene and its deltas (DOC-RBE-001) into an *engine scene plan*. That covers node hierarchy, instanced meshes, physically based materials, lights and IES profiles, sun, sky and environment, collision shapes from the RSP collision hints, navigation surfaces (walkable floors, stairs and ramps), door-opening animations, and the level of detail per element.
  - **Adapter** (one per engine): applies the plan through that engine's API. The Godot adapter builds Godot scene nodes, `MeshInstance3D`/`MultiMesh`, `StandardMaterial3D`, `OmniLight3D`/`SpotLight3D`/`DirectionalLight3D`, `WorldEnvironment`, physics bodies and a navigation mesh. It also maps element GlobalIds to node metadata so that picking and cross-selection work.
- **Hosting.** The engine runs either **embedded** in the Reviera process (the engine as a library where the engine supports it; for Godot this is *LibGodot*, available since Godot 4.6 on Linux, Windows and macOS, ADV-XR-002, rendering into a wx-hosted native window like the built-in viewport), or as a **bundled child process** launched and supervised by Reviera and connected over the Reviera Scene Protocol (ADV-RSP-001). The adapter decides; both appear to the user as one backend with `walkthrough`, `vr` and (later) `ar` capabilities. A crash in a child process never affects the model (DOC-RBE-001).
- **Licensing.** Only engines whose licences allow bundling with a GPL-3.0 program are integrated this way: Godot (MIT) is the first, and others under MIT, BSD, Apache-2.0 or similar may follow. The OpenXR loader is Apache-2.0. Engines under proprietary licences are never bundled; they can only attach as third-party external bridges (DOC-RBE-002).
- **Same contract as every backend.** The layer is a render backend: read-only scene, no model edits except through commands, and capabilities declared through `describe()`. It is tested with the same conformance suite.

**VR (OpenXR).**
- Room-scale and seated modes on any OpenXR runtime, PC-tethered or standalone streaming, at 1:1 scale.
- Locomotion: teleport (default, for comfort), smooth movement with optional vignette, snap turn, and a *walk* mode that respects collision and stairs as in ADV-WALK-001.
- Interaction with controllers or hand tracking:
  - point to pick an element (shows its category, type and mark)
  - measure between two points
  - capture a viewpoint with a voice or text note as a BCF issue
  - switch design options, time of day and light groups from a wrist menu
- Scale model mode: shrink the building to tabletop size and grab, rotate or section it with a hand-held section plane. Section planes are session-only.
- Comfort and safety: the runtime's guardian or boundary is always respected, frame-rate targets are 72/90 Hz with automatic detail reduction, and a single *recentre* action is available.

**AR and mixed reality (later).**
- Headset passthrough (OpenXR passthrough extensions where the runtime offers them) shows the model over the real room or site.
- **On-site placement** uses the project's survey point and true north (Chapter 6, COL-POS-001) with a manual two-point alignment. Model elements can occlude and be occluded where the device provides depth.
- **Tabletop AR** places a scaled model on a detected surface.
- Phone and tablet AR (ARCore/ARKit) is considered only through engines whose licences and export paths allow it. *Deferred.*

**Multi-user review (later).** Several participants in one session (desktop, VR or AR), with avatars, a shared pointer and voice left to external tools. The session host streams the scene. *Deferred (P3).*

**Data.** Nothing new in the model. Saved paths, captures and views use ADV-WALK-001's storage. Engine and adapter settings (quality preset, comfort options, controller bindings) are user preferences.

**Commands.** The ADV-WALK-001 `walkthrough.*` commands, plus `xr.start(view_id?, backend_id?, mode: vr|ar|tabletop, start?) -> session_id`; `xr.stop(session_id) -> ()`; `xr.align(session_id, method: survey_point|two_point, points?) -> ()`; `xr.set_option(session_id, key, value) -> ()`; `xr.runtime_info() -> runtime_info` (query: OpenXR runtime, devices, supported extensions).

**Acceptance criteria.**
- [ ] With the Godot adapter, the sample project opens in a desktop walkthrough within 15 s and stays at 60 fps on the reference GPU.
- [ ] In VR on a supported headset, the session holds 90 Hz on the sample project with automatic detail reduction enabled, and teleporting onto a stair landing places the user at landing height.
- [ ] A pick in VR selects the same element in Reviera's Project Browser and Properties palette.
- [ ] Killing the engine's child process mid-session leaves Reviera responsive, the model unchanged, and the session reported as ended.
- [ ] Swapping the Godot adapter for a test adapter (the conformance client) needs no change to the translator or the core.

### XR platform support baseline (Godot)
**ID:** ADV-XR-002 · **Priority:** P2 · **Ref:** web: Godot OpenXR integration project update (Khronos blog); Godot 4.6 release notes (LibGodot); Godot 4.7 XR coverage; Godot OpenXR Vendors plugin releases; Godot XR updates (godotengine.org); visionOS contribution announcement (press); Godot forum on mobile AR. All public, accessed 2026-10-12 (SOURCE_REGISTER S0686–S0698).

**Purpose.** Pin down which XR devices and features the Godot translation layer (ADV-XR-001) can rely on, and what Reviera must not promise yet. The findings below are a snapshot of public information as of 2026-10-12 and must be re-checked when the layer is built.

**Findings (public sources, October 2026).**

| Area | Status in Godot | Consequence for Reviera |
|------|-----------------|-------------------------|
| Embedding | *LibGodot* (Godot 4.6, January 2026) builds the engine as a library on Linux, Windows and macOS through a `GodotInstance` entry point | The **embedded** hosting option of ADV-XR-001 is realistic on all three desktop platforms; the child-process option remains the fallback |
| XR standard | OpenXR is Godot's XR path; OpenXR 1.1 support including frame synthesis (4.6–4.7); foveated rendering through Vulkan subsampled images and better composition layers for sharp UI and text (4.7, June 2026) | Reviera targets **OpenXR only**; no vendor SDKs in the core. UI panels in VR use composition layers |
| PC VR | Any conformant OpenXR runtime; controller *render models* shipped in 4.5 (SteamVR, PICO) | Desktop VR on Windows and Linux via the user's OpenXR runtime (for example SteamVR); macOS has no mainstream PC-VR runtime |
| Standalone headsets | Meta Quest, PICO, HTC (Vive XR Elite, Focus Vision), Magic Leap 2, Samsung Galaxy XR / Android XR (production-ready with Vendors plugin 5.1, May 2026), Valve Steam Frame (day-one, Godot 4.7); a single universal Android OpenXR APK for all Android-based headsets (4.5/4.6) | Standalone headsets are reached by the *exported walkthrough package* (ADV-BRG-001), not by running Reviera on the headset. Streaming from the desktop (PC VR link) is the live-session path |
| Vendor extras | OpenXR Vendors plugin (MIT): passthrough with filters and projected passthrough, and other vendor extensions | Use for passthrough and MR where available, behind capability flags |
| Mixed reality | OpenXR Spatial Entities in 4.6: spatial anchors, plane detection, marker (QR) tracking; demonstrated on PICO | Supports ADV-XR-001's on-site alignment: QR markers at known survey points can supplement two-point alignment; anchors persist a placement between sessions |
| Apple Vision Pro | Apple is contributing visionOS support upstream (windowed first, then immersive); no fixed timeline | Not promised; track upstream |
| Phone and tablet AR | ARKit plugin exists only for Godot 3; ARCore support for Godot 4 is partial and in progress | Mobile AR stays **deferred** (P3), as ADV-XR-001 already says |
| Renderer | Godot recommends its *Mobile* renderer for XR projects | The adapter's XR sessions default to the Mobile renderer; desktop walkthroughs may use the Forward+ renderer for quality |

**Behaviour & rules.**
- The adapter declares capabilities from what the active OpenXR runtime reports at session start (`xr.runtime_info`): passthrough, hand tracking, spatial anchors, plane detection, marker tracking, foveation. Features whose extension is missing are hidden, not shown as errors.
- The minimum supported Godot version for the adapter is the first release with LibGodot and Spatial Entities (4.6). The adapter pins one tested Godot release per Reviera release.
- On-site AR alignment order of preference: QR marker at a surveyed point (where marker tracking exists); then two-point manual alignment; then survey point plus compass (lowest accuracy, flagged in the UI).
- Standalone-headset packages are built from an RSP snapshot (ADV-RSP-001) as a universal Android OpenXR APK, with no model editing and with captures saved locally for import into Reviera as BCF.

**Acceptance criteria.**
- [ ] On a runtime without passthrough, the AR modes are not offered and VR still works.
- [ ] A universal walkthrough APK built from the sample project installs and runs on at least two different Android-based headsets.
- [ ] A QR marker placed at a surveyed point aligns the model to within 20 mm of the surveyed position at 5 m distance.

### Reference bridges
**ID:** ADV-BRG-001 · **Priority:** P3 (deferred; candidates for after the initial version) · **Ref:** owner directive 2026-10-12

**Purpose.** Prove the protocol with real engines, and give users ready-made photo-real options.

**Behaviour & rules.** Candidate bridges, each a separate project that uses only the public RSP:
- **Blender bridge:** a Blender add-on (GPL, Python) that keeps a live Blender scene in sync, maps Reviera materials to Principled BSDF and IES lights to Blender lights, renders with Cycles or EEVEE, and sends results to the render gallery (`render.result`). It also exports camera paths both ways.
- **Godot:** integrated first-party through the internal translation layer (ADV-XR-001), not as an external bridge. A further option is a *standalone walkthrough package* for clients: a Godot export of a scene snapshot that runs without Reviera installed.
- **Bevy viewer** (MIT/Apache-2.0): a Rust reference implementation, used as the protocol conformance test client.
- Bridges to proprietary engines are left to third parties (DOC-RBE-002).

**Acceptance criteria.**
- [ ] Each official bridge passes the RSP conformance suite (ADV-RSP-001) before release.

## 7.2 Advanced features specified in other chapters

| Feature | Where | IDs |
|---------|-------|-----|
| Pluggable render backends and their hosting rules | Ch 5 §5.12 | DOC-RBE-001, DOC-RBE-002 |
| Live SQLite model store: crash recovery, sketch-session recovery, encryption at rest, time travel, combined BIMserver timeline, shared snapshots | Ch 2b | DM-LIVE-001 … 020 |
| Reviera Extension Data embedded in IFC | Ch 2a | DM-RXD-* |
| Workspace customisation: layout bookmarks, workflow scenarios, view bookmarks | Ch 1 §1.29.4–§1.29.5 | UI-LAYOUT-001 … 007, UI-SCEN-001 … 004 |
| Built-in MCP server and agent pane | Ch 2 §2.16, Ch 1 §1.31 | DM-MCP-*, UI-AGENT-* |
| Embodied carbon analysis | Ch 4 | ANA-CARB-001 |

## 7.3 Command catalog

| Command | Parameters | Returns | Undoable | Req ID |
|---------|------------|---------|----------|--------|
| `render.export_scene` | path, view_id?, scope?: view\|model, detail?: coarse\|medium\|fine, include_lights?, include_collision_hints? | report | No | ADV-RSP-001 |
| `render.protocol_info` | — | {version, schema_urls} | No (query) | ADV-RSP-001 |
| `walkthrough.start` | view_id?, backend_id?, start?, mode?: walk\|fly | session_id | No | ADV-WALK-001 |
| `walkthrough.stop` | session_id | () | No | ADV-WALK-001 |
| `walkthrough.record` | session_id, on | () | No | ADV-WALK-001 |
| `walkthrough.save_path` | session_id, name | view_id | Yes | ADV-WALK-001 |
| `walkthrough.capture` | session_id, comment?, as?: view\|bcf | id | Yes | ADV-WALK-001 |
| `walkthrough.set_time` | session_id, datetime | () | No (session state) | ADV-WALK-001 |
| `walkthrough.set_option` | session_id, key, value | () | No (session state) | ADV-WALK-001 |
| `xr.start` | view_id?, backend_id?, mode: vr\|ar\|tabletop, start? | session_id | No | ADV-XR-001 |
| `xr.stop` | session_id | () | No | ADV-XR-001 |
| `xr.align` | session_id, method: survey_point\|two_point, points? | () | No (session state) | ADV-XR-001 |
| `xr.set_option` | session_id, key, value | () | No (session state) | ADV-XR-001 |
| `xr.runtime_info` | — | runtime_info | No (query) | ADV-XR-001 |

## 7.4 Open questions and deviations from the reference

**Open questions.**

| # | Question | Proposed handling |
|---|----------|-------------------|
| Q7-1 | Embedded (LibGodot) or child-process hosting as the default for the Godot adapter | Prototype both in the proof of concept; prefer embedded for latency, keep child process for crash isolation |
| Q7-2 | Licensing and support of the exported standalone walkthrough package (Godot export templates, store distribution for headsets) | Check the export templates' licences and each headset store's terms before offering store distribution |
| Q7-3 | visionOS and phone/tablet AR | Track upstream Godot work; revisit after the initial version |
| Q7-4 | Multi-user review sessions (networking, identity, voice) | Deferred (P3); BIMserver identities are the likely basis |

**Deviations.** Everything in this chapter is beyond the reference product, which offers a built-in renderer, a vendor cloud renderer, and walkthrough animation only (see Chapter 5 D5-19).

## 7.5 Traceability

| Source | Requirements |
|--------|--------------|
| Owner directive 2026-10-12: pluggable renderer for future photo-real, game-engine or Blender walkthroughs | ADV-RSP-001, ADV-WALK-001, ADV-BRG-001 (with Ch 5 DOC-RBE-001/002) |
| Owner directive 2026-10-12: translation layer to Godot and similar engines for walkthrough and full VR/AR | ADV-XR-001 |
| Owner request 2026-10-12: research on Godot VR/AR support; public Godot, Khronos and press sources (SOURCE_REGISTER S0686–S0698) | ADV-XR-002, ADV-XR-001 (hosting) |
| Khronos glTF 2.0 and OpenXR public specifications (referenced by name) | ADV-RSP-001, ADV-XR-001 |

No reference-product sources were used in this chapter.
