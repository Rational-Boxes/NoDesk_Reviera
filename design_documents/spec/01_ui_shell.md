# Chapter 1 — User Interface Shell

**Area code:** UI · **Status:** Draft 1 · **Date:** 2026-10-12 · **Licence of this document:** GPL-3.0-or-later (as part of the Reviera source tree)
**Primary source:** `research/01_ui_shell.md` (cited below as `research/01 §n`). Screenshots under `screenshots/ui/` are internal reference only and are never shipped as Reviera assets.

---

## 1.0 About this chapter

### 1.0.1 Scope
This chapter specifies the application shell of Reviera: every window region, palette, toolbar, menu, in-canvas control and interaction convention that a user meets before and while editing a model. It also specifies how the shell is built on wxWidgets, how every visible control is bound to the single command registry, how UI state is exposed to Lua and MCP, the built-in AI agent pane, the BIMserver-mode user interface, accessibility, HiDPI and persistence.

The chapter **does not** specify the modelling behaviour behind the tools that appear in the ribbon. Each tool in the ribbon catalog (§1.8) names its owning chapter by area code and the command it invokes. The owning chapter is authoritative for that command's parameters, rules and IFC mapping.

| Owner code | Owning chapter |
|---|---|
| UI | Chapter 1 (this chapter) |
| DM | Chapter 2 — Data model, families, parameters, project setup, file I/O, Reviera Extension Data (RXD), extensibility (Lua, add-ins, MCP) |
| ARC | Chapter 3 — Architectural modelling and modify tools (includes datums, work planes, sketching, generic modify tools) |
| STR | Chapter 4 — Structure, steel, concrete/precast |
| MEP | Chapter 4 — Mechanical, electrical, plumbing, fabrication |
| ANA | Chapter 4 — Analysis (structural analytical model, energy, loads, route, carbon) |
| DOC | Chapter 5 — Views, graphics, annotation, schedules, sheets, output |
| COL | Chapter 6 — Collaboration, linking, coordination, BIMserver |

### 1.0.2 Terminology used in this chapter

| Term | Meaning in Reviera |
|---|---|
| **Shell** | The main application frame and everything in it except the content drawn inside a view. |
| **Command** | A typed, registered operation (§1.1.1). The only way the shell changes the model or the application state. |
| **Tool** | A UI affordance (button, menu item, gallery entry, key sequence) that is bound to a command. |
| **Tool session** | An interactive, multi-step collection of command arguments in the canvas (picks, sketches, typed values) that ends by invoking a command, or is cancelled. |
| **Context** | The combination of active document, active view, active tool session, selection set and edit mode (sketch, group edit, family edit) that drives what the shell shows. |
| **Contextual tab** | A ribbon tab that exists only while a given context holds (§1.9). |
| **Pane** | A dockable window managed by the docking manager (Properties, Project Browser, System Browser, Agent, …). |
| **View window** | A dockable document window that displays one view, sheet or schedule. |
| **Orientation Cube** | Reviera's 3D orientation widget (functional counterpart of the reference's cube widget). |
| **Navigation Wheel** | Reviera's cursor-following radial navigation menu (functional counterpart of the reference's wheel menus). |
| **Command Search** | Reviera's search box for commands, help, views and files (counterpart of the reference's title-bar help search). |
| **Theme token** | A named colour or metric (for example `canvas.selection`) used by all drawing code instead of literal colours. |

### 1.0.3 Design principles
1. **One registry, many surfaces.** Ribbon, Quick Access Toolbar, menus, context menus, key sequences, Command Search, Lua and MCP all draw on the same command registry. Nothing in the shell performs model work directly.
2. **Context in, presentation out.** A single *context engine* decides what the ribbon, Properties palette, status bar and in-canvas controls show. Surfaces are renderers of that state.
3. **Tool options are data.** Options for an active tool are a declarative descriptor rendered by the shell into contextual ribbon panels and the Properties palette (§1.10). There is no separate Options Bar.
4. **Never block the UI thread.** All model work runs on the core's worker pool through the command queue; the shell remains responsive and shows progress (TECH_STACK §3.3).
5. **Keyboard first, mouse friendly.** Every command is reachable without a mouse (key sequence, keytips or Command Search); every key action has a visible equivalent.
6. **Own look.** Reviera uses its own icon set, colours and wording. No reference artwork is used.
7. **Mode-agnostic shell.** Local and BIMserver modes share the same shell; mode-specific elements appear only where behaviour differs (§1.32).

---

## 1.1 Shell architecture

### 1.1.1 Command descriptors and UI metadata
**ID:** UI-ARCH-001 · **Priority:** P0 · **Ref:** research/01 §27 items 1–2; TECH_STACK §2

**Purpose.** Give every registered command the metadata the shell needs to present it, so that UI surfaces are generated from the registry rather than hand-written.

**Behaviour & rules.**
- Every command in the registry shall carry a **UI descriptor** with the following fields. Fields marked *req* are required for any command that appears on a UI surface.

| Field | Type | Notes |
|---|---|---|
| `id` *req* | string `domain.verb[.sub]` | Stable; never localised; used by shortcuts, QAT, Lua, MCP. |
| `label` *req* | localisable string | Short name shown on buttons and menus. |
| `label_long` | localisable string | Used in Command Search, the shortcut dialog and tooltips when `label` is ambiguous. |
| `icon` | icon key | Resolves to an SVG in the active icon theme (§1.28). |
| `tooltip` *req* | localisable string | One sentence (first tooltip stage, §1.25). |
| `tooltip_extended` | localisable rich text + optional illustration key | Second tooltip stage. |
| `help_topic` | string | Topic key into the offline help (F1). |
| `ribbon_paths` | list of `tab/panel[/split]` | Zero or more locations. The ribbon is built from these plus the layout file (§1.8.1). |
| `default_shortcuts` | list of key bindings | From the default map (§1.26.5). |
| `invocation` *req* | enum | `direct`, `interactive`, `toggle`, `dialog`, `menu` (see UI-ARCH-002). |
| `availability` *req* | predicate id | Evaluated by the context engine; decides enabled/disabled and visible/hidden. |
| `toggle_state` | state query id | For `toggle` commands, returns on/off for highlighting. |
| `discipline` | set of discipline flags | Used by discipline filtering (UI-RIB-007). |
| `priority_tier` | P0–P3 | Builds can omit commands above a configured tier. |
| `exposure` | flags `ui`, `lua`, `mcp` | Default all three. A few shell-internal commands are not exposed to MCP (for example `agent.*`, §1.31). |
| `undoable` | bool | Mirrors the core definition; shown in catalogs and used by the agent pane. |
| `reserved_keys` | bool | True when its shortcut is reserved (§1.26.3). |

- Commands contributed by Lua add-ins shall provide the same descriptor through their manifest; the shell treats them exactly like built-in commands.
- A command without `label`, `tooltip`, `invocation` or `availability` shall be rejected at registration if it declares any `ribbon_paths` or `default_shortcuts`.

**Commands.** `ui.command.list(filter?) -> [descriptor]`, `ui.command.describe(id) -> descriptor`.

**Acceptance criteria.**
- [ ] Removing a command from the registry removes it from the ribbon, QAT, menus, Command Search and shortcut dialog without any other code change.
- [ ] A Lua add-in that registers a command with a `ribbon_paths` entry produces a working button on the next ribbon rebuild.
- [ ] `ui.command.describe("wall.create")` over MCP returns the label, tooltip, shortcuts and ribbon paths.

### 1.1.2 Invocation kinds and the UI-to-core path
**ID:** UI-ARCH-002 · **Priority:** P0 · **Ref:** research/01 §6.1, §27 item 2; TECH_STACK §3.3

**Purpose.** Define how activating a tool turns into a command execution.

**Behaviour & rules.**
- **direct**: the command runs immediately with arguments taken from the current context (for example `element.delete` with the current selection). The shell places a request on the core command queue and returns at once.
- **interactive**: the shell starts a **tool session** (UI-ARCH-003). The session collects arguments from picks, sketches, typed values and tool options, and then submits one or more command requests. A session may stay active after a submission ("chain" or repeated placement) until the user presses Esc or picks another tool.
- **toggle**: the command flips a boolean state (for example Thin Lines). The button is drawn in its pressed state while `toggle_state` is true.
- **dialog**: the shell opens a dialog; the dialog's OK/Apply submits commands. Cancel submits nothing.
- **menu**: the tool only opens a drop-down (for example a split button's arrow or a "Plan Views" drop-down).
- Every model-changing submission runs inside a core transaction. A tool session that submits several commands as one user action (for example a wall chain finished with Esc) shall wrap them in one **transaction group** so a single Undo reverts the whole action.
- While the core is executing a request, the shell shall stay responsive: navigation, hover highlighting and cancel remain available. If a request runs longer than 500 ms, the status bar shows a progress indicator with a Cancel button where the command supports cancellation.
- Commands submitted from Lua or MCP while a UI tool session is active are queued; they do not interleave with an open transaction. The status bar shows "External request waiting" until the session ends or the user allows it to proceed (UI-API-006).

**Acceptance criteria.**
- [ ] Clicking Delete with three elements selected produces one undo entry named after the command.
- [ ] Drawing a four-segment wall chain and pressing Esc produces one undo entry.
- [ ] During a 10-second regeneration the user can still orbit the active 3D view.

### 1.1.3 Context engine
**ID:** UI-ARCH-003 · **Priority:** P0 · **Ref:** research/01 §8, §10, §27 item 3

**Purpose.** Compute, in one place, everything the shell needs to know about the current situation.

**Behaviour & rules.**
- The context engine holds: active document; active view window; active tool session (command id, step, tool-option values); selection set (element ids grouped by category); edit mode stack (none, sketch, group edit, in-place family edit, family editor document, view-on-sheet activation, reveal-hidden mode); selection option toggles (UI-SEL-004); operating mode (Local/BIMserver) and connection state.
- On any change it publishes a **context snapshot** to all surfaces. Surfaces never query the model directly to decide visibility; they read the snapshot and evaluate command `availability` predicates against it.
- Snapshots are immutable and versioned. The ribbon, Properties palette, status bar and canvas overlays shall update within one UI frame (≤ 16 ms budget for the shell side) after a snapshot is published for selections of up to 10 000 elements; larger selections may degrade the Properties palette to "computing…" while the common parameter set is computed off-thread.
- The engine also exposes the snapshot to Lua and MCP (§1.30) and emits change notifications.

**Acceptance criteria.**
- [ ] Selecting a wall shows the wall contextual tab, wall instance properties and wall drag controls, all from one snapshot change.
- [ ] Selecting 50 000 elements by window does not freeze the UI; the Properties palette shows a progress state and then the common parameters.

### 1.1.4 Tool option descriptors
**ID:** UI-ARCH-004 · **Priority:** P0 · **Ref:** research/01 §9 (spec note), Figure 20 `screenshots/ui/revit2027_options_bar_removed.png`, `screenshots/ui/options_bar.png`

**Purpose.** Represent per-tool options (height, location line, chain, offset, radius, and so on) as data so they can be rendered consistently.

**Behaviour & rules.**
- An interactive command declares a list of **tool options**, each with: key, label, value type (bool, enum, length, angle, number, element reference such as a level), default, persistence (`session`, `user`, `none`), and placement hint (`ribbon`, `properties`, `both`).
- The shell renders options with placement `ribbon` or `both` as controls in contextual ribbon panels (§1.9.3), and options with `properties` or `both` as a "Tool Options" group at the top of the Properties palette.
- Changing an option while the tool session is active affects the next submission; for options that alter an in-progress preview (for example Location Line) the preview updates immediately.
- Option values with persistence `user` are remembered per command between sessions (§1.29).
- Tool options are readable and writable from Lua and MCP while a tool session is running (`tool.get_options`, `tool.set_option`).

**Acceptance criteria.**
- [ ] The wall placement tool shows Height/Depth, top constraint, Location Line, Chain, Offset and Radius in a contextual panel, and the same values in the Properties "Tool Options" group; changing one updates the other.
- [ ] Restarting Reviera keeps the last-used wall Location Line.

### 1.1.5 wxWidgets realisation
**ID:** UI-ARCH-005 · **Priority:** P0 · **Ref:** TECH_STACK §3.4; research/01 §1, §27

**Purpose.** Fix how each shell element is built on wxWidgets (3.3 or later) so implementation choices are consistent.

**Behaviour & rules.**

| Shell element | wxWidgets realisation | Custom work required |
|---|---|---|
| Main frame | `wxFrame` with native decorations; `wxAuiManager` owns all panes | Frame-level DPI change handling |
| Quick Access Toolbar | `wxAuiToolBar` in a slim strip directly above the ribbon (default) or below it | Customise drop-down; split buttons with history |
| Command Search / help | `wxSearchCtrl` at the right of the QAT strip; results in a `wxPopupTransientWindow` hosting a virtual `wxDataViewCtrl` | Fuzzy matcher over the registry |
| File menu | Custom-drawn "File" button at the left of the ribbon tab row opening a full-height `wxPopupTransientWindow` with a two-column layout; on macOS the native `wxMenuBar` also exposes File, Edit, View, Window and Help | Split items, right pane switching |
| Ribbon | `wxRibbonBar` with `wxRibbonPage`, `wxRibbonPanel`, `wxRibbonButtonBar`, `wxRibbonToolBar`, `wxRibbonGallery`; a Reviera `wxRibbonArtProvider` subclass drawing from theme tokens | Contextual pages (show/hide pages plus page highlight), slide-out pinning, panel floating, Ctrl-drag tab reorder, four minimise modes, keytips, embedded option controls |
| Dialog launchers | `wxRIBBON_PANEL_EXT_BUTTON` on `wxRibbonPanel` | Binding to command ids |
| Contextual tabs | Pre-created hidden `wxRibbonPage`s shown with `ShowPage` and marked with `AddPageHighlight`; generated content per context | Context-driven rebuild and caching |
| Dockable panes | `wxAuiManager` + `wxAuiPaneInfo` (Properties, Project Browser, System Browser, Agent, Notifications, Server, Lua Console, add-in panes) | Grouping panes as tabs, persistence keyed by monitor layout |
| View windows | `wxAuiNotebook` in the centre pane, with split support; torn-off views go into secondary `wxFrame`s each holding its own `wxAuiNotebook` | Tear-off/re-dock, cross-frame Ctrl+Tab |
| Canvas | A plain `wxWindow` whose native handle hosts the GPU surface of the renderer (TECH_STACK §3.2) | All canvas overlays (temporary dimensions, grips, snaps, Orientation Cube, Navigation Bar, wheels) are drawn by the renderer's overlay pass, not by child windows |
| In-canvas text entry | A borderless `wxTextCtrl` positioned over the canvas for temporary-dimension and listening-dimension input | Positioning under DPI scale |
| View Control Bar | A `wxPanel` docked at the bottom of each view window with `wxBitmapButton`s and popup menus | Per-view-type content |
| Properties palette | `wxPropertyGridManager` with categories, custom editors (unit-aware length, element picker, "Edit…" buttons, global-parameter association), read-only shading, `<varies>` display | Type Selector header, filter combo, Apply semantics |
| Type Selector | `wxComboCtrl` with a custom `wxComboPopup` holding a `wxSearchCtrl` and a virtual list with thumbnails | Keyword search and match highlighting |
| Project Browser | `wxDataViewCtrl` with a virtual tree model, `wxSearchCtrl`, and a tab strip of `wxBitmapToggleButton`s | Live filtering and highlight, drag to canvas/sheet |
| System Browser | `wxDataViewCtrl` (tree with columns) and a small toolbar | Column settings, hover pre-highlight sync |
| Status bar | A custom `wxPanel` docked at the bottom by AUI (not `wxStatusBar`) so it can host combos and toggles | Shortcut-match display, background task popup |
| Tooltips | `wxToolTip` for plain controls; a custom `wxPopupWindow` for two-stage ribbon tooltips | Delay state machine |
| Keytips | Overlay of small `wxPopupWindow` badges over the ribbon and QAT | Key routing while visible |
| Options dialog | `wxDialog` with a `wxTreebook` and a search box filtering pages | Per-setting lock indicator |
| Notifications | In-app toasts in a custom `wxPopupWindow` stack, plus `wxNotificationMessage` for OS notifications when the app is not focused | Notification centre pane |
| Agent pane | AUI pane with a `wxHtmlWindow` transcript and multi-line `wxTextCtrl` input | Tool-call cards, approval buttons |
| Themes | `wxApp::SetAppearance` (light/dark/system) on platforms that support it; `MSWEnableDarkMode` on Windows; Reviera art providers read theme tokens | Canvas theme is independent and handled by the renderer |
| Icons | `wxBitmapBundle::FromSVG` for every icon | Icon theme packs (light/dark/high-contrast) |
| Accessibility | Native controls rely on platform accessibility; custom-drawn controls implement `wxAccessible` (Windows) and platform equivalents through the binding layer | See §1.33 |

- The Rust↔wxWidgets binding is **`wxDragon`** (TECH_STACK §3.4, §5); any extensions are kept as clean patches offered upstream. This chapter is written against a **UI bridge** interface: the Rust core publishes context snapshots and accepts command requests; the C++ or Rust-wrapped wx side renders. Neither option may let wx code reach into the element database directly.

**Acceptance criteria.**
- [ ] On Windows, Linux (GTK 3) and macOS the ribbon, docking, view tabs and Properties palette function identically apart from platform-native decorations.
- [ ] No canvas overlay is implemented as a native child window over the GPU surface (verified by code review checklist).

### 1.1.6 Threading and responsiveness
**ID:** UI-ARCH-006 · **Priority:** P0 · **Ref:** TECH_STACK §3.3, §3.7.1

**Purpose.** Fix which thread does what, so that no model, network or script work can freeze the window and no worker thread can corrupt a widget.

**Behaviour & rules.**
- The wx event loop runs on the main thread and owns every widget. Core results reach the UI thread through a queued event (`CallAfter` equivalent); no core thread touches a widget.
- Network operations (BIMserver, agent model provider, MCP over HTTP) run on the async runtime and report progress through the same mechanism.
- Long-running UI-thread work (for example populating a 100 000-row Project Browser) shall be virtualised or chunked so that no single UI-thread slice exceeds 50 ms.
- **Thread roles.**

| Thread | Owns | Never does |
|---|---|---|
| Main (UI) | wx widgets, input routing, tool-session state machine, context-snapshot rendering | Model reads beyond the published snapshot; blocking I/O |
| Core worker pool | Command execution, regeneration, snapshot building, snap queries | Widget calls |
| Live-store writer | Group-committing change sets to the local store (Ch. 2b) | Anything on the UI's critical path |
| Async runtime | BIMserver, agent provider, MCP over HTTP, update checks | Model writes except through the command queue |
| Scripting | Lua handlers and scripts | Direct widget calls (it uses commands such as `ui.notify`) |
| Renderer | GPU submission for every canvas | Blocking on regeneration (it draws the last committed scene) |

- **Back-pressure.** When more than 200 queued UI events from the core are pending, the bridge coalesces events of the same kind (snapshot, progress, notification) and keeps only the newest of each, so a burst of commits cannot flood the event loop.
- **Modal dialogs** run a nested event loop on the main thread; core results that arrive meanwhile are still delivered and applied to non-modal surfaces.
- **Debug watchdog.** Debug and test builds run a watchdog that samples the main thread every 50 ms and logs any stall above 250 ms with a stack trace. Release builds keep only a counter, visible in Help › About › Diagnostics.
- **Shutdown.** `app.exit` asks the core to finish or cancel the running command, flushes the live store and waits at most 5 s for worker threads before closing windows; it never closes a widget while a worker could still post to it.

**Acceptance criteria.**
- [ ] With a 500 MB model open, a UI watchdog in debug builds records no main-thread stall above 250 ms during open, select-all, and view switching.
- [ ] A test that calls a widget method from a worker thread fails a debug assertion.
- [ ] During a BIMserver download over a throttled 1 Mbit/s link, the user can pan, select and open the Options dialog.
- [ ] Committing 1 000 small transactions from a Lua loop produces no more than one snapshot redraw per UI frame.

---

## 1.2 Layout map

**ID:** UI-LAYOUT-007 · **Priority:** P0 · **Ref:** research/01 §1 · `screenshots/ui/ui_overview_annotated.png`, `screenshots/ui/ui_insert_tab_floor_plan.png`

**Purpose.** Define the default arrangement of the main window.

**Default layout (top to bottom).**

| Zone | Contents | Realisation |
|---|---|---|
| Native title bar | Platform window decorations; window title `<model name>[*] — <view type>: <view name> — Reviera`; `*` marks unsaved changes; in BIMserver mode the title adds `@ <server>/<project> r<revision>` | `wxFrame` title |
| QAT strip | Home button, Quick Access Toolbar (§1.5), document title area (optional), right side: Command Search (§1.6), server connection indicator (BIMserver mode), Agent pane toggle, Help menu | `wxAuiToolBar` |
| Ribbon tab row | File button, permanent tabs, add-in tabs, contextual tabs (accent coloured), ribbon state controls at the far right | `wxRibbonBar` |
| Ribbon panel area | Panels and tools of the active tab; panel titles along the bottom with slide-out arrows and dialog launchers | `wxRibbonBar` |
| Left dock | Properties palette (top) and Project Browser (below), both panes | AUI left dock |
| Centre | View windows as tabs (tiling and splits allowed); each window has a View Control Bar at bottom-left and scroll bars where applicable; 3D windows show the Orientation Cube (top-right) and Navigation Bar (right edge) | `wxAuiNotebook` |
| Right dock (empty by default) | Agent pane, System Browser, Notifications, Server pane, Lua Console when opened | AUI right/bottom docks |
| Status bar | Prompt, progress, mode and selection controls (§1.20) | Custom panel docked at bottom |

**Behaviour & rules.**
- The layout is fully rearrangeable (§1.15) and restorable to this default with `ui.layout.reset`.
- Every zone except the ribbon tab row and the centre can be hidden from View › Windows › User Interface (UI-DOCK-004).
- Minimum supported window size: 1024 × 640 logical pixels; below that, the ribbon collapses panels (UI-RIB-002) and docks auto-hide.
- **Bookmarked layouts**: the user can save the whole arrangement under a name and switch between saved arrangements instantly. This covers panes, ribbon, QAT, visible panels and, optionally, theme, scale and open views. Specified in §1.29.4 (UI-LAYOUT-001…006).

**Acceptance criteria.**
- [ ] A fresh profile starts with the layout in the table above on all three platforms.
- [ ] `ui.layout.reset()` restores that layout without restarting.
- [ ] Saving the layout "Docs" with the Project Browser floating, then switching to "Default" and back, restores the floating browser (see also §1.29.4).

---

## 1.3 Reviera Home

### 1.3.1 Home screen
**ID:** UI-HOME-001 · **Priority:** P0 · **Ref:** research/01 §2, §2.1, §2.2 · `screenshots/ui/home_classic.png`, `screenshots/ui/home_new_recent.png`, `screenshots/ui/home_new_projects.png`

**Purpose.** Provide a start page for opening and creating models and families, and a place to return to between tasks.

**Workflow.**
1. Reviera starts and shows Home in place of the ribbon and view area.
2. The user opens a recent item, browses for a file, creates a new model from a template, or connects to a BIMserver project.
3. Home closes and the ribbon and views appear. **Ctrl+D** or the QAT Home button returns to Home; the back arrow (or Ctrl+D again, or Esc) returns to the active model.

**UI.**
- **Left rail**:
  - *Models*: **Open…** (`file.open`), **New…** (`file.new` with the New Model dialog: template list from Options › File Locations, plus "Model / Model template" choice and schema choice IFC4 or IFC4X3 — the latter only in Local mode).
  - *Families*: **Open…** (`family_doc.open`), **New…** (`family_doc.new`, opens the family template chooser).
  - Pages: **Recent**, **Servers** (BIMserver mode entry point, §1.32.2), **Samples & Learning** (bundled sample models and the offline manual).
  - Footer links: What's New (local release notes), Manual (offline help), Community (project website), Report a Problem (opens the issue tracker URL in the browser), Licence & Source (GPL notice and source location).
- **Recent page** (right area):
  - Grid view (thumbnails) or list view (columns: Name, Pinned, Modified, Location, Kind, Size, Mode). Columns can be shown/hidden and reordered.
  - Two groups: Models and Families. Server projects appear in Models with a server badge.
  - Search box filtering by name and location; Filter flyout by modified date (All / Today / Last 7 days / Last 30 days / Last 90 days) and kind (Models / Families / Templates).
  - Pin (keeps an item at the top and exempt from list truncation), Remove from list, Open containing folder, Open read-only (Local mode) and Open revision… (server items).
  - Missing files are shown greyed with a "Locate…" action.
- **Thumbnail source**: the preview image that Reviera stores with the model's Reviera Extension Data at save (Ch. 2); when absent, a generic kind icon. Server items use a per-user local thumbnail cache.

**Behaviour & rules.**
- Home is shown at start-up unless the user launched Reviera with a file argument or chose "Reopen last session" in Options (UI-PREF-002).
- The Recent list holds up to 50 items by default (configurable 0–200; 0 hides the list). It is stored per user, never inside a model.
- While Home is visible, keyboard shortcuts for model commands are inactive except reserved file shortcuts (Ctrl+N, Ctrl+O) and Ctrl+D.
- Home uses only local data plus configured servers; it never contacts any service the user has not configured.
- There is no usage-analytics page (the reference's "insights" page is not reproduced; see §1.35).

**Commands.** `ui.home.show()`, `ui.home.hide()`, `ui.home.toggle()`, `ui.recent.list(kind?) -> [RecentItem]`, `ui.recent.pin(path_or_uri, pinned)`, `ui.recent.remove(path_or_uri)`, `ui.recent.clear()`.

**Acceptance criteria.**
- [ ] Starting Reviera with no arguments shows Home; Ctrl+D with a model open toggles between Home and the model.
- [ ] A pinned item remains at the top after 60 other files are opened.
- [ ] Filtering by "Families" and "Last 7 days" shows only families modified in the last week.
- [ ] A recent server project opens through the server login flow if no valid session token exists.

---

## 1.4 File menu

**ID:** UI-FILE-001 · **Priority:** P0 · **Ref:** research/01 §3 · `screenshots/ui/file_tab_menu.png`

**Purpose.** Offer document-level operations (new, open, save, export, print, close), the recent/open document lists, Options and Exit.

**Workflow.**
1. The user clicks **File** (left end of the tab row) or presses Alt, F.
2. A full-height panel opens. The left column lists item groups; hovering or clicking an item's arrow shows its sub-items in the right pane; clicking an item's main area runs its default.
3. Clicking outside or pressing Esc closes the panel.

**UI.** Left column items, their default action and their sub-items:

| Item | Default (click) | Sub-items (right pane) | Commands (owner) | Pri |
|---|---|---|---|---|
| New | New Model dialog | Model; Family; Conceptual Mass Family; Title Block; Annotation Symbol; Model Template | `file.new(kind, template?)` (DM) | P0 (mass/title block/symbol P1) |
| Open | Open dialog | Model (IFC/IFCZIP); Family; From Server…; Sample Files; IFC open options… | `file.open(path, options?)`, `family_doc.open`, `bimserver.open` (COL), `cde.select_documents` (*From CDE…*, COL), `file.ifc_open_options` (DM) | P0 (server P1, CDE P2) |
| Save | Save (writes the model using its current RXD carrier: embedded, or `.ifc` + `.reviera` sidecar written atomically) | — | `file.save(document_id? (default: active)) -> status` (DM) | P0 |
| Save As | Save As dialog with an **Options…** button (RXD carrier, below) | Model; Family; Template; Library ▸ Family / Group / View | `file.save_as(document_id? (default: active), path, kind?, format?: ifc\|ifczip, carrier?: embedded\|external_file\|external_dir\|bundle) -> status`, `family.save_as`, `group.save`, `view.save_to_library` (DM) | P0 (library P1) |
| Export | — | CAD ▸ DWG, DXF, DGN, SAT, STL; Images & Animations ▸ Image, Walkthrough, Sun Study; Reports ▸ Schedule, Room/Area Report; PDF; OBJ; FBX; glTF; gbXML; Family Types; Database (ODBC/SQLite); IFC ▸ (with Reviera data, Clean IFC (strip Reviera data), another schema or MVD); BCF; Export options ▸ (CAD export setups, IFC export setups) | `export.*` (DOC/DM) | PDF P0; CAD, image, schedule P1; others P2 |
| Print | Print dialog | Print; Print Preview; Print Setup; Batch Print | `print.*` (DOC) | P0 (batch P2) |
| Server | — (only shown when BIMserver features are enabled) | Connect…; Publish to Server…; Check In…; Get Latest; Sync; Export Local Copy…; Disconnect | `server.*` (COL) | P1 |
| Close | Close active model (*built: File › Close Model, Ctrl+Shift+W (Ctrl+W closes the view), asking to save changes*) | — | `file.close()` (DM) | P0 |

- **Right pane, default content**: **Recent Documents** with a sort control (Most recent first, By name, By size, By kind) and pins; a toggle switches to **Open Documents**, which lists every open view of every open model; clicking a view activates it (`view.activate`).
- **Footer**: **Options** (`ui.options.open`) and **Exit Reviera** (`app.exit`).

**Behaviour & rules.**
- File is not a ribbon page; it cannot be reordered or hidden.
- Items are disabled when unavailable (for example Save with no model open; Check In when not connected).
- **Reviera Extension Data (RXD) handling (Local mode).** Reviera-specific project data that plain IFC cannot express (Reviera Extension Data, "RXD"; format defined in `02a_extension_data_format.md`, Ch. 2) is **embedded in the IFC file as property sets by default**, so a model is normally one `.ifc` file. An external sidecar `<name>.reviera` is an optional alternative carrier. The shell behaves as follows:
  - **Save As options** (Options… button in the Save As dialog, also in Options › File Locations as the default for new models):
    - **RXD carrier**: ( ) *Embed Reviera data in the IFC (recommended)* — default; ( ) *Write separate sidecar file* — writes `<name>.ifc` plus `<name>.reviera`.
    - The chosen carrier is remembered for the model and used by subsequent **Save**. With the sidecar carrier, Save writes both files as one atomic operation (both written to temporary files, then both replaced); if either write fails, neither original is replaced and an error dialog names the failing file.
  - **Export clean IFC** (File › Export › IFC › *Clean IFC (strip Reviera data)*, `export.ifc(strip_rxd=true, path, schema?)`, P0): writes a copy without any RXD property sets or entities, for exchange with tools that dislike unknown property sets. It never changes the open model's path or carrier, and the dialog states that parametric intent (constraints, sketches, view definitions and similar) will not be in the copy.
  - **Open**: RXD embedded in the IFC is read automatically. When the model uses the sidecar carrier, opening `<name>.ifc` also reads `<name>.reviera` when present; selecting a `.reviera` file in the Open dialog opens its paired IFC.
  - **RXD missing** (a foreign IFC with no RXD, or an expected sidecar not found): the model opens with default UI and documentation settings and a **non-blocking notice** in the notification area: "This model has no Reviera data; defaults are in use. It will be added when you save." For a missing sidecar the notice also offers "Locate data file…" (`file.locate_rxd`).
  - **RXD stale** (RXD records refer to GlobalIds that no longer exist, or its recorded model fingerprint differs, for example after the IFC was edited in another tool): Reviera opens the model, reconciles automatically by GlobalId (Ch. 2 rules), and shows a **reconciliation report** — a notice that opens a dialog listing what was kept, what was re-attached, what was dropped as orphaned (with element and record identifiers), and what was reset to defaults. The report offers **Keep reconciled data** (default; takes effect on next Save), **Discard Reviera data and use defaults**, and **Save report…** (text/CSV). When reconciliation would drop more than a threshold (default 10 % of records) the report opens automatically as a modal dialog instead of a notice.
  - Project-scoped UI data (open views and their tab/tile arrangement, Project Browser state, selection sets, browser organisation schemes) are **RXD documents** stored with the model through whichever carrier it uses (§1.29). Per-user preferences are never written into RXD.
- Closing a model with unsaved changes prompts Save / Don't Save / Cancel; in BIMserver mode a further prompt offers Check In when there are local changes not checked in.
- On macOS the same items are mirrored in the native menu bar's File menu; Options appears as the application "Settings…" item and Exit as "Quit Reviera".

**Acceptance criteria.**
- [ ] Clicking the main area of Open shows the Open dialog; clicking its arrow shows the Open sub-items without opening a dialog.
- [ ] The Open Documents list contains every view window, including ones floated on a second monitor, and activating one brings its frame to front.
- [ ] Every item in the table is reachable with keytips (§1.25.3).
- [ ] A new model saved with default options produces a single `.ifc` file that reopens with its open views, browser state and selection sets restored.
- [ ] Choosing "Write separate sidecar file" produces `.ifc` + `.reviera`; killing the process during a later Save leaves both previous files intact and mutually consistent.
- [ ] Export clean IFC produces a file containing no RXD property sets, and the open model's carrier and path are unchanged.
- [ ] Opening a foreign IFC without RXD shows the defaults notice and no modal dialog.
- [ ] Opening a model whose RXD references 5 deleted GlobalIds shows a reconciliation report listing those 5 records as orphaned.

---

## 1.5 Quick Access Toolbar

**ID:** UI-QAT-001 · **Priority:** P0 · **Ref:** research/01 §4 · `screenshots/ui/quick_access_toolbar.png`, `screenshots/ui/qat_customize_dropdown.png`

**Purpose.** Keep the most-used commands one click away regardless of the active ribbon tab.

**Default contents.**

| # | Tool | Kind | Command (owner) |
|---|---|---|---|
| 0 | Home | button | `ui.home.toggle` (UI) |
| 1 | Open | button | `file.open` (DM) |
| 2 | Save | button | `file.save` (DM) |
| 3 | Sync with Server | split (Sync now / Sync with options…) — visible only in BIMserver mode | `bimserver.sync` (COL) |
| 4 | Undo | split with history list | `edit.undo(steps)` (DM) |
| 5 | Redo | split with history list | `edit.redo(steps)` (DM) |
| 6 | Print | button | `print.run` (DOC) |
| 7 | Export PDF | button | `export.pdf` (DOC) |
| 8 | Activate Controls | toggle | `ui.controls.activate` (UI) |
| 9 | Measure | split (Between Two References / Along Element) | `measure.between_references`, `measure.along_element` (UI) |
| 10 | Aligned Dimension | split (all dimension kinds) | `dimension.create_*` (DOC) |
| 11 | Tag by Category | split (Tag by Category / Tag All) | `tag.create`, `tag.tag_all` (DOC) |
| 12 | Text | button | `text.create` (DOC) |
| 13 | Default 3D View | split (Default 3D / Camera / Walkthrough) | `view.create_3d` etc. (DOC) |
| 14 | Section | button | `view.create_section` (DOC) |
| 15 | Thin Lines | toggle | `ui.thin_lines.toggle` (UI) |
| 16 | Close Inactive Views | button | `view.close_inactive` (UI) |
| 17 | Switch Windows | drop-down of open views | `window.switch` (UI) |
| — | New | available, not shown by default | `file.new` (DM) |
| — | Customize ▾ | drop-down: tick/untick defaults; Customize…; Show Below/Above the Ribbon | — |

**Behaviour & rules.**
- Right-click on any ribbon tool, gallery or the Type Selector offers **Add to Quick Access Toolbar**. Tools inside contextual tabs whose availability depends on a transient context (for example Finish Sketch) may be added but are disabled outside that context.
- Right-click on a QAT item offers **Remove**, **Add Separator**, **Move Left/Right**, **Customize…**.
- The **Customize Quick Access Toolbar** dialog lists items with Move Up, Move Down, Add Separator, Remove, Reset to Default, and an "Add command…" picker that searches the whole registry (so commands without a ribbon location can be added).
- Undo/Redo drop-downs list transaction names with the originating source (UI, Lua, MCP agent name) and allow undoing several steps at once.
- Placement: above the ribbon (default) or below the ribbon.
- QAT configuration is stored in `ui-state.toml` (§1.29) and can be exported and imported with the ribbon layout.

**Commands.** `ui.qat.get() -> [item]`, `ui.qat.add(command_id, index?)`, `ui.qat.remove(index)`, `ui.qat.move(from, to)`, `ui.qat.add_separator(index)`, `ui.qat.set_position(above|below)`, `ui.qat.reset()`.

**Acceptance criteria.**
- [ ] Adding "Rotate" from its ribbon right-click menu puts it at the end of the QAT; it survives restart.
- [ ] The Undo drop-down shows entries made by an MCP agent with the agent's session name.
- [ ] Reset to Default restores the table above exactly.

---

## 1.6 QAT strip right side: Command Search, help and identity

### 1.6.1 Command Search
**ID:** UI-SEARCH-001 · **Priority:** P0 · **Ref:** research/01 §5 · `screenshots/ui/infocenter_search.png`, `screenshots/ui/help_menu.png`

**Purpose.** One search box to find and run any command, open a view, open a recent file, or find a help topic. Replaces the reference's online-help search box with a local, broader tool.

**Workflow.**
1. The user clicks the search box or presses **Ctrl+K**.
2. As the user types, results appear grouped as **Commands**, **Views & Sheets** (active model), **Recent Files**, **Help Topics**, **Settings** (Options pages).
3. Arrow keys move; Enter runs the command, activates the view, opens the file or help topic, or opens Options at that setting. Esc closes.

**Behaviour & rules.**
- Matching is case-insensitive, accent-insensitive and fuzzy over label, long label, command id, synonyms declared in descriptors, and the shortcut keys. Wildcards `*` and `?` are honoured when present.
- Each command result shows icon, label, ribbon path(s), shortcut(s) and whether it is currently available. Unavailable commands are listed but cannot be run; the reason ("needs a selection", "only in plan views") is shown.
- Results are ranked by match quality, then by the user's recent usage (stored locally).
- Help search runs against the offline manual index bundled with the application; no network request is made unless the user enables "Search online documentation" in Options.

**Commands.** `ui.command_search(query, limit?) -> [SearchResult]`.

**Acceptance criteria.**
- [ ] Typing "algn" ranks Align first; Enter starts the Align tool.
- [ ] Typing a view name opens that view.
- [ ] With networking disabled, search still returns help topics.

### 1.6.2 Help menu and About
**ID:** UI-HELP-001 · **Priority:** P0 · **Ref:** research/01 §5

**Behaviour & rules.**
- A **Help** button at the right of the QAT strip opens the offline manual. Its drop-down lists: Manual, What's New, Keyboard Shortcut Reference (generated from the live shortcut map), Lua API Reference (generated from the registry), MCP Tool Reference (generated), Report a Problem, Community, Check for Updates (only if enabled), About Reviera.
- **F1** opens context help: for the tool under the cursor when its tooltip is visible, for the active dialog, or for the active tool session. Help topics are looked up by `help_topic` in the command descriptor or by dialog id.
- **About Reviera** shows version, build hash, licence (GPL-3.0-or-later) with the full text, third-party component licences, and where to obtain the corresponding source code (TECH_STACK §4.2 distribution duty).

**Acceptance criteria.**
- [ ] F1 while hovering the Wall button with its tooltip shown opens the wall help topic.
- [ ] About shows the GPL text and a list of third-party licences that matches the build's dependency manifest.

*Built (2026-10-14):* the **Help** menu with *Reviera Help* (F1) and *About BIM Reviera*. Help opens as a tab beside the views (a webview: WebKitGTK, WebView2 or WKWebView): a landing page with Getting started, Navigating, Selecting and modifying, Model settings and Scripting sections, and the **sample models** from the samples folder's `samples.json`, each with an *Open* link that opens a fresh copy of it (so the sample stays as shipped). Links `reviera:sample/<name>` and `reviera:action/<id>` reach the application; web links open in the system browser. *(Owner request, 2026-10-14: "Add the start of the help menu. Seed a landing hypertext help view. Help will be an inline tab/document-window. The help landing will link to the examples with a link that opens each sample file.")* Not yet: the manual's other pages, context help for the tool under the cursor, the generated references, and About's licence texts.

### 1.6.3 User identity
**ID:** UI-IDENT-001 · **Priority:** P0 (Local) / P1 (server) · **Ref:** research/01 §5, §24.1

**Behaviour & rules.**
- In Local mode the **user name** comes from Options › General (default: the operating-system login name). It is written into change attribution and ownership data.
- In BIMserver mode the user name shown and used for attribution is the account name of the active server session; the Options field is shown read-only with the note "Set by server login" while connected (§1.32).
- There is no vendor account sign-in. The QAT strip shows a **connection indicator** only in BIMserver mode (§1.32.5).
- **Validation.** The Local user name is 1–64 characters, trimmed, with no control characters; an empty field reverts to the operating-system login name on OK. The setting key is `general.user_name`.
- **Where the name is used.** Undo and History entries (with the source, §1.30.6), live-store change sets (Ch. 2b), saved-selection and view-bookmark authorship, the default check-in comment template, and ownership claims. The IFC `IfcOwnerHistory` written on save uses the same name (Ch. 2).
- **Change during a session.** A new name applies to entries created after OK; existing entries keep the name they were made with. Changing the name while elements are owned in Local mode shows a warning that ownership records keep the old name.
- **Agents and scripts.** Changes made by a Lua script or an MCP session are attributed to "<user name> via <script or session name>", never to the script alone.
- The user name is readable by scripts and agents with `ui.user.get() -> {name, source: options|server, server_uri?}`; it can only be changed through `ui.options.set("general.user_name", …)`, which fails while a server session is active.

**Commands.** `ui.user.get() -> {name, source, server_uri?}`.

**Acceptance criteria.**
- [ ] Changing the user name in Options changes the name recorded on new undo entries and ownership claims in Local mode.
- [ ] While connected to a server, the user-name field cannot be edited.
- [ ] Clearing the field and pressing OK restores the operating-system login name.
- [ ] A wall created by an MCP session named "planner" is attributed to "<user> via planner" in the History pane, and its Undo entry reads "Create wall — agent: planner" (§1.30.6).

---

## 1.7 Ribbon behaviours

### 1.7.1 Structure and control kinds
**ID:** UI-RIB-001 · **Priority:** P0 · **Ref:** research/01 §6.1 · `screenshots/ui/ribbon_contextual_tab.png`

**Purpose.** Organise commands as tabs → panels → tools.

**Behaviour & rules.**
- A **tab** holds panels; a **panel** holds tools and has a title strip along its bottom edge.
- Supported tool kinds:

| Kind | Description | wx realisation |
|---|---|---|
| Large button | Icon above label | `wxRibbonButtonBar` large button |
| Small button | Icon beside label; stacked 2 or 3 high | `wxRibbonButtonBar` small/medium button |
| Icon-only button | Icon with tooltip | `wxRibbonToolBar` tool |
| Split button | Main part runs the **last-used** member (remembered per user); arrow lists all members | `wxRIBBON_BUTTON_HYBRID` + menu; last-used stored in `ui-state.toml` |
| Drop-down button | Arrow only; opens a menu | `wxRIBBON_BUTTON_DROPDOWN` |
| Toggle button | Drawn pressed while on | `wxRIBBON_BUTTON_TOGGLE` bound to `toggle_state` |
| Embedded control | Combo, numeric/length entry, check box inside a panel (mostly contextual option panels) | `wxRibbonPanel` hosting a sizer with native controls |
| Gallery | Scrollable grid of choices with an expand arrow (type galleries, draw-shape gallery) | `wxRibbonGallery` |

- Every tool is bound to exactly one command id (split and drop-down members each bound to their own).
- Disabled tools remain visible and show the reason for unavailability in their tooltip.

**Acceptance criteria.**
- [ ] After choosing "Roof by Extrusion" from the Roof split button, the main part of the button runs Roof by Extrusion until another member is used, including after restart.

### 1.7.2 Adaptive resizing
**ID:** UI-RIB-002 · **Priority:** P0 · **Ref:** research/01 §6.1

**Behaviour & rules.**
- As available width shrinks, panels reduce in this order: large → small buttons; small → icon-only; finally the panel collapses into a single drop-down button that opens the full panel as a popup. Panels on the right collapse first; each panel may declare a collapse priority.
- Growing the window reverses the process.
- Collapsed panels stay keyboard- and keytip-reachable.
- **Reduction steps.** Each panel declares, in `ribbon-default.toml`, an ordered list of size variants it supports (`large`, `medium` = small buttons with labels, `small` = icon-only, `collapsed`). Panels without a declaration get all four. The layout pass removes one step at a time from the panel with the highest collapse priority (default: rightmost first) until the tab fits, so the result is deterministic for a given width.
- **Fixed parts.** The Select panel (§1.8.2) never collapses below small buttons. Embedded option controls (§1.10) shrink their label first, then become icon-plus-value, and only then move into the collapsed panel's popup.
- **Collapsed panel popup.** The single button shows the panel's icon and title; clicking it opens the full panel as a popup that closes after a tool is chosen or on Esc. Split buttons keep their last-used member inside the popup.
- **Labels.** When a large button becomes small or icon-only, its label moves into the tooltip; no label is truncated with an ellipsis inside the ribbon.
- **Hysteresis.** Layout recalculation runs at most once per frame during a window drag and uses 16 px of hysteresis, so panels do not flicker between two sizes at a boundary width.
- **Font scale.** The same algorithm runs after a UI font-scale or DPI change (§1.33.2, §1.33.3).

**Acceptance criteria.**
- [ ] At 1024 px wide the Architecture tab shows every panel, with at least one panel collapsed, and every tool can still be reached.
- [ ] Narrowing the window by 1 px steps from 1920 to 1024 px never changes the panel layout twice for the same width, and widening back restores the full layout.
- [ ] A collapsed panel's popup can be opened with keytips and its tools run from the keyboard.
- [ ] No ribbon label is shown truncated at any width.

### 1.7.3 Slide-out panels and dialog launchers
**ID:** UI-RIB-003 · **Priority:** P1 · **Ref:** research/01 §6.2, §6.3 · `screenshots/ui/ribbon_expanded_panel.png`, `screenshots/ui/ribbon_expanded_panel_pinned.png`, `screenshots/ui/ribbon_dialog_launcher.png`

**Behaviour & rules.**
- A panel may declare **overflow tools**. An arrow beside its title opens a slide-out showing them. The slide-out closes when the pointer leaves it or the user clicks elsewhere, unless the **pin** at its lower-left is set; a pinned slide-out stays open while its tab is active and is remembered per tab.
- A panel may declare a **dialog launcher** command, drawn as a small corner button in the title strip (for example Mechanical Settings on the HVAC panel). Launchers are ordinary commands (shortcut-assignable, QAT-addable).
- **Opening a slide-out.** Clicking the arrow, or pressing Down on a focused panel title, opens the slide-out directly below the panel at the panel's width or wider. It overlaps the canvas; it does not push the canvas down. Only one unpinned slide-out is open at a time.
- **Closing.** An unpinned slide-out closes when the pointer has been outside it for 500 ms, after a tool in it is chosen, on Esc, or when another tab is activated. A pinned slide-out ignores pointer exit and tool choice; it hides when its tab is not active and reappears when the tab is activated again.
- **Pin state** is stored per tab and panel in `ui-state.toml` and is part of the *Ribbon* part of a layout bookmark (UI-LAYOUT-001). It survives restart.
- **Minimised ribbon.** In the minimised states (UI-RIB-004) a pinned slide-out behaves as unpinned, because the panel itself is a temporary overlay.
- **Floating panels** (UI-RIB-005) show their overflow tools below the main tools with no slide-out.
- **Launcher rules.** A launcher is drawn only if its command is available in the current context; otherwise it is shown disabled with the reason in its tooltip. A panel has at most one launcher. Launchers have keytips like any other tool.

**Commands.** `ui.ribbon.pin_slideout(tab_id, panel_id, pinned: bool) -> ()`; launchers run their own registered command.

**Acceptance criteria.**
- [ ] Pinning the Room & Area slide-out keeps it open while switching between Architecture and Annotate tabs and returning.
- [ ] An unpinned slide-out closes after a tool in it is chosen; a pinned one stays open.
- [ ] Assigning a shortcut to the Mechanical Settings launcher command and pressing it opens the same dialog as the launcher button.

### 1.7.4 Minimise states
**ID:** UI-RIB-004 · **Priority:** P1 · **Ref:** research/01 §6.4

**Behaviour & rules.**
- Ribbon states: **Full**, **Tabs only**, **Panel titles only**, **Panel buttons only** (one icon per panel).
- A state button right of the last tab toggles between Full and the user's preferred minimised state; its arrow chooses the preferred state or "Cycle through all".
- In minimised states, clicking a tab, title or panel button shows the content as a temporary overlay that closes after a tool is chosen.
- Double-clicking a tab label toggles Full ↔ preferred minimised state.
- Right-clicking empty ribbon space toggles **Show panel titles**.
- **State definitions.**
  - *Full*: tabs and panels always visible (default).
  - *Tabs only*: only the tab row is shown; clicking a tab shows its whole panel area as an overlay.
  - *Panel titles only*: the tab row plus a strip of panel titles; clicking a title shows that panel as an overlay.
  - *Panel buttons only*: the tab row plus one icon per panel; clicking an icon shows that panel as an overlay.
- **Cycle.** When the preferred state is "Cycle through all", the state button steps Full → Tabs → Titles → Buttons → Full.
- **Overlays.** An overlay closes when a tool is chosen, on Esc, when the pointer has been outside it for 500 ms, or when the canvas is clicked. It never takes keyboard focus from an in-canvas text editor.
- **Contextual tabs in a minimised state.** A contextual tab appears in the tab row as usual. When "Activate contextual tab on selection" is on (UI-RIB-006), its tab label is highlighted, but its panels are not opened automatically, so that the canvas stays uncovered.
- **Keytips** work in every state; choosing a tab keytip opens its overlay.
- The current state and the preferred minimised state are stored in `ui-state.toml` and in the *Ribbon* part of a layout bookmark. Focus mode (UI-DOCK-004) uses *Tabs only* temporarily without changing the stored preference.

**Commands.** `ui.ribbon.set_state(full|tabs|titles|buttons)`, `ui.ribbon.cycle_state()`.

**Acceptance criteria.**
- [ ] Each of the four states can be selected, survives restart, and every tool remains reachable through overlays.
- [ ] With the preferred state set to *Panel titles only*, the state button toggles between Full and Panel titles only.
- [ ] In *Tabs only*, selecting a wall highlights the contextual tab label without opening an overlay.

### 1.7.5 Rearranging tabs and panels
**ID:** UI-RIB-005 · **Priority:** P1 · **Ref:** research/01 §6.5

**Behaviour & rules.**
- **Ctrl+drag a tab label** to reorder tabs. File cannot move; contextual tabs always stay at the right end.
- **Drag a panel title** to reorder panels within its tab, or drag it off the ribbon to create a **floating panel** (an always-on-top mini toolbar). Floating panels can be snapped to each other into a group and moved by a grip; hovering a floating panel shows a **Return to Ribbon** control.
- Tabs and individual panels can be hidden from the ribbon right-click menu (**Show Tabs ▸**, **Show Panels ▸**).
- **Reset Ribbon** (right-click menu and Options › User Interface) restores default order, visibility and states without touching the QAT; **Reset All UI** restores both.

**Commands.** `ui.ribbon.get_layout() -> RibbonLayout`, `ui.ribbon.move_tab(tab_id, index)`, `ui.ribbon.move_panel(tab_id, panel_id, index)`, `ui.ribbon.float_panel(tab_id, panel_id, pos?)`, `ui.ribbon.dock_panel(panel_id)`, `ui.ribbon.set_visible(tab_id, panel_id?, visible)`, `ui.ribbon.reset()`.

**Acceptance criteria.**
- [ ] A floated Modify panel remains usable while the Annotate tab is active and returns to its original position with Return to Ribbon.
- [ ] Reset Ribbon does not change the QAT.

### 1.7.6 Tab-switching behaviour
**ID:** UI-RIB-006 · **Priority:** P0 · **Ref:** research/01 §6.6

**Behaviour & rules.**
- **On selection or tool start**: when "Activate contextual tab on selection" is on (default on), the contextual tab becomes the active tab; when off, it appears but the current tab stays active.
- **After clearing a selection or finishing a tool**: per user choice, either stay on the Modify tab or return to the tab that was active before (default: return to previous). Set separately for the model environment and the family editor.
- **Ctrl+`** activates the first contextual tab when one exists.
- Tab activation from the keyboard never changes the selection.
- **Settings keys.** `ui.ribbon.activate_contextual_on_selection` (bool, default true), `ui.ribbon.after_clear.model` and `ui.ribbon.after_clear.family` (`stay_on_modify` | `return_to_previous`, default `return_to_previous`). All three are on Options › User Interface (UI-PREF-002) and can be part of a scenario's preference overlay (UI-SCEN-001).
- **"Previous tab"** is the last permanent tab that was active before the contextual tab took focus. If that tab has since been hidden (by discipline filtering or the user), Reviera returns to Modify instead.
- **Tool start from a ribbon button** keeps the user on the tab the tool came from while the tool's contextual tab is shown, unless the tool's option panels exist only on the contextual tab; then the contextual tab is activated whatever the setting, so the options can be seen.
- **Changing selection category** (for example from walls to doors) while a contextual tab is active keeps the contextual tab active; its content is replaced (§1.9.1).
- **Tool chaining.** After `tool.repeat_last` or Create Similar, the tab-switching rules apply as for a fresh tool start.
- **Ctrl+`** with no contextual tab does nothing and shows "No contextual tab" in the status bar for 2 s.
- Lua and MCP calls to `ui.ribbon.activate_tab` follow the same rules and fail with an error when the tab is hidden or unknown.

**Commands.** `ui.ribbon.activate_tab(tab_id)`, `ui.ribbon.activate_contextual()`.

**Acceptance criteria.**
- [ ] With the option off, selecting a wall while on the Annotate tab shows the wall contextual tab but leaves Annotate active; Ctrl+` then activates it.
- [ ] With "return to previous", selecting a wall from the View tab and pressing Esc returns to the View tab; with "stay on Modify" it stays on Modify.
- [ ] The model and family-editor settings are independent: changing one does not change the other.

### 1.7.7 Discipline filtering
**ID:** UI-RIB-007 · **Priority:** P1 · **Ref:** research/01 §6.7, §24.2

**Behaviour & rules.**
- Options › User Interface › **Disciplines and tools** offers check boxes: Architecture; Structure; Steel; Concrete & Precast; Structural analysis; Systems – mechanical; Mechanical analysis; Systems – electrical; Electrical analysis; Systems – piping; Piping analysis; Massing & Site; Energy analysis; Route analysis; Carbon analysis.
- Clearing a box hides every command whose `discipline` set is fully covered by cleared boxes, and removes any tab left empty.
- An organisation may lock this setting through the system defaults file (§1.29.3); the check boxes are then disabled with a "Managed by administrator" note.
- Hidden commands remain available to Lua and MCP and through Command Search (shown as "hidden by discipline settings", runnable).
- **The `ui.disciplines` setting.** The check boxes are stored as one per-user setting with the key `ui.disciplines`: a set of discipline flags, `architecture`, `structure`, `steel`, `concrete`, `structural_analysis`, `mechanical`, `mechanical_analysis`, `electrical`, `electrical_analysis`, `piping`, `piping_analysis`, `massing_site`, `energy_analysis`, `route_analysis`, `carbon_analysis`. The default is all flags set. This key is what an administrator locks (§1.29.3) and what a scenario's preference overlay may change (UI-SCEN-001).
- **What it filters.** Ribbon tabs, panels and tools; QAT items (hidden, not removed, so they return when the discipline is re-enabled); context-menu entries; Properties-palette parameter groups that belong only to cleared disciplines (for example Structural when every structural flag is cleared); and the Options pages of cleared analysis features. It does **not** filter the Project Browser, Command Search, Lua, MCP, keyboard shortcuts or the model: elements of every discipline remain visible and editable.
- **Commands with no `discipline` set** are treated as common and are never hidden.
- **Shortcuts to hidden commands** still run them; the status bar adds "(hidden by discipline settings)".
- **At least one modelling discipline.** Clearing every flag of Architecture, Structure and the three Systems disciplines is refused with the message "Keep at least one modelling discipline".
- Changes take effect on OK without restart; the ribbon rebuilds once.
- This filter is a UI preference. It is unrelated to the *view* discipline (Architectural, Structural, Mechanical, …) owned by Ch. 5.

**Commands.** `ui.disciplines.get() -> {enabled[], locked: bool}`, `ui.disciplines.set(enabled[]) -> ()` (fails when locked or when no modelling discipline would remain). Both are thin wrappers over `ui.options.get/set("ui.disciplines")`.

**Acceptance criteria.**
- [ ] Clearing Structure, Steel, Concrete & Precast and Structural analysis removes the Structure, Steel and Concrete tabs and the structural tools on shared panels.
- [ ] A locked discipline setting cannot be changed in Options.
- [ ] With Structure cleared, typing the column shortcut still starts the column tool, and Command Search lists it as hidden but runnable.
- [ ] Re-enabling a discipline restores the QAT items that were hidden by it, in their original positions.
- [ ] `ui.disciplines.set([])` returns an error and changes nothing.

### 1.7.8 Add-in contributions
**ID:** UI-RIB-008 · **Priority:** P0 · **Ref:** research/01 §7.13

**Behaviour & rules.**
- Lua add-ins declare tabs, panels and tools in their manifest (DM chapter defines the manifest). Add-in tools without their own tab go to **Add-Ins › External Tools**.
- Add-in tabs are inserted after the Add-Ins tab by default; users can reorder them.
- Add-in ribbon items are rejected if they reuse a built-in tab or panel id, except for the documented extension points `addins/*` and `modify/addins`.
- An add-in that is not loaded (for example due to the licence check in TECH_STACK §4.2) contributes nothing to the ribbon.
- **Declared items.** A manifest entry names a tab id (new, or the extension points), a panel id, and for each tool: command id, kind (§1.7.1), size variants, optional keytip and collapse priority. Icons are SVG files inside the add-in package; a missing icon falls back to a generic add-in glyph and logs a warning.
- **Id namespacing.** Add-in tab and panel ids are prefixed with the add-in's namespace (`addin.<namespace>.<id>`), so two add-ins cannot collide. Two add-ins declaring the same display name get distinct tabs with the add-in name in the tooltip.
- **Validation.** At load, every tool must reference a command the same add-in registers (or a built-in command); invalid entries are skipped and listed in the Add-in Manager's load report, and the rest of the add-in's ribbon still loads.
- **Discipline and priority.** Add-in commands may declare `discipline` flags and are filtered by UI-RIB-007 like built-in ones.
- **Load and unload at run time.** Enabling or disabling an add-in in the Add-in Manager (Ch. 2) rebuilds the ribbon without restart. User customisations that refer to an unloaded add-in (QAT items, moved panels, shortcuts) are kept and reappear when it loads again.
- **Limits.** One add-in may contribute at most 3 tabs and 20 panels; extra entries are rejected with a load-report message.

**Commands.** `ui.ribbon.rebuild() -> ()` regenerates the ribbon from the registry, the default layout file and the user's customisations (used after add-in load/unload and by scripts); add-in ribbon entries themselves come from the manifest (Ch. 2).

**Acceptance criteria.**
- [ ] Installing a sample add-in that declares a tab "Survey" with one panel shows that tab after Add-Ins on next start, and its button runs the add-in's command.
- [ ] An add-in tool that names an unregistered command is skipped and reported, while the add-in's other tools appear.
- [ ] Disabling the add-in removes its tab without restart; re-enabling it restores the tab and any QAT item the user had added from it.

---

## 1.8 Ribbon catalog

### 1.8.1 Catalog conventions
**ID:** UI-RIBCAT-001 · **Priority:** P0 · **Ref:** research/01 §7 · `screenshots/ui/ui_overview_annotated.png`, `screenshots/ui/ribbon_insert_tab_legacy.png`, `screenshots/ui/view_tab_windows_panel_canvas_theme.png`

**Purpose.** Define Reviera's default ribbon: its tabs, panels and tools, with the command each tool invokes and the owning chapter.

**Rules.**
- The ribbon default layout is a data file (`ribbon-default.toml`) shipped with Reviera and generated/validated against the registry at build time; a tool referencing an unknown command fails the build.
- The tables below are normative for **grouping and placement**. Command ids for non-UI owners are the binding names the ribbon uses; if an owning chapter defines a different name, the owning chapter wins and this table is updated (tracked in §1.35).
- Column meanings: **Kind**: L large, S small, I icon-only, Sp split member, Dd drop-down member, T toggle, G gallery, Dl dialog launcher, So slide-out item. **Keys**: Reviera default key sequence (§1.26.5). **Pri**: the tool's priority tier. P3 rows are listed only to record the decision not to ship them, with the open alternative.
- Command arguments in braces (for example `wall.create{structural}`) mean "the same command with that preset argument".
- Default tab order: **File · Architecture · Structure · Steel · Concrete · Systems · Insert · Annotate · Analyze · Massing & Site · Collaborate · View · Manage · Add-Ins · Modify**, then add-in tabs (after Add-Ins), then contextual tabs.

### 1.8.2 Common Select panel
**ID:** UI-RIBCAT-002 · **Priority:** P0 · **Ref:** research/01 §7.0, §18.4

The first panel of every permanent and contextual tab.

| Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|
| Modify (end tool, select mode) | L | `tool.modify` | UI | MD | P0 |
| Select ▾ › Select Links | Dd T | `selection.options.set{links}` | UI | — | P1 |
| Select ▾ › Select Underlay Elements | Dd T | `selection.options.set{underlay}` | UI | — | P0 |
| Select ▾ › Select Pinned Elements | Dd T | `selection.options.set{pinned}` | UI | — | P0 |
| Select ▾ › Select Elements by Face | Dd T | `selection.options.set{by_face}` | UI | — | P0 |
| Select ▾ › Drag Elements on Selection | Dd T | `selection.options.set{drag_on_select}` | UI | — | P0 |
| (contextual only) Filter | L | `selection.filter_dialog` | UI | — | P0 |
| (contextual only) Save / Load / Edit Selection | S | `selection.set_save`, `selection.set_load`, `selection.set_edit` | UI | — | P1 |

### 1.8.3 Architecture tab
**ID:** UI-RIBCAT-010 · **Priority:** P0 · **Ref:** research/01 §7.1 · `screenshots/ui/ui_overview_annotated.png`

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| Build | Wall › Wall | Sp | `wall.create` | ARC | WA | P0 |
| Build | Wall › Structural Wall | Sp | `wall.create{structural}` | STR | — | P1 |
| Build | Wall › Wall by Face | Sp | `wall.create_by_face` | ARC | — | P1 |
| Build | Wall › Wall Sweep | Sp | `wallsweep.create` | ARC | — | P1 |
| Build | Wall › Wall Reveal | Sp | `wallreveal.create` | ARC | — | P1 |
| Build | Door | L | `door.place` | ARC | DR | P0 |
| Build | Window | L | `window.place` | ARC | WN | P0 |
| Build | Component › Place Component | Sp | `component.place` | ARC | CM | P0 |
| Build | Component › Model In-Place | Sp | `family.create_in_place` | DM | — | P1 |
| Build | Column › Architectural Column | Sp | `column.place` | ARC | — | P0 |
| Build | Column › Structural Column | Sp | `column.place` | STR | CL | P1 |
| Build | Roof › Roof by Footprint | Sp | `roof.create_footprint` | ARC | — | P0 |
| Build | Roof › Roof by Extrusion | Sp | `roof.create_extrusion` | ARC | — | P1 |
| Build | Roof › Roof by Face | Sp | `roof.create_by_face` | ARC | — | P1 |
| Build | Roof › Soffit | Sp | `soffit.create` | ARC | — | P1 |
| Build | Roof › Fascia | Sp | `fascia.create` | ARC | — | P1 |
| Build | Roof › Gutter | Sp | `gutter.create` | ARC | — | P1 |
| Build | Ceiling | S | `ceiling.create` | ARC | — | P0 |
| Build | Floor › Floor | Sp | `floor.create` | ARC | FL | P0 |
| Build | Floor › Structural Floor | Sp | `floor.create{structural}` | STR | SB | P1 |
| Build | Floor › Floor by Face | Sp | `floor.create_by_mass_floor` | ARC | — | P1 |
| Build | Floor › Slab Edge | Sp | `slabedge.create` | ARC | — | P1 |
| Build | Curtain System | S | `curtainsystem.create_by_face` | ARC | — | P1 |
| Build | Curtain Grid | S | `curtain.add_grid` | ARC | — | P1 |
| Build | Mullion | S | `mullion.add` | ARC | — | P1 |
| Circulation | Railing › Sketch Path | Sp | `railing.create` | ARC | — | P1 |
| Circulation | Railing › Place on Host | Sp | `railing.place_on_host` | ARC | — | P1 |
| Circulation | Ramp | S | `ramp.create` | ARC | — | P1 |
| Circulation | Stair | S | `stair.create` | ARC | — | P0 |
| Model | Model Text | S | `modeltext.create` | ARC | — | P1 |
| Model | Model Line | S | `modelline.create` | ARC | LI | P0 |
| Model | Model Group › Place Group | Sp | `group.place` | ARC | — | P1 |
| Model | Model Group › Create Group | Sp | `group.create` | ARC | GP | P1 |
| Model | Model Group › Load as Group | Sp | `group.load` | DM | — | P1 |
| Room & Area | Room | L | `room.place` | ARC | RM | P0 |
| Room & Area | Room Separator | S | `roomseparator.create` | ARC | — | P0 |
| Room & Area | Tag Room › Tag Room | Sp | `tag.create_room` | DOC | RT | P0 |
| Room & Area | Tag Room › Tag All Not Tagged | Sp | `tag.tag_all{category=rooms}` | DOC | — | P1 |
| Room & Area | Area › Area | Sp | `area.place` | ARC | — | P1 |
| Room & Area | Area › Area Plan | Sp | `view.create_plan{area}` | DOC | — | P1 |
| Room & Area | Area Boundary | S | `areaboundary.create` | ARC | — | P1 |
| Room & Area | Tag Area | S | `tag.create{area}` | DOC | — | P1 |
| Room & Area | Area and Volume Computations | So / Dl | `room.computation_settings` | ARC | — | P1 |
| Room & Area | Color Schemes | So | `colorscheme.edit` | DOC | — | P1 |
| Opening | By Face | S | `opening.create_by_face` | ARC | — | P1 |
| Opening | Shaft | S | `opening.create_shaft` | ARC | — | P1 |
| Opening | Wall Opening | S | `opening.create_wall` | ARC | — | P1 |
| Opening | Vertical | S | `opening.create_vertical` | ARC | — | P1 |
| Opening | Dormer | S | `opening.create_dormer` | ARC | — | P2 |
| Datum | Level | S | `level.create` | ARC | LL | P0 |
| Datum | Grid | S | `grid.create` | ARC | GR | P0 |
| Work Plane | Set | L | `workplane.set` | ARC | PK | P0 |
| Work Plane | Show | S T | `workplane.show` | ARC | — | P1 |
| Work Plane | Reference Plane | S | `refplane.create` | ARC | RP | P0 |
| Work Plane | Viewer | S | `workplane.viewer` | ARC | — | P2 |


### 1.8.4 Structure tab
**ID:** UI-RIBCAT-011 · **Priority:** P1 · **Ref:** research/01 §7.2

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| Structure | Beam | L | `beam.create` | STR | BM | P1 |
| Structure | Wall (split, as Architecture › Build) | Sp | `wall.create{structural}` … | STR | — | P1 |
| Structure | Column | L | `column.place` | STR | CL | P1 |
| Structure | Floor (split, as Architecture › Build) | Sp | `floor.create{structural}` … | STR | SB | P1 |
| Structure | Truss | S | `truss.create` | STR | — | P1 |
| Structure | Brace | S | `brace.create` | STR | BR | P1 |
| Structure | Beam System | S | `beam_system.create_auto` | STR | BS | P1 |
| Connection | Structural Connection | S | `structural_connection.create` | STR | — | P2 |
| Connection | Show Connection Warnings | S T | `structural_connection.show_warnings` | STR | — | P2 |
| Connection | Connection Settings | Dl | `structural_settings.show{connection}` | STR | — | P2 |
| Foundation | Isolated | S | `footing.place` | STR | — | P1 |
| Foundation | Wall Foundation | S | `wall_foundation.create` | STR | FT | P1 |
| Foundation | Slab › Foundation Slab | Sp | `foundation_slab.create` | STR | — | P1 |
| Foundation | Slab › Slab Edge | Sp | `slabedge.create` | ARC | — | P1 |
| Analytical Model | Member | S | `analytical_member.create` | ANA | — | P2 |
| Analytical Model | Panel | S | `analytical_panel.create_by_boundary`, `analytical_panel.create_by_extrusion` | ANA | — | P2 |
| Analytical Model | Opening | S | `analytical_opening.create` | ANA | — | P2 |
| Analytical Model | Link | S | `analytical_link.create` | ANA | — | P2 |
| Analytical Model | Automate | S | `analytical_automation.physical_to_analytical` | ANA | — | P2 |
| Analytical Model | Nodes | S | `analytical_node.merge` | ANA | — | P2 |
| Analytical Model | Boundary Conditions | S | `boundary_condition.create` | ANA | — | P2 |
| Model | Component; Model Text; Model Line; Model Group | (as Architecture › Model) | `component.place`, `modeltext.create`, `modelline.create`, `group.*` | ARC | — | P0–P1 |
| Opening | (as Architecture › Opening) | — | `opening.*` | ARC | — | P1 |
| Datum | Level; Grid | S | `level.create`, `grid.create` | ARC | LL, GR | P0 |
| Work Plane | Set; Show; Reference Plane; Viewer | — | `workplane.*`, `refplane.create` | ARC | — | P0–P2 |

Reinforcement tools are on the Concrete tab (§1.8.6).

### 1.8.5 Steel tab
**ID:** UI-RIBCAT-012 · **Priority:** P2 · **Ref:** research/01 §7.3

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| Connection | Structural Connection | L | `structural_connection.create` | STR | — | P2 |
| Connection | Show Connection Warnings | S T | `structural_connection.show_warnings` | STR | — | P2 |
| Fabrication Elements | Plate | L | `plate.create` | STR | — | P2 |
| Fabrication Elements | Bolts › Bolts | Sp | `bolt_pattern.create` | STR | — | P2 |
| Fabrication Elements | Bolts › Anchors | Sp | `anchor_pattern.create` | STR | — | P2 |
| Fabrication Elements | Bolts › Holes | Sp | `hole_pattern.create` | STR | — | P2 |
| Fabrication Elements | Bolts › Shear Studs | Sp | `shear_stud_pattern.create` | STR | — | P2 |
| Fabrication Elements | Welds | S | `weld.create` | STR | — | P2 |
| Modifiers | Corner Cut | S | `steel_modifier.corner_cut` | STR | — | P2 |
| Modifiers | Skewed Cope | S | `steel_modifier.cope_skewed` | STR | — | P2 |
| Modifiers | Shorten | S | `steel_modifier.shorten` | STR | — | P2 |
| Modifiers | Contour Cut | S | `steel_modifier.contour_cut` | STR | — | P2 |
| Parametric Cuts | Cope | S | `steel_cut.cope` | STR | — | P2 |
| Parametric Cuts | Miter › Miter | Sp | `steel_cut.miter` | STR | — | P2 |
| Parametric Cuts | Miter › Saw Cut – Flange | Sp | `steel_cut.saw{flange}` | STR | — | P2 |
| Parametric Cuts | Miter › Saw Cut – Web | Sp | `steel_cut.saw{web}` | STR | — | P2 |
| Parametric Cuts | Cut Through | S | `steel_cut.cut_through` | STR | — | P2 |
| Parametric Cuts | Cut By | S | `steel_cut.cut_by` | STR | — | P2 |
| Work Plane | Set; Show; Reference Plane; Viewer | — | `workplane.*`, `refplane.create` | ARC | — | P0–P2 |

### 1.8.6 Concrete tab (reinforcement and precast)
**ID:** UI-RIBCAT-013 · **Priority:** P2 · **Ref:** research/01 §7.2 (Reinforcement), §7.4, [2027] Concrete Detailing note in §7

Reviera gathers cast-in-place reinforcement and precast tools on one tab from the start (see §1.35).

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| Reinforcement | Rebar | L | `rebar.place` | STR | — | P2 |
| Reinforcement | Area Reinforcement | S | `area_reinforcement.create` | STR | — | P2 |
| Reinforcement | Path Reinforcement | S | `path_reinforcement.create` | STR | — | P2 |
| Reinforcement | Fabric Area | S | `fabric_area.create` | STR | — | P2 |
| Reinforcement | Fabric Sheet | S | `fabric_sheet.place` | STR | — | P2 |
| Reinforcement | Rebar Coupler | S | `rebar_coupler.place` | STR | — | P2 |
| Reinforcement | Cover | S | `rebar_cover.assign` | STR | — | P2 |
| Reinforcement | Rebar Cover Settings | So | `rebar_cover_type.manage` | STR | — | P2 |
| Reinforcement | Reinforcement Settings | So / Dl | `reinforcement_settings.show` | STR | — | P2 |
| Reinforcement | Reinforcement Numbers | So | `reinforcement_numbering.show` | STR | RN | P2 |
| Precast Segmentation | Split | L | `precast.split` | STR | PL | P2 |
| Precast Connections | Mounting Parts | S | `precast.place_mounting_parts` | STR | — | P2 |
| Precast Reinforcement | Reinforce | S | `precast.reinforce` | STR | RF | P2 |
| Precast Reinforcement | Custom Fabric Sheet | S | `precast.generate_custom_fabric` | STR | — | P2 |
| Precast Fabrication | Shop Drawings | S | `precast.shop_drawings` | STR | — | P2 |
| Precast Fabrication | CAM Export | S | `precast.cam_export` | STR | — | P2 |
| Precast Configuration | Configuration | L | `precast_config.show` | STR | CFG | P2 |
| Precast Updates | Automatic Updates | T | `precast.set_auto_update` | STR | — | P2 |

### 1.8.7 Systems tab
**ID:** UI-RIBCAT-014 · **Priority:** P1 · **Ref:** research/01 §7.5

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| HVAC | Duct | L | `duct.create` | MEP | DT | P1 |
| HVAC | Duct Placeholder | S | `duct_placeholder.create` | MEP | — | P1 |
| HVAC | Duct Fitting | S | `duct_fitting.place` | MEP | DF | P1 |
| HVAC | Duct Accessory | S | `duct_accessory.place` | MEP | DA | P1 |
| HVAC | Convert to Flex Duct | S | `duct.convert_to_flex` | MEP | CV | P1 |
| HVAC | Flex Duct | S | `flex_duct.create` | MEP | FD | P1 |
| HVAC | Air Terminal | L | `air_terminal.place` | MEP | AT | P1 |
| HVAC | Mechanical Settings | Dl | `mep_settings.show{mechanical}` | MEP | MS | P1 |
| Fabrication | Fabrication Parts | L | `fabrication_part.place` | MEP | PB | P2 |
| Fabrication | Multi-Point Routing | S | `fabrication_route.multi_point` | MEP | MR | P2 |
| P&ID | P&ID Modeler | L | `pid.open` | MEP | — | P2 |
| Mechanical | Mechanical Equipment | L | `mech_equipment.place` | MEP | ME | P1 |
| Mechanical | Mechanical Control Device | S | `mech_control_device.place` | MEP | — | P2 |
| Plumbing & Piping | Pipe | L | `pipe.create` | MEP | PI | P1 |
| Plumbing & Piping | Pipe Placeholder | S | `pipe_placeholder.create` | MEP | — | P1 |
| Plumbing & Piping | Parallel Pipes | S | `pipe.create_parallel` | MEP | — | P1 |
| Plumbing & Piping | Pipe Fitting | S | `pipe_fitting.place` | MEP | PF | P1 |
| Plumbing & Piping | Pipe Accessory | S | `pipe_accessory.place` | MEP | PA | P1 |
| Plumbing & Piping | Flex Pipe | S | `flex_pipe.create` | MEP | FP | P1 |
| Plumbing & Piping | Plumbing Fixture | L | `plumbing_fixture.place` | MEP | PX | P1 |
| Plumbing & Piping | Plumbing Equipment | S | `mech_equipment.place` | MEP | PE | P1 |
| Plumbing & Piping | Sprinkler | S | `sprinkler.place` | MEP | SK | P1 |
| Plumbing & Piping | Mechanical Settings | Dl | `mep_settings.show{mechanical}` | MEP | MS | P1 |
| Electrical | Wire › Arc Wire | Sp | `wire.create{arc}` | MEP | AW | P2 |
| Electrical | Wire › Spline Wire | Sp | `wire.create{spline}` | MEP | — | P2 |
| Electrical | Wire › Chamfered Wire | Sp | `wire.create{chamfered}` | MEP | — | P2 |
| Electrical | Cable Tray | L | `cable_tray.create` | MEP | CT | P1 |
| Electrical | Conduit | L | `conduit.create` | MEP | CN | P1 |
| Electrical | Parallel Conduits | S | `conduit.create_parallel` | MEP | — | P1 |
| Electrical | Cable Tray Fitting | S | `cable_tray_fitting.place` | MEP | TF | P1 |
| Electrical | Conduit Fitting | S | `conduit_fitting.place` | MEP | NF | P1 |
| Electrical | Electrical Equipment | L | `electrical_equipment.place` | MEP | EE | P1 |
| Electrical | Device ▾ (Electrical Fixture, Communication, Data, Fire Alarm, Lighting Switch, Nurse Call, Security, Telephone) | Dd | `device.place{kind}` | MEP | — | P1 |
| Electrical | Lighting Fixture | L | `lighting_fixture.place` | MEP | LF | P1 |
| Electrical | Electrical Settings | Dl | `mep_settings.show{electrical}` | MEP | ES | P1 |
| Risers | Create Riser | S | `mep_run.create_riser` | MEP | — | P2 |
| Model | Component › Place Component; Model In-Place | Sp | `component.place`, `family.create_in_place` | ARC / DM | CM | P0/P1 |
| Work Plane | Set; Show; Reference Plane; Viewer | — | `workplane.*`, `refplane.create` | ARC | — | P0–P2 |

### 1.8.8 Insert tab
**ID:** UI-RIBCAT-015 · **Priority:** P0 · **Ref:** research/01 §7.6 · `screenshots/ui/ribbon_insert_tab_legacy.png`

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| Link | Link Model (IFC) | L | `link.add_ifc` | COL | — | P1 |
| Link | Link CAD | L | `link.add_cad` | COL | — | P1 |
| Link | Decal › Place Decal | Sp | `decal.place` | DOC | — | P2 |
| Link | Decal › Decal Types | Sp | `decal.types` | DM | — | P2 |
| Link | Point Cloud | S | `link.add_pointcloud` | COL | — | P2 |
| Link | Coordination Model | — | not provided (IFC links are the open alternative; COL deviation D7-7) | — | — | P3 |
| Link | Link PDF | S | `link.add_pdf` | COL | — | P1 |
| Link | Link Image | S | `link.add_image` | COL | — | P1 |
| Link | Link Server Project | S | `link.add_ifc{bimserver}` | COL | — | P1 |
| Link | Link Topography (vendor cloud) | — | not provided | — | — | P3 |
| Link | DWF Markup | — | not provided (BCF is the open alternative, Collaborate tab) | — | — | P3 |
| Manage | Manage Links | L | `link.manage` | COL | — | P1 |
| Import | Import CAD | L | `import.cad` | DM | — | P1 |
| Import | Import gbXML | S | `gbxml.import_loads` | DM | — | P2 |
| Import | Import PDF | S | `import.pdf` | DM | — | P1 |
| Import | Import Image | S | `import.image` | DM | — | P1 |
| Import | Manage Images | Dl | `image.manage` | DM | — | P1 |
| Import | Import Family Types | S | `family.import_types` | DM | — | P1 |
| Load from Library | Load Family | L | `family.load` | DM | — | P0 |
| Load from Library | Browse Content Library | S | `library.browse` | DM | — | P1 (local folders), P2 (organisation libraries) |
| Load from Library | Load as Group | S | `group.load` | DM | — | P1 |
| Load from Library | Insert from File › Insert Views from File | Sp | `view.insert_from_file` | DOC | — | P1 |
| Load from Library | Insert from File › Insert 2D Elements from File | Sp | `detail.insert_from_file` | DOC | — | P2 |

### 1.8.9 Annotate tab
**ID:** UI-RIBCAT-016 · **Priority:** P0 · **Ref:** research/01 §7.7

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| Dimension | Aligned | L | `dim.create_aligned` | DOC | DI | P0 |
| Dimension | Linear | S | `dim.create_linear` | DOC | — | P0 |
| Dimension | Angular | S | `dim.create_angular` | DOC | — | P0 |
| Dimension | Radial | S | `dim.create_radial` | DOC | — | P0 |
| Dimension | Diameter | S | `dim.create_diameter` | DOC | — | P1 |
| Dimension | Arc Length | S | `dim.create_arc_length` | DOC | — | P1 |
| Dimension | Spot Elevation | S | `spot.create_elevation` | DOC | EL | P1 |
| Dimension | Spot Coordinate | S | `spot.create_coordinate` | DOC | — | P1 |
| Dimension | Spot Slope | S | `spot.create_slope` | DOC | — | P1 |
| Dimension | Dimension Types (slide-out: Linear, Angular, Radial, Diameter, Spot Elevation, Spot Coordinate, Spot Slope types) | So / Dl | `dimtype.edit{kind}` | DOC | — | P1 |
| Detail | Detail Line | L | `detail.line` | DOC | DL | P0 |
| Detail | Region › Filled Region | Sp | `region.create{filled}` | DOC | — | P1 |
| Detail | Region › Masking Region | Sp | `region.create{masking}` | DOC | — | P1 |
| Detail | Component › Detail Component | Sp | `detail.component` | DOC | — | P1 |
| Detail | Component › Repeating Detail | Sp | `detail.repeating` | DOC | — | P1 |
| Detail | Component › Legend Component | Sp | `legend.place_component` | DOC | — | P1 |
| Detail | Revision Cloud | S | `revcloud.create` | DOC | — | P1 |
| Detail | Detail Group › Place | Sp | `detail.place_group` | DOC | — | P1 |
| Detail | Detail Group › Create | Sp | `detail.create_group` | DOC | — | P1 |
| Detail | Insulation | S | `detail.insulation` | DOC | — | P1 |
| Text | Text | L | `text.create` | DOC | TX | P0 |
| Text | Check Spelling | S | `text.check_spelling` | DOC | F7 | P1 |
| Text | Find/Replace | S | `text.find_replace` | DOC | FR | P1 |
| Text | Text Types | Dl | `texttype.edit` | DOC | — | P1 |
| Tag | Tag by Category | L | `tag.create` | DOC | TG | P0 |
| Tag | Tag All | L | `tag.tag_all` | DOC | — | P1 |
| Tag | Multi-Category Tag | S | `tag.create{multi_category}` | DOC | — | P1 |
| Tag | Material Tag | S | `tag.create_material` | DOC | — | P1 |
| Tag | Area Tag | S | `tag.create{area}` | DOC | — | P1 |
| Tag | Room Tag | S | `tag.create_room` | DOC | RT | P0 |
| Tag | Space Tag | S | `tag.create{space}` | DOC | — | P1 |
| Tag | View Reference | S | `viewref.create` | DOC | — | P1 |
| Tag | Tread Number | S | `tag.stair_numbers` | DOC | — | P2 |
| Tag | Beam Annotations | S | `tag.beam_annotations` | DOC | — | P2 |
| Tag | Multi-Rebar › Aligned; Linear | Sp | `multi_rebar_annotation.place{aligned|linear}` | DOC | — | P2 |
| Tag | Keynote › Element; Material; User | Sp | `keynote.create{element|material|user}` | DOC | — | P1 |
| Tag | Tag by Example | S | `tag.by_example` | DOC | — | P2 |
| Tag | Loaded Tags and Symbols | So | `tag.set_default_type` | DOC | — | P1 |
| Tag | Keynoting Settings | So | `keynote.settings` | DOC | — | P1 |
| Color Fill | Color Fill Legend | S | `colorlegend.place` | DOC | — | P1 |
| Color Fill | Duct Legend | S | `mep_color_legend.place{duct}` | DOC | — | P2 |
| Color Fill | Pipe Legend | S | `mep_color_legend.place{pipe}` | DOC | — | P2 |
| Symbol | Symbol | L | `symbol.place` | DOC | — | P1 |
| Symbol | Span Direction | S | `floor.set_span_direction` | DOC | — | P1 |
| Symbol | Beam System Symbol | S | `tag.beam_system` | DOC | — | P2 |
| Symbol | Stair Path | S | `tag.stair_path` | DOC | — | P1 |
| Symbol | Area / Path / Fabric Reinforcement Symbols | S | `tag.reinforcement_symbol{kind}` | DOC | — | P2 |
| Focus | Isolate Annotations | T | `view.isolate_annotations` | DOC | — | P2 |

### 1.8.10 Analyze tab
**ID:** UI-RIBCAT-017 · **Priority:** P1 · **Ref:** research/01 §7.8

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| Loads | Boundary Conditions | S | `boundary_condition.create` | ANA | — | P2 |
| Loads | Loads | L | `load.create_point`, `load.create_line`, `load.create_area` | ANA | — | P2 |
| Loads | Load Cases | S | `load_case.manage` | ANA | — | P2 |
| Loads | Load Combinations | S | `load_case.manage{combinations}` | ANA | — | P2 |
| Analytical Model Tools | Adjust / Reset | S | `analytical_model.adjust`, `analytical_model.reset` | ANA | — | P2 |
| Analytical Model Tools | Check Member Supports | S | `analytical_model.check_supports` | ANA | — | P2 |
| Analytical Model Tools | Consistency Checks | S | `analytical_model.check_consistency` | ANA | — | P2 |
| Spaces & Zones | Space | L | `space.place` | MEP | — | P1 |
| Spaces & Zones | Space Separator | S | `space_separator.create` | MEP | — | P1 |
| Spaces & Zones | Space Naming | S | `space.name_from_rooms` | MEP | — | P2 |
| Spaces & Zones | Space Tag | S | `tag.create{space}` | DOC | — | P1 |
| Spaces & Zones | Zone | S | `system_zone.create` | MEP | — | P2 |
| Spaces & Zones | Area and Volume Computations | So | `room.computation_settings` | ARC | — | P1 |
| Spaces & Zones | Color Schemes | So | `colorscheme.edit` | DOC | — | P1 |
| Reports & Schedules | Schedule/Quantities | L | `schedule.create` | DOC | — | P0 |
| Reports & Schedules | Panel Schedules | S | `panel_schedule.create` | MEP | PS | P2 |
| Reports & Schedules | Duct Pressure Loss Report | S | `pressure_loss_report.export` | ANA | — | P2 |
| Reports & Schedules | Pipe Pressure Loss Report | S | `pressure_loss_report.export` | ANA | — | P2 |
| Reports & Schedules | Heating and Cooling Loads | S | `hc_loads.run` | ANA | — | P2 |
| Check Systems | Check Duct Systems | S T | `mep_check.systems{duct}` | MEP | DC | P1 |
| Check Systems | Check Pipe Systems | S T | `mep_check.systems{pipe}` | MEP | PC | P1 |
| Check Systems | Check Circuits | S | `mep_check.systems{circuit}` | MEP | EC | P2 |
| Check Systems | Show Disconnects | S T | `mep_check.show_disconnects` | MEP | — | P1 |
| Color Fill | (as Annotate › Color Fill) | — | `colorlegend.place` | DOC | — | P1–P2 |
| Energy | Location | S | `location.set` | DM | — | P1 |
| Energy | Energy Settings | Dl | `energy_settings.show` | ANA | — | P2 |
| Energy | Create Energy Model | L | `energy_model.create` | ANA | — | P2 |
| Energy | Delete Energy Model | S | `energy_model.delete` | ANA | — | P2 |
| Energy | Systems Analysis | S | `systems_analysis.run` | ANA | — | P2 |
| Energy | Cloud optimisation service | — | not provided | — | — | P3 |
| Route Analysis | Path of Travel | L | `path_of_travel.create` | ANA | — | P2 |
| Route Analysis | Reveal Obstacles | S T | `route.reveal_obstacles` | ANA | — | P2 |
| Route Analysis | Multiple Paths | S | `path_of_travel.create_multiple` | ANA | — | P2 |
| Route Analysis | One Way Indicator | S | `one_way_indicator.place` | ANA | — | P2 |
| Route Analysis | People Content | S | `people_content.place` | ANA | — | P2 |
| Route Analysis | Spatial Grid | S | `spatial_grid.create` | ANA | — | P2 |
| Electrical / Mechanical Analysis | Analytical loads, panels, analytical duct and pipe | S | `analysis.electrical.*`, `analysis.mechanical.*` | ANA | — | P2 |
| Carbon | Embodied Carbon | L | `embodied_carbon.run` | ANA | — | P2 |


### 1.8.11 Massing & Site tab
**ID:** UI-RIBCAT-018 · **Priority:** P1 · **Ref:** research/01 §7.9

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| Conceptual Mass | In-Place Mass | L | `mass.begin_inplace` | ARC | — | P2 |
| Conceptual Mass | Place Mass | S | `mass.place` | ARC | — | P2 |
| Conceptual Mass | Show Mass › by View Settings; Form and Floors; Surface Types; Zones and Shades | Sp T | `mass.show{mode}` | DOC | — | P2 |
| Model by Face | Curtain System | S | `curtainsystem.create_by_face` | ARC | — | P1 |
| Model by Face | Roof | S | `roof.create_by_face` | ARC | — | P1 |
| Model by Face | Wall | S | `wall.create_by_face` | ARC | — | P1 |
| Model by Face | Floor | S | `floor.create_by_mass_floor` | ARC | — | P1 |
| Model Site | Toposolid › Create from Sketch | Sp | `toposolid.create{sketch}` | ARC | — | P1 |
| Model Site | Toposolid › Create from Import | Sp | `toposolid.create{import}` | ARC | — | P1 |
| Model Site | Site Component | S | `sitecomponent.place` | ARC | — | P1 |
| Model Site | Parking Component | S | `sitecomponent.place{parking}` | ARC | — | P1 |
| Model Site | Site Settings | So / Dl | `site.settings` | ARC | — | P1 |
| Model Site | Smooth Shading | So T | `toposolid.toggle_smooth` | ARC | TS | P2 |
| Modify Site | Subdivide | S | `toposolid.subdivide` | ARC | — | P1 |
| Modify Site | Graded Region / Excavate | S | `toposolid.create_graded_region` | ARC | — | P2 |
| Modify Site | Split | S | `toposolid.split` | ARC | — | P1 |
| Modify Site | Merge | S | `toposolid.merge` | ARC | — | P1 |
| Modify Site | Label Contours | S | `contour.label` | DOC | — | P1 |
| Modify Site | Property Line | S | `propertyline.create` | ARC | — | P1 |

### 1.8.12 Collaborate tab
**ID:** UI-RIBCAT-019 · **Priority:** P1 · **Ref:** research/01 §7.10; TECH_STACK §3.7

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| Server | Connect / Disconnect | L T | `bimserver.login`, `bimserver.logout` | COL | — | P1 |
| Server | Server Projects | S | `ui.server.browser` | UI | — | P1 |
| Server | Publish to Server | S | `bimserver.publish_local` | COL | — | P1 |
| Server | Export Local Copy | S | `model.export_local_copy` | COL | — | P1 |
| Synchronize | Sync › Sync Now | Sp | `bimserver.sync` | COL | — | P1 |
| Synchronize | Sync › Sync with Options… | Sp | `bimserver.sync{dialog}` | COL | — | P1 |
| Synchronize | Get Latest | L | `bimserver.get_latest` | COL | RL | P1 |
| Synchronize | Check In | S | `bimserver.check_in` | COL | CI | P1 |
| Synchronize | Relinquish All Mine | S | `ownership.relinquish{all}` | COL | — | P1 |
| Communicate | Editing Requests | L | `editrequest.manage` | COL | ER | P1 |
| Manage Collaboration | Worksets | L | `workset.manage` | COL | — | P1 |
| Manage Collaboration | Gray Inactive Worksets | T | `view.grey_inactive_worksets` | COL | — | P1 |
| Manage Models | Revision History | S | `ui.server.revisions` | UI | — | P1 |
| Manage Models | Compare Revisions | S | `bimserver.compare` | COL | — | P1 |
| Manage Models | Restore Backup | S | `model.restore_backup` | DM | — | P1 |
| Coordinate | Copy/Monitor › Use Current Model; Select Link | Sp | `copymonitor.start{source}` | COL | — | P1 |
| Coordinate | Coordination Review | S | `coordreview.show` | COL | — | P1 |
| Coordinate | Coordination Settings | S | `coordreview.settings` | COL | — | P1 |
| Coordinate | Reconcile Hosting | S | `hosting.reconcile` | COL | — | P2 |
| Coordinate | Interference Check › Run | Sp | `clash.run` | COL | — | P1 |
| Coordinate | Interference Check › Show Last Report | Sp | `clash.last_report` | COL | — | P1 |
| Issues | BCF Issues | L | `bcf.issues_pane` | COL | — | P1 |
| Issues | Import / Export BCF | S | `bcf.import`, `bcf.export` | COL | — | P1 |
| Issues | Connect Issue Server (BCF API) | S | `bcf.api_connect` | COL | — | P1 |
| Issues | Sync Issues | S | `bcf.api_sync` | COL | — | P1 |
| Issues | Issue Report | S | `bcf.report` | COL | — | P1 |
| Issues | Server Profiles | S | `api.profile.list` (opens the server profiles dialog: BIMserver, BCF API, CDE; sign in or out, test, proxy and certificates) | COL | — | P1 |
| Share | Vendor shared views | — | not provided (BCF viewpoints are the open alternative) | — | — | P3 |

Server-mode tools are hidden in Local mode except Connect and Publish to Server (§1.32).

### 1.8.13 View tab
**ID:** UI-RIBCAT-020 · **Priority:** P0 · **Ref:** research/01 §7.11 · `screenshots/ui/view_tab_windows_panel_canvas_theme.png`

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| Graphics | Visibility/Graphics | L | `view.visibility_graphics` | DOC | VG, VV | P0 |
| Graphics | Filters | S | `view.filters` | DOC | — | P1 |
| Graphics | Thin Lines | S T | `ui.thin_lines.toggle` | UI | TL | P0 |
| Graphics | Show Hidden Lines | S | `view.show_hidden_lines` | DOC | — | P1 |
| Graphics | Remove Hidden Lines | S | `view.remove_hidden_lines` | DOC | — | P1 |
| Graphics | Cut Profile | S | `view.cut_profile` | DOC | — | P2 |
| Graphics | View Templates › Apply to Current View | Sp | `template.apply` | DOC | — | P1 |
| Graphics | View Templates › Create from Current View | Sp | `template.create_from_view` | DOC | — | P1 |
| Graphics | View Templates › Manage | Sp | `template.manage` | DOC | — | P1 |
| Graphics | Graphic Display Options | Dl | `view.set_graphic_display` | DOC | GD | P0 |
| Presentation | Render | L | `render.start` | DOC | RR | P2 |
| Presentation | Render Gallery (local) | S | `render.gallery` | DOC | — | P2 |
| Presentation | Walk Through | L | `walkthrough.start` | ADV | WT | P2 |
| Presentation | VR / AR ▾ (Start VR, Start AR, Tabletop) | Sp | `xr.start{vr\|ar\|tabletop}` | ADV | — | P2 (VR), P3 (AR) |
| Presentation | Render Backends | S | `render.backend.list` (opens the backends dialog: installed backends, capabilities, defaults, test, connect info) | DOC | — | P2 |
| Presentation | Cloud rendering | — | not provided | — | — | P3 |
| Inspect | Section Box | L T | `view.set_section_box{toggle}` | DOC | SX | P0 |
| Inspect | Selection Box | S | `view.fit_section_box` | DOC | BX | P1 |
| Inspect | Slice Plane › Horizontal; Vertical; On Face | Sp | `view.set_slice_planes` | DOC | — | P1 |
| Inspect | Clear Slices | S | `view.set_slice_planes{[]}` | DOC | — | P1 |
| Inspect | Hide ▾ (Element, Element and Children, Category, Type) | Sp | `view.temp_hide{by}` | UI | HH, HC | P0 |
| Inspect | Isolate ▾ (Element, Element and Children, Category, Type, Ghost Others) | Sp | `view.temp_isolate{by}` | UI | HI, IC, HG | P0 |
| Inspect | Translucent ▾ (Element, Element and Children, Category, Type) | Sp | `view.temp_translucent{by}` | UI | HT | P1 |
| Inspect | Reset Temporary Hide/Isolate | S | `view.temp_reset` | UI | HR | P0 |
| Inspect | Model Tree | S T | `ui.pane.toggle{model_tree}` | UI | MT | P1 |
| Create | 3D View › Default 3D View | Sp | `view.create_3d{default}` | DOC | 3D | P0 |
| Create | 3D View › Camera | Sp | `view.create_camera` | DOC | — | P1 |
| Create | 3D View › Walkthrough | Sp | `view.create_walkthrough` | DOC | — | P2 |
| Create | Section | L | `view.create_section` | DOC | — | P0 |
| Create | Callout › Rectangle | Sp | `view.create_callout{rectangle}` | DOC | — | P1 |
| Create | Callout › Sketch | Sp | `view.create_callout{sketch}` | DOC | — | P1 |
| Create | Plan Views › Floor Plan | Dd | `view.create_plan{plan}` | DOC | — | P0 |
| Create | Plan Views › Reflected Ceiling Plan | Dd | `view.create_plan{ceiling}` | DOC | — | P0 |
| Create | Plan Views › Structural Plan | Dd | `view.create_plan{structural}` | DOC | — | P1 |
| Create | Plan Views › Plan Region | Dd | `view.create_plan_region` | DOC | — | P1 |
| Create | Plan Views › Area Plan | Dd | `view.create_plan{area}` | DOC | — | P1 |
| Create | Elevation › Elevation | Sp | `view.create_elevation` | DOC | — | P0 |
| Create | Elevation › Framing Elevation | Sp | `view.create_framing_elevation` | DOC | — | P1 |
| Create | Drafting View | S | `view.create_drafting` | DOC | — | P1 |
| Create | Duplicate View › Duplicate; With Detailing; As Dependent | Dd | `view.duplicate{mode}` | DOC | — | P0 |
| Create | Legends › Legend; Keynote Legend | Dd | `view.create_legend{kind}` | DOC | — | P1 |
| Create | Schedules › Schedule/Quantities | Dd | `schedule.create` | DOC | — | P0 |
| Create | Schedules › Material Takeoff | Dd | `schedule.create{material_takeoff}` | DOC | — | P1 |
| Create | Schedules › Sheet List; Note Block; View List | Dd | `schedule.create{kind}` | DOC | — | P1 |
| Create | Schedules › Graphical Column Schedule | Dd | `schedule.create{graphicalcolumn}` | DOC | — | P2 |
| Create | Scope Box | S | `scopebox.create` | DOC | — | P1 |
| Sheet Composition | Sheet | L | `sheet.create` | DOC | — | P0 |
| Sheet Composition | Sheet Collection | S | `sheetcollection.create` | DOC | — | P1 |
| Sheet Composition | Place View | S | `sheet.place_view` | DOC | — | P0 |
| Sheet Composition | Title Block | S | `titleblock.place` | DOC | — | P0 |
| Sheet Composition | Revisions | S | `revision.manage` | DOC | — | P1 |
| Sheet Composition | Guide Grid | S | `guidegrid.create` | DOC | — | P1 |
| Sheet Composition | Matchline | S | `matchline.create` | DOC | — | P1 |
| Sheet Composition | View Reference | S | `viewref.create` | DOC | — | P1 |
| Sheet Composition | Viewports › Activate View; Deactivate View | Dd | `viewport.activate`, `viewport.activate{off}` | DOC | — | P0 |
| Sheet Composition | Viewports › Align Views; Swap Views | Dd | `position.assign`, `viewport.swap_view` | DOC | — | P1 |
| Windows | Switch Windows | L Dd | `window.switch` | UI | — | P0 |
| Windows | Close Inactive Views | S | `view.close_inactive` | UI | — | P0 |
| Windows | Tab Views | S | `window.tab_all` | UI | TW | P0 |
| Windows | Tile Views | S | `window.tile` | UI | WT | P0 |
| Windows | Canvas Theme | S T | `ui.canvas_theme.toggle` | UI | CA | P0 |
| Windows | User Interface ▾ › pane and chrome check boxes (Orientation Cube, Navigation Bar, Project Browser, System Browser, Properties, Status Bar, Status Bar – Worksets, Status Bar – Design Options, Agent, Notifications, Lua Console, Fabrication Parts, P&ID Modeler) | Dd T | `ui.pane.toggle{pane}` | UI | — | P0 |
| Windows | User Interface ▾ › Browser Organization | Dd | `browser.organization_dialog` (UI dialog over `browser.create_organization` / `browser.set_organization`, Ch. 5) | UI | — | P1 |
| Windows | User Interface ▾ › Keyboard Shortcuts | Dd | `ui.shortcuts.dialog` | UI | KS | P0 |
| Windows | User Interface ▾ › Layouts (Apply ▸ list · Save current… · Update · Switch back · Manage layouts… · Reset) | Sp | `ui.layout.apply`, `ui.layout.save`, `ui.layout.update`, `ui.layout.switch_back`, `ui.layout.manage`, `ui.layout.reset` | UI | Ctrl+Alt+1…9, Ctrl+Alt+0 | P1 |

Section deliberately has no default key: `SE` is the Endpoints snap override (§1.23). Users can bind one (§1.26).

### 1.8.14 Manage tab
**ID:** UI-RIBCAT-021 · **Priority:** P0 · **Ref:** research/01 §7.12

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| Settings | Materials | L | `material.browser` | DM | — | P0 |
| Settings | Object Styles | S | `objectstyle.set` | DOC | — | P0 |
| Settings | Snaps | S | `snap.settings_dialog` | UI | — | P0 |
| Settings | Project Information | S | `project.info.edit` | DM | — | P0 |
| Settings | Project Parameters | S | `param.project.manage` | DM | — | P1 |
| Settings | Shared Parameters | S | `paramlib.manage` | DM | — | P1 |
| Settings | Global Parameters | S | `global.manage` | DM | GL | P1 |
| Settings | Transfer Project Standards | S | `standards.transfer` | DM | — | P1 |
| Settings | Purge Unused | S | `purge.run` | DM | — | P1 |
| Settings | Project Units | S | `units.edit` | DM | UN | P0 |
| Settings | Structural Settings › Structural; Connection | Sp | `structural_settings.show`, `structural_settings.show{connection}` | STR | — | P1/P2 |
| Settings | MEP Settings › Mechanical; Electrical; Building/Space Type; Load Classifications; Demand Factors; Fabrication; P&ID | Dd | `mep_settings.show{kind}` | MEP | MS, ES, BT, FS | P1–P2 |
| Settings | Panel Schedule Templates | Dd | `panel_schedule_template.manage` | MEP | — | P2 |
| Settings | Additional Settings › Line Styles; Line Weights | Dd | `line_style.edit`, `line_weight.set` | DOC | — | P0 |
| Settings | Additional Settings › Line Patterns; Fill Patterns; Material Assets; Sheet Issues/Revisions; Arrowheads; Detail Level; Spot Dimensions; Callout/Elevation/Section Tags; Halftone/Underlay | Dd | `line_pattern.edit`, `fill_pattern.edit`, `material.assets`, `revision.manage`, `settings.arrowhead.create`, `settings.detail_level_by_scale.set`, `spot.settings`, `view_tag.settings`, `settings.halftone.set` | DOC / DM | — | P1 |
| Settings | Additional Settings › Temporary Dimensions | Dd | `ui.temp_dim.settings` | UI | — | P0 |
| Settings | Additional Settings › Multiple Values Indication | Dd | `ui.properties.varies_settings` | UI | — | P1 |
| Settings | Additional Settings › Analysis Display Styles; Assembly Code | Dd | `analysis_style.manage`, `classification.load` | ANA / DM | — | P2 |
| Settings | Additional Settings › Sun Settings | Dd | `sun.settings` | DOC | SU | P1 |
| Project Location | Location | S | `location.set` | DM | — | P1 |
| Project Location | Coordinates › Acquire; Publish; Specify at Point; Report; Reset Shared | Dd | `coordinates.{verb}` | DM | — | P1 |
| Project Location | Position › Relocate Project; Rotate True North; Mirror Project; Rotate Project North | Dd | `position.{verb}` | DM | — | P1 |
| Design Options | Design Options | L | `designoption.manage` | DM | — | P1 |
| Design Options | Add to Set | S | `designoption.add_elements` | DM | — | P1 |
| Design Options | Pick to Edit | S | `designoption.set_active` | DM | — | P1 |
| Design Options | Active Option (combo) | embedded | `designoption.set_active` | DM | — | P1 |
| Manage Project | Manage Links | S | `link.manage` | COL | — | P1 |
| Manage Project | Manage Images | S | `image.manage` | DM | — | P1 |
| Manage Project | Decal Types | S | `decal.types` | DM | — | P2 |
| Manage Project | Starting View | S | `settings.starting_view.set` | DOC | — | P1 |
| Phasing | Phases | L | `phase.manage` | DM | — | P1 |
| Selection | Save; Load; Edit | S | `selection.set_save`, `selection.set_load`, `selection.set_edit` | UI | — | P1 |
| Inquiry | IDs of Selection | S | `selection.show_ids` | UI | — | P0 |
| Inquiry | Select by ID | S | `selection.by_id` | UI | — | P0 |
| Inquiry | Review Warnings | S | `warnings.review` | COL | — | P0 |
| Scripting | Script Manager | L | `script.manager` | DM | — | P1 |
| Scripting | Lua Console | S | `ui.pane.toggle{lua_console}` | UI | Ctrl+Shift+L | P0 |
| Scripting | Script Security | S | `script.security` | DM | — | P0 |
| Scripting | Add-in Manager | S | `addin.manage` | DM | — | P1 |
| Numbering | Rule-Based Numbering | S | `numbering.rule_based` | DM | — | P2 |
| Visual Programming | Node-based programming | — | deferred (future Lua node editor) | — | — | P3 |
| Generative Design | Vendor generative design | — | not provided (Lua batch studies are the open alternative) | — | — | P3 |

### 1.8.15 Add-Ins tab
**ID:** UI-RIBCAT-022 · **Priority:** P0 · **Ref:** research/01 §7.13

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| External | External Tools ▾ (one item per add-in command without its own tab) | Dd | `registry.execute{command_id}` | DM | — | P0 |
| External | Run Script… | S | `script.run_file` | DM | — | P0 |
| External | Add-in Manager | S | `addin.manage` | DM | — | P1 |

The tab is hidden when no add-in contributes to it and Run Script is the only item, unless the user has pinned it visible.

### 1.8.16 Modify tab (permanent, and base of every contextual tab)
**ID:** UI-RIBCAT-023 · **Priority:** P0 · **Ref:** research/01 §7.14, §8 · `screenshots/ui/ribbon_contextual_tab.png`

| Panel | Tool | Kind | Command | Owner | Keys | Pri |
|---|---|---|---|---|---|---|
| Select | (common, §1.8.2) | — | — | UI | — | P0 |
| Properties | Properties | L T | `ui.pane.toggle{properties}` | UI | PP, Ctrl+1 | P0 |
| Properties | Type Properties | S | `type.edit_properties` | DM | — | P0 |
| Properties | Family Category and Parameters (family editor only) | S | `family_doc.category_params` | DM | — | P1 |
| Properties | Family Types (family editor only) | S | `family_doc.types` | DM | — | P1 |
| Clipboard | Paste › Paste | Sp | `clipboard.paste` | ARC | Ctrl+V | P0 |
| Clipboard | Paste › Aligned to Selected Levels; Selected Views; Current View; Same Place; Picked Level | Sp | `clipboard.paste{mode}` | ARC | — | P1 |
| Clipboard | Cut | S | `clipboard.cut` | ARC | Ctrl+X | P0 |
| Clipboard | Copy | S | `clipboard.copy` | ARC | Ctrl+C | P0 |
| Clipboard | Match Type Properties | S | `element.match_type` | ARC | MA | P1 |
| Geometry | Cope › Apply; Remove | Sp | `steel_cut.cope`, `steel_cut.uncope` | STR | CP | P2 |
| Geometry | Cut › Cut Geometry; Uncut Geometry | Sp | `geometry.cut`, `geometry.uncut` | ARC | — | P1 |
| Geometry | Join › Join; Unjoin; Switch Join Order | Sp | `geometry.join`, `geometry.unjoin`, `geometry.switch_join_order` | ARC | — | P0 |
| Geometry | Split Face | S | `face.split` | ARC | SF | P1 |
| Geometry | Paint › Paint; Remove Paint | Sp | `face.paint`, `face.remove_paint` | ARC | PT | P1 |
| Geometry | Join/Unjoin Roof | S | `roof.join` | ARC | — | P1 |
| Geometry | Beam/Column Joins | S | `framing.join_edit` | STR | — | P1 |
| Geometry | Wall Joins | S | `wall.join_set_type` | ARC | — | P0 |
| Geometry | Demolish | S | `element.demolish` | DM | — | P1 |
| Modify | Align | L | `element.align` | ARC | AL | P0 |
| Modify | Offset | S | `element.offset` | ARC | OF | P0 |
| Modify | Mirror – Pick Axis | S | `element.mirror{pick_axis}` | ARC | MM | P0 |
| Modify | Mirror – Draw Axis | S | `element.mirror{draw_axis}` | ARC | DM | P0 |
| Modify | Move | S | `element.move` | ARC | MV | P0 |
| Modify | Copy | S | `element.copy` | ARC | CO, CC | P0 |
| Modify | Rotate | S | `element.rotate` | ARC | RO | P0 |
| Modify | Trim/Extend to Corner | S | `element.trim_corner` | ARC | TR | P0 |
| Modify | Trim/Extend Single Element | S | `element.trim_extend` | ARC | — | P0 |
| Modify | Trim/Extend Multiple Elements | S | `element.trim_extend_multiple` | ARC | — | P0 |
| Modify | Split Element | S | `element.split` | ARC | SL | P0 |
| Modify | Split with Gap | S | `element.split_with_gap` | ARC | — | P1 |
| Modify | Array | S | `element.array_linear`, `element.array_radial` | ARC | AR | P0 |
| Modify | Scale | S | `element.scale` | ARC | RE | P1 |
| Modify | Pin | S | `element.pin` | ARC | PN | P0 |
| Modify | Unpin | S | `element.unpin` | ARC | UP | P0 |
| Modify | Delete | S | `element.delete` | ARC | DE, Delete | P0 |
| View | Hide in View › Hide Elements | Sp | `view.hide_elements` | DOC | EH | P0 |
| View | Hide in View › Hide Category | Sp | `view.set_category_visibility` | DOC | VH | P0 |
| View | Hide in View › Hide by Filter | Sp | `filter.create_from_selection`, `view.add_filter` | DOC | — | P1 |
| View | Override Graphics › by Element | Sp | `view.override_elements` | DOC | EOD | P1 |
| View | Override Graphics › by Category; by Filter | Sp | `view.set_category_override`, `filter.create_from_selection`, `view.set_filter_override` | DOC | — | P1 |
| View | Linework | S | `view.set_linework` | DOC | LW | P1 |
| View | Displace Elements | S | `view.displace_elements` | DOC | — | P2 |
| View | Section Box from Selection | S | `view.fit_section_box` | DOC | BX | P1 |
| Measure | Measure › Between Two References | Sp | `measure.between_references` | UI | — | P0 |
| Measure | Measure › Along an Element | Sp | `measure.along_element` | UI | — | P0 |
| Measure | Aligned Dimension (split of all dimension and spot tools) | Sp | `dimension.create_*`, `spot.create_*` | DOC | DI | P0 |
| Create | Create Similar | S | `tool.create_similar` | ARC | CS | P0 |
| Create | Create Group | S | `group.create` | ARC | GP | P1 |
| Create | Create Assembly | S | `assembly.create` | DOC | — | P2 |
| Create | Create Parts | S | `parts.create` | ARC | — | P2 |
| Work Plane | Set; Show; Viewer | — | `workplane.*` | ARC | — | P0–P2 |
| Add-ins | (extension point `modify/addins`) | — | add-in commands | DM | — | P1 |

Measure has no default key because `ME` is Mechanical Equipment (§1.26.5).

---

## 1.9 Contextual tabs

### 1.9.1 Contextual tab model
**ID:** UI-CTX-001 · **Priority:** P0 · **Ref:** research/01 §8 · `screenshots/ui/ribbon_contextual_tab.png`, `screenshots/ui/dark_theme_full_ui.png`

**Purpose.** Bring the tools relevant to the current selection or tool session to the ribbon automatically.

**Behaviour & rules.**
- A contextual tab appears whenever (a) one or more elements are selected, (b) an interactive tool session is active, or (c) an edit mode is active (sketch, group edit, in-place edit, reveal hidden, annotation isolation, viewport activation).
- Its label follows the pattern **"Modify · <Context>"**, where Context is the category name (plural) for selections ("Modify · Walls"), the tool name for placement ("Modify · Place Wall"), "Multi-Select" for mixed categories, or the edit mode name ("Modify · Edit Floor Boundary").
- The tab is drawn with the theme accent `ribbon.contextual` in both themes and remains at the right end of the tab row. Element-specific panels inside it carry the same accent on their title strip.
- **Content** = the generic Modify panels (Select, Properties, Clipboard, Geometry, Modify, View, Measure, Create) filtered by availability, followed by **context panels** contributed by the owning chapters for that category, tool or mode, followed by **tool option panels** (§1.10).
- Context panels are declared as data: each owning chapter registers `(context selector, panel id, tools[])` entries. A context selector matches on category set, tool command id, or edit mode. The shell composes them; it does not hard-code any.
- When selection and tool context change rapidly (for example dragging a crossing window), tab rebuilds are coalesced so the ribbon updates at most once per frame.
- Generated contextual pages are cached by context key so that reselecting the same category does not rebuild widgets.

**Commands.** `ui.ribbon.get_contextual() -> {label, panels[]}`, `ui.ribbon.register_context_panel(selector, panel)` (Lua add-ins, DM manifest).

**Acceptance criteria.**
- [ ] Selecting a wall and a door shows "Modify · Multi-Select" with the Filter tool; selecting only doors shows "Modify · Doors" with the Host and Mode panels.
- [ ] An add-in that registers a panel for the selector `category = IfcWall` sees its panel on "Modify · Walls".

### 1.9.2 Default context panels
**ID:** UI-CTX-002 · **Priority:** P0 · **Ref:** research/01 §8 table; Figure 20 `screenshots/ui/revit2027_options_bar_removed.png`

The following context panels are part of the default layout. Owners are authoritative for their commands.

| Context | Context panels and tools | Owner |
|---|---|---|
| Walls selected | **Mode**: Edit Profile (`wall.edit_profile`), Reset Profile (`wall.reset_profile`); **Modify Wall**: Wall Opening (`opening.create_wall`), Attach Top/Base (`wall.attach`), Detach Top/Base (`wall.detach`) | ARC |
| Place Wall | **Draw** gallery (§1.9.4); tool options: Placement side, Constraint (Hosted Wall, Auto Join), Levels (Height/Depth, top constraint, unconnected height), Location Line, Chain, Offset, Radius, Join Status | ARC |
| Doors / Windows selected | **Host**: Pick New Host (`hosting.rehost`); **Mode**: Edit Family (`family.edit`); in-canvas flip controls (§1.22.3) | ARC / DM |
| Sketch modes (floor, roof, ceiling, opening, filled region, stair boundary…) | **Mode**: Finish Edit Mode ✓ (`sketch.finish`), Cancel Edit Mode ✗ (`sketch.cancel`); **Draw**: Boundary Line, Slope Arrow, Span Direction (as applicable) and the shape gallery; **Work Plane**; tool options: Offset, Extend into Wall, Chain, Radius | ARC (DOC for 2D sketches) |
| Multi-Select | **Filter** (`selection.filter_dialog`); **Selection**: Save, Load, Edit; Activate Controls | UI |
| Edit Group | Add (`group.add_members`), Remove (`group.remove_members`), Attach Detail (`group.attach_detail`), Group Properties, Finish (`group.finish`), Cancel (`group.cancel`) | ARC |
| Model groups selected | Edit Group (`group.edit`), Ungroup (`group.ungroup`), Link (`group.convert_to_link`), Restore All Excluded (`group.restore_member`) | ARC |
| Create Stair | **Mode**; **Components**: Run, Landing, Support galleries; **Tools**; tool options: Location Line, Offset, Actual Run Width, Automatic Landing | ARC |
| Tags selected | **Host**: Add/Remove Host; **Leaders**: Merge Leaders; tool options: Orientation, Angle, Leader on/off, Leader Start/End | DOC |
| View selected (in canvas or browser) | **Region**: View Range (`view.set_view_range`); **Crop**: Edit Crop, Reset Crop, Size Crop | DOC |
| Scale tool | tool options: Graphical / Numerical, Scale factor | ARC |
| Reveal Hidden mode | Unhide Element (`view.unhide_elements`), Unhide Category (`view.set_category_visibility`), Toggle Reveal Hidden Mode (`view.reveal_hidden.toggle`) | DOC / UI |
| Activated viewport on sheet | Deactivate View (`viewport.activate`) | DOC |
| BIMserver: elements owned by others selected | **Ownership**: Request Editing (`editrequest.create`), Show Owner (`ownership.query`) | COL |

**Acceptance criteria.**
- [ ] In any sketch mode, every non-sketch ribbon tool is disabled except navigation, view and the Mode panel; Esc twice or Cancel leaves the mode without changes.

### 1.9.3 Edit modes and canvas feedback
**ID:** UI-CTX-003 · **Priority:** P0 · **Ref:** research/01 §8 (final paragraph), §14

**Behaviour & rules.**
- Modal edit modes (sketch, group edit, in-place edit, family-in-context edit) **take over** the ribbon: only tools valid in the mode are enabled, and the mode's Finish/Cancel controls are always visible.
- Elements not editable in the mode are drawn in the theme token `canvas.editing_dimmed` (a user-definable colour, default a desaturated tone of the background contrast colour) so the editable set stands out.
- Leaving the mode by any route (Finish, Cancel, Esc twice, closing the view) is unambiguous; closing the view while in a mode prompts "Finish / Cancel / Stay".
- Undo inside a mode undoes steps within the mode; after Finish, the whole mode session is one undo entry.
- **Mode stack.** Modes can nest (for example a sketch inside a group edit). The context engine keeps them as a stack (UI-ARCH-003); Finish and Cancel act on the top mode only. The canvas label (§1.17.2) lists every level, innermost last, for example "Editing Group: Unit A › Edit Boundary".
- **What stays available.** Navigation, view switching to views where the mode is valid, the View Control Bar, Properties of the edited object, Measure and the Modify tools that the mode declares. Commands that would change elements outside the mode are disabled with the reason "Not available while editing <mode>". Lua and MCP requests that would do so are queued (UI-API-006).
- **Esc handling.** The first Esc ends the current drawing step; the second Esc, with no step active, asks the mode to cancel; if the mode has unsaved steps, a small in-canvas prompt "Discard changes? Esc again to discard, Enter to finish" appears rather than a dialog.
- **Finish validation.** If the mode's owner rejects Finish (for example an open sketch loop), the mode stays active, the offending items are highlighted in `canvas.alert`, and the message from the owner is shown in the status bar and as a notice with "Continue editing" and "Cancel edit mode".
- **Crash recovery.** Every edit mode is an interactive tool under §2.1 of the conventions; its micro-operations are journalled so that **Resume sketch** can restore it after a crash (UI-RECOV-001, DM-LIVE-015).
- **Model switching.** Activating a view of a different model while in a mode prompts Finish / Cancel / Stay, as for closing the view.

**Commands.** Finish and Cancel are owner commands (`sketch.finish`, `sketch.cancel`, `group.finish`, `group.cancel`, Ch. 3). The current mode stack is part of `ui.get_state()` (`edit_mode`, UI-API-002).

**Acceptance criteria.**
- [ ] During floor boundary sketching, walls are drawn in the dimmed editing colour and cannot be selected.
- [ ] After Finish, one Undo removes the floor.
- [ ] Finishing a floor sketch with an open loop keeps sketch mode and highlights the gap.
- [ ] A sketch nested in a group edit shows both mode names in the canvas label; Finish ends only the sketch.

### 1.9.4 Draw gallery
**ID:** UI-CTX-004 · **Priority:** P0 · **Ref:** research/01 §7.14 (Draw)

**Behaviour & rules.**
- Tools that create curve-based elements or sketches show a **Draw** panel with: element line kinds valid for the context (Boundary Line, Model Line, Reference Line/Plane, Slope Arrow…), placement surface (Draw on Work Plane / Draw on Face) and a **shape gallery**: Line, Rectangle, Inscribed Polygon, Circumscribed Polygon, Circle, Start-End-Radius Arc, Centre-Ends Arc, Tangent End Arc, Fillet Arc, Spline, Ellipse, Partial Ellipse, Pick Lines, Pick Walls (or Pick Faces where valid).
- The active shape is a tool option (`shape`) and is remembered per tool.
- Shapes are drawing aids only: the submitted command receives curves (owner: ARC `sketch.add_curve`).
- **Availability.** The owning tool declares which shapes it accepts. Shapes the tool cannot use are not shown (for example Spline is not offered for walls, Pick Walls is offered only where boundaries can follow walls). The default shape is the tool's first declared shape, normally Line.
- **Pick lines in the gallery** create curves that keep a link to the picked edge where the owner supports it (shown by a lock glyph after picking); the tool option `pick_lock` (default off) makes that link a constraint.
- **Shape options.** Each shape adds its own tool options (§1.10): number of sides and inscribed/circumscribed for polygons, radius on/off and value for arcs and fillets, Chain and Offset for open shapes, and Close loop for line chains. Options are remembered per tool and shape with persistence `user`.
- **Keyboard.** The gallery is reachable through keytips; while a draw tool is active, Ctrl+Shift+O moves focus into the option controls (§1.10). Typed lengths and angles use listening dimensions (§1.22.1).
- **Micro-operations.** Each click, typed value and shape change is recorded as a micro-operation of the active tool session, so an interrupted sketch can be replayed (conventions §2.1).
- **Placement surface.** Draw on Work Plane uses the active work plane (Ch. 3); Draw on Face makes each pick select a face first. The choice is a tool option `surface`.

**Commands.** `tool.set_option("shape", line|rectangle|polygon_inscribed|polygon_circumscribed|circle|arc_3pt|arc_centre|arc_tangent|arc_fillet|spline|ellipse|ellipse_partial|pick_lines|pick_walls|pick_faces)` and `tool.get_options()` while a draw tool is active; curves are submitted with the owner's commands.

**Acceptance criteria.**
- [ ] Choosing Rectangle in Place Wall creates four joined walls with one click-pair.
- [ ] Switching the floor sketch to Circle and back to Line keeps each shape's last radius and offset settings.
- [ ] The wall tool's gallery does not offer Spline.

---

## 1.10 Tool option presentation (no Options Bar)

**ID:** UI-OPT-001 · **Priority:** P0 · **Ref:** research/01 §9 · `screenshots/ui/options_bar.png`, `screenshots/ui/revit2027_options_bar_removed.png`

**Purpose.** Present the options of the active tool without a separate Options Bar row.

**Behaviour & rules.**
- Reviera has **no Options Bar**. Tool options (UI-ARCH-004) are rendered as:
  1. **Contextual ribbon panels**, grouped by the option's `group` attribute (for example Placement, Constraint, Levels, Location). Controls are embedded wx controls sized for one-line use.
  2. A **Tool Options** category at the top of the Properties palette, always expanded while a tool is active.
- The two renderings are views of the same data; editing either updates the other immediately.
- Options with keyboard relevance are reachable by keytips (for example Alt, then the option's keytip) and by **Tab focus cycling** from the canvas into the first option control with **Ctrl+Shift+O** (Reviera addition, returns to canvas on Enter or Esc).
- A user preference "Show tool options in a strip below the ribbon" (P2) renders the same options as a single row for users who prefer that arrangement; it is off by default.
- Lua and MCP read and write tool options with `tool.get_options` / `tool.set_option`.

**Acceptance criteria.**
- [ ] Every option shown in the reference's Options Bar for wall, floor sketch, stair, tag, pipe and duct placement is available in the corresponding contextual panel.
- [ ] Changing Location Line in the ribbon updates the Properties Tool Options row and the in-canvas preview.
- [ ] With the P2 strip preference on, the same options appear in a single row and stay synchronised.

---

## 1.11 Properties palette

### 1.11.1 Palette structure and content rules
**ID:** UI-PROP-001 · **Priority:** P0 · **Ref:** research/01 §10 · `screenshots/ui/properties_palette_annotated.png`, `screenshots/ui/properties_filter_dropdown.png`

**Purpose.** View and edit the instance properties of whatever is in context, and reach type properties.

**UI (top to bottom).**
1. **Type Selector** (§1.12).
2. **Properties filter** combo: "<Category> (n)", "Common (n)" for mixed selections, or "View: <name>" when nothing is selected. Choosing a category narrows the grid to that category **without changing the selection**.
3. **Edit Type** button: opens Type Properties for the shown entity's type.
4. **Tool Options** category (only while a tool session is active, §1.10).
5. **Parameter grid**: collapsible groups (Constraints, Construction, Graphics, Materials and Finishes, Dimensions, Identity Data, Phasing, Structural, Mechanical, Electrical, Plumbing, Energy, Text, IFC, Other…) with two columns: name and value.
6. **Footer**: sort buttons (group order, A→Z, Z→A), Help link for the shown category, and **Apply**.

**What it shows.**

| Context | Content |
|---|---|
| Nothing selected, no tool | Active view's properties |
| View selected in Project Browser | That view's properties |
| Tool session active | Properties of the element about to be created (instance defaults) plus Tool Options |
| One category selected | Instance properties of the selection |
| Several categories | Only properties common to all; filter combo lists each category with counts |
| Type selected in Project Browser | Type properties, read-only, with an Edit Type button |
| MEP element with Systems tab active | System properties of its system |
| Family editor document | Family parameters for the active family |
| Elements in a linked model | Read-only instance properties, marked "Linked" |

**Behaviour & rules.**
- **Value editors**: text; number with units (accepts unit suffixes and simple arithmetic like `3000+150` or `12'6"`, evaluated by the units service of Ch. 2); check box; enum combo (searchable as in §1.12); element reference combo (levels, phases, materials) with search; colour; "Edit…" buttons opening dialogs (Visibility/Graphics, View Range, Crop, Material); a small "associate global parameter" button on parameters that support it.
- **Read-only** values are shaded and not focusable for editing; their tooltip states why ("computed", "controlled by view template", "owned by <user>").
- **Mixed values** show `<varies>` (text configurable in Manage › Additional Settings › Multiple Values Indication). Entering a value assigns it to all selected elements.
- **Parameters controlled by a view template** show a lock badge and are read-only.
- **IFC group**: shows GlobalId, IFC class and PredefinedType (read-only) and the property sets on the element, so users can see what will be exported.
- **Apply semantics**: edits apply when the user presses Apply, presses Enter in the editor, or moves the pointer out of the palette (Auto-apply, default on; preference). Pending edits are marked with a change bar. Esc reverts pending edits.
- Each Apply is one undo entry named "Edit properties (<n> elements)".
- **Zoom**: Ctrl+wheel scales the grid font between 80 % and 200 %.
- **Sorting**: in group order, project-defined parameters appear after built-in ones within each group, alphabetically by default (preference: creation order).
- The palette is modeless and dockable (§1.15); hidden with PP, Ctrl+1, or View › User Interface.

**Data (IFC mapping).** The palette displays and edits parameters as defined in Ch. 2 (property sets, quantities, and Reviera Extension Data). It adds no persistent data of its own.

**Commands.** `ui.properties.get(target?) -> PropertyGridModel` (groups, rows, value, display string, editable flag, reason), `ui.properties.set_filter(category|view)`, `ui.properties.apply()`, `ui.properties.revert()`. Property writes are performed by `element.set_param(ids, {name: value})` (Ch. 2).

**Acceptance criteria.**
- [ ] With nothing selected, the palette shows the active view's properties; selecting a view in the Project Browser shows that view's properties without activating it.
- [ ] Selecting 3 walls with different heights shows `<varies>` for Unconnected Height; typing 3000 and Enter sets all three in one undo step.
- [ ] Narrowing the filter to Doors in a mixed selection does not change the status-bar selection count.
- [ ] Entering `2400+600` in a length field stores 3000 mm.
- [ ] Moving the pointer out of the palette applies a pending edit when Auto-apply is on, and does not when it is off.

### 1.11.2 Type Properties dialog entry points
**ID:** UI-PROP-002 · **Priority:** P0 · **Ref:** research/01 §10 (Edit Type)

**Behaviour & rules.**
- **Edit Type** targets the type of the entity whose properties are shown (view type, tool's type, or selection's type). It is disabled when the selection contains more than one type, with a tooltip explaining why.
- The ribbon's **Type Properties** button targets the selection's type or the type selected in the Project Browser.
- The Type Properties dialog itself (duplicate, rename, preview, parameter grid) is owned by Ch. 2; the shell supplies only the entry points.
- **All entry points**, each resolving the target type as stated:

| Entry point | Target type |
|---|---|
| Properties › Edit Type | Type of the entity shown in the palette: the active tool's chosen type, the selection's single type, the browser-selected view's view type, or the active view's view type when nothing is selected |
| Modify › Properties › Type Properties | The selection's single type, or the type node selected in the Project Browser |
| Project Browser › type node › Type Properties (or double-click when the double-click action is "Edit type properties", UI-SEL-005) | That type |
| Type Selector right-click › Type Properties | The type under the pointer in the list |
| Canvas right-click on a single-type selection › Type Properties | The selection's type |

- **Disabled cases**, each with its reason in the tooltip: several types selected ("Selected elements have different types"); elements without types (for example model lines in some contexts: "This element has no type"); linked elements ("Types of linked models are read-only" — the dialog still opens read-only from the browser); BIMserver elements owned by another user (the dialog opens read-only, "Owned by <user>").
- **During a tool session**, editing the type of the tool's chosen type affects the next placements; Duplicate in the dialog switches the tool to the new type on OK.
- **Undo.** OK in the dialog is one undo entry; Cancel discards every change made in the dialog, including duplicates.
- **Narrowed filter.** When the Properties filter is narrowed to one category of a mixed selection (UI-PROP-001), Edit Type targets the single type of that category, if there is one.

**Commands.** `type.edit_properties(type_id?)` (Ch. 2; defaults to the type resolved above).

**Acceptance criteria.**
- [ ] With two wall types selected, Edit Type is disabled and its tooltip says "Selected elements have different types".
- [ ] With a door and a wall selected and the filter narrowed to Doors, Edit Type opens the door's type.
- [ ] Duplicating a type from Edit Type during wall placement makes the next wall use the new type.

---

## 1.12 Type Selector

**ID:** UI-TYPE-001 · **Priority:** P0 · **Ref:** research/01 §10.1 · `screenshots/ui/type_selector_search.png`

**Purpose.** Choose the type to place, or change the type of the selected elements.

**Behaviour & rules.**
- Shown at the top of Properties when a placement tool is active or the selection contains elements that can share a type. It displays a preview image, the family name and the type name.
- Opening it focuses a **search field**. Words may be typed in any order; matching is case-insensitive and matched characters are highlighted. Results are grouped by family with thumbnails.
- Keys: **Down/Tab** moves into the list; **Up/Down/Tab/Shift+Tab** move; **Enter** chooses; **Esc** first clears the search, then closes. **Home/End/PageUp/PageDown** move by item or page.
- Choosing a type for a selection changes it **immediately** (no Apply) as one undo step; incompatible elements are skipped with a status-bar message.
- The list shows **recently used types** at the top (last 5, per category, per user).
- The same searchable-list control is used for every long combo in the shell (property enum values, dialog tables, category pickers).
- Right-click menu: **Add to Quick Access Toolbar** (adds a compact type combo to the QAT) and **Add to Modify Tab** (adds a type gallery panel).

**Commands.** `ui.type_selector.list(category|tool) -> [TypeEntry]`; type changes are performed by `element.set_type(ids, type_id)` (Ch. 2); for an active tool, `tool.set_option("type_id", id)`.

**Acceptance criteria.**
- [ ] Typing "200 conc" finds "Concrete 200mm" in a list of 300 wall types within 50 ms.
- [ ] Changing the type of 20 selected doors is one undo step.

---

## 1.13 Project Browser

### 1.13.1 Tree, tabs and search
**ID:** UI-PB-001 · **Priority:** P0 · **Ref:** research/01 §11 · `screenshots/ui/project_browser_search.gif`, `screenshots/ui/ui_insert_tab_floor_plan.png`

**Purpose.** Navigate and manage every view, sheet, schedule, family, group and link in the model.

**UI.**
- Title: "Project Browser — <model name>".
- **Search box** (Ctrl+F when the canvas or browser has focus): live filtering, matches highlighted at every tree level, arrow keys move through results, Enter opens.
- **Tab strip** (icon toggles): **All, Views, Legends, Schedules, Sheets, Families, Groups, Links**; dynamic tabs **Panel Schedules, Reports, Assemblies, Server Revisions** (the last in BIMserver mode) appear when such items exist. Search covers all tabs.
- **Top-level nodes (All tab)**: Views (by current organisation scheme), Legends, Schedules/Quantities, Sheets, Families (category › family › type), Groups (Model, Detail), Links, Assemblies, Panel Schedules, Reports, Coordination Models.
- The **active view** is bold. **Sheet placement icons** show whether a view is on a sheet, not placed, or (for schedules and legends) placed on several sheets; they can be turned off from the right-click menu.

**Interactions.**

| Action | Gesture | Command |
|---|---|---|
| Open view | Double-click, Enter, or right-click › Open; multiple selected views open together after a confirmation (with "don't ask again") | `view.open` (Ch. 5) |
| Rename | Slow double-click, F2, right-click › Rename | `view.rename` (Ch. 5), `type.rename`, `family.rename` (Ch. 2) |
| Open hosting sheet | Right-click › Open Sheet (disabled when not placed or placed on several sheets) | `sheet.open_for_view` (Ch. 5) |
| Place view on sheet | Drag view onto a sheet node or into an open sheet; or right-click a sheet › Add View; the viewport follows the cursor until clicked | `sheet.place_view` (Ch. 5) |
| Remove from sheet | Right-click the view under a sheet › Remove from Sheet | `viewport.remove` (Ch. 5) |
| New sheet | Right-click Sheets › New Sheet | `sheet.create` (Ch. 5) |
| Duplicate view | Right-click › Duplicate View › Duplicate / With Detailing / As Dependent | `view.duplicate` (Ch. 5) |
| Find referring views | Right-click › Find Referring Views | `view.find_referring` (Ch. 5) |
| Expand / collapse | Right-click › Expand Selected, Collapse Selected, Expand All, Collapse All; keyboard Right/Left, `*` | — |
| List zoom | Ctrl+wheel | — |
| Family type actions | Right-click › Select All Instances (in View / in Project), Type Properties, Duplicate, Rename, Delete, Reload, Edit; drag a type into a view to place it | `selection.select_all_instances`, `type.*`, `family.*` (Ch. 2), `component.place` |
| Group actions | Drag to place; right-click › Delete, Rename, Select All Instances, Properties, Duplicate, Reload, Save Group | `group.*` (Ch. 3) |
| Link actions | Right-click › Add Link, Manage Links, Create Instance, Reload, Reload From, Unload (for everyone / for me) | `link.*` (Ch. 6) |
| Rendered images | Drag onto a sheet | `sheet.place_image` (Ch. 5) |
| Delete | Delete key or right-click › Delete (with confirmation listing dependent items) | `view.delete`, `sheet.delete` (Ch. 5) |

- **Find in Project Browser**: from the canvas, right-click an element or view › Find in Project Browser (**FF**) reveals and selects its node (views, sheets, legends, families including nested families, groups, links, assembly views, view references and schedules on sheets).
- Selecting a node shows its properties in the Properties palette without opening it (UI-PROP-001).
- The browser is virtualised: models with 20 000 views and 5 000 family types must expand and filter interactively.

**Data.** Expansion state, active tab and scroll position are project-scoped UI data stored as an RXD document (§1.29.2); organisation schemes are defined in Ch. 5 and stored as RXD per Ch. 2.

**Commands.** `browser.search(text) -> [node]`, `browser.reveal(element_or_view_id)`, `browser.expand(node_id, recursive?)`, `browser.collapse(node_id|all)`, `browser.set_tab(tab_id)`, `browser.get_tree(tab_id?, depth?) -> tree`, `browser.get_selected() -> [node]`.

**Acceptance criteria.**
- [ ] Typing "lev" highlights "Level 1", "Level 2" and any family named "Leveller" in under 100 ms in a model with 5 000 views.
- [ ] Dragging a floor plan onto a sheet node opens the sheet and attaches the viewport to the cursor.
- [ ] FF on a selected door reveals its type node under Families › Doors.
- [ ] Reopening a model restores the browser's expanded nodes from its RXD.

### 1.13.2 Browser organisation dialog
**ID:** UI-PB-002 · **Priority:** P1 · **Ref:** research/01 §11 (Browser Organization)

**Behaviour & rules.**
- View › Windows › User Interface › **Browser Organization** opens a dialog with three tabs (**Views**, **Sheets**, **Schedules**), each listing organisation schemes with radio buttons for the active scheme and New / Edit / Rename / Delete.
- Editing a scheme opens a two-page dialog: **Filtering** (up to 3 rules: parameter, operator, value, combined with AND) and **Grouping & Sorting** (up to 6 group levels, each with parameter, "use first N characters" option, and a final sort parameter and direction). Any view, sheet or schedule parameter, including project and shared parameters, may be used.
- The top node of each list shows the active scheme name, for example "Views (by Discipline)".
- The dialog is a front end to `browser.create_organization`, `browser.set_organization` and related commands defined in Ch. 5; scheme definitions are stored as RXD. Which scheme is active can be a project default (RXD) and a per-user override (user config).
- Schemes transfer between models with Transfer Project Standards (Ch. 2).

**Acceptance criteria.**
- [ ] A scheme grouping views by Discipline then by the first 2 characters of View Name reorganises the tree immediately on OK.
- [ ] Switching active scheme is undoable when it changes the project default, and not undoable when it changes only the per-user override.

---

## 1.14 System Browser

**ID:** UI-SB-001 · **Priority:** P1 · **Ref:** research/01 §12

**Purpose.** Show MEP systems, zones and analytical systems as a tree, and find unassigned components.

**UI.**
- Opened with **F9**, View › User Interface › System Browser, or canvas right-click › Browsers › System Browser. Docked at the bottom of the centre area by default.
- **View selector**: Systems | Zones | Analytical Systems. In Systems view, a discipline filter: All / Mechanical / Piping / Electrical.
- Toolbar: **Auto-fit columns** (double-click a header also fits) and **Column Settings** (per discipline category: General, Mechanical, Piping, Electrical; show/hide and order).
- Tree: **System classification › System › Components**, plus an **Unassigned** folder per discipline.
- In Analytical Systems view: Add Zone Equipment, Add Air System, Add Water Loop (Ch. 4 commands).

**Behaviour & rules.**
- Hovering a row pre-highlights the element in every open view where it is visible.
- Selection is synchronised both ways with the canvas; Ctrl/Shift multi-select works.
- Row right-click: Expand / Expand All / Collapse / Collapse All; **Select** (in canvas); **Show** (opens or cycles through views containing the element, zooming to it); Delete; Properties.
- The tree updates incrementally after each committed transaction.

**Commands.** `sysbrowser.get_tree(view: systems|zones|analytical, discipline?) -> tree`, `sysbrowser.show_element(element_id)` (cycles views); system data commands are owned by Ch. 4.

**Acceptance criteria.**
- [ ] A newly placed air terminal not connected to a duct appears under Unassigned › Mechanical within one second of placement.
- [ ] Choosing Show on a pipe opens a view that contains it and zooms to it; choosing Show again moves to the next such view.

---

## 1.14a Model Tree

### 1.14a.1 Model Tree pane
**ID:** UI-TREE-001 · **Priority:** P1 · **Ref:** owner requirement (2026-10-13); builds on DOC-GFX-008

**Purpose.** Show the model's elements as a tree, so whole branches can be selected, hidden, isolated or made translucent for inspection.

**UI.**
- A dockable pane (View › User Interface › Model Tree, or MT), docked with the Project Browser by default.
- Tree: **Project › Site › Building › storeys**, in elevation order; under each storey, one node per category ("Walls (12)"), then the elements by name or mark; under a host element, the elements it hosts (a wall's doors and windows); under an aggregate, its parts. Elements with no storey are listed under *Not on a level*, and views, types and other non-physical objects are left out.
- Each node shows its temporary state in the active view: hidden nodes are greyed, translucent nodes are in italics (or marked "(translucent)" where the toolkit has no italics), and isolated nodes are bold.
- A search box filters by name, mark or category.

**Interactions.**
- Clicking a node selects its element (or, for storey and category nodes, every element below it) in the canvas; selecting in the canvas reveals and highlights the node.
- Right-click: Hide, Isolate, Isolate (Ghost Others), Translucent, Show, each acting on the node *with its children*; Zoom to (fits the view to the branch); Section Box (fits the active 3D view's section box to the branch, BX).
- Double-click on an element zooms the active view to it.

**Behaviour & rules.**
- The tree reflects the current model: Reviera elements are placed by their own data (a wall under its base level, a door under its host wall, a floor or ceiling under its level, a grid line under its grid system); elements read from IFC are placed by the file's spatial containment, aggregation, voiding and filling relationships.
- The tree updates after each committed transaction, keeping expansion and scroll position.
- Hiding a storey node hides everything below it, and so on for every node: the tree's actions use the *subtree* scope of DOC-GFX-008.

**Commands.** `model.tree(depth?) -> tree` (nodes: `{id?, label, kind: project|site|building|storey|category|element, children[]}`); visibility uses the `view.temp_*` commands (UI-VCB-001).

**Acceptance criteria.**
- [ ] On the FZK house sample, the tree lists two storeys with their walls, slabs, doors and windows, and each door appears under the wall that hosts it.
- [ ] Right-click › Isolate on "Level 1" leaves only the elements of Level 1 in the active view; the other views are unchanged.
- [ ] Creating a wall adds it under its level within one second, without collapsing the tree.

---

## 1.15 Docking

### 1.15.1 Dockable panes
**ID:** UI-DOCK-001 · **Priority:** P0 · **Ref:** research/01 §13 · `screenshots/ui/dockable_windows_grouped.png`

**Purpose.** Let users arrange palettes and browsers freely.

**Behaviour & rules.**
- Dockable panes: Properties, Project Browser, System Browser, Agent (§1.31), Notifications, Server Projects and Revision History (§1.32), Lua Console, BCF Issues, Fabrication Parts, P&ID Modeler, Reconcile Hosting, Background Tasks, and add-in panes declared in manifests.
- **Docking gestures**: drag a pane by its caption; a translucent outline and dock guides show the landing position. Drop at a window edge to dock there; drop on another pane's caption to **group as tabs**; drop on the lower half of a pane to **stack** below it. Holding **Ctrl** while dragging suppresses docking. **Double-clicking a caption** toggles docked/floating.
- Grouped panes show their tabs along the bottom of the group.
- Panes and view windows cannot be grouped together.
- Each pane has a caption menu: Float, Dock, Auto-hide (collapse to an edge tab that slides out on hover), Close.
- Floating panes can be moved to any monitor and keep their position across sessions (§1.29).

**wx realisation.** `wxAuiManager` with `wxAuiPaneInfo` per pane; grouping uses AUI notebook docking; the dock guides are the AUI "hint" window drawn with theme tokens.

**Commands.** `ui.pane.list() -> [{id, visible, docked, floating, rect, group}]`, `ui.pane.show(id)`, `ui.pane.hide(id)`, `ui.pane.toggle(id)`, `ui.pane.float(id, rect?)`, `ui.pane.dock(id, side, position?)`, `ui.pane.group(id, with_id)`.

**Acceptance criteria.**
- [ ] Dragging Project Browser onto the Properties caption groups them as tabs; double-clicking the Properties tab caption floats only Properties.
- [ ] Holding Ctrl while dragging Properties near the left edge leaves it floating.

### 1.15.2 Hiding chrome
**ID:** UI-DOCK-004 · **Priority:** P0 · **Ref:** research/01 §13 (Hiding chrome), §7.11

**Behaviour & rules.**
- View › Windows › **User Interface** lists check boxes for: Orientation Cube, Navigation Bar, Project Browser, System Browser, Properties, Status Bar, Status Bar – Worksets, Status Bar – Design Options, Agent, Notifications, Lua Console, plus discipline panes (Fabrication Parts, P&ID Modeler, Reconcile Hosting) and add-in panes.
- **Full-screen** (Reviera addition, P1, **F11**): the main window goes OS full-screen and all chrome is hidden (menu bar, ribbon including its tab row, QAT, panes, status bar, View Control Bar and view tabs); only the canvas remains. *(Owner requirement, 2026-10-20, CIRCLE_MENU_FULL_SCREEN.md: "Option to go full-screen on F11. When in full-screen pushing the mouse pointer against the top will show the menu and ribbon menus, and the exit full-screen button. Pushing against the bottom of the screen cause the palette menus to appear and clicking on the model dismisses the panels.")*
- **Focus mode** (**Ctrl+Shift+F11**) is the same chrome-free state and the same edge reveals without OS full-screen: the window keeps its size and the desktop's title bar. Focus mode and full-screen are one mode with two window states; switching between them (F11 while in focus mode, or the reverse) keeps the hidden state and changes only the window.
- **Top-edge reveal.** Pushing the pointer against the top edge of the screen (or window, in focus mode) for 150 ms (the dwell, adjustable in Options › User Interface) slides in the menu bar and the ribbon over the canvas, with an **Exit full-screen** button at the right end of the tab row. The overlay stays while the pointer is over it or a ribbon menu or dialog it opened is open; it hides 600 ms after the pointer leaves it, or at once when the pointer clicks the model. Starting a tool from it hides it, and the tool's options appear in the floating options strip.
- **Bottom-edge reveal.** Pushing against the bottom edge slides in all the panes that were visible before full-screen (Properties, Project Browser, System Browser, Agent, Lua Console, discipline and add-in panes) together with the status bar, as one overlay over the lower part of the canvas, in their docked arrangement compressed to a strip of up to 40 % of the canvas height (the height is draggable and remembered for the session). The canvas does not resize. A click on the model, a left click in the canvas outside the overlay, or Esc dismisses it; editing a pane value does not.
- **Defaults confirmed by the owner (2026-10-20):** the 150 ms reveal dwell, 500 ms on an edge shared with another monitor, the bottom overlay at up to 40 % of the canvas height, Ctrl+Cmd+F for full-screen on macOS and Ctrl+Alt+T for Background tasks.
- **Edge reveal rules.** Reveals never trigger while a mouse button is held (so drags and box selections reaching an edge do not open them) or while a tool's rubber-band is active. On a multi-monitor setup the edges are those of the monitor the window is on, and an edge shared with another monitor triggers only after a dwell of 500 ms, as the pointer can cross it. Reduce motion (§1.27.2) shows and hides them without sliding. Touch and pen: a swipe in from the edge does the same.
- **Chrome element ids.** Each check box maps to an id used by commands, layout bookmarks and scenarios: `orientation_cube`, `navigation_bar`, `status_bar`, `status_bar.worksets`, `status_bar.design_options`, `view_control_bar`, `view_tabs`, `qat`, plus every pane id of UI-DOCK-001 (`properties`, `project_browser`, `system_browser`, `agent`, `notifications`, `lua_console`, `history`, …). Panes are shown and hidden with `ui.pane.*`; the other ids with `ui.chrome.set_visible`.
- **What hiding does.** Hiding a pane keeps its dock position and size, so showing it again restores it where it was. Hiding the Orientation Cube or Navigation Bar applies to every view window, including floating frames. Hiding the View Control Bar or view tabs is allowed only through `ui.chrome.set_visible` and the *Presentation* layout (UI-LAYOUT-002); the menu shows them so the user can bring them back.
- **Never hidden.** The ribbon tab row and the canvas cannot be hidden (§1.2), except that full-screen and focus mode hide the tab row until the top-edge reveal brings it back. The File button stays reachable with Alt, F.
- **Full-screen and focus mode details.** They remember the exact prior state, including floating panes, the window's size and position and the ribbon state, in memory only. Floating panes and floating view frames on other monitors stay where they are. Leaving (F11 or Ctrl+Shift+F11 as entered, Esc with nothing selected, no tool and no overlay shown, or the Exit full-screen button of the top reveal) restores that state. Applying a layout bookmark leaves the mode first. The mode does not persist across restart. On macOS F11 is taken by the system, so full-screen uses Ctrl+Cmd+F, the system's own shortcut, and the green window button enters the same mode; on Wayland the compositor's full-screen request is used and the reveals work on the window's own edges.
- **Storage.** Visibility flags are stored in `layout.toml` and are the *Visible panels* part of a layout bookmark (UI-LAYOUT-001).

*Built (2026-10-20), full-screen and focus mode:* View › User Interface › **Full Screen** (F11) and **Focus Mode** (Ctrl+Shift+F11), also in the Window menu. Entering saves the AUI layout and hides the menu bar (kept attached so its accelerators work: hidden on GTK, the native menu taken off the window on Windows, left alone on macOS), the Quick Access Toolbar, the ribbon, every pane and the status bar; full-screen also makes the window full-screen. The pointer is polled every 40 ms against the display's edges in full-screen and the window's in focus mode (polling sees it over the view tabs and the revealed ribbon, where the views get no mouse events). At the top for the dwell (150 ms, `reveal_dwell_ms` in `ui-settings.json`) the menu bar, Quick Access Toolbar and ribbon come back with an **Exit Full Screen** button to their right; they go 600 ms after the pointer is more than 24 px down into the views, on a press in a view (the click is swallowed), or when a command starts from them. At the bottom the panes shown before come back docked in one row along the bottom, 40 % of the window high, with the status bar; a press in a view or Esc dismisses them, working in a pane does not. Nothing reveals while a mouse button is held or the radial menu is open. F11 also leaves focus mode; Esc with nothing selected and no tool leaves either mode; leaving restores the saved layout exactly. **Deviation:** the reveals dock rather than overlay the view, so the view makes room while they show: native panes cannot be laid over the GPU surface on every platform (a Wayland compositor places top-level windows itself). Not yet: Ctrl+Cmd+F on macOS (F11 is the accelerator everywhere), the 500 ms dwell on an edge shared with another monitor, touch and pen swipes, `ui.fullscreen.reveal` and `.get` as commands, a draggable remembered height for the bottom row, the floating options strip for tools started from the top reveal, the Background tasks shortcut (the status-bar task list is not built).

**Commands.** `ui.chrome.get() -> {element_id: bool}`, `ui.chrome.set_visible(element_id, visible: bool) -> ()`, `ui.focus_mode.toggle() -> ()`, `ui.fullscreen.toggle() -> ()`, `ui.fullscreen.reveal(edge: top|bottom|none) -> ()` (shows or hides an overlay, for scripts and tests), `ui.fullscreen.get() -> {mode: off|focus|fullscreen, revealed: top|bottom|none}`; panes use `ui.pane.show`, `ui.pane.hide` and `ui.pane.toggle` (UI-DOCK-001).

**Acceptance criteria.**
- [ ] Unticking Status Bar hides it; re-ticking restores it at the same height.
- [x] Focus mode toggles twice back to the identical layout. *(Built 2026-10-20.)*
- [x] F11 makes the window full-screen with only the canvas; pushing the pointer to the top shows the menu, the ribbon and Exit full-screen; clicking Exit full-screen restores the window and every pane where it was.
- [ ] In full-screen, pushing the pointer to the bottom shows Properties and the Project Browser as an overlay; changing a wall's height in it keeps the overlay; clicking the model dismisses it and the canvas never resized. *(Built with the panes docked along the bottom, so the canvas makes room; see the deviation above.)*
- [x] A box selection dragged to the bottom edge does not open the bottom overlay.
- [ ] Hiding the Navigation Bar removes it from a floating view frame on a second monitor as well.
- [ ] Hiding Properties, restarting, and showing it again restores it at its previous dock position and width.

---

## 1.16 View windows: tabs, tiling, splits and multiple monitors

### 1.16.1 Tabbed view windows
**ID:** UI-VIEWWIN-001 · **Priority:** P0 · **Ref:** research/01 §13 · `screenshots/ui/tab_views.png`

**Behaviour & rules.**
- Every open view, sheet or schedule has its own **tab** showing a view-kind icon, the view name, a model badge when several models are open, and a close button. A view open in two windows is not allowed; activating an already-open view focuses its tab.
- When tabs overflow, a drop-down at the right of the tab strip lists all tabs (searchable).
- Drag tabs to reorder; drag a tab to a window edge or onto another tab group to **split** (left, right, top, bottom).
- Tab right-click menu: Close, Close Others, Close All, Close All in This Model, Float, Move to New Window, Dock to Main Window, Find in Project Browser, Copy View Name.
- Middle-click on a tab closes it.
- Closing the last view of a model asks whether to close the model.
- **View switching order** for Ctrl+Tab / Ctrl+Shift+Tab: **tab position** or **most recently used** (Options › User Interface). Ctrl+Tab cycles through all views including floating windows; holding Ctrl shows a switcher overlay listing views.

**Commands.** `view.activate(view_id, {new_window?: bool, float?: bool, monitor?: int}) -> window_id` (opens the view if needed, via `view.open` of Ch. 5, then focuses it); `view.close(view_id)`; `view.close_inactive()`; `window.list() -> [{window_id, view_id, model_id, frame_id, rect, active}]`; `window.switch(view_id)` (alias of `view.activate` for the Switch Windows menu); `window.next(direction)`.

**Acceptance criteria.**
- [ ] Opening 40 views shows an overflow drop-down that lists all 40.
- [ ] With MRU order selected, Ctrl+Tab returns to the previously active view.
- [ ] `view.activate` from MCP on a closed view opens and focuses it, returning its window id.

### 1.16.2 Tile, tab-all and close-inactive
**ID:** UI-VIEWWIN-002 · **Priority:** P0 · **Ref:** research/01 §13 · `screenshots/ui/tile_views.gif`

**Behaviour & rules.**
- **Tile Views** (WT) arranges all view windows docked in the main frame into a near-square grid; **Tab Views** (TW) merges all docked view windows back into one tab group. Neither affects floating windows.
- **Zoom All to Fit** (ZA) fits every visible view window.
- **Close Inactive Views** keeps, per model: the focused view in each tile, at least one view per open model or family, and all floating windows; everything else closes.
- Editing in one tile updates all tiles live after the transaction commits.
- **Switch Windows** lists open views grouped by model with a check mark on the active one.
- **Tile grid rule.** For *n* docked view windows, Tile uses `c = ceil(sqrt(n))` columns and `r = ceil(n / c)` rows, filled left to right in tab order; the last row's tiles share the full width. Each tile keeps its own View Control Bar and zoom. Tiling 1 view does nothing; tiling more than 16 views is allowed but each tile is clamped to a minimum of 240 × 180 logical pixels, and tiles beyond what fits stay as tabs in the last tile.
- **Tab Views** keeps the active view active and orders tabs as they were before tiling.
- **Tile memory.** Tab Views remembers the tiled arrangement it leaves: the tab groups, which views are in each, and the divider positions. The next Tile Views restores that arrangement exactly while the same views are open (views closed since are dropped). If views were opened since, Tile Views tiles afresh by the grid rule. This is session state of the window, not saved with the model. (Owner requirement, 2026-10-13.)
- **Zoom All to Fit** applies Zoom to Fit to every visible tile (including floating frames), not to hidden tabs; it adds one entry to each view's navigation history.
- **Close Inactive Views** never closes a view with an active edit mode or tool session, and never closes the last view of a family editor document. Views it closes keep their RXD navigation state (§1.29.2). It shows "Closed n views" in the status bar.
- **Switch Windows** with more than 30 open views adds a search field at the top of the list.
- All four commands are presentation only: no undo entry, no modified flag.

**Commands.** `window.tile()`, `window.tab_all()`, `view.zoom_all_to_fit()`, `view.close_inactive()`, `window.switch(view_id)` (UI-VIEWWIN-001).

**Acceptance criteria.**
- [ ] With 4 views open, Tile Views shows a 2 × 2 grid; Tab Views restores a single tab group.
- [ ] Tile two views, drag the divider to 75 %, Tab Views, then Tile Views: the divider is back at 75 %.
- [ ] Close Inactive Views with 2 models open leaves at least one view per model.
- [ ] With 5 views open, Tile Views gives 3 columns over 2 rows, and the two tiles in the second row share its full width.
- [ ] Close Inactive Views does not close a view in which a floor sketch is active.

### 1.16.3 Floating windows and multiple monitors
**ID:** UI-VIEWWIN-003 · **Priority:** P1 · **Ref:** research/01 §13 · `screenshots/ui/multi_monitor_views.png`

**Behaviour & rules.**
- Dragging a tab outside the main window (or choosing Float / Move to New Window) creates a **secondary frame** containing its own tab area, View Control Bar, Orientation Cube and Navigation Bar. Further tabs can be dropped into it.
- Secondary frames have no ribbon; keyboard shortcuts and the main ribbon act on whichever view window has focus.
- Secondary frames appear in the operating system's window list and task switcher as "Reviera — <view name>".
- **Dock to Main Window** (tab menu) returns a view to the main frame.
- Each frame handles its own DPI (§1.33.3). Moving a frame between monitors with different scale factors re-lays it out without restarting.
- Layout persistence records each floating frame's monitor and rectangle; if that monitor is missing at restore, the frame is placed on the primary monitor at the same relative position, clamped to be fully visible.

**Commands.** `window.float(view_id, rect?, monitor?)`, `window.dock_to_main(view_id)`, `ui.monitors() -> [{index, rect, scale, primary}]`.

**Acceptance criteria.**
- [ ] A view floated onto a 200 %-scale second monitor renders sharply and its controls are correctly sized.
- [ ] Disconnecting the second monitor and restarting places the floating frame on the primary monitor, fully visible.

---

## 1.17 Canvas (drawing area)

### 1.17.1 Canvas host and rendering contract
**ID:** UI-CANVAS-001 · **Priority:** P0 · **Ref:** research/01 §14; TECH_STACK §3.2

**Purpose.** Display views, sheets and schedules and host all direct-manipulation feedback.

**Behaviour & rules.**
- Each view window contains one canvas: a native child window handed to the `wgpu` renderer (TECH_STACK §3.2, §5.2). Schedules use a grid control instead of the GPU canvas (Ch. 5).
- The renderer draws, in order: model and annotation content (Ch. 5); **overlay pass** for pre-highlight, selection, temporary dimensions, grips, snap glyphs, alignment lines, rubber-band rectangles, mode border and label, Orientation Cube, Navigation Bar, Navigation Wheel; then the in-canvas text editor (a native control positioned on top).
- Overlay drawing never waits for model regeneration: during a long transaction the last committed scene remains navigable and overlays stay live.
- **Refresh** (F5) redraws all views from current data.
- Views not currently visible stay open with their state; their GPU resources may be released under memory pressure and recreated on activation.
- Target interaction performance: ≥ 30 frames per second while navigating a 3D view of 1 million triangles on reference hardware (defined in Ch. 0 non-functional requirements), with "simplify during navigation" allowed to drop detail.

**Acceptance criteria.**
- [ ] During a 5-second regeneration, hovering elements still pre-highlights them from the last committed scene.
- [ ] F5 redraws every visible view.

### 1.17.2 Canvas colours and mode indicators
**ID:** UI-CANVAS-002 · **Priority:** P0 · **Ref:** research/01 §14, §25 · `screenshots/ui/temporary_hide_isolate.gif`, `screenshots/ui/dark_theme_full_ui.png`, `screenshots/ui/dark_theme_black_canvas_2023.png`

**Behaviour & rules.**
- Canvas colours are theme tokens (§1.28): `canvas.background`, `canvas.selection` (with "semi-transparent" flag), `canvas.preselection`, `canvas.alert`, `canvas.calculating`, `canvas.editing_dimmed`, `canvas.reveal_hidden`, `canvas.temp_mode_border`, `canvas.temp_view_props_border`, `canvas.reveal_constraints_border`, `canvas.displacement_border`.
- With a dark background, linework drawn in black (or near-black, luminance < 15 %) is displayed in the inverse colour; other colours are kept unless the user enables "adapt colours to background" (P1).
- **Mode indicators**: whenever a view is in a temporary state, Reviera draws a coloured **frame** inside the view border **and** a text label in the top-left corner naming the state(s), so colour is never the only signal:

| State | Border token | Label text |
|---|---|---|
| Temporary Hide/Isolate active | `canvas.temp_mode_border` | "Temporary Hide/Isolate" |
| Reveal Hidden Elements | `canvas.reveal_hidden` (hidden elements also drawn in this colour) | "Reveal Hidden Elements" |
| Temporary View Properties | `canvas.temp_view_props_border` | "Temporary View Properties" |
| Reveal Constraints | `canvas.reveal_constraints_border` | "Reveal Constraints" |
| Highlight Displacement Sets | `canvas.displacement_border` | "Displacement Sets" |
| Isolate Annotations | `canvas.temp_mode_border` | "Annotations Isolated" |
| Sketch / group / in-place edit | none (dimming instead) | mode name, e.g. "Editing Group: Unit A" |

- Several states stack: labels are listed one per line; the border shows the most recent.
- Defaults are chosen to be distinguishable for common colour-vision deficiencies (§1.33).

**Acceptance criteria.**
- [ ] Activating Temporary Isolate shows both the frame and the label; the label is read by the platform screen reader when the view gains focus.
- [ ] Switching to the dark canvas draws black wall cut lines in white.

---

## 1.18 View Control Bar

**ID:** UI-VCB-001 · **Priority:** P0 · **Ref:** research/01 §15 · `screenshots/ui/view_control_bar.gif`, `screenshots/ui/temporary_hide_isolate.gif`

**Purpose.** Per-view graphic controls, always at hand at the bottom-left of each view window.

**Behaviour & rules.**
- Shown at the bottom of every view window (main and secondary frames), left of the horizontal scroll bar. Contents depend on the view kind; controls whose property is governed by an applied view template are disabled with a tooltip naming the template.
- Controls (owner of the underlying command in brackets):

| # | Control | Behaviour | Command (owner) | Pri |
|---|---|---|---|---|
| 1 | Scale | Text button ("1:100"); list of standard scales for the project's unit system plus Custom…; hidden in perspective views | `view.set_property(view, "Scale", v)` (DOC) | P0 |
| 2 | Detail Level | Coarse / Medium / Fine | `view.set_property(…"DetailLevel")` (DOC) | P0 |
| 3 | Visual Style | Wireframe, Hidden Line, Shaded, Shaded with Edges, Consistent Colours, Textures, Realistic, Path-traced (3D only, P2), Graphic Display Options… | `view.set_property(…"VisualStyle")`, `view.set_graphic_display` (DOC) | P0 |
| 4 | Sun Path | On / Off / Sun Settings… | `view.set_property(…"SunPath")`, `sun.settings` (DOC) | P1 |
| 5 | Shadows | On / Off | `view.set_property(…"Shadows")` (DOC) | P1 |
| 6 | Render | Opens the render dialog (3D only) | `render.start` (DOC) | P2 |
| 7 | Crop View | Toggle cropping | `view.set_property(…"CropEnabled")` (DOC) | P0 |
| 8 | Show Crop Region | Toggle crop boundary visibility | `view.set_property(…"CropVisible")` (DOC) | P0 |
| 9 | Lock 3D View | Save Orientation and Lock / Restore Orientation and Lock / Unlock | `view.lock_3d`, `view.restore_orientation` (DOC) | P1 |
| 10 | Temporary Hide/Isolate | With a selection: Isolate Category (IC), Hide Category (HC), Isolate Element (HI), Hide Element (HH), Translucent Element (HT), Isolate, Ghost Others (HG), and the same with children or by type (DOC-GFX-008 scopes). While active: Reset (HR), Apply to View | `view.temp_isolate`, `view.temp_hide`, `view.temp_translucent`, `view.temp_show`, `view.temp_reset` (UI); `view.temp_apply` (UI → DOC `view.hide_elements`) | P0 |
| 11 | Reveal Hidden Elements | Toggles reveal mode with its contextual tab | `view.reveal_hidden.toggle` (UI) | P0 |
| 12 | Collaboration Display | BIMserver mode only: Off, Ownership Status, Owners, Model Updates (changed since my last Get Latest), Worksets, Settings… | `view.set_worksharing_display(mode)` (COL) | P1 |
| 13 | Temporary View Properties | Enable, Temporarily Apply Template…, Restore View Properties, recent templates | `view.temp_properties.*` (DOC) | P1 |
| 14 | Show Analytical Model | Structural/MEP analytical views | `view.set_property(…"ShowAnalytical")` (ANA) | P2 |
| 15 | Highlight Displacement Sets | Toggle | `view.highlight_displacement` (DOC) | P2 |
| 16 | Reveal Constraints | Toggle (CX) | `view.reveal_constraints.toggle` (UI) | P1 |
| 17 | Preview Visibility | Family editor only | `family_doc.preview_visibility.toggle` (DM) | P1 |
| 18 | Thin Lines | Mirror of the global toggle (TL) for quick access | `ui.thin_lines.toggle` (UI) | P0 |

- **Temporary hide/isolate** is session state of the view window: it does not change the model, is not undoable, does not affect printing or export, and is cleared when the model closes unless converted with Apply to View (which becomes a normal undoable DOC command).
- **Reveal hidden mode** draws hidden elements in `canvas.reveal_hidden`, adds the "Reveal Hidden Elements" contextual tab (§1.9.2), and is session state.
- The bar's buttons are keyboard reachable (F6 cycles focus between ribbon, panes, canvas and the View Control Bar; arrow keys move within it).

**Commands.** `view.temp_isolate(view_id?, element_ids[], by: element|subtree|category|type, ghost_others?)`, `view.temp_hide(view_id?, element_ids[], by)`, `view.temp_translucent(view_id?, element_ids[], by, opacity?)`, `view.temp_show(view_id?, element_ids[], by)`, `view.temp_reset(view_id? (default: active)) -> ()`, `view.temp_apply(view_id? (default: active)) -> ()`, `view.reveal_hidden.toggle(view_id?)`, `view.reveal_constraints.toggle(view_id?)`, `view.get_temp_state(view_id?) -> {hidden[], isolated[], translucent[], ghost_others, reveal, constraints}`.

**Acceptance criteria.**
- [ ] HI on two selected walls isolates them in the active view only; printing that view prints all elements.
- [ ] Apply to View converts the temporary state into permanent hidden elements in one undo step.
- [ ] On a view with a template controlling Detail Level, the Detail Level control is disabled and its tooltip names the template.

---

## 1.19 Navigation

### 1.19.1 Mouse, keyboard and 3D input devices
**ID:** UI-NAV-001 · **Priority:** P0 · **Ref:** research/01 §16.1

**Behaviour & rules.**
- **Middle-drag** pans; **wheel** zooms about the cursor; **double-click middle** zooms to fit.
- **Shift + middle-drag** (3D) orbits about the pivot: the selection's centre if something is selected, else the point of the model under the cursor where the drag starts (the first visible surface the ray meets), else the model's visible extents centre, else a user-defined centre set with **Define Centre of Rotation** (R3, right-click in 3D).
- **Ctrl + Shift + middle-drag** (3D, Reviera addition) looks around from the eye point in perspective views.
- **F8** starts orbit with the left button until Esc (for users without a middle button). A preference offers **left-button navigation profile** for trackpads: Alt+drag orbit, Alt+Ctrl+drag pan (P1).
- Trackpads: two-finger scroll pans, pinch zooms, Shift+two-finger scroll orbits in 3D (P1).
- Zoom commands: Zoom In Region (ZR), Zoom Out 2× (ZO), Zoom to Fit (ZF), Zoom All to Fit (ZA), Zoom Sheet Size (ZS), Previous Pan/Zoom (ZP), Next Pan/Zoom (ZN, Reviera addition).
- Each view keeps a navigation history of up to 50 states for Previous/Next.
- **3D mice** (6-DoF devices) are supported through the open device protocol available on each platform (for example spacenavd on Linux) with modes 2D, Object, Walk and Fly (P2).
- Navigation does **not** create undo entries and does **not** mark the model modified. The latest camera of each view is kept as view state: it goes to the live store at once (Ch. 2b), and is written to the RXD document `views/state` (latest camera and Home per view, by view id) along with the next save that happens for any other reason. A view's saved camera in `views/<viewId>` changes only through edits such as `view.set_camera`; when it changes after the latest navigation, the saved camera wins. Closing a model whose only changes are navigation does not prompt to save. An explicit `view.set_camera` call (from a script, an agent, or *Save Orientation*) is an ordinary undoable edit that marks the model modified. In BIMserver mode, navigation never causes a check-in, and the default 3D view is per user.

**Commands.** `view.zoom(view_id?, mode: in_region|out|fit|all_fit|sheet_size|previous|next, rect?)`, `view.pan(view_id?, dx, dy)`, `view.orbit(view_id?, yaw_deg, pitch_deg, pivot?)`, `view.set_camera(view_id, eye, target, up, projection?: orthographic|perspective, fov?) -> ()`, `view.get_camera(view_id) -> Camera`, `view.set_pivot(view_id, point)`, `view.zoom_to_elements(view_id?, element_ids)`.

**Acceptance criteria.**
- [ ] Wheel zoom keeps the model point under the cursor fixed to within one pixel.
- [ ] Shift+middle-drag with a door selected orbits about the door.
- [ ] `view.zoom_to_elements` from MCP frames the given elements in the active view.

### 1.19.2 Orientation Cube
**ID:** UI-NAV-002 · **Priority:** P0 · **Ref:** research/01 §16.2, §24.10 · `screenshots/ui/viewcube.png`

**Behaviour & rules.**
- Shown in a corner of 3D views (default top-right). Semi-transparent when idle (configurable inactive opacity), opaque on hover.
- Clicking any of its 26 regions (6 faces, 12 edges, 8 corners) animates to that orientation; dragging orbits freely; in face views, roll arrows rotate 90° and adjacent-face arrows step to neighbouring faces.
- A **projection button** (Reviera addition, owner request) shows the current projection, PERSP or ISO, and switches between perspective and isometric (parallel) projection with one click. The same toggle is in the View menu and in the right-click menu, and has the keyboard sequence PI.
- A **Home** button returns to the view's home orientation; a **compass ring** below shows N/E/S/W (project north or true north per view setting); clicking a letter faces that direction; dragging the ring rotates about the vertical axis.
- Right-click menu: Go Home; Save View (creates a named 3D view, DOC); Lock to Selection; Perspective / Orthographic; Set Current View as Home; Set Front to View ▸; Reset Front; Show Compass; Orient to View ▸ (any plan, elevation, section or 3D view; adds a section box when orienting to a plan or section); Orient to Direction ▸ (N, S, E, W, NE, NW, SE, SW, Top, Bottom); Orient to a Plane…; Help; Options….
- Face labels use the project's localisation; their content is generated text, not artwork.
- Options (§1.27): show/hide; all 3D views or active only; position (4 corners); size (Auto, Tiny, Small, Normal, Large); inactive opacity (0–100 %); snap to closest view when dragging; fit to view on change; animated transitions (and their duration); keep scene upright; show compass.
- Keyboard: when the canvas has focus, **Alt+Home** goes home; **Alt+numpad** keys choose faces (P1 accessibility addition).

**Commands.** `view.orient(view_id?, preset: top|bottom|front|back|left|right|ne_iso|…|home)`, `view.orient_to_view(view_id?, source_view_id, add_section_box?)`, `view.orient_to_direction(view_id?, dir)`, `view.set_home(view_id?)`, `view.set_projection(view_id?, perspective|orthographic)`.

**Acceptance criteria.**
- [ ] Each of the 26 regions produces a distinct, correct orientation.
- [ ] Orient to View › Level 1 plan produces a top-down 3D view with a section box matching the plan's view range.
- [ ] With inactive opacity 0 %, the cube appears only on hover.

### 1.19.3 Navigation Bar
**ID:** UI-NAV-003 · **Priority:** P1 · **Ref:** research/01 §16.3 · `screenshots/ui/navigation_bar.png`

**Behaviour & rules.**
- A slim vertical bar along one edge of each view window (drawn by the overlay pass) with: Orientation Cube toggle, Navigation Wheel (split: 2D, Full, View Object, Tour Building, mini variants), Pan Active View (only on an activated viewport on a sheet), Zoom (split with the zoom commands, last used remembered), 3D Mouse (when a device is present), Fly (perspective views).
- **Customise** menu at the bottom: tick which tools show; Docking ▸ Link to Orientation Cube / Top / Bottom / Left / Right (top/bottom makes it horizontal); dragging along an edge also repositions it.
- Hidden with View › User Interface › Navigation Bar.
- **Default.** Shown in every 3D view and every 2D view window, docked to the right edge and linked to the Orientation Cube (it sits just below the cube in 3D views and at the top of the right edge in 2D views). Tools that do not apply to the view are hidden rather than disabled: the Orientation Cube toggle and Fly in 2D views, Pan Active View except on an activated viewport.
- **Split tools.** Navigation Wheel and Zoom are split buttons whose main part runs the last-used member, remembered per user, as on the ribbon (UI-RIB-001).
- **Linked docking.** When linked to the Orientation Cube, moving the cube to another corner moves the bar with it. When docked to the top or bottom edge, the bar is horizontal and its tools appear in the same order left to right.
- **Opacity.** The bar uses the same inactive opacity as the Orientation Cube (§1.19.2) and becomes opaque on hover; it never covers the View Control Bar or scroll bars.
- **Keyboard.** The bar's tools are commands with their own shortcuts (`view.zoom` modes, `ui.nav.wheel`, `ui.nav.device_mode`), so the bar is never the only way to reach them; it is skipped in the F6 focus cycle.
- **Persistence.** Customise choices and docking position are per user, stored in `settings.toml`, and are part of the *Canvas chrome* part of a layout bookmark.

**Commands.** `ui.navbar.get() -> {tools[], dock: linked|top|bottom|left|right}`, `ui.navbar.set(patch) -> ()`; visibility with `ui.chrome.set_visible("navigation_bar", …)` (UI-DOCK-004).

**Acceptance criteria.**
- [ ] Unticking Navigation Wheel in Customise removes it from every view's bar and persists.
- [ ] Moving the Orientation Cube to the bottom-left corner moves a linked Navigation Bar to the left edge below the cube.
- [ ] Docking the bar to the top makes it horizontal in every open view window.
- [ ] A 2D plan view's bar shows no Orientation Cube toggle and no Fly tool.

### 1.19.4 Navigation Wheel
**ID:** UI-NAV-004 · **Priority:** P2 · **Ref:** research/01 §16.4 · `screenshots/ui/steeringwheel_full_navigation.png`, `screenshots/ui/steeringwheel_2d.png`

**Behaviour & rules.**
- A radial **tracking menu** that follows the cursor. Wedges start a navigation tool on press, act while dragging and end on release.
- Wheels: **2D** (Zoom, Pan, Rewind) — the only wheel in 2D views; **Full** (Zoom, Rewind, Pan, Orbit, Centre, Walk, Look, Up/Down) big and mini; **View Object** (Centre, Zoom, Rewind, Orbit); **Tour Building** (Forward, Look, Rewind, Up/Down).
- While a wheel is open, middle-drag pans, wheel zooms and Shift+middle orbits as usual.
- **Rewind** shows a strip of thumbnails of earlier view states; dragging along it restores any of them.
- Wheel right-click menu: switch wheel; Fit to Window; Restore Original Centre; Level Camera; Increase/Decrease Walk Speed; Orient to View; Orient to a Plane; Undo View Orientation Changes; Save View; Increase/Decrease Focal Length; Move/Re-centre Crop Boundary; Help; Options; Close Wheel.
- Options (§1.27): text visibility (messages, tooltips, cursor text); big and mini size and opacity; invert look vertical axis; walk parallel to ground; walk speed factor; zoom by one step per click; keep scene upright when orbiting.
- The wheel is drawn by the overlay pass in Reviera's own visual design; it shall not reproduce reference artwork.

**Acceptance criteria.**
- [ ] Shift+W (or the Navigation Bar) opens the Full wheel at the cursor; dragging the Orbit wedge orbits; releasing ends the tool with the wheel still shown; Esc closes it.
- [ ] Rewind restores a view state from 10 steps earlier.

---

## 1.20 Status bar

**ID:** UI-STATUS-001 · **Priority:** P0 · **Ref:** research/01 §17 · `screenshots/ui/status_bar.png`

**Purpose.** Show prompts, progress, mode state and selection controls.

**Elements (left to right).**

| Element | Function | Command / source | Pri |
|---|---|---|---|
| Prompt | Next-step text for the active tool; on hover, "Category : Family : Type" of the pre-highlighted element; while typing a key sequence, the first matching command (§1.26.4); snap name while drawing | context engine | P0 |
| Progress | Progress bar with Cancel for long operations (open, save, regenerate, export, server transfer) | core progress events | P0 |
| Server status (BIMserver mode) | Connection state, local changes not checked in (count), new revision available badge; click opens Server pane | §1.32.5 | P1 |
| Workset | Icon opens Worksets dialog; combo shows and sets the active workset | `workset.manage`, `workset.set_active` (COL) | P1 |
| Editing Requests | Count of pending ownership requests; click opens dialog | `editrequest.manage` (COL) | P1 |
| Design Options | Icon opens Design Options; combo sets the active option; Add to Set | `design_option.*` (DM) | P1 |
| Active Option Only | Restrict selection to the active design option | `selection.options.set{active_option_only}` (UI) | P1 |
| Exclude Options | Exclude design-option elements from selection | `selection.options.set{exclude_options}` (UI) | P1 |
| Editable Only | Restrict selection to elements the user may edit (ownership) | `selection.options.set{editable_only}` (UI) | P1 |
| Select Links / Underlay / Pinned / by Face / Drag on Selection | Five selection toggles (§1.21.4) | `selection.options.set{…}` (UI) | P0 |
| Background Tasks | Spinner while background tasks run (colour fills, analysis, background export, server sync); click lists tasks with progress and Cancel; warning icon when background processing is disabled | `ui.background_tasks` (UI), Ctrl+Alt+T | P0 |
| Agent activity | Shows when an MCP/agent session is connected or running a command; click opens the Agent pane or the MCP sessions list | §1.31 | P1 |
| Selection count / Filter | Funnel with the number of selected elements; click opens the Filter dialog | `selection.filter_dialog` (UI) | P0 |

**Behaviour & rules.**
- Hidden entirely with View › User Interface › Status Bar; the Workset and Design Options groups can be hidden separately.
- The prompt is also exposed to screen readers as a live region (§1.33).
- The status bar spans the main frame only; secondary frames show a compact prompt line at their bottom.

**Commands.** `ui.status.get() -> {prompt, progress?, selection_count, toggles}`, `ui.background_tasks() -> [Task]`, `ui.background_tasks.cancel(task_id)`.

**Acceptance criteria.**
- [ ] Hovering a door shows "Doors : <family> : <type>" in the prompt.
- [ ] Clicking the selection count after a crossing selection opens the Filter dialog listing categories with counts.

---

## 1.21 Selection

### 1.21.1 Pre-highlight and picking
**ID:** UI-SEL-001 · **Priority:** P0 · **Ref:** research/01 §18.1

**Behaviour & rules.**
- **Hover** pre-highlights the element under the cursor (heavier lines in `canvas.preselection`), shows its identity in the status bar, and after a delay (§1.25) shows an in-canvas tooltip.
- **Click** selects it. The selection is model-wide: it shows in every view where the elements are visible and in the browsers.
- **Tab / Shift+Tab** cycle forward/backward among candidates under the cursor before clicking: overlapping elements, **chains** of joined walls or lines, elements inside links, shape handles and snap candidates. The status bar shows "n of m" while cycling.
- Right-click on a pre-highlighted element selects it (if not already selected) and opens the context menu.
- **Selection is not undoable** and is not recorded in the model; it is session state. Commands record the selection they acted on for redo purposes.
- **Esc** clears the selection; with a tool active, the first Esc ends the current step, the second ends the tool.

**Acceptance criteria.**
- [ ] With three walls joined in a chain, hovering one and pressing Tab once highlights the chain; clicking selects all three.
- [ ] Selecting a wall in a plan highlights it in an open 3D view.

### 1.21.2 Multiple selection
**ID:** UI-SEL-002 · **Priority:** P0 · **Ref:** research/01 §18.2

**Behaviour & rules.**

| Gesture | Effect |
|---|---|
| Ctrl+click | Add to selection (cursor shows +) |
| Shift+click | Remove from selection (cursor shows −) |
| Drag left → right | **Window**: solid rectangle; selects elements entirely inside |
| Drag right → left | **Crossing**: dashed rectangle; selects elements inside or touching |
| Ctrl / Shift with a box | Add / remove the boxed set |
| Tab to a chain, click | Select the whole chain (follows the smallest turning angle at junctions with more than two members) |
| SA, or right-click › Select All Instances ▸ Visible in View / In Entire Project | All instances of the selected types (in a design option: within the active option) |
| Ctrl+Left, or right-click › Select Previous | Restore the previous selection |
| Ctrl+A (Reviera addition) | Select all selectable elements visible in the active view, honouring the selection toggles |
| Esc | Clear |

- Box selection honours the selection option toggles (§1.21.4) and design-option/editability filters.
- **Reviera: boxes in 3D views.** In a 3D view a left drag orbits (§1.24), so a box there starts with Ctrl (add) or Shift (remove) held. In plans a left drag that starts away from a selected element boxes; the middle and right buttons pan. Only what the view shows is boxed: its view range, section box, slices and hidden elements.
- *Built (2026-10-14):* Ctrl+click, Shift+click, window and crossing boxes (with Ctrl/Shift in 3D), Esc; the box picks model elements, not annotation. Not yet: Tab chains, Select All Instances, Select Previous, Ctrl+A, `selection.box`, the + and − cursors.
- Tools that need several picks (for example "pick walls" modes) use an **add/remove picking mode** with Finish and Cancel on the contextual tab.

**Commands.** `selection.get() -> {ids[], by_category}`, `selection.set(ids[], mode?: replace|add|remove (default replace)) -> ()`, `selection.clear()`, `selection.previous() -> ()`, `selection.select_all_instances(type_ids? (default: types of the current selection), scope: view|project) -> element_ids[]`, `selection.chain(element_id)`, `selection.box(view_id?, rect, mode: window|crossing, combine: replace|add|remove)`, `selection.select_all_visible(view_id?)`.

**Acceptance criteria.**
- [ ] A left-to-right box around half a wall does not select it; right-to-left does.
- [ ] `selection.set([a, b])` from Lua updates the canvas, the Properties palette and the status-bar count.
- [ ] Ctrl+Left after clearing restores the cleared selection.

### 1.21.3 Filter dialog and saved selections
**ID:** UI-SEL-003 · **Priority:** P0 (filter) / P1 (saved sets) · **Ref:** research/01 §18.3, §18.2 (saved selection sets), §7.12 (Selection panel)

**Behaviour & rules.**
- **Filter** dialog: list of categories in the selection with Count and a check box each; Check All / Check None; running total; OK keeps checked categories.
- **Saved selection sets** (Manage › Selection, or the contextual Selection panel): Save (name), Load (replace or add), Edit (opens an editing mode where clicks add or remove members, with Finish/Cancel). Sets are stored with the model as **RXD documents**, so they travel with the IFC (Ch. 2); saving, editing and deleting a set are undoable.
- Saved sets are also usable as view filter sources (Ch. 5).

**Commands.** `selection.filter_dialog()`, `selection.filter(category_ids[]) -> element_ids[]` (keeps only those), `selection.set_save(name, ids?) -> set_id`, `selection.set_load(set_id, mode: replace|add)`, `selection.set_edit(set_id)`, `selection.set_delete(set_id)`, `selection.set_list() -> [set]`.

**Acceptance criteria.**
- [ ] Unchecking Furniture in the Filter dialog removes all furniture from the selection.
- [ ] A saved set survives save, close and reopen; renaming it is undoable.

### 1.21.4 Selection option toggles
**ID:** UI-SEL-004 · **Priority:** P0 · **Ref:** research/01 §18.4, §17

**Behaviour & rules.**
- Global, per-user toggles shown in the Select panel drop-down and as status-bar icons:
  - **Select Links** — on: a link highlights as a whole and Tab reaches its elements; off: links cannot be selected but can still be snapped and aligned to.
  - **Select Underlay Elements**.
  - **Select Pinned Elements** — off: pinned elements cannot be picked or boxed (protects grids and links).
  - **Select Elements by Face** — on: clicking a face interior selects (not in wireframe); off: edges only.
  - **Drag Elements on Selection** — on: press-drag moves an element without selecting it first.
  - **Active Option Only**, **Exclude Options**, **Editable Only** (status bar).
- Toggle states persist per user, not per model.

**Commands.** `selection.options.get() -> {links, underlay, pinned, by_face, drag_on_select, active_option_only, exclude_options, editable_only}`, `selection.options.set(name, value)`.

**Acceptance criteria.**
- [ ] With Select Pinned off, a crossing box over a pinned grid does not select it.
- [ ] Toggling Select Links in the ribbon updates the status-bar icon immediately.

### 1.21.5 Double-click behaviour
**ID:** UI-SEL-005 · **Priority:** P1 · **Ref:** research/01 §18.5

**Behaviour & rules.**
- Options › User Interface › **Double-click actions…** sets, per element kind (Family instance, Sketched element, View inside a sheet, View outside a sheet, Assembly, Group, Stair/Railing, Schedule on sheet), one of: Do nothing / Edit type properties / Edit element (enter its edit mode) / Activate view / Open view.
- Defaults: Family → Edit element (opens the family editor for loadable families); Sketched element → Edit element; Group → Edit element; View on sheet → Activate view; Schedule on sheet → Open view.
- **Remaining defaults.** View outside a sheet (a view marker such as a section head or callout in another view) → Open view; Assembly → Edit element (assembly edit mode, Ch. 5); Stair/Railing → Edit element (component edit mode, Ch. 3). Each kind offers only the actions that make sense for it; for example *Activate view* is not offered for Family instance.
- **Deactivate.** Double-clicking outside an activated viewport on a sheet deactivates it, whatever the settings; this is fixed behaviour, not an option.
- **Elements that cannot be edited** (linked elements, elements owned by another user in BIMserver mode, pinned elements when Select Pinned is off) ignore the double-click and show the reason in the status bar. In-place families follow the *Family instance* setting but open in-place editing rather than the family editor.
- **Edit in place versus editor.** For loadable families, *Edit element* opens the family editor as a separate document. If that family is already open in the editor, its window is activated instead.
- **Double-click timing** follows the operating system's double-click interval and distance.
- **Storage.** Options keys `ui.double_click.<kind>` (for example `ui.double_click.group = "nothing"`), per user. They can be locked by an administrator and changed by a scenario's preference overlay.
- **Dialog.** The Double-click actions dialog is a two-column grid (Element kind, Action combo) with Restore defaults, OK and Cancel; OK applies immediately without restart.

**Commands.** Read and write with `ui.options.get/set("ui.double_click.<kind>", action)`; the dialog is a page of `ui.options.open("user_interface", "ui.double_click")`.

**Acceptance criteria.**
- [ ] Setting Group to "Do nothing" makes double-clicking a group have no effect.
- [ ] Double-clicking a loadable door with default settings opens the door family in the family editor; doing it again activates the same editor window.
- [ ] Double-clicking a linked wall does nothing and the status bar says why.


### 1.21.6 Quick modify menu (middle click)
**ID:** UI-SEL-006 · **Priority:** P1 · **Ref:** owner requirement (2026-10-14); Reviera's own design (Appendix A.2a)

**Purpose.** A short menu of the commonest modify actions at the cursor, without travelling to the ribbon. *(Owner request, 2026-10-14: "a center-click action menu on model objects to bring up a quick modify menu. When two model objects selected test if they overlap and the center-click menu will include CSG operations.")*

**Behaviour & rules.**
- A **middle click** (press and release without moving; a middle drag still pans) on a model element opens the menu at the cursor. If the element is not selected it is selected first, replacing the selection, or added to it with Ctrl. A middle click on nothing offers only Reset Hide/Isolate, when the view has temporary hiding or isolation (hidden elements cannot be clicked); otherwise, and while a tool runs or on a sheet, it does nothing.
- The menu's title names the element (or the count), then: Move, Copy, Rotate, Mirror – Pick Axis, Mirror – Draw Axis; Flip Wall (all walls) or Flip Hand and Flip Facing (all doors and windows); Hide in View, Make Translucent (Make Opaque when the whole selection already is; `view.temp_translucent` / `view.temp_show`), Isolate, Reset Hide/Isolate (when the view has any temporary hiding, translucency or isolation; `view.temp_reset`), Section Box; **Create Component…** (DM-LIB-003); **Path Array** (ARC-MOD-033); Pin and/or Unpin (as the selection is); Delete. Each runs the same command as its ribbon button.
- **Create Component…** turns the current selection into a reusable component. *(Owner requirement, 2026-10-14: "from the center-click, the current selection can be turned into a reusable component".)* It opens the Create Component dialog of DM-LIB-003 for the selection, with any members Tab-selected inside groups and components. The dialog's destination is the library folder used last, and Replace selection with the component is on.
- **Two elements selected:** the menu checks whether their bodies overlap (`overlap.find`, accepted overlaps included, pairs related by design such as a window and its wall left out). If they do, it shows the overlap's volume and the booleans of ARC-MOD-032: Cut A by B, Cut B by A, Intersect, Union, Join (by precedence) and Accept the Overlap (`overlap.resolve`). A cut or intersect asks whether the tool's own body hides. If they do not overlap, the menu says so.

**Commands.** None of its own: the items are the registry's (`element.*`, `wall.flip`, `insert.flip_*`, `view.temp_*`, `overlap.find`, `overlap.resolve`, `component.create`).

**Acceptance criteria.**
- [x] Selecting a wall and Ctrl+clicking the pier through it, then middle-clicking the pier, shows "Overlap of 0.36 m³" and the booleans; Cut A by B cuts the wall and the overlap is gone from the next menu. *(Built 2026-10-14.)*
- [x] A middle click on an unselected element selects it and opens its menu; a middle drag pans.
- [x] Make Translucent, then the menu again, offers Make Opaque; after Hide in View, a middle click on empty space offers Reset Hide/Isolate, which shows the element again.
- [ ] Selecting a desk, its chair and its lamp, then middle-clicking one of them and choosing Create Component…, saves an IFC component of the three into the library folder used last, and the three become one placed copy of it.

### 1.21.7 Radial menu (experimental)
**ID:** UI-SEL-007 · **Priority:** P2 · **Ref:** owner requirement (2026-10-20, CIRCLE_MENU_FULL_SCREEN.md); Reviera's own design

**Purpose.** A ring menu at the pointer as an alternative presentation of the middle-click menu, reaching every ribbon tool by direction rather than by travelling to the ribbon. *(Owner requirement, 2026-10-20: "Center click radial menu. … The radial menu is a ring around the mouse pointer with a maximum of 8 items. A left click dismisses the menu. When the mouse pointer touches a menu item, it re-centers and the ring updates to the next tier of menus. Center click on an element presents the same menus as currently. Center click on the background opens a menu set that mirrors the main menu and ribbon items. Make the radial menu a user controllable application feature, it is experimental.")*

**Behaviour & rules.**
- **Experimental and off by default.** Options › User Interface › Experimental › **Radial menu** (§1.27.2) turns it on. Off, the middle click opens the popup of UI-SEL-006 as before. The setting is per user and takes effect at once.
- **Opening.** A middle click (press and release without moving; a middle drag still pans) opens the ring centred on the pointer. The pointer is never moved: where the pointer is too close to the canvas edge for the ring to fit, the ring is drawn whole, shifted inside the canvas, and its centre dead zone is where it is drawn. The ring is drawn by Reviera over the canvas (not a native menu), so it looks the same on every platform and under Wayland.
- **Layout.** At most 8 wedges per tier, the first at the top, then clockwise. Each wedge shows its icon and a short label; the selected element's name or count is in the centre. A tier with more than 8 items shows 7 and a **More…** wedge (bottom) that opens the rest as the next tier. Disabled items are shown dimmed and cannot be entered or run.
- **Moving through tiers.** The pointer touching a wedge that has a submenu (crossing into the wedge's ring band) makes it the active tier: the ring re-centres on the pointer and shows that wedge's items, with the path (e.g. *Architecture › Build › Wall*) in a breadcrumb above the ring. Moving the pointer back into the centre dead zone (after it has left it) returns to the previous tier, re-centring that tier on the pointer. Wedges have a hysteresis of 4 px so jitter on a boundary does not flip tiers.
- **Leaf items.** Touching a leaf wedge only highlights it and shows its tooltip; a **left click** on the highlighted wedge runs it, through the same command as its ribbon button. A left click anywhere else (on the model or the centre), Esc, or a right click dismisses the ring without running anything. Keyboard: arrow keys move between wedges, Enter enters or runs, Backspace goes back a tier.
- **On an element.** The items are those of the quick modify menu (UI-SEL-006) in the same order, grouped into tiers of 8 where needed (Move/Copy/Rotate/Mirror; Hide/Isolate/Translucent/Section Box; Overlap booleans when two overlapping elements are selected; Component and Path Array; Pin/Unpin; Delete). The selection rules of UI-SEL-006 apply unchanged.
- **On the background.** The first tier mirrors the main menu and ribbon: one wedge per visible ribbon tab (respecting the Disciplines and tools check boxes of §1.7.7, contextual tabs included while active), plus a **Menu** wedge for the File/main menu, overflowing into More… past 7. Each tab's next tier is its panels, then each panel's tools, including split-button and drop-down items as one further tier. A background middle click while the view has temporary hiding or isolation adds a Reset Hide/Isolate wedge first. While a tool runs it still does nothing.
- **Add-ins and scripts.** Ribbon items added by Lua or add-ins (§1.7) appear in their tab and panel like built-in ones; nothing has to be registered twice.
- **Defaults confirmed by the owner (2026-10-20):** off by default; at most 8 wedges with More… past 7; the pointer never moved; touch highlights and left click runs; a right click also dismisses; arrow keys, Enter and Backspace work in the ring; 4 px wedge hysteresis.
- **Accessibility.** Reduce motion turns off the re-centring animation; the Larger grips and hit targets setting enlarges the ring. A screen reader announces the active tier and highlighted wedge.

*Built (2026-10-20):* Options are not built yet, so the setting is View › User Interface › **Radial Menu (Experimental)** (ribbon and Window menu), kept per user in `ui-settings.json` beside the other settings (`radial_menu`, off by default). With it on, a middle click opens the ring instead of the popup; the popup and the ring are built from one list of the quick menu's items (`quick_items`), so they cannot drift apart. On an element the first tier is Transform (Move, Copy, Rotate, the mirrors and flips), Visibility (Hide, Translucent/Opaque, Isolate, Reset, Section Box), Create Component…, Pin or Unpin, Delete, and, with two overlapping elements, a tier named by the overlap ("Overlap of 0.36 m³") with the booleans. On the background the first tier is **Menu** (at the top: the menu bar's menus and their items), the contextual tab while shown, then every ribbon tab; tabs open their panels, panels their tools, and Reset Hide/Isolate comes first while the view has temporary hiding. Tiers past eight show seven and More… at the bottom, paged the same way below. Disabled ribbon actions are dimmed and do not run. The ring is drawn by the renderer as a screen-space layer over the view (`RenderBackend::set_hud`, after the Orientation Cube), wedges first at the top and clockwise, label boxes at 110 px (boxes to the sides grow outward), a 24 px dead zone, a back mark in tiers below the first, the selection's name above the ring and the breadcrumb above that, in the stroke font (which gained `( ) + | < > › ? & … ³` for labels), light or dark with the UI theme. Touching a wedge's ring band highlights it with 4 px hysteresis; reaching its label enters a tier; a left click runs the highlighted leaf (or enters a highlighted tier); a left click elsewhere, a right click or Esc closes; arrows move round, Enter enters or runs, Backspace goes back. Running an item goes through the same menu-id handler as the menus. Near an edge the ring is shifted to fit and the pointer is not moved. Not yet: `ui.radial.open`, `.tree` and `.close` as commands for Lua and MCP (window-only `ui.*` actions are not registered commands yet; the tier tree is checked by the unit tests), icons in the wedges, Lua and add-in ribbon items (there are none in the ribbon yet), Options › User Interface › Experimental, enlarging with Larger hit targets, screen-reader announcements, the reduce-motion setting (the ring does not animate).

**Commands.** `ui.radial.open(x?, y?, target: element|background)` (opens the ring at a canvas point, default the pointer), `ui.radial.tree(target) -> RadialTier` (the tiers as the ring would show them, for scripts, MCP and tests), `ui.radial.close()`. The items themselves run the registry's commands.

**Acceptance criteria.**
- [x] With the experimental setting off, a middle click on a wall opens the popup menu as before; with it on, it opens the ring around the pointer and the pointer has not moved. *(Built 2026-10-20.)*
- [x] On the background, touching Architecture then Build re-centres the ring twice and shows Build's tools; moving back to the centre shows the Architecture tier again; touching Wall highlights it and a left click starts the wall tool.
- [x] A ribbon panel with 11 tools shows 7 and More…, and More… shows the other 4.
- [x] A left click on the model with the ring open dismisses it and runs nothing; a middle drag pans with the setting on.
- [ ] `ui.radial.tree{background}` lists every visible ribbon tab and the Menu wedge, and no tier has more than 8 items.
---

## 1.22 In-canvas controls

### 1.22.1 Temporary dimensions and listening dimensions
**ID:** UI-CTRL-001 · **Priority:** P0 · **Ref:** research/01 §19.1

**Behaviour & rules.**
- When placing or when one element is selected, Reviera shows **temporary dimensions** from the element to the nearest perpendicular references (walls, grids, levels, reference planes) according to the reference preferences in Manage › Additional Settings › Temporary Dimensions (walls: centreline / faces / core centre / core faces; doors and windows: centreline / opening edges).
- **Clicking a value** opens an in-canvas editor; entering a new value moves or resizes the element accordingly (one undo step). Unit expressions are accepted as in the Properties palette.
- **Witness-line grips** can be dragged to another reference, or clicked repeatedly to cycle the reference on that element (Edit Witness Lines, EW).
- A **make-permanent** control under each value converts it into a permanent dimension (DOC `dim.create_aligned`).
- While placing, the displayed length steps by **snap increments** chosen by zoom level (§1.23).
- **Listening dimensions**: during a draw step, typing digits opens the editor pre-filled; Enter fixes the length (or angle when the tool expects one) along the current direction.
- Appearance: text size and background (transparent/opaque) in Options › Graphics.
- With several elements selected, temporary dimensions, pins and constraint padlocks are hidden until **Activate Controls** (AC) is toggled on.

**Commands.** `tempdim.list(element_id) -> [{index, ref_a, ref_b, value}]`, `tempdim.set_value(element_id, index, value)` (resolves to the owning move/resize command; undoable), `tempdim.make_permanent(element_id, index) -> dimension_id`, `ui.controls.activate(on?)`, `ui.temp_dim.settings()`.

**Acceptance criteria.**
- [ ] Clicking the temporary dimension between a selected wall and a grid and typing 1200 moves the wall so the distance is 1200 mm; one Undo reverts it.
- [ ] While drawing a wall, typing 4500 and Enter makes a 4500 mm segment.

### 1.22.2 Grips and shape handles
**ID:** UI-CTRL-002 · **Priority:** P0 · **Ref:** research/01 §19.2

**Behaviour & rules.**
- **End grips** (filled dots) at the ends of curve-driven elements in plan: drag to lengthen, shorten or reorient within the work plane. In a selected chain, shared ends move together. Right-click an end grip of a wall: Allow Join / Disallow Join (ARC `wall.set_join_allowed`).
- **Shape handles** (single arrows) in elevation and 3D: drag faces in one direction. Pressing Tab over one switches to moving the element rather than resizing.
- **Constrained handles** (double arrows): move along a line (for example labelled dimensions in families).
- **Edge handles**: Tab over an edge in elevation/3D picks the whole edge.
- **Dimension text grip**: drags the text of a dimension away from its line.
- **Rotation handle**: a draggable rotation grip with a movable, snapping centre (used by rotate-capable elements and the Rotate tool).
- All grips are drawn in Reviera's own glyph style at a minimum hit target of 8 logical pixels (larger with the accessibility preference, §1.33).
- Dragging produces a live preview; release submits one command and one undo step. Esc during the drag cancels.

**Acceptance criteria.**
- [ ] Dragging a wall end grip to another wall's end joins them on release; Esc during the drag restores the original.
- [ ] Every grip is reachable with Tab cycling when the pointer is over its element.

### 1.22.3 Flip controls and Spacebar
**ID:** UI-CTRL-003 · **Priority:** P0 · **Ref:** research/01 §19.3

**Behaviour & rules.**
- **Flip controls** (paired arrows) appear on selected elements that support flipping: walls (reverse layer orientation), doors and windows (**Flip Facing** and **Flip Hand**), and family-defined flips (single = 180° rotation, double = mirror; horizontal or vertical).
- **Spacebar** with a selection flips or rotates it: free-standing components rotate 90° about their insertion point per press; doors and windows cycle facing/hand combinations; walls flip. If any selected element cannot respond, or the elements share no common direction, nothing happens and the status bar says why.
- Spacebar during placement rotates the component preview by 90° (or aligns to the nearest wall/grid under the cursor, cycling).

- **Flip control placement.** Controls are drawn on the side the flip would move to: for walls beside the exterior face, for doors and windows at the opening on the facing side (facing) and at the hinge side (hand). They are hit targets of at least 8 logical pixels (12 with the larger-targets preference, §1.33.2) and have tooltips "Flip facing", "Flip hand" or "Flip wall orientation".
- **Mixed selections.** Clicking a flip control on one element flips only that element. Space applies to the whole selection only when every element supports the same flip; mixed doors and walls are refused ("Selected elements have no common flip").
- **Door cycle order** for Space: original → flip hand → flip facing and hand → flip facing → original.
- **Rotation centre.** Free-standing components rotate about their insertion point; a selection of several components rotates each about its own insertion point.
- **Hosted elements** stay hosted when their host flips; how their facing responds is defined by the owning chapter (Ch. 3).
- **Undo.** Each Space press or flip-control click is one undo entry named after the owner command, for example "Flip door hand".
- **Locked or pinned** elements are skipped with a status-bar message; ownership in BIMserver mode is checked first.

**Commands.** `ui.flip_selection()` (dispatches to the owner commands `element.flip` / `insert.flip_facing` / `insert.flip_hand` / `element.rotate`, Ch. 3).

**Acceptance criteria.**
- [ ] Pressing Space four times with a door selected visits four distinct facing/hand combinations and returns to the start.
- [ ] Space with a door and a wall selected changes nothing and the status bar explains why.
- [ ] Clicking the flip control of a wall reverses its layer order in one undo step.

### 1.22.4 Pins, locks and constraints
**ID:** UI-CTRL-004 · **Priority:** P0 · **Ref:** research/01 §19.4

**Behaviour & rules.**
- Pinned elements show a **pin** glyph when selected; clicking it unpins (glyph shows struck-through); clicking again re-pins. PN pins, UP unpins.
- Dimensions and alignments that can constrain show a **padlock**; clicking locks/unlocks the constraint (ARC/DOC constraint commands). Multi-segment dimensions show an **EQ** toggle.
- **Reveal Constraints** (CX) shows all constraints in the view with the mode border (§1.17.2).
- **When glyphs show.** Pin, padlock and EQ glyphs appear for a single selected element, or for several only after **Activate Controls** (UI-CTRL-001). A padlock also appears immediately after an Align operation, at the aligned reference, until the next command, so the user can lock the alignment in one click.
- **Pin rules.** Unpinning by clicking the glyph is one undo entry ("Unpin"). Moving, deleting or changing a pinned element is refused with "Element is pinned" and an *Unpin and continue* action in the status bar; deleting a selection that includes pinned elements deletes the others and reports how many were skipped. Pinned state is model data owned by Ch. 3.
- **Padlock states.** Open padlock = unconstrained; closed = constrained; a red padlock marks a constraint that cannot be satisfied after a change, with the failure reported by the owner chapter. Clicking a closed padlock removes the constraint.
- **EQ states.** "EQ" struck through = not equal; "EQ" = equal; clicking toggles. While equal, the segment values may be shown as "EQ" text according to the dimension's display setting (Ch. 5).
- **Reveal Constraints** is session state of the view window (like Temporary Hide/Isolate): it draws every locked dimension, alignment and equality constraint in `canvas.reveal_constraints_border` colour, is not undoable and is cleared when the view closes.

**Commands.** `element.pin`, `element.unpin` (Ch. 3); constraint lock and equality commands are owned by Ch. 3 and Ch. 5; `view.reveal_constraints.toggle(view_id?)` (UI-VCB-001); `ui.controls.activate(on?)` (UI-CTRL-001).

**Acceptance criteria.**
- [ ] Clicking a padlock on an aligned dimension locks it; moving the referenced grid then moves the constrained wall.
- [ ] Trying to move a pinned grid shows "Element is pinned" and leaves it in place.
- [ ] Right after an Align, clicking the padlock at the aligned reference creates the constraint in one undo step.
- [ ] CX shows the mode border and label and draws every constraint in the view; CX again removes them.

### 1.22.5 3D transform gizmo
**ID:** UI-CTRL-005 · **Priority:** P2 · **Ref:** research/01 §19.5

**Behaviour & rules.**
- In massing, family-form and displacement contexts, a triad with axis arrows and plane handles appears at the selection. Axis colours follow the theme tokens `gizmo.x`, `gizmo.y`, `gizmo.z` (default red/green/blue, distinguishable by arrowhead labels X/Y/Z as well as colour) and `gizmo.local` for local mode.
- Spacebar toggles global/local orientation while the gizmo is active.
- **Handles.** Three axis arrows (move along one axis), three plane squares (move in the plane of two axes) and, where the owner allows rotation, three arcs (rotate about one axis). Hovering a handle highlights it and shows its constraint as a dashed infinite line or plane.
- **Placement.** The gizmo sits at the centroid of the selection, or at the selected point or edge midpoint when a sub-element (vertex, edge, face) is selected. It keeps a constant screen size (default 96 logical pixels) independent of zoom.
- **Local orientation** aligns the axes with the selected element's own coordinate system (for a face, its normal is local Z). The current mode is shown as "Global" or "Local" next to the gizmo.
- **Precision.** While dragging, snaps (§1.23) and listening dimensions (§1.22.1) work: typing a number and Enter moves exactly that distance along the active axis. Rotation steps by the angular snap increment.
- **Undo.** One drag is one undo entry, submitted as the owner's move or rotate command (Ch. 3). Esc during the drag cancels it.
- **Micro-operations.** A gizmo drag is a short interactive session; its start point, axis and typed value are journalled under conventions §2.1.

**Commands.** `ui.gizmo.toggle_orientation() -> global|local`; the transforms themselves are `element.move` and `element.rotate` (Ch. 3).

**Acceptance criteria.**
- [ ] Dragging the Z arrow moves the selection only vertically; the label "Z" is visible on the handle.
- [ ] With a sloped face selected and Local mode on, the Z arrow follows the face normal.
- [ ] Dragging the XY plane square and typing 1500 then Enter moves the selection 1500 mm along the drag direction in one undo step.

---

## 1.23 Snaps and alignment

**ID:** UI-SNAP-001 · **Priority:** P0 · **Ref:** research/01 §20

**Purpose.** Make picks precise without typing coordinates.

**Behaviour & rules.**
- While drawing or placing, candidate **snap points** are shown with distinct glyphs (endpoint, midpoint, intersection, centre, quadrant, perpendicular, tangent, nearest, work-plane grid, point) and the snap kind is named in the status bar and next to the cursor.
- **Alignment lines** (dashed) indicate inferences: collinear with existing lines, parallel and perpendicular tracking, horizontal/vertical lock near 0° and 90°, tangent continuation from arc ends, alignment with reference planes and same-category elements, and alignment with linked or imported CAD geometry.
- **Shift** forces horizontal/vertical (orthogonal to the work plane axes) while held.
- **Tab / Shift+Tab** cycle among coincident snap candidates.
- **Snaps dialog** (Manage › Settings › Snaps): Snaps Off; length increment list (semicolon-separated, coarse to fine, chosen by zoom); angular increment list; check boxes for each object snap; Snap to Remote Objects; Snap to Point Clouds; Restore Defaults. Settings are **per user** (not per model) and persist.
- **Snap overrides** apply to the **next pick only**, from the right-click menu › Snap Overrides or two-key sequences:

| Override | Keys | Override | Keys |
|---|---|---|---|
| Snaps Off | SO | Endpoints | SE |
| Midpoints | SM | Nearest | SN |
| Work Plane Grid | SW | Quadrants | SQ |
| Intersections | SI | Centres | SC |
| Perpendicular | SP | Tangents | ST |
| Points | SX | Remote Objects | SR |
| Point Clouds | SY | Close loop | SZ |
| Mid between two points | S2 | Turn override off | SS |

- With **Nearest** off, **jump snaps** let the cursor pick snap points some distance away.
- Snapping is computed in the core against a spatial index and returned to the overlay within one frame for typical views; the UI never blocks waiting for snap results (it shows the raw cursor until results arrive).

**Commands.** `snap.settings.get() -> SnapSettings`, `snap.settings.set(patch)`, `snap.settings_dialog()`, `snap.override_next(kind)`, `snap.query(view_id, point, tolerance_px?) -> [SnapCandidate]` (used by Lua/MCP to compute precise points without a mouse).

**Acceptance criteria.**
- [ ] Typing SM while drawing a wall makes the next click snap only to midpoints; the following click uses normal snaps again.
- [ ] Holding Shift while drawing constrains the segment to horizontal or vertical.
- [ ] Snap settings survive restart and are the same in every model.

---

## 1.24 Context menus

**ID:** UI-MENU-001 · **Priority:** P0 · **Ref:** research/01 §21

**Purpose.** Offer relevant commands at the pointer.

**Behaviour & rules.**
- Context menus are generated from registered **menu contributions** `(target selector, group, command id, order)`; owners and add-ins contribute; unavailable entries are hidden (not disabled) unless the entry declares otherwise.
- Default contents:

| Target | Items |
|---|---|
| Canvas, nothing selected | Cancel; Repeat <last command> (also Enter in idle); Recent Commands ▸; Select Previous; Find Referring Views; Zoom In Region; Zoom Out; Zoom to Fit; Previous/Next Pan/Zoom; Browsers ▸ (Project Browser, System Browser); View Properties; Open Sheet (if placed); Find in Project Browser; (on sheets) Activate View |
| Canvas, element(s) selected | Cancel; Repeat; Recent Commands ▸; Select Previous; Select All Instances ▸ (Visible in View, In Entire Project); Select Joined Elements; Delete; Hide in View ▸ (Elements, Category, by Filter); Override Graphics in View ▸ (Element, Category, Filter); Create Similar; Edit Family; Find in Project Browser; Find Referring Views; Zoom ▸; Properties; group items (Edit Group, Ungroup, Exclude, Move to Project, Restore Excluded); ownership items in BIMserver mode (Request Editing, Show Owner, Relinquish); "Ask Agent about selection" (when the Agent pane is configured) |
| While a draw tool is active | Cancel; Snap Overrides ▸; tool-specific items (for example Close loop) |
| 3D views | Define Centre of Rotation (R3); Orient ▸ |
| Wall end grip | Allow Join / Disallow Join |
| Ribbon tool | Add to Quick Access Toolbar; Assign Keyboard Shortcut…; Show Panel Titles; Show Tabs ▸; Show Panels ▸; Reset Ribbon |
| QAT item | Remove; Add Separator; Move Left/Right; Customize… |
| Type Selector | Add to Quick Access Toolbar; Add to Modify Tab |
| View tab | Close; Close Others; Close All; Float; Move to New Window; Dock to Main Window; Find in Project Browser; Copy View Name |
| Project Browser, System Browser | See §1.13, §1.14 |
| Orientation Cube, Navigation Wheel | See §1.19.2, §1.19.4 |
| Pane caption | Float; Dock; Auto-hide; Close |

- Menus open with right-click, the Menu key, or Shift+F10, at the pointer or (keyboard) at the focused element.
- "Assign Keyboard Shortcut…" opens the shortcuts dialog filtered to that command.

**Commands.** `ui.context_menu.items(target_descriptor) -> [MenuItem]` (for agents and tests), `tool.repeat_last() -> session_id`, `tool.recent() -> [command_id]`.

**Acceptance criteria.**
- [ ] Right-clicking a selected door shows Edit Family and Select All Instances; right-clicking empty canvas does not.
- [ ] Shift+F10 opens the canvas menu at the selection's screen centre.

---

## 1.25 Tooltips and keytips

### 1.25.1 Ribbon and control tooltips
**ID:** UI-TIP-001 · **Priority:** P0 · **Ref:** research/01 §22 · `screenshots/ui/tooltip_ribbon.png`

**Behaviour & rules.**
- Two stages: after a short hover, the **brief** tooltip shows the label, the first-stage description and the first assigned shortcut in brackets (for example "Wall (WA)"). After continued hover, the **extended** tooltip adds the extended description, an optional illustration from Reviera's own help assets, the ribbon path, all shortcuts and "Press F1 for help".
- Disabled tools show the unavailability reason in the brief tooltip.
- **Tooltip assistance** level (Options › User Interface): **None** (no tooltips; F1 still works on focused controls), **Minimal** (brief only), **Normal** (default; brief then extended), **High** (both immediately).
- Tooltips are also delivered as accessible descriptions of the control (§1.33).
- **Timing** (Normal level): brief tooltip after 400 ms of hover; extended after a further 1 200 ms. Moving to an adjacent control while a tooltip is shown replaces it after 100 ms (no new delay). The tooltip hides on pointer exit, key press, click or after 20 s. At High, both stages appear together after 100 ms. Delays are settings keys `ui.tooltips.delay_brief_ms` and `ui.tooltips.delay_extended_ms` (range 0–5 000 ms) under the level key `ui.tooltips.level` (`none` | `minimal` | `normal` | `high`).
- **Content source.** All text comes from the command descriptor (UI-ARCH-001): `label` / `label_long`, `tooltip`, `tooltip_extended`, `default_shortcuts` and the user's actual bindings, `ribbon_paths`. Illustrations are Reviera's own SVG or animated assets referenced by `tooltip_extended`; no video is streamed from the network.
- **Split buttons** show the tooltip of the member under the pointer; for the main part, the last-used member.
- **Add-in tools** show the add-in's name as a final line ("From add-in: Survey Tools").
- **Placement.** Below the control, clamped to the monitor's work area; never covering the control itself. Tooltips follow the UI theme and font scale.
- **Keyboard focus.** When a ribbon tool gets keyboard focus (keytips, F6, arrow keys), its brief tooltip is shown after the same delay, so keyboard users get the same information.
- **F1** with a tooltip visible opens that command's help topic (UI-HELP-001), at every assistance level including None.

**Acceptance criteria.**
- [ ] With Normal assistance, hovering Trim/Extend to Corner shows a brief tooltip within 500 ms and the extended tooltip within 2 s.
- [ ] With None, no tooltip appears.
- [ ] After reassigning Wall to `WW`, the Wall tooltip reads "Wall (WW)".
- [ ] Moving the keyboard focus to the Door tool shows its brief tooltip.

### 1.25.2 In-canvas tooltips
**ID:** UI-TIP-002 · **Priority:** P0 · **Ref:** research/01 §22, §18.1

**Behaviour & rules.**
- Hovering an element shows "Category : Family : Type" (and for linked elements, the link name; for elements owned by others in BIMserver mode, the owner).
- During tools, a cursor-adjacent hint shows the next step and the active snap.
- The same text is shown in the status bar.
- **Element tooltip content**, one line each, only when applicable: "Category : Family : Type"; mark or name if set; "Link: <link name>"; "Owned by <user>" (BIMserver mode); "Pinned"; "Design option: <set> › <option>" when not in the main model; "Hidden by temporary isolate" never appears (hidden elements are not hovered).
- **Delay.** 600 ms of stillness over the same element (setting `ui.tooltips.canvas_delay_ms`); any pointer movement of more than 4 logical pixels hides it. Tab cycling (UI-SEL-001) updates the tooltip immediately to the cycled candidate.
- **Tool hints.** The cursor hint shows at most two short lines: the step prompt from the tool (for example "Pick end point") and, while snapping, the snap name. Hints are suppressed while the in-canvas text editor is open.
- **Assistance level.** *None* hides element tooltips and cursor hints (the status bar still shows the text); *Minimal* shows tool hints only; *Normal* and *High* show both.
- **Placement and look.** Offset 16 logical pixels below-right of the cursor, flipped to stay inside the view window; drawn by the renderer's overlay pass in UI theme colours (not canvas colours), so it stays readable on light and dark canvases.

**Acceptance criteria.**
- [ ] Hovering a linked wall shows the link name in the tooltip.
- [ ] With Minimal assistance, hovering a wall shows no tooltip while the wall tool's "Pick start point" hint still appears.
- [ ] Pressing Tab over overlapping elements changes the tooltip to each candidate in turn.

### 1.25.3 Keytips
**ID:** UI-TIP-003 · **Priority:** P0 · **Ref:** research/01 §22 · `screenshots/ui/keytips.png`

**Behaviour & rules.**
- Pressing and releasing **Alt** (or **F10**) overlays letter badges on File, QAT items, ribbon tabs and (on Reviera addition) the pane captions. Typing a badge opens that tab and shows badges for its tools; typing a tool's badge runs it or opens its menu; Esc steps back one level; any mouse click hides badges.
- Keytips are generated: each tab/tool may declare a preferred keytip; conflicts are resolved automatically by adding a second letter. Localised builds generate keytips from localised labels.
- While keytips are visible, key-sequence shortcuts are suspended.
- **Levels.** Level 1: File (`F`), QAT items (digits `1`–`9`, then `01`–`09` for items 10–18), ribbon tabs, contextual tabs and (Reviera addition) pane captions. Level 2: the tools of the chosen tab, including embedded option controls and dialog launchers. Level 3: members of a split or drop-down button, or items of a collapsed panel or slide-out.
- **Generation.** A tool's keytip is its declared preference if free at that level; otherwise the first letter of the label, then the first letters of following words, then two-letter combinations from the label, then a two-character code. Assignments are stable: they depend only on the ribbon content, not on the order in which panels were loaded, so a user learns them once. Two-character keytips wait for the second key without a timeout.
- **Collapsed and hidden content.** Collapsed panels (UI-RIB-002) get a panel keytip that opens the popup and continues at level 3. Tools hidden by discipline filtering (UI-RIB-007) get no keytip.
- **Badges.** Drawn in UI theme colours at the bottom centre of each control, at least 16 logical pixels high; disabled tools show dimmed badges that do nothing when typed.
- **Exit.** Esc steps back one level; Esc at level 1, Alt again, a mouse click, or focus leaving the main window hides all badges. Running a tool hides them.
- **Floating panels** show badges only when the user presses Alt while pointing at them.

**Commands.** `ui.keytips.show()`, `ui.keytips.hide()`.

**Acceptance criteria.**
- [ ] Alt, then the Annotate tab keytip, then the Text keytip starts the Text tool.
- [ ] No two badges visible at the same level are identical.
- [ ] The same ribbon content produces the same keytips after a restart and after loading the panels in a different order.
- [ ] A collapsed panel's tools can be run with three keytip presses.

---

## 1.26 Keyboard shortcut system

### 1.26.1 Shortcut model
**ID:** UI-KEY-001 · **Priority:** P0 · **Ref:** research/01 §23.1

**Purpose.** Let users run any command from the keyboard with short typed sequences or chorded keys.

**Behaviour & rules.**
- Two binding forms:
  1. **Key sequence**: 1 to 5 alphanumeric keys (letters, digits and `/`, `;`, `,`, `.`) typed without modifiers while the canvas, ribbon or a non-text pane has focus.
  2. **Chord**: one key with modifiers (Ctrl, Shift, Alt in combination; Alt alone with a letter is reserved for keytips; on macOS Ctrl maps to Cmd for reserved chords, while sequences are unaffected).
- A command may have several bindings; a binding may be assigned to several commands (resolved by the status bar, §1.26.4).
- **Execution rule for sequences**: a typed sequence runs as soon as it matches a binding and is not a prefix of any longer binding; if it is also a prefix of a longer binding, Reviera waits for the next key up to the **sequence timeout** (default 800 ms, configurable 300–3000 ms) and then runs the shorter match; Space runs the displayed match at once; Esc or Backspace edits/cancels the buffer.
- Sequences are inactive while a text field has focus; chords stay active unless the focused control consumes them (for example Ctrl+C in a text field copies text).
- **Context scopes**: a binding may be scoped to a context (for example `text_editor`, `sketch_mode`, `schedule_view`, `rebar_placement`); scoped bindings take precedence in their scope and do not conflict with global ones.
- Shortcuts are stored per user in `shortcuts.toml` (§1.29) as a list of `{command, keys, scope}` entries and can be exported and imported in that format.

**Acceptance criteria.**
- [ ] With `CO` and `COL` both bound, typing C, O runs `CO` after the timeout; typing C, O, L runs `COL` immediately.
- [ ] Typing WA in the canvas starts the wall tool; typing WA inside a Properties text field enters text.

### 1.26.2 Keyboard Shortcuts dialog
**ID:** UI-KEY-002 · **Priority:** P0 · **Ref:** research/01 §23.2

**Behaviour & rules.**
- Opened with **KS**, View › User Interface › Keyboard Shortcuts, Options › User Interface › Customize…, or a ribbon tool's right-click "Assign Keyboard Shortcut…".
- **Search** (case-insensitive substring over label, long label, command id and keys).
- **Filter**: All, With shortcuts, Without shortcuts, Reserved, Conflicts, by ribbon tab, by owner chapter, Add-in commands.
- **Grid**: Command | Shortcuts | Locations (ribbon paths) | Scope | Owner, sortable by any column.
- **Press keys** field with **Assign** and **Remove**; conflicts are shown inline before assigning ("Also used by: …"), with the option to assign anyway or replace.
- Reserved commands are shown greyed with their keys in angle brackets; their key field is disabled. Each reserved command has an assignable twin command id (for example `file.save` is reserved on Ctrl+S, and `file.save` may also get a sequence such as `SV`).
- **Import**, **Export**, **Reset to Defaults**, **Print / Save as HTML reference**, OK, Cancel.
- Changes take effect on OK without restart.

**Commands.** `ui.shortcuts.dialog(filter?)`, `ui.shortcuts.list(filter?) -> [{command, keys[], scope, reserved}]`, `ui.shortcuts.assign(command_id, keys, scope?)`, `ui.shortcuts.remove(command_id, keys)`, `ui.shortcuts.import(path, mode: merge|replace)`, `ui.shortcuts.export(path)`, `ui.shortcuts.reset()`.

**Acceptance criteria.**
- [ ] Assigning `WW` to Wall shows no conflict; assigning `MV` to Wall shows "Also used by: Move".
- [ ] Export then Import on another machine reproduces the same map.

### 1.26.3 Reserved keys
**ID:** UI-KEY-003 · **Priority:** P0 · **Ref:** research/01 §23.3

These bindings are fixed and cannot be reassigned (Ctrl reads Cmd on macOS):

| Keys | Function | Command |
|---|---|---|
| Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S / Ctrl+P | New model / Open / Save / Save As / Print | `file.new`, `file.open`, `file.save`, `file.save_as`, `print.run` |
| Ctrl+W, Ctrl+F4 | Close active model | `file.close` |
| Alt+F4 (Cmd+Q on macOS) | Exit | `app.exit` |
| Ctrl+C, Ctrl+Insert / Ctrl+X, Shift+Delete / Ctrl+V, Shift+Insert | Copy / Cut / Paste | `clipboard.*` |
| Ctrl+Z, Alt+Backspace / Ctrl+Y, Ctrl+Shift+Z | Undo / Redo | `edit.undo`, `edit.redo` |
| Ctrl+A | Select all visible (canvas) | `selection.select_all_visible` |
| Ctrl+F | Project Browser search | `browser.search` (focus) |
| Ctrl+K | Command Search | `ui.command_search` (focus) |
| Ctrl+D | Toggle Reviera Home | `ui.home.toggle` |
| Ctrl+Left | Select Previous | `selection.previous` |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous view window | `window.next` |
| Ctrl+` | Activate first contextual tab | `ui.ribbon.activate_contextual` |
| F1 | Context help | `ui.help.context` |
| F2 | Rename (browser) | — |
| F5 | Refresh | `view.refresh` |
| F6 / Shift+F6 | Cycle focus between ribbon, panes, canvas, View Control Bar, status bar | — |
| F7 | Check spelling | `text.check_spelling` |
| F8, Shift+W | Orbit (dynamic view) / Navigation Wheel | `ui.nav.orbit_mode`, `ui.nav.wheel` |
| F9 | System Browser | `ui.pane.toggle{system_browser}` |
| F10, Alt | Keytips | `ui.keytips.show` |
| F11 | Full-screen (Ctrl+Cmd+F on macOS) | `ui.fullscreen.toggle` |
| Ctrl+Alt+T | Background tasks | `ui.background_tasks` |
| Shift+F10, Menu key | Context menu | — |
| Tab / Shift+Tab | Cycle candidates forward / backward | — |
| Esc | Cancel step / clear selection / leave tool | — |
| Enter | Confirm; in idle canvas, repeat last command | `tool.repeat_last` |
| Space | Flip / rotate selection or placement | `ui.flip_selection` |
| Delete | Delete selection | `element.delete` |
| Ctrl+0 / Ctrl+= / Ctrl+- | Reset / increase / decrease zoom of schedule grids and list panes | — |
| Ctrl+wheel | Zoom Properties and browser lists | — |

### 1.26.4 Running sequences from the status bar
**ID:** UI-KEY-004 · **Priority:** P0 · **Ref:** research/01 §23.4

**Behaviour & rules.**
- While a sequence is being typed, the status bar prompt shows the typed keys and the first matching command; **Down/Right** cycle to the next match, **Up/Left** to the previous; **Space** runs the shown match without typing further keys.
- When a completed sequence is bound to several commands, Reviera does not run any: the status bar lists the candidates and the user picks with arrows and Space (or Enter).
- This applies to sequences only, not chords.
- **Display format.** The prompt area shows `Keys: CO › Copy (CO) — 3 more`; while cycling, the shown command, its full sequence and its availability. Unavailable matches (for example a tool that needs a selection) are listed in grey and are skipped by Space, which then runs the next available match.
- **Ordering of matches.** Exact matches first, then longer sequences in alphabetical order of their keys; within the same keys, scoped bindings for the current context before global ones (UI-KEY-001).
- **Timeout interaction.** Cycling with arrow keys stops the sequence timeout; the buffer stays until Space, Enter, Esc or 10 s without input.
- **Ambiguous completed sequences** (several commands bound to the same keys in the same scope) show a numbered list; typing the digit 1–9 also picks a candidate.
- **No match.** A key that makes the buffer match nothing is ignored, and the prompt shows "No command for <keys>" for 1.5 s; the buffer keeps its previous content.
- **Accessibility.** The prompt is a live region, so screen readers announce the shown command at the chosen verbosity (§1.33.4).
- **Scripts and tests** can resolve a sequence without typing it.

**Commands.** `ui.shortcuts.resolve(keys, scope?) -> [{command_id, exact: bool, available: bool}]`.

**Acceptance criteria.**
- [ ] Typing R then Down shows successive commands whose sequences start with R; Space runs the shown one.
- [ ] With `XY` bound to two commands in the same scope, typing XY shows a numbered list and typing 2 runs the second command.
- [ ] Typing a key that matches nothing shows "No command for …" and leaves the buffer unchanged.
- [ ] `ui.shortcuts.resolve("WA")` returns the wall command as an exact match.

### 1.26.5 Reviera default shortcut map
**ID:** UI-KEY-005 · **Priority:** P0 · **Ref:** research/01 §23.5, §20

**Purpose.** Provide a default map that is efficient and familiar to users of two-letter mnemonic schemes, defined by Reviera's command ids.

**Rules applied in building the map.**
- Two-letter mnemonics are used where they are widely known in the industry so that users can transfer muscle memory; Reviera's map is maintained as its own data file (`shortcuts-default.toml`) keyed by Reviera command ids.
- **No default sequence is bound to two commands.** Where traditional mnemonics collide, Reviera gives the sequence to one command (the more frequent) and leaves the other unbound: `RC` → Repeat Last Command (coping removal has none); `RG` → Remove from Group (render gallery has none); `PC` → Check Pipe Systems (point-cloud snap override moves to `SY`); `SZ` → Close-loop snap (system-zone sketch has none); `SE` → Endpoint snap (Section has none); `ME` → Mechanical Equipment (Measure has none); `ST` → Tangent snap (Stair has none).
- Additions not found in the traditional scheme: `3D` default 3D view, `ZN` next pan/zoom, `CI` check in, `FL` floor, Ctrl+K Command Search, Ctrl+J Agent pane, Ctrl+Shift+L Lua Console, Ctrl+Shift+O focus tool options, F11 full-screen, Ctrl+Shift+F11 focus mode, Ctrl+Alt+T background tasks, Ctrl+A select all visible.
- Scoped bindings (text editor, rebar placement) apply only in their scope.

**Modify and edit (global scope)**

| Keys | Command id | Action |
|---|---|---|
| MD | `tool.modify` | End tool, select mode |
| AL | `element.align` | Align |
| MV | `element.move` | Move |
| CO, CC | `element.copy` | Copy |
| RO | `element.rotate` | Rotate |
| MM | `element.mirror` | Mirror by picked axis |
| DM | `element.mirror` | Mirror by drawn axis |
| AR | `element.array_linear` | Array |
| RE | `element.scale` | Scale |
| OF | `element.offset` | Offset |
| TR | `element.trim_corner` | Trim/Extend to corner |
| SL | `element.split` | Split element |
| PN / UP | `element.pin` / `element.unpin` | Pin / Unpin |
| DE | `element.delete` | Delete |
| MA | `element.match_type` | Match type properties |
| CS | `tool.create_similar` | Create similar |
| GP | `group.create` | Create group |
| EG / UG | `group.edit` / `group.ungroup` | Edit / ungroup group |
| AP / RG | `group.add_members` / `group.remove_members` | Add to / remove from group (group edit) |
| AD | `group.attach_detail` | Attach detail group |
| FG / CG | `group.finish` / `group.cancel` | Finish / cancel group edit |
| EX / MP | `group.exclude_member` / `group.move_member_to_project` | Exclude member / move member out of group |
| RB / RA | `group.restore_member` / `group.restore_member{all}` | Restore excluded member(s) |
| PT | `face.paint` | Paint |
| SF | `face.split` | Split face |
| CP | `steel_cut.cope` | Apply coping |
| LW | `view.set_linework` | Linework |
| AC | `ui.controls.activate` | Activate controls and dimensions |
| PP, Ctrl+1 | `ui.pane.toggle{properties}` | Properties palette |
| R3 | `view.set_pivot` (interactive) | Define centre of rotation |
| EW | `tempdim.edit_witness` | Edit witness lines |
| SA | `selection.select_all_instances{scope=project}` | Select all instances in model |
| RC | `tool.repeat_last` | Repeat last command |

**Create (global scope)**

| Keys | Command id | Action |
|---|---|---|
| WA | `wall.create` | Wall |
| DR | `door.place` | Door |
| WN | `window.place` | Window |
| CM | `component.place` | Place component |
| FL | `floor.create` | Floor |
| CL | `column.place` | Structural column |
| SB | `floor.create{structural}` | Structural floor |
| RM / RT | `room.place` / `tag.create_room` | Room / room tag |
| LL / GR | `level.create` / `grid.create` | Level / grid |
| RP | `refplane.create` | Reference plane |
| LI | `modelline.create` | Model line |
| PK | `workplane.set` | Set work plane |
| BM / BR / BS | `beam.create` / `brace.create` / `beam_system.create_auto` | Beam / brace / beam system |
| FT | `wall_foundation.create` | Wall foundation |
| RN | `reinforcement_numbering.show` | Reinforcement numbers |
| PL / RF / CFG | `precast.split` / `precast.reinforce` / `precast_config.show` | Precast split / reinforce / configuration |
| // | `cme.divide_surface` | Divide surface (family forms) |

**Annotate**

| Keys | Command id | Action |
|---|---|---|
| DI | `dim.create_aligned` | Aligned dimension |
| EL | `spot.create_elevation` | Spot elevation |
| DL | `detail.line` | Detail line |
| TX | `text.create` | Text |
| TG | `tag.create` | Tag by category |
| FR | `text.find_replace` | Find/replace |
| Ctrl+B / Ctrl+I / Ctrl+U (scope `text_editor`) | `text.format{bold\|italic\|underline}` | Bold / italic / underline |
| Ctrl+Shift+A (scope `text_editor`) | `text.format{all_caps}` | All caps |
| Ctrl+= / Ctrl+Shift+= (scope `text_editor`) | `text.format{subscript\|superscript}` | Subscript / superscript |
| Ctrl+Q (scope `text_editor`) | `text.close_editor` | Close text editor |

**Systems (MEP)**

| Keys | Command id | Keys | Command id |
|---|---|---|---|
| DT | `duct.create` | DF | `duct_fitting.place` |
| DA | `duct_accessory.place` | FD | `flex_duct.create` |
| CV | `duct.convert_to_flex` | AT | `air_terminal.place` |
| ME | `mech_equipment.place` | PI | `pipe.create` |
| PF | `pipe_fitting.place` | PA | `pipe_accessory.place` |
| FP | `flex_pipe.create` | PX | `plumbing_fixture.place` |
| PE | `mech_equipment.place` | SK | `sprinkler.place` |
| CT | `cable_tray.create` | TF | `cable_tray_fitting.place` |
| CN | `conduit.create` | NF | `conduit_fitting.place` |
| EE | `electrical_equipment.place` | LF | `lighting_fixture.place` |
| AW | `wire.create{arc}` | PS | `panel_schedule.create` |
| DC | `mep_check.systems` | PC | `mep_check.systems` |
| EC | `mep_check.systems` | MS | `mep_settings.show` |
| ES | `mep_settings.show` | FS | `mep_settings.show{fabrication}` |
| BT | `mep_settings.show{building_space_type}` | PB | `fabrication_part.place` |
| MR | `fabrication_route.multi_point` | EP | `fabrication_part.edit` |
| RI | `circuit.renumber` | F9 | `ui.pane.toggle{system_browser}` |

**View and visibility**

| Keys | Command id | Keys | Command id |
|---|---|---|---|
| VG, VV | `view.visibility_graphics` | TL | `ui.thin_lines.toggle` |
| HH | `view.temp_hide{by=element}` | HI | `view.temp_isolate{by=element}` |
| HC | `view.temp_hide{by=category}` | IC | `view.temp_isolate{by=category}` |
| HR | `view.temp_reset` | RH | `view.reveal_hidden.toggle` |
| HT | `view.temp_translucent{by=element}` | HG | `view.temp_isolate{by=element, ghost_others}` |
| MT | `ui.pane.toggle{model_tree}` | SX | `view.set_section_box{toggle}` |
| EH | `view.hide_elements` | VH | `view.set_category_visibility` |
| EU | `view.unhide_elements` | VU | `view.set_category_visibility` |
| EOD | `view.override_elements` | EOH | `view.override_elements{halftone_toggle}` |
| VOH | `view.set_category_override{halftone_toggle}` | BX | `view.fit_section_box` |
| VR | `view.set_view_range` | GD | `view.set_graphic_display` |
| WF | `view.set_visual_style{wireframe}` | HL | `view.set_visual_style{hidden_line}` |
| SD | `view.set_visual_style{shaded_edges}` | CX | `view.reveal_constraints.toggle` |
| CA | `ui.canvas_theme.toggle` | SU | `sun.settings` |
| RR | `render.start` | TS | `toposolid.toggle_smooth` |
| 3D | `view.create_3d` | PV | `family_doc.shape_points.toggle` |
| TV (scope `rebar`) | `rebar.set_view_visibility` | BP | `toposolid.excavate` |

**Navigation and windows**

| Keys | Command id | Keys | Command id |
|---|---|---|---|
| ZR, ZZ | `view.zoom{in_region}` | ZO, ZV | `view.zoom{out}` |
| ZF, ZE, ZX | `view.zoom{fit}` | ZA | `view.zoom_all_to_fit` |
| ZS | `view.zoom{sheet_size}` | ZP, ZC | `view.zoom{previous}` |
| ZN | `view.zoom{next}` | WT / TW | `window.tile` / `window.tab_all` |
| KS | `ui.shortcuts.dialog` | FF | `browser.reveal` |
| 32 / 3O / 3W / 3F | `ui.nav.device_mode{2d|object|walk|fly}` | Ctrl+J | `ui.pane.toggle{agent}` |
| Ctrl+Shift+L | `ui.pane.toggle{lua_console}` | Ctrl+Shift+F11 | `ui.focus_mode.toggle` |
| F11 | `ui.fullscreen.toggle` | Ctrl+Alt+T | `ui.background_tasks` |

**Settings and collaboration**

| Keys | Command id | Keys | Command id |
|---|---|---|---|
| UN | `units.edit` | GL | `global.manage` |
| ER | `editrequest.manage` | RL, RW | `bimserver.get_latest` |
| CI | `bimserver.check_in` | | |

**Snap overrides** (next pick only): SO, SE, SM, SN, SW, SQ, SI, SC, SP, ST, SX, SR, SY, SZ, S2, SS (§1.23); coordination-model snap toggle LM and family shape-point snap SG.

**Rebar placement scope**: CD clear spacing, CF cover, LC close loop, LH two-way hooks, AF set orientation, FL flip set (overrides global FL in this scope), JP/JY/JZ bar justification (Ch. 4 defines the commands).

**Acceptance criteria.**
- [ ] Loading `shortcuts-default.toml` into the registry yields zero global-scope sequence conflicts.
- [ ] Every command id in the tables exists in the registry of a full (P2) build; P0/P1 builds silently skip bindings to absent commands.
- [ ] The Help › Keyboard Shortcut Reference page is generated from the live map and matches the tables above in a fresh profile.

---

## 1.27 Options dialog

### 1.27.1 Dialog structure
**ID:** UI-PREF-001 · **Priority:** P0 · **Ref:** research/01 §24 · `screenshots/ui/options_colors_tab_theme.png`

**Purpose.** Central place for application-wide, per-user preferences.

**Behaviour & rules.**
- Opened from File › Options, Command Search, or `ui.options.open(page?, setting?)`. Available with or without a model open.
- Left: page tree with a **search box** that filters pages and highlights matching settings. Right: the page.
- Settings are **per user** and global (not per model), stored in `settings.toml` (§1.29). Settings locked by the administrator defaults file show a lock icon and are read-only.
- OK applies and closes; Cancel discards; Apply applies without closing; each page has **Restore page defaults**.
- Settings that need a restart are marked and the user is told on OK.

**Commands.** `ui.options.open(page?, setting?)`, `ui.options.get(key) -> value`, `ui.options.set(key, value)`, `ui.options.list(prefix?) -> [{key, value, default, locked, requires_restart}]`, `ui.options.reset(prefix?)`.

**Acceptance criteria.**
- [ ] Typing "opacity" in the search box shows the Orientation Cube and Navigation Wheel pages with the matching settings highlighted.
- [ ] `ui.options.set("ui.tooltips.level", "minimal")` from Lua takes effect immediately.

### 1.27.2 Pages and settings
**ID:** UI-PREF-002 · **Priority:** P0 (pages marked otherwise) · **Ref:** research/01 §24.1–24.12

| Page | Settings | Pri |
|---|---|---|
| **General** | Save reminder interval (off, 15 min … 4 h); Sync/Check-in reminder interval (BIMserver mode); user name (§1.6.3); log file retention (count and age of session logs, the counterpart of journals); default view discipline; start-up behaviour (Show Home / Reopen last session / Open nothing); language; units display defaults for new models; autosave of recovery copies (interval, location) | P0 |
| **User Interface** | Disciplines and tools check boxes (§1.7.7); Keyboard shortcuts Customize…; Double-click actions Customize… (§1.21.5); Tooltip assistance (None / Minimal / Normal / High); Show recent files on Home and count; Tab-switching behaviour (after clearing selection or finishing: stay on Modify / return to previous, for model and family editor); Activate contextual tab on selection; View switching order (tab position / most recently used); Show tool options strip (P2); Ribbon state reset; Reset all UI; **Experimental**: Radial menu on middle click (off by default, §1.21.7); edge reveal dwell time in full-screen (§1.15.2) | P0 |
| **Appearance** | UI theme (Light / Dark / Follow system, default Follow system); Canvas scheme (Light / Dark); canvas colours: Background, Selection (+semi-transparent), Pre-selection, Alert, Calculating, Editing (dimmed), Reveal Hidden, mode border colours; icon theme (Standard / High contrast); UI font size (90–200 %); reduce motion (disables animated transitions) | P0 |
| **Graphics** | Allow navigation during redraw; simplify display during navigation; anti-aliasing (off / per view / all views); temporary dimension text size and background; draw visible elements only (occlusion culling); frame-rate target; transparency quality | P0 |
| **Hardware** | Detected GPU, driver and API (Vulkan / Metal / DirectX 12 / OpenGL fallback) with a compliance check list; Use hardware acceleration; GPU memory budget; renderer back-end override (requires restart) | P0 |
| **File Locations** | Model templates list (name, path; add, remove, reorder) used by New Model and Home; default folder for user files; family template folder; family library folders; point cloud root; content library locations; **default RXD carrier for new models** (embedded / sidecar, §1.4); Places (shortcuts in file dialogs) | P0 |
| **Rendering** | Additional appearance/texture paths (ordered); render engine and quality defaults; render output folder | P2 |
| **Spelling** | Ignore words in capitals / with digits / mixed case; find repeated words; main dictionary (language); additional dictionaries with Edit; Restore defaults | P1 |
| **Navigation** | Orientation Cube options (§1.19.2); Navigation Bar options; Navigation Wheel options (§1.19.4); mouse profile (standard / left-button); trackpad gestures; zoom direction invert; 3D mouse settings | P0 (wheel P2) |
| **Scripting & Add-ins** | Application scripts enabled; model-embedded scripts policy (ask / disable / enable, default disable); Lua sandbox trust levels; add-in folders; licence-check override for non-GPL-compatible add-ins (clearly labelled, off by default, TECH_STACK §4.2) | P0 |
| **AI & MCP** | MCP server enabled (default on, stdio only); streamable-HTTP transport (off by default; bind address, port, TLS, token management); default permission level per transport (read-only / model-edit / full-scripting); preview/confirm for destructive operations; Agent pane: model provider(s), endpoints, model names, API keys (stored in the OS keychain), local-only mode, tool approval policy, context attachments policy, transcript retention (§1.31) | P0 (agent P1) |
| **Remote Analysis** | Subscriptions to remote analysis services: name, secure service URL, API key and secret (the secret in the OS keychain), status, plan, usage and expiry, Test Connection, Refresh, Set as Default, Advanced (CA certificate, certificate pin) (DM-RPC-011). *Built (2026-10-20) as the Remote Analysis dialog (Analyze › Remote Analysis, Manage › Settings, File menu) until the Options dialog exists.* | P2 |
| **Server** | Saved BIMserver connections (URL, user, remember token in keychain); local working-copy cache location (with free-space check) and size limit; notification preferences (in-app / OS / none; poll interval fallback); auto Get Latest before Check In; default check-in comment template | P1 |
| **Accessibility** | Larger grips and hit targets; cursor size; high-contrast canvas palette; colour-vision-deficiency safe palette; keyboard focus ring thickness; screen-reader verbosity for canvas announcements; reduce motion; sequence timeout (§1.26.1) | P1 |

**Acceptance criteria.**
- [ ] Every setting listed is present, persisted and readable via `ui.options.get`.
- [ ] A setting locked by the administrator defaults file cannot be changed through the dialog, Lua or MCP.

---

## 1.28 Themes

**ID:** UI-THEME-001 · **Priority:** P0 · **Ref:** research/01 §25 · `screenshots/ui/dark_theme_full_ui.png`, `screenshots/ui/dark_theme_black_canvas_2023.png`, `screenshots/ui/options_colors_tab_theme.png`

**Purpose.** Provide light and dark appearances for the UI and, independently, for the canvas.

**Behaviour & rules.**
- Two **independent axes**:
  1. **UI theme**: Light, Dark, or Follow system (default). Covers the ribbon, QAT, panes, status bar, View Control Bar, view tabs, **and all dialogs** (no dialog is exempt).
  2. **Canvas scheme**: Light (light background, dark linework) or Dark (dark background, light linework). Toggled with View › Windows › Canvas Theme or CA.
- All colours come from **theme tokens** defined in theme files (`themes/<name>.toml`) shipped with Reviera; users may add custom theme files. Token groups: `ui.*` (window, panel, text, accent, focus ring, disabled), `ribbon.*` (tab, active tab, contextual accent, panel title), `canvas.*` (§1.17.2), `gizmo.*`, `status.*` (info, warning, error, success).
- **Platform realisation**: on Windows, wxWidgets dark mode support (`MSWEnableDarkMode` / `wxApp::SetAppearance`); on macOS, the app appearance is set to light, dark or system; on Linux/GTK, Reviera requests the GTK dark or light variant at start-up and follows `prefers-color-scheme` portal changes when "Follow system" is chosen. Custom-drawn controls (ribbon art provider, overlays, status bar) read tokens and repaint on theme change.
- Switching themes takes effect immediately without restart; icon bundles switch to the matching variant (light-on-dark or dark-on-light).
- A **High contrast** theme pair (light and dark) is shipped (§1.33); when the operating system's high-contrast mode is on and the theme is Follow system, Reviera uses it.
- Contextual tab accent, selection and pre-selection colours meet contrast requirements against both canvas schemes (§1.33.2).

**Commands.** `ui.theme.get() -> {ui, canvas, effective_ui}`, `ui.theme.set(ui: light|dark|system|<custom>)`, `ui.canvas_theme.set(light|dark)`, `ui.canvas_theme.toggle()`, `ui.theme.list() -> [name]`.

**Acceptance criteria.**
- [ ] With Follow system, changing the OS appearance switches Reviera's UI theme within 2 seconds without restart.
- [ ] Dark UI with Light canvas and Light UI with Dark canvas both work; every dialog follows the UI theme.
- [ ] A custom theme file overriding `ribbon.contextual` changes the contextual tab colour.

---

## 1.29 UI persistence

### 1.29.1 Per-user stores
**ID:** UI-PERSIST-001 · **Priority:** P0 · **Ref:** research/01 §26, §6.5, §4

**Behaviour & rules.**
- Per-user configuration lives in the platform configuration directory: Linux `$XDG_CONFIG_HOME/reviera/` (default `~/.config/reviera/`), Windows `%APPDATA%\Reviera\`, macOS `~/Library/Application Support/Reviera/`. Caches (thumbnails, server working copies, agent transcripts) live in the platform cache/data directory.
- Files (all human-readable TOML, versioned with a `schema_version` key and migrated forward automatically):

| File | Contents | Reset command |
|---|---|---|
| `settings.toml` | Every Options value (§1.27), selection toggles, snap settings, tool-option memory, split-button last-used members, Type Selector recents | `ui.options.reset()` |
| `ui-state.toml` | Ribbon layout (tab and panel order, hidden items, floating panels, pinned slide-outs, minimise state), QAT contents and position | `ui.ribbon.reset()`, `ui.qat.reset()` |
| `shortcuts.toml` | Keyboard bindings (differences from the default map) | `ui.shortcuts.reset()` |
| `layout.toml` | The current dock layout per **monitor configuration fingerprint**, the active bookmark and auto-switch rules. Bookmarks themselves are in `layouts/*.toml` (§1.29.4). Older named-workspaces | `ui.layout.reset()` |
| `recent.toml` | Home recent list and pins | `ui.recent.clear()` |
| `servers.toml` | Saved BIMserver connections (no secrets; tokens in the OS keychain) | — |
| `agent.toml` | Agent provider configuration (no secrets) | — |

- Each store can be reset independently from Options › User Interface or by deleting the file; a corrupt file is renamed `*.bad` and defaults are used, with a notice.
- Writes are atomic (write temp, rename).

**Acceptance criteria.**
- [ ] Deleting `ui-state.toml` resets ribbon and QAT but keeps shortcuts and options.
- [ ] A truncated `settings.toml` produces a notice and default settings; the damaged file is kept as `settings.toml.bad`.

### 1.29.2 Project-scoped UI data (RXD)
**ID:** UI-PERSIST-002 · **Priority:** P0 · **Ref:** research/01 §26 (per-model row); TECH_STACK §3.7.3

**Behaviour & rules.**
- Project-scoped UI data are **Reviera Extension Data documents** stored with the model through its RXD carrier (embedded in the IFC by default, or the optional sidecar; Ch. 2 and `02a_extension_data_format.md`):
  - **UI document**: starting view (Ch. 5 command), last session's open views with their tab/tile/split arrangement and per-view navigation state (when "Save view arrangement with model" is on, default on in Local mode), saved selection sets (§1.21.3).
  - **Browser document**: Project Browser expansion state and active tab, browser organisation scheme definitions and project-default active scheme (§1.13).
- In **BIMserver mode**, the open-view arrangement and browser expansion state are kept **per user in the local working-copy cache**, not checked in, so users do not overwrite each other's layouts; selection sets, organisation schemes and the starting view are checked in as model data.
- Changes that are purely presentational (expansion state, open views) mark the model as modified only for the RXD part and never create undo entries; they are saved with the next Save but never trigger a save prompt on their own (a preference can change this).
- Per-user preferences are never stored in RXD.
- Export clean IFC (§1.4) omits all of these.

**Acceptance criteria.**
- [ ] Opening a model shows the views open at its last save, arranged as they were, when the preference is on.
- [ ] Expanding browser nodes and closing without other edits does not prompt to save.
- [ ] In BIMserver mode, user A's open-view layout is not changed by user B's check-in.

### 1.29.3 Administrator defaults
**ID:** UI-PERSIST-003 · **Priority:** P1 · **Ref:** research/01 §26 (all-users template), §6.7 (locked disciplines)

**Behaviour & rules.**
- A system-wide defaults directory (Linux `/etc/reviera/`, Windows `%PROGRAMDATA%\Reviera\`, macOS `/Library/Application Support/Reviera/`) may contain `settings.toml`, `ui-state.toml`, `shortcuts.toml` and `layout.toml` used as defaults for new user profiles, and a `locked` list of setting keys users cannot change.
- Locked settings apply to Lua and MCP too.
- **Directory contents.**

| Entry | Use |
|---|---|
| `settings.toml`, `ui-state.toml`, `shortcuts.toml`, `layout.toml` | Seed values copied into a **new** profile on first launch; never re-applied to an existing profile except for locked keys |
| `locked.toml` | `keys = [ … ]`: setting keys (exact, or a prefix ending in `.*`) that users cannot change, for example `ui.disciplines`, `ai.mcp.*`; the value in force is the one in the defaults `settings.toml`, or the built-in default if absent |
| `layouts/`, `scenarios/` | Organisation layouts and scenarios, read-only for users (UI-LAYOUT-002, UI-SCEN-004) |
| `themes/` | Extra theme files offered to every user (§1.28) |

- **Override order**, highest first: locked administrator value → scenario overlay (UI-SCEN-001) → user value → administrator seed → built-in default. A scenario that lists a locked key is shown as "not applied" for that key.
- **Existing profiles.** At each launch Reviera re-reads `locked.toml`; locked keys take the administrator value for the session without overwriting the user's stored value, so unlocking later restores the user's own choice.
- **Visibility.** Locked settings show a lock icon and the note "Managed by administrator" in Options; `ui.options.list` reports `locked: true`; `ui.options.set` on a locked key returns the error `setting_locked` and changes nothing.
- **Errors.** A malformed defaults file is ignored with a notice to the user and a log entry naming the file and line; Reviera then starts with built-in defaults for the affected store and still applies any valid `locked.toml`.
- **Read-only location.** Reviera never writes to the defaults directory; it only reads it.

**Acceptance criteria.**
- [ ] A locked `ui.disciplines` setting is enforced for a new and an existing user profile.
- [ ] Removing the key from `locked.toml` restores the user's previously chosen value at the next launch.
- [ ] `ui.options.set("ui.disciplines", …)` from MCP returns `setting_locked` while the key is locked.
- [ ] A seed `shortcuts.toml` is applied to a new profile and not to an existing one.

### 1.29.4 Workspaces I: bookmarked layouts and personal layout preferences

> **Workspace model (overview).** The workspace is a **high-level, user-owned customisation** of how each person works, on top of the application's defaults and below any administrator locks. It has three building blocks:
> - **layout bookmarks** (§1.29.4): where things are on screen
> - **workflow scenarios** (§1.29.5): how the user works in a given process, as preferences, shortcuts and entry actions
> - **view bookmarks** (§1.29.5): where in the model they work
>
> All three are personal by default, portable between machines, switchable on demand or by context, and fully scriptable through Lua and MCP. Projects may only *recommend*; they never impose.
**Owner directive (2026-10-12).** The UI shall support *bookmarked* layouts and personal preferences for UI layout choices. This is a Reviera addition: the reference product keeps one UI state per user, with no named layouts.

#### UI-LAYOUT-001 — Saving and applying layout bookmarks
**ID:** UI-LAYOUT-001 · **Priority:** P1 · **Ref:** Owner directive 2026-10-12; research/01 §13 (docking), §26 (UI persistence)

**Purpose.** Lets users switch the workspace between tasks with one action, for example modeling on two monitors, then sheet documentation, then coordination review on a laptop, and get back to a known arrangement after experimenting.

**What a bookmark can hold.** Each part is optional. The user ticks the parts when saving, and parts left unticked are left alone when the bookmark is applied.

| Part | Contents | Default |
|------|----------|---------|
| **Dock layout** | Position, size, grouping and tab order of every pane (Properties, Project Browser, System Browser, History, Agent, Notifications, Server, Lua Console, add-in panes); docked, floating or auto-hidden; floating frames with their monitor | ✔ |
| **Visible panels** | Which panes and zones are shown or hidden (UI-DOCK-004) | ✔ |
| **Ribbon** | Minimise state, tab order, hidden tabs and panels, floating panels, pinned slide-outs, the active tab | ✔ |
| **Quick Access Toolbar** | Its contents and position (above or below the ribbon) | ✔ |
| **Status bar** | Which status-bar items are shown | ✔ |
| **Canvas chrome** | View-tab style, whether the Orientation Cube and Navigation Bar are shown, the selection toggles shown | ✔ |
| **Appearance** | Theme (light, dark or system), UI density (comfortable or compact), UI font scale | ☐ |
| **View arrangement** | The open views and their tab, tile and split arrangement. Views are matched by RXD view id in the current project (§1.29.2), so this part applies only when the same project is open. Missing views are skipped and listed in a notice. | ☐ |

**Workflow.**
1. Arrange the UI.
2. Choose **View › Windows › Layouts ▾ › Save current layout…**, or click the layout chip on the status bar and choose **Save…**.
3. Enter a name, tick the parts to include, and optionally pick an icon colour and a shortcut slot (1–9).
4. **Apply** it from any of:
   - the Layouts menu (split button: the main part re-applies the last layout, the arrow opens the list)
   - the status-bar layout chip
   - command search ("Layout: Documentation")
   - **Ctrl+Alt+1…9**
   - Lua or MCP
5. **Switch back** (Ctrl+Alt+0) returns to the arrangement in use just before the last apply, even if it was never saved.
6. **Update "<name>"** saves the current arrangement over the bookmark. Rename, duplicate, delete and set as startup layout are available in **Manage layouts…**.

**Behaviour & rules.**
- **Applying is atomic and flicker-free.** The frame is frozen while the AUI perspective, ribbon state and visibility are applied, then thawed. The target is under 300 ms, with no relayout flash.
- **Applying a layout never changes the model**, never creates undo entries and never marks the project as modified.
- **Interactive sessions survive.** Applying a layout during a sketch or tool session keeps the session and its contextual tab. The contextual tab stays visible even if the bookmark hides the ribbon tab it came from.
- **Add-in panes** that a bookmark mentions but that aren't installed are skipped silently, and their state is kept for later. Panes that appeared after the bookmark was saved keep their current state.
- **The status-bar chip** shows the active bookmark's name. It shows *"<name> (modified)"* once the user changes the arrangement afterwards, and the chip's menu then offers **Update** or **Save as new**.

**Data.** Per user, one TOML file per bookmark in `<config>/reviera/layouts/<slug>.toml`, holding:
- `schema_version`, `name`, `icon_color`, `slot` and `parts`
- per-part data: the AUI perspective string, a ribbon state table, a QAT list, visibility flags and appearance keys
- a `[displays.<fingerprint>]` table for each monitor setup it was saved on (UI-LAYOUT-003)

These files are never stored in RXD or in the model. The one exception is UI-LAYOUT-006.

**Save layout dialog.** Fields: *Name* (required, 1–64 characters, unique among personal layouts ignoring case; a clash offers *Replace* or *Choose another name*), the part check boxes of the table above with their defaults, *Icon colour* (eight theme colours), *Shortcut slot* (None, 1–9; choosing a slot already used shows which layout holds it and moves it on OK). Built-in and organisation names may be reused for personal layouts; the list then shows the badge to tell them apart.

**Manage layouts dialog** (`ui.layout.manage`). A list in three groups (Built-in, Organisation, Personal), each row showing icon colour, name, slot, the parts it holds, and the display setups it has variants for. The right side shows the pane-position diagram (UI-LAYOUT-002). Buttons:
- *Apply*, *Update from current* (personal only), *Rename* (personal only; same validation as Save), *Duplicate* (any; creates a personal copy), *Delete* (personal only; asks for confirmation and names any scenario or auto-switch rule that uses it, which then fall back to "no layout change"), *Export…* and *Import…* (UI-LAYOUT-005), *Set as startup* (UI-LAYOUT-002).
- An *Auto-switch* tab for the rules of UI-LAYOUT-004, and a *Display variants* list per layout with *Delete variant* (UI-LAYOUT-003).
- Changes are written immediately; the dialog has *Close*, not OK/Cancel.

**Commands.**
- `ui.layout.save(name, parts[], slot?, icon_color?) -> layout_id`
- `ui.layout.apply(layout_id | name | slot, parts?) -> report` (applies all saved parts, or only the listed subset; the report names skipped panes and parts overridden by administrator locks)
- `ui.layout.load(name) -> ()`: kept as a short form of `ui.layout.apply(name)` for scripts; fails with `not_found` when no layout has that name
- `ui.layout.update(layout_id) -> status`, `ui.layout.switch_back() -> status`
- `ui.layout.rename(layout_id, name) -> status`, `ui.layout.delete(layout_id) -> status` (both refuse built-in and organisation layouts with `read_only`)
- `ui.layout.list(group?: builtin|org|personal) -> [LayoutInfo]` (`LayoutInfo` = id, name, group, slot, icon_color, parts, display fingerprints, is_active, is_modified)
- `ui.layout.manage() -> ()` opens the Manage layouts dialog
- `ui.layout.reset() -> ()` restores the default arrangement of §1.2; it is not a bookmark and deletes none.

**Acceptance criteria.**
- [ ] Save "Modeling" (two monitors, Properties floating on monitor 2) and "Docs" (one window, Project Browser on the right, ribbon minimised to tabs). Ctrl+Alt+1 and Ctrl+Alt+2 switch between them in under 300 ms, with no visible relayout flash.
- [ ] Applying a bookmark saved without *Appearance* leaves the current theme unchanged.
- [ ] Applying a layout during floor sketch mode keeps sketch mode and its contextual tab.
- [ ] Switch back restores the unsaved arrangement that was in use before the last apply.
- [ ] Applying a layout doesn't mark the project as modified.

#### UI-LAYOUT-002 — Built-in, organisation and personal layouts
**ID:** UI-LAYOUT-002 · **Priority:** P1 · **Ref:** Owner directive 2026-10-12; §1.29.3

- **Built-in starter layouts** (read-only; the user can duplicate one to customise it):

  | Layout | Arrangement |
  |--------|-------------|
  | *Modeling* | the default layout (§1.2) |
  | *Documentation* | Project Browser enlarged with the Sheets tab active, Properties narrow, the ribbon opening on the Annotate tab |
  | *Coordination* | History, Notifications and Server panes docked on the right, the clash and BCF panes open |
  | *Compact* | for screens 1440 px wide or less: ribbon minimised to panel titles, palettes auto-hidden at the edges |
  | *Presentation* | minimal chrome: ribbon hidden, status bar reduced, canvas only |

- **Organisation layouts** are read from the administrator defaults directory (`layouts/` under §1.29.3). They are listed with an *Org* badge and are read-only for users. An administrator can mark one as the **startup layout for new profiles**.
- **Personal layouts** are the user's own. The **startup layout** preference sets what the UI uses at launch:

  | Option | Behaviour |
  |--------|-----------|
  | **Last used** (default) | The arrangement from the end of the previous session |
  | **A specific bookmark** | That bookmark, every launch |
  | **Ask at launch** | The user picks from a list on the Home screen |

- **Lists and locks.**
  - The **Manage layouts** dialog lists the built-in, organisation and personal layouts in three groups, with a preview of each: a diagram of pane positions drawn from the saved data, not a screenshot.
  - Settings the administrator has locked (§1.29.3) are applied whatever the bookmark says. The dialog shows which parts were overridden.
- **Ask at launch.** The Home screen shows a "Choose a layout" strip above Recent with one button per layout (built-in, organisation, personal), preselecting the last used; choosing nothing and opening a model uses the last used one.
- **Missing startup layout.** If the startup layout has been deleted or its organisation file removed, Reviera falls back to *Last used* and shows a notice.

**Commands.** `ui.layout.duplicate(layout_id, name) -> layout_id` (any group; the copy is personal), `ui.layout.set_startup(last_used | layout_id | ask) -> status`, `ui.layout.list(group?)` (UI-LAYOUT-001).

**Acceptance criteria.**
- [ ] A fresh profile lists the five built-in layouts. Deleting one isn't possible; duplicating one creates an editable personal copy.
- [ ] An organisation layout placed in the defaults directory appears with the *Org* badge. When set as the startup layout for new profiles, a new user's first launch uses it.

#### UI-LAYOUT-003 — Multi-monitor and display-aware layouts
**ID:** UI-LAYOUT-003 · **Priority:** P1 · **Ref:** research/01 §13 (multiple monitors)

- **Variants per display setup.** A bookmark keeps one variant per **monitor configuration fingerprint**: the count, arrangement, resolution and scale of the monitors.
  - Applying the bookmark uses the variant that matches the current displays exactly, if there is one.
  - Otherwise Reviera **adapts the nearest variant**:
    - Floating frames are mapped to the monitor in the same relative position.
    - Frames whose monitor no longer exists are docked into the right dock group, keeping their tab order.
    - Sizes are scaled by the DPI ratio and clamped to the work area.
  - A notice offers **Save a variant for this display setup**.
- **Display changes while running.** When the monitor setup changes during a session (a laptop docked or undocked, a monitor unplugged), Reviera keeps windows on screen at once. It then re-applies the active bookmark's matching variant if one exists, or offers to save one.
- **No window off screen.** After any apply, every frame must be at least 80 % visible on some monitor.
- **Fingerprint.** A hash of the sorted list of (monitor position relative to the primary, resolution, scale factor). Monitor names and serial numbers are not used, so swapping one monitor for an identical model matches.
- **Variant management.** *Save a variant for this display setup* stores the current arrangement as an extra variant of the active bookmark; Manage layouts lists the variants with *Delete variant*. A bookmark keeps at most 8 variants; saving a ninth replaces the least recently used one after a confirmation.

**Commands.** `ui.layout.save_display_variant(layout_id) -> status` (stores the current arrangement as the variant for the current fingerprint); `ui.monitors()` (UI-VIEWWIN-003) returns the data the fingerprint is built from.

**Acceptance criteria.**
- [ ] Apply a two-monitor bookmark with only the laptop screen connected: all panes are visible and floating panes are docked on the right. Reconnecting the second monitor restores the two-monitor variant automatically.

#### UI-LAYOUT-004 — Context-bound layouts (auto-switch)
**ID:** UI-LAYOUT-004 · **Priority:** P2 · **Ref:** Owner directive 2026-10-12

- **Rules.** Users can bind a bookmark to a **context** in *Manage layouts › Auto-switch*. Available contexts:
  - the active view type is a sheet, a schedule, a 3D view or a drafting view
  - the Family Editor is active
  - the project is in BIMserver mode
  - a given monitor setup (fingerprint) is connected
  - the main window is narrower than a set width
- **Order.** Rules are evaluated in user-set order, and the first match is applied when the context starts. When the context ends, the layout that was active before it is restored.
- **Manual changes win.** A layout the user applies by hand stays until the context changes again. A global **Auto-switch** toggle in the chip menu turns all rules off.
- **Partial bookmarks are encouraged.** A bookmark bound to a context should normally include only the parts that matter for that context, such as *Ribbon* and *Visible panels*, so switching feels light.
- **Auto-switch tab** (in Manage layouts). A table of rules with columns *Order*, *Context* (combo of the contexts above, with a width value or a display setup picker where needed), *Layout* (combo; built-in, organisation or personal) and *Enabled*. Buttons: *Add*, *Remove*, *Move up*, *Move down*. Validation: a rule needs a context and a layout; two enabled rules with the same context are allowed but the later one is greyed with "Never reached". Rules are stored in `layout.toml`.
- **Debounce.** A context must hold for 300 ms before its rule applies, so quickly tabbing through views does not cause a chain of switches.

**Commands.** `ui.layout.rules.list() -> [rule]` and `ui.layout.rules.set(rules[]) -> status` where `rule = {context, layout_id, order, enabled}`; `ui.layout.auto_switch(enabled: bool) -> status` sets the global toggle.

**Acceptance criteria.**
- [ ] Bind "Docs" to *sheet view active*. Opening a sheet applies "Docs"; activating a plan view returns to the previous layout. With Auto-switch off, nothing changes.

#### UI-LAYOUT-005 — Personal preference profile: export, import and sync
**ID:** UI-LAYOUT-005 · **Priority:** P1 · **Ref:** Owner directive 2026-10-12; §1.29.1

- **Export profile…** writes a `.reviera-profile` file: a ZIP of the user's chosen stores, from `settings.toml`, `ui-state.toml`, `shortcuts.toml`, `layouts/*.toml` and `layout.toml`. Secrets are never included (`servers.toml` is written without tokens), and machine-specific paths are made relative or dropped.
- **Import profile…** previews what will change, then lets the user **merge** (incoming layouts are added and name clashes renamed) or **replace**.
- **Profile sync folder** (optional preference): the user picks a folder, for example one synced by Nextcloud, Syncthing or a network share. Personal layouts and selected stores are mirrored to it, and changes are picked up at launch and when the window regains focus. Conflicts keep the newer file and save the older one as `<name>.conflict.toml`.
- **Share a single layout** with *Export layout…* and *Import layout…* (one `.toml` file each), for example to give a colleague a documentation layout.
- **Import preview dialog.** Lists each incoming store and layout with its action (*Add*, *Replace*, *Rename to "<name> (2)"*, *Skip: newer schema*), and a count of setting keys that would change with a *Show keys* expander. The user can untick individual items. *Import* applies them in one step; nothing is written if the user cancels. Files with a newer `schema_version` than this build understands are skipped and listed.
- **Clash policy for single layouts** (`on_clash`): `rename` (default; adds " (2)", " (3)", …), `replace`, or `skip`.
- **Sync folder validation.** The folder must exist and be writable; Reviera writes a small `reviera-sync.toml` marker there. If the folder becomes unavailable (network share offline), syncing pauses with a status-bar notice and resumes when it returns.

**Commands.**
- `ui.profile.export(path, stores[]) -> status`, `ui.profile.import(path, mode: merge|replace, preview?) -> ChangeSummary` (with `preview = true` nothing is written; the summary lists what would change)
- `ui.layout.export(layout_id, path) -> status`, `ui.layout.import(path, on_clash: rename|replace|skip) -> layout_id`
- `ui.profile.set_sync_folder(path | null) -> status` (null turns syncing off; the folder's contents are left in place)

**Acceptance criteria.**
- [ ] Export a profile on machine A and import it on machine B with *merge*: B's own layouts are kept and A's layouts are added. A clash on "Docs" becomes "Docs (2)".
- [ ] An exported profile contains no BIMserver token or agent API key; a search of the archive finds no secret values.

#### UI-LAYOUT-006 — Recommended project layout (team standard)
**ID:** UI-LAYOUT-006 · **Priority:** P2 · **Ref:** §1.29.2

- **Recommending.** A project can carry one or more **recommended layouts**, stored in the RXD document `ui/state` under `recommended_layouts`. Only the parts *Dock layout*, *Visible panels*, *Ribbon* and *View arrangement* can be recommended. Personal *Appearance* is never part of a project.
- **On open.** When a user opens the project, the layout chip shows **Project layout available**. Applying it is always the user's choice; it never applies automatically. The user can also save it as a personal bookmark.
- **Who can set it.** Recommended layouts are project data. In BIMserver mode, changing them requires edit rights and goes up with the next check-in.
- **Recommending workflow.** In Manage layouts, a personal or organisation layout has *Recommend for this project…*. The dialog shows which parts will be copied (only the four allowed parts; others are listed as "not shared") and asks for an optional note shown to other users. The layout is **copied** into the RXD document, so later edits to the personal original do not change the project copy; *Update recommendation* copies it again.
- **Data.** `ui/state` › `recommended_layouts[]`: `{id, name, note, parts, perspective, ribbon, visible, view_arrangement, author, created}`. The view arrangement uses RXD view ids, so it resolves in every copy of the project. Panes of add-ins that a user does not have are skipped as in UI-LAYOUT-001.
- **Undo and modified state.** Recommending or removing a recommendation is an ordinary undoable edit of project data and marks the project modified. Applying a recommended layout is a personal action: not undoable, no modified flag.
- **Dismissal.** The chip's "Project layout available" hint can be dismissed per project; it reappears only when the recommendation changes.
- **Limits.** At most 5 recommended layouts per project; the first is the one offered by the chip, the rest are listed in its menu.
- **Clean IFC export** (§1.4) drops these records with the rest of RXD.

**Commands.** `ui.layout.recommend(layout_id, remove?: bool) -> status` (adds or updates the project copy; `remove = true` deletes it; undoable), `ui.layout.list_recommended() -> [LayoutInfo]` (the active project's recommendations, with `group = project`); applying uses `ui.layout.apply` with the returned id.

**Acceptance criteria.**
- [ ] User A sets "Coordination review" as recommended and checks in. When user B opens the project, the chip offers it; applying it doesn't change B's theme or personal bookmarks.
- [ ] Editing A's personal "Coordination review" after recommending it does not change the project copy until *Update recommendation*.
- [ ] Undo after *Recommend for this project* removes the recommendation.

### 1.29.5 Workspaces II: workflow scenarios and view bookmarks
**Owner directive (2026-10-12).** *"Users can adjust the workspace to their liking, save it as a bookmark and switch on demand. Users define a preference for each work process and scenario, matching their optimal workflow and preferences."*

This builds on §1.29.4. A **layout bookmark** stores *where things are*. A **workflow scenario** stores *how the user wants to work* in a particular process: a layout plus preference, shortcut and view-bookmark overlays, switched on demand or automatically.

#### UI-SCEN-001 — Workflow scenarios
**ID:** UI-SCEN-001 · **Priority:** P1 · **Ref:** Owner directive 2026-10-12; research/01 §24 (Options), §20 (snaps), §18 (selection)

**Purpose.** Different work processes need different setups. Modeling, detailing, sheet documentation, coordination review, site work, MEP routing and client presentation each have their own best combination. Users can capture each one once and recall it with one action, instead of changing a dozen settings every time.

**What a scenario holds.** Every part except *Name* is optional.

| Part | Contents |
|------|----------|
| **Name, icon colour, description, slot** | Slot 1–9 maps to **Ctrl+Shift+1…9**. Layout bookmarks keep Ctrl+Alt. |
| **Layout** | A reference to a layout bookmark (§1.29.4), or no layout change |
| **Preference overlay** | Any subset of user preferences (§1.27, §1.29.1). It changes **only the keys it lists**. Examples: snap types and distances; object-snap priorities; selection toggles (links, underlay, pinned, by face, drag); temporary-dimension reference (wall centre or faces); tool option defaults (wall location line, auto-join, chain); view navigation (zoom behaviour, orbit centre, mouse mapping); canvas graphics (anti-aliasing, shadows for performance, background); tooltip level; keytips; autosave and sync reminders; units *display* precision for temporary dimensions; and the default discipline filter for the ribbon. |
| **Shortcut overlay** | Bindings that apply only while the scenario is active. A detailing scenario might bind `DL` to detail line and `RG` to filled region, while modeling keeps its own uses. Conflicts are checked against the base map when the scenario is saved. |
| **View bookmarks** | A set of view bookmarks (UI-SCEN-002) that can be opened together when the scenario is applied, for example the plan, its 3D view and a section, tiled |
| **Entry actions** (P2) | Optional: set the active ribbon tab, open a pane (History, Agent), apply a temporary view property, or run a **personal** Lua macro from the user's script folder. Macros run at the user's chosen trust level; project-embedded scripts never run here. |

**Overlay rules.**
- **Base and overlay.** The user's normal preferences are the **base**. Applying a scenario lays its overlays on top of the base, and switching to another scenario first removes the current overlay. The base is never overwritten. *"No scenario"* means base only.
- **Changing a setting while a scenario is active.** Options shows the setting's source ("from scenario *Detailing*" or "personal default"). Changing it asks once, *for this scenario* or *for everything* (the base), and remembers the answer as a preference.
- **Administrator locks** (§1.29.3) always win. The scenario editor marks locked keys as not applied.
- Applying a scenario never touches the model or undo, never marks the project as modified, and keeps an interactive session that is already running (as in UI-LAYOUT-001).

**Switching on demand.**
- The status bar has a single **Workspace chip**, for example *"Detailing · Docs layout"*. Clicking it lists scenarios, then layouts, then *Switch back* (Ctrl+Shift+0), *Capture…*, *Manage…* and the *Auto-switch* toggle.
- Scenarios are also available from **View › Windows › Workspaces ▾**, from command search ("Scenario: Detailing"), with Ctrl+Shift+1…9, and from Lua or MCP.
- **Capture current setup as scenario…** creates a scenario from where the user is now. It picks up the current layout, every preference that differs from the base, the current shortcut differences, and optionally the open views as view bookmarks. The user reviews the differences before saving.

**Built-in starter scenarios.** These are read-only; the user can duplicate one to customise it.

| Scenario | Layout | Example overlay |
|----------|--------|-----------------|
| Modeling | Modeling | Default snaps, auto-join on, select-pinned off |
| Detailing | Documentation | Snaps to endpoints and intersections only, selecting links and underlay off, detail-line shortcuts, Annotate tab active |
| Sheet documentation | Documentation | Sheets tab active in the Project Browser, select-by-face off, temporary dimensions set to faces |
| Coordination review | Coordination | Select links on, select pinned off, History and Notifications panes open, worksharing display on |
| Site | Modeling | Larger snap distances, Massing & Site tab active |
| MEP routing | Modeling | Systems tab active, System Browser open, connector snaps first |
| Presentation | Presentation | Tooltips off, temporary dimensions hidden, smooth navigation, high-quality viewport graphics |

**Capture dialog.** *Capture current setup as scenario…* opens a review dialog with a *Name* field (same validation as layout names), a choice of layout (*use the active layout*, *save the current arrangement as a new layout*, or *no layout change*), and three lists with a check box per row: preferences that differ from the base (key, base value, current value), shortcut differences (keys, base command, current command), and open views (to become view bookmarks, off by default). *Save* creates the scenario with the ticked rows only.

**Commands.**
- `ui.scenario.list(group?: builtin|org|personal|project) -> [ScenarioInfo]` (id, name, group, slot, icon colour, layout id, counts of overlay keys, shortcuts and bookmarks, is_active)
- `ui.scenario.apply(scenario_id | name | slot) -> report` (lists keys applied, keys skipped because locked, and missing view bookmarks)
- `ui.scenario.switch_back() -> status` (returns to the scenario, or base, active before the last apply)
- `ui.scenario.capture(name, include: [layout, prefs, shortcuts, open_views]) -> ScenarioDraft` (returns the differences for review; nothing is saved)
- `ui.scenario.save(ScenarioDef) -> scenario_id` (saves a reviewed draft or a definition written by a script)
- `ui.scenario.update(scenario_id, from_current?: bool) -> status` (with `from_current`, replaces the overlay with the current differences from the base)
- `ui.scenario.set_pref(key, value, target: scenario|base) -> status` (the scripted equivalent of the "for this scenario / for everything" question; `scenario` needs an active scenario).

**Acceptance criteria.**
- [ ] Create "Detailing" with select-links off and `DL` → detail line. Applying it changes only those two things; switching to "Modeling" restores select-links and the original `DL` binding.
- [ ] With "Detailing" active, changing the snap distance and choosing *for this scenario* changes only "Detailing". Choosing *for everything* changes the base, and "Detailing" still overrides it if it lists that key.
- [ ] *Capture current setup* lists exactly the settings that differ from the base, before it saves.
- [ ] A scenario that sets a key the administrator has locked shows the key as not applied, and the locked value stays in force.

#### UI-SCEN-002 — View bookmarks
**ID:** UI-SCEN-002 · **Priority:** P1 · **Ref:** research/01 §16 (navigation, saved home view), research/04 §1 (views); Chapter 6 (BCF)

**Purpose.** Saved **viewpoints inside views**: a zoom region in a plan, or a camera and section box in a 3D view. The user can jump back to the exact place in the model they work on repeatedly.

**What a view bookmark holds.**
- the view id
- in 2D views, the zoom and pan region
- in 3D views, the camera, projection and section box
- optional extras: temporary hide and isolate state, a temporary visual style override, the active workset or design option filter for display, and a selection set reference

**Workflow.**
1. Choose **Bookmark this view position** (Ctrl+Shift+B, or the right-click menu on a view tab) and give it a name. (Ctrl+D is reserved for Reviera Home, §1.26.3.)
2. Bookmarks appear in a **Bookmarks** tab of the Project Browser, grouped by view, and in the Workspace chip's menu.
3. Double-click a bookmark (or use command search) to open its view and restore the position.

**Scope and storage.**
- **Personal (default).** Stored per user and per project in the user data directory, keyed by the project's `DataId`. Personal bookmarks are never stored in the model, so they don't clutter other users' projects.
- **Shared.** *Share with project* moves a bookmark into the RXD document `views/bookmarks`, which travels with the model and through BIMserver.
- **BCF.** A bookmark can be exported as a **BCF viewpoint** (camera, section box, visibility). A BCF viewpoint can be imported as a bookmark (Chapter 6, BCF).
- **Missing views.** If a bookmark's view no longer exists, the bookmark is shown greyed out, with *Re-target to view…* and *Delete*.
- **Save dialog.** *Name* (required, 1–80 characters, unique per view; default "<view name> 1", "… 2"), check boxes for the optional extras (off by default), and *Personal* / *Shared with project* (shared needs edit rights in BIMserver mode). Saving is presentation only for personal bookmarks; sharing is an undoable project edit.
- **Re-target.** *Re-target to view…* offers views of a compatible kind (2D region bookmarks to 2D views, camera bookmarks to 3D views). The stored region or camera is reused as is; if it lies outside the new view's extents, Reviera zooms to fit and reports it.
- **Applying** opens or activates the view (`view.activate`), restores the region or camera without adding an undo entry, and adds one entry to the view's navigation history so Previous Pan/Zoom returns to where the user was. Optional extras are applied as temporary view state (§1.18) and never change the view's saved properties.
- **BCF mapping.** Export writes one BCF viewpoint with the camera (perspective or orthogonal), the section box as clipping planes, and the visibility of the temporary hide/isolate state as component visibility by IFC GlobalId. Import creates a personal 3D bookmark on the default 3D view, or on a chosen 3D view, and reports elements in the viewpoint that are not in the model. BCF version handling is owned by Ch. 6.

**Commands.**
- `view.bookmark.save(name, view_id?, include: [camera, section_box, temp_hide, visual_style, selection_set]) -> bookmark_id` (default view: active)
- `view.bookmark.apply(bookmark_id) -> status`
- `view.bookmark.list(view_id?, scope: personal|shared|all) -> [ViewBookmark]`
- `view.bookmark.delete(bookmark_id) -> status` (deleting a shared bookmark is an undoable project edit)
- `view.bookmark.share(bookmark_id) -> status` (moves a personal bookmark into `views/bookmarks`; undoable)
- `view.bookmark.retarget(bookmark_id, view_id) -> status`
- `view.bookmark.export_bcf(bookmark_id, path) -> status`, `view.bookmark.import_bcf(path) -> bookmark_id`.

**Acceptance criteria.**
- [ ] Bookmark a zoomed region of "Level 2" and a 3D camera with a section box. After closing and reopening the project, both bookmarks restore exactly.
- [ ] A personal bookmark doesn't appear for another user of the same BIMserver project. A shared bookmark appears after their next sync.
- [ ] Exporting a 3D bookmark as BCF and re-importing it reproduces the camera and section box.

#### UI-SCEN-003 — Scenario triggers and precedence
**ID:** UI-SCEN-003 · **Priority:** P2 (manual switching is P1, under UI-SCEN-001) · **Ref:** UI-LAYOUT-004

- **What can trigger a scenario.**
  - The auto-switch rules from UI-LAYOUT-004 can target a **scenario** as well as a layout. The contexts are the same: view type, Family Editor, BIMserver mode, monitor setup and window width. Two more are added: **the active ribbon tab or tool family** (for example any Systems tool), and **the project**, meaning a personal default scenario per project.
  - Scenarios can also be **started by script**, through Lua or MCP (`ui.scenario.apply`), so a user's agent can, for example, prepare a review session.
- **Precedence**, highest first:

  | Rank | Source | Example |
  |------|--------|---------|
  | 1 | Applied by hand | Picked from the chip, a shortcut or command search |
  | 2 | Context rule, in rule order | Sheet view active |
  | 3 | The user's per-project default | — |
  | 4 | The project's recommended scenario | Only if the user has opted in once per project |
  | 5 | The startup choice (UI-LAYOUT-002) | — |

  When a context ends, the scenario that was active before it is restored.
- **No ping-pong.** A rule can't re-trigger within 2 s of the user overriding it by hand. An optional "notify on auto-switch" toast offers *Undo switch*.
- **Per-project default.** Stored per user and per project (keyed by the project's `DataId`) in the user data directory, never in the model. It applies when the project's first view opens and is restored to the previous scenario when the project's last window closes.
- **Project recommendation opt-in.** The first time a project with a recommended scenario is opened, the Workspace chip asks once: *Use the project's scenario* / *Not now* / *Never for this project*. The answer is stored with the per-project default.

**Commands.** `ui.scenario.set_project_default(scenario_id | null) -> status` (for the active project; null clears it); context rules use `ui.layout.rules.set` with a rule target of either a layout id or a scenario id (UI-LAYOUT-004); `ui.scenario.apply` (UI-SCEN-001) starts a scenario from a script.

**Acceptance criteria.**
- [ ] Rule *Systems tool active → "MEP routing"*. Starting a duct tool applies the scenario. Finishing the tool and activating an architectural tool restores the previous scenario. A manual switch during the duct session isn't overridden.

#### UI-SCEN-004 — Scenario editor, storage and portability
**ID:** UI-SCEN-004 · **Priority:** P1 · **Ref:** UI-LAYOUT-005

- **The editor.** **Manage workspaces…** lists scenarios (built-in, organisation, personal) and layouts. For each scenario it has these tabs:
  - **General**: name, slot, icon, description
  - **Layout**
  - **Preferences**: only the overridden keys are listed. *Add setting…* searches every Options key. *Remove* returns a key to the base.
  - **Shortcuts**: the overlay bindings, with conflicts highlighted
  - **View bookmarks**
  - **Entry actions**
  - **Triggers**

  It also has **Compare with base**, showing the effective differences.
- **Storage.** `<config>/reviera/scenarios/<slug>.toml`, which references layouts by id and view bookmarks by id. Organisation scenarios come from the defaults directory and are read-only, with an *Org* badge.
- **Portability.** Scenarios are part of `ui.profile.export` and `ui.profile.import` and of the profile sync folder (UI-LAYOUT-005). They can also be shared one at a time; a scenario's referenced layout is bundled with it on export.
- **Team-recommended scenarios.** A project can recommend a scenario in the same way as a layout (UI-LAYOUT-006). Its preference overlay is limited to non-personal keys, such as snaps and selection toggles. Appearance, mouse mapping and shortcuts are never pushed by a project.
- **Editor rules.** Built-in and organisation scenarios open read-only, with *Duplicate to edit*. Edits to a personal scenario are saved when the editor closes or another scenario is selected; *Revert* discards edits since the editor opened. A scenario that is active while edited is re-applied on save. Validation: a unique name (1–64 characters), a slot not used by another scenario (a clash offers to move it), shortcut overlay conflicts shown in red and allowed only after confirmation, preference values checked against each key's type and range.
- **Delete.** Deleting a personal scenario that is active returns to base first; rules and per-project defaults that name it are listed in the confirmation and are removed with it.
- **Import clash policy** (`on_clash`): `rename` (default), `replace`, `skip`. A bundled layout that clashes follows the same policy.
- **Recommend.** The stored copy goes to RXD document `ui/state` › `recommended_scenarios[]`; keys outside the allowed set are dropped with a list shown before saving.

**Commands.**
- `ui.scenario.manage(scenario_id?) -> ()` opens Manage workspaces at the given scenario
- `ui.scenario.diff(scenario_id) -> [PrefDiff]` (key, base value, scenario value, locked; the data behind *Compare with base*)
- `ui.scenario.duplicate(scenario_id, name) -> scenario_id`, `ui.scenario.delete(scenario_id) -> status`
- `ui.scenario.save(ScenarioDef) -> scenario_id` (also used by the editor)
- `ui.scenario.export(scenario_id, path) -> status`, `ui.scenario.import(path, on_clash) -> scenario_id`
- `ui.scenario.recommend(scenario_id, remove?: bool) -> status` (undoable project edit).

**Acceptance criteria.**
- [ ] Export a scenario that references a personal layout. Importing it on another machine brings in both, and the scenario's layout reference resolves.
- [ ] *Compare with base* lists the same keys that applying the scenario changes.

---

## 1.30 UI state for Lua and MCP

### 1.30.1 Exposure principle
**ID:** UI-API-001 · **Priority:** P0 · **Ref:** TECH_STACK §2, §3.5, §3.6; research/01 §27 item 2

**Purpose.** Let scripts and AI agents see and drive what the user sees: active view, selection, open windows, tool state.

**Behaviour & rules.**
- Every UI command in this chapter is a registry command and therefore available as `reviera.<id>` in Lua and as an MCP tool, unless marked otherwise in the catalog (§1.34).
- UI commands that only change presentation (view activation, zoom, selection, pane visibility) are **not undoable** and do not open model transactions; they run on the UI thread via the command queue and return once applied.
- MCP permission levels (TECH_STACK §3.6): UI **read** commands (`ui.get_*`, `selection.get`, `window.list`, `ui.screenshot`) require read-only; UI **write** commands that change presentation only (`view.activate`, `selection.set`, `view.zoom*`) are allowed at read-only level by default but can be restricted in Options › AI & MCP ("Agents may change my view and selection": ask / allow / deny); commands that change persistent UI settings (`ui.options.set`, `ui.shortcuts.*`, `ui.ribbon.*`) require full-scripting level.

**Acceptance criteria.**
- [ ] An MCP client at read-only level can call `ui.get_active_view` and `selection.get` but receives a permission error from `ui.options.set`.

### 1.30.2 State queries
**ID:** UI-API-002 · **Priority:** P0

**Commands.**
- `ui.get_state() -> UiState` with fields: `mode` (local|bimserver), `documents[]` (id, path or server URI, modified flags), `active_document`, `active_view` (id, name, kind), `windows[]` (as `window.list`), `selection` (ids, counts by category), `tool` (command id, step, options) or null, `edit_mode` stack, `ribbon.active_tab`, `theme`, `home_visible`, `server` (connection state, project, revision, pending changes).
- `ui.get_active_view() -> {view_id, name, kind, document_id, window_id}`.
- `ui.get_active_document() -> {document_id, path|uri, schema, modified}`.
- `tool.get_active() -> {command_id, step, prompt, options} | null`.
- `ui.screenshot(view_id?, width?, height?, overlays: bool = false) -> PNG` renders a view (offscreen if not visible) for agents that need to "see" the model; size is capped (default 2048 px longest side).

**Behaviour & rules.**
- **Consistency.** Every query answers from the latest published context snapshot (UI-ARCH-003), never from a half-applied state; each result carries the snapshot's `version` so a client can tell whether two answers belong together.
- **Size limits.** `selection` in `ui.get_state()` returns at most 1 000 ids plus `truncated: true` and the full count; clients use `selection.get()` for the complete list. `windows[]` and `documents[]` are not truncated.
- **No model access.** These queries never open a transaction and never wait for one; during a long command they answer immediately with the previous snapshot and `busy: true`.
- **Screenshot rules.** Width and height default to the view window size (or 1280 × 960 when the view is not open) and are clamped to the cap; aspect ratio is kept when only one is given. With `overlays = false` no selection, pre-highlight, grips or mode borders are drawn. Rendering a closed view does not open it, does not add it to the recent list and releases its GPU resources afterwards. Views that are not graphical (schedules) return an error `not_graphical`. A screenshot respects temporary hide/isolate of the view's window if it is open.
- **Errors.** Unknown ids return `not_found`; queries never return partial objects.
- **Permissions.** All of these are read commands, allowed at the read-only MCP level (UI-API-001).

**Acceptance criteria.**
- [ ] `ui.get_state()` reflects a selection made by the user within one second.
- [ ] `ui.screenshot` of a non-active view returns an image without changing the active view.
- [ ] During a 10-second regeneration, `ui.get_state()` returns within 50 ms with `busy: true`.
- [ ] With 5 000 elements selected, `ui.get_state()` reports a count of 5 000, 1 000 ids and `truncated: true`.
- [ ] `ui.screenshot` of a schedule returns `not_graphical`.

### 1.30.3 MCP resources and change notifications
**ID:** UI-API-003 · **Priority:** P0

**Behaviour & rules.**
- MCP resources: `reviera://ui/state`, `reviera://ui/active-view`, `reviera://ui/selection`, `reviera://ui/windows`, `reviera://ui/tool`, `reviera://view/{id}/image` (screenshot). Clients may subscribe; Reviera sends resource-updated notifications on change, coalesced to at most 10 per second per resource.
- Lua scripts subscribe with `reviera.on("selection_changed" | "active_view_changed" | "tool_changed" | "document_opened" | "document_closed" | "theme_changed", fn)`; handlers run on the scripting thread and must not block (long handlers are cut off with a warning per the scripting chapter).
- **Resource contents.** Each resource returns JSON of the same shape as the matching query: `ui/state` = `ui.get_state()`, `ui/active-view` = `ui.get_active_view()`, `ui/selection` = `selection.get()`, `ui/windows` = `window.list()`, `ui/tool` = `tool.get_active()`. `view/{id}/image` returns a PNG made with `ui.screenshot(id)` at default size and is regenerated only when read, never pushed.
- **Coalescing.** Updates within a 100 ms window are merged into one notification that carries the latest snapshot version; a client that reads after the notification always gets the newest state, never an intermediate one.
- **Event payloads (Lua).** `selection_changed(ids, by_category)`, `active_view_changed(view_id, previous_view_id)`, `tool_changed(command_id | nil, step)`, `document_opened(document_id)`, `document_closed(document_id)`, `theme_changed(ui, canvas)`. Events are delivered in the order they happened; a handler that raises an error is logged and kept subscribed; after 3 consecutive time-outs the handler is unsubscribed with a notice.
- **Unsubscribing.** `reviera.on` returns a handle with `:off()`; MCP clients unsubscribe per resource. Subscriptions end when the session or script ends.
- **Permissions.** Subscriptions are read access; a client at any MCP level may subscribe. The "Agents may change my view and selection" setting (UI-API-001) restricts only writes; it never blocks reading or subscribing.

**Acceptance criteria.**
- [ ] A subscribed MCP client receives an update for `reviera://ui/selection` after the user clicks an element.
- [ ] A crossing selection that changes the selection 40 times in one second produces at most 10 notifications, and the last one matches the final selection.
- [ ] A Lua handler for `active_view_changed` receives the new and previous view ids.

### 1.30.4 Driving the UI
**ID:** UI-API-004 · **Priority:** P0

**Commands and rules.**
- `view.activate(view_id, opts?)`, `selection.set(ids[], mode?: replace|add|remove (default replace)) -> ()`, `selection.highlight(ids, duration_ms?)` (temporary pre-highlight to draw the user's attention; no selection change), `view.zoom_to_elements(view_id?, ids)`, `ui.pane.show(id)`, `ui.ribbon.activate_tab(id)`, `ui.notify(message, level: info|warning|error, actions?)`.
- `tool.start(command_id, type_id?, preset_args?) -> session_id` starts an interactive tool session for the user to complete (for example an agent prepares a wall tool with a chosen type); `tool.cancel()`, `tool.finish() -> ()`.
- Agents and scripts should prefer non-interactive commands with explicit arguments; `tool.start` exists to hand over to the user.
- **Highlight.** `selection.highlight` draws the elements in `canvas.preselection` with a pulsing outline (a steady outline when Reduce motion is on) for `duration_ms` (default 2 000, range 200–30 000). A new call replaces the previous highlight from the same session. Elements not visible in any open view are reported back as `not_visible` ids.
- **Notifications.** `ui.notify` shows an in-app toast and adds the entry to the Notifications pane. `actions` is a list of up to 3 `{label, command_id, args}`; clicking one runs that command with the caller's permissions. Toasts from scripts and agents carry the source name ("From agent: planner"). More than 5 notifications per 10 s from one session are collapsed into one "n more messages" entry.
- **Tool hand-over.** `tool.start` fails with `busy` when the user is already in a tool session or edit mode (it never interrupts the user). The started session belongs to the user; the caller is told when it ends through the `tool_changed` event (UI-API-003). `tool.cancel` and `tool.finish` act only on a session the same caller started, unless the caller has full-scripting permission.
- **Presentation-only rule.** All commands here except `tool.finish` (which submits the tool's commands) are presentation only: not undoable, no modified flag; they follow the "Agents may change my view and selection" setting (UI-API-001) and the concurrency rules of UI-API-006.
- **Errors.** Unknown ids return `not_found`; hidden or unknown ribbon tabs and panes return `not_available`; a denied presentation change returns `permission_denied` and shows nothing to the user.

**Acceptance criteria.**
- [ ] `selection.highlight` flashes the given elements in all visible views without changing `selection.get() -> {ids[], by_category}`.
- [ ] `ui.notify` with an action "Open schedule" shows a toast whose button opens that schedule.
- [ ] `tool.start` while the user is sketching returns `busy` and the sketch continues undisturbed.
- [ ] With "Agents may change my view and selection" set to *deny*, `view.activate` from MCP returns `permission_denied` and the active view does not change.

### 1.30.5 Interactive picks for scripts
**ID:** UI-API-005 · **Priority:** P1

**Behaviour & rules.**
- `ui.pick.point(prompt, view_id?, snap?: bool) -> Point3 | cancelled`, `ui.pick.elements(prompt, filter?, multiple?: bool) -> [id] | cancelled`, `ui.pick.box(prompt) -> Rect`, `ui.prompt.text(title, label, default?) -> string | cancelled`, `ui.prompt.choice(title, options[]) -> index | cancelled`.
- Picks show the prompt in the status bar with the requester's name ("Script 'Renumber' asks: pick the first door"); Esc cancels.
- Over MCP these require an interactive user; they time out (default 120 s) and are refused when Reviera runs headless.
- **Pick sessions.** Each call starts a short interactive session with its own contextual tab "Pick for <requester>" holding Finish (for multiple picks), Cancel and the prompt. Only one pick request is active at a time; further requests queue in arrival order and the status bar shows "n pick requests waiting".
- **Filters.** `filter` accepts category (IFC class or Reviera category key), type id, view-visibility, `editable_only` and a list of allowed ids. Elements that fail the filter cannot be pre-highlighted; the cursor shows a "not allowed" badge over them.
- **Point picks** use normal snapping unless `snap = false`; coordinate entry (§1.33.1) works. The returned point is in project coordinates (Ch. 2) and in internal units.
- **Box picks** follow window/crossing by drag direction and return the rectangle in view coordinates with the view id.
- **Prompts.** `ui.prompt.text` and `ui.prompt.choice` show small modal dialogs titled with the requester's name; text input is limited to 4 096 characters; choice shows up to 50 options with search when more than 10.
- **Interruptions.** Closing the view, starting another tool or switching documents cancels the pick and returns `cancelled`. A timed-out request returns `timeout`.
- **No hidden picks.** A script cannot pick on behalf of the user without the prompt being shown; picks are never satisfied by synthetic input.

**Commands.** `ui.pick.point`, `ui.pick.elements`, `ui.pick.box`, `ui.prompt.text`, `ui.prompt.choice` as listed above.

**Acceptance criteria.**
- [ ] A Lua script calling `ui.pick.elements("Pick doors", {category="IfcDoor"}, true)` lets the user pick only doors and returns their ids on Finish.
- [ ] Two scripts asking for picks at the same time are served one after the other, each with its own prompt.
- [ ] An MCP pick request with no answer for 120 s returns `timeout`.

### 1.30.6 Concurrency with the user
**ID:** UI-API-006 · **Priority:** P0 · **Ref:** TECH_STACK §3.3

**Behaviour & rules.**
- Requests from Lua or MCP that would open a model transaction while the user is in a tool session or edit mode are **queued**; the status bar shows "External request waiting (n)" with a button that lists them and lets the user allow them now (ending the user's tool), defer, or reject.
- Presentation-only requests (`view.activate`, `selection.set`, zoom) are applied immediately unless the user is in the middle of a drag or text edit, in which case they are applied when it ends.
- While an agent operation runs, the status bar agent indicator shows its name; the user can stop it (cancelling the current command and rejecting queued ones from that session).
- Undo history entries record their source (UI, Lua script name, MCP session/agent name).

**Acceptance criteria.**
- [ ] An MCP `wall.create` request sent while the user is drawing a floor sketch waits until the sketch finishes or the user allows it.
- [ ] The Undo drop-down shows "Create wall — agent: <session name>" for an agent-created wall.

---

## 1.31 Agent pane

### 1.31.1 Purpose and architecture
**ID:** UI-AGENT-001 · **Priority:** P1 · **Ref:** research/01 §5 ([2027] assistant palette) · `screenshots/ui/revit2027_autodesk_assistant_button.png`, `screenshots/ui/revit2027_autodesk_assistant_panel.png`

**Purpose.** Offer a built-in, open, vendor-neutral AI assistant that works through Reviera's own MCP server, as Reviera's alternative to the reference's proprietary assistant.

**Behaviour & rules.**
- The Agent pane is a dockable pane (default: right dock, hidden) toggled with Ctrl+J, the QAT-strip Agent button, or View › User Interface › Agent.
- It contains a built-in **MCP client** connected to Reviera's own MCP server through an in-process transport, plus optionally to **additional MCP servers** the user configures (for example a document-search server or a company standards server). Each server's tools are namespaced.
- It talks to a **model provider** configured by the user: any provider reachable through a supported chat API, including locally hosted models; no provider is bundled or preconfigured, and nothing is sent anywhere until the user configures one. Keys are stored in the OS keychain.
- Agent sessions are ordinary MCP sessions: they get the permission level chosen for the pane (default: model-edit with confirmation), and their commands go through the same queue, transactions and undo attribution as any other client (§1.30.6).
- The pane is optional; builds without networking can omit the provider connectors while keeping the pane for local providers.

**Acceptance criteria.**
- [ ] With no provider configured, the pane shows a setup page and makes no network connection.
- [ ] Tools from Reviera's MCP server appear in the agent's tool list exactly as an external MCP client would see them.

### 1.31.2 Conversation UI
**ID:** UI-AGENT-002 · **Priority:** P1

**UI.**
- **Header**: provider/model selector, permission level badge, New conversation, History (list of past conversations with search and timestamps), Settings.
- **Transcript**: user and assistant messages (rendered Markdown subset: paragraphs, lists, code, tables); **tool-call cards** showing tool name, arguments (collapsible JSON), status (pending approval, running, done, failed, undone), result summary, and for model-changing calls the undo entry name with an **Undo** button.
- **Context chips** above the input: Active view, Selection (n elements), Screenshot of active view, Open schedule; each can be toggled before sending. Default attachments are set in Options.
- **Input**: multi-line text; Enter sends, Shift+Enter new line; slash-commands for quick actions (`/select`, `/explain-warnings`, `/schedule`); a Stop button while the agent runs.
- **Suggested prompts** on an empty conversation, generated from the current context (for example "Tag all untagged doors in this view").
- Canvas right-click "Ask Agent about selection" opens the pane with the Selection chip attached.
- **History list.** The History button opens a list of past conversations for this user (newest first) with title (the first user message, editable), date, provider and model, number of model-changing calls, and the project it was held in. A search field filters by text in titles and messages. Actions: *Open* (read-only transcript; *Continue* starts a new turn in it if its provider is still configured), *Rename*, *Delete*, *Export as Markdown…*. Conversations whose retention has expired are not listed (UI-AGENT-003).
- **Stop.** Stop cancels the provider request, cancels the running tool call where the command supports cancellation, and drops queued calls of that turn; calls already committed stay and can be undone with *Undo agent turn*.

**Commands.** These commands are for Lua and the pane itself and are not exposed over MCP (§1.34 notes):
- `agent.conversation.new(provider?, model?) -> conversation_id`
- `agent.send(conversation_id, text, attachments?) -> ()` (attachments are context-chip ids: `active_view`, `selection`, `screenshot`, `open_schedule`)
- `agent.stop(conversation_id) -> ()`
- `agent.history(filter?) -> [ConversationSummary]` (filter by text, date range, project).

**Acceptance criteria.**
- [ ] Each tool call the agent makes appears as a card before or as it runs; failed calls show the error.
- [ ] Clicking Undo on a tool-call card undoes exactly that call's transaction (or reports that later changes depend on it).

### 1.31.3 Approval, safety and undo
**ID:** UI-AGENT-003 · **Priority:** P1 · **Ref:** TECH_STACK §3.6 (safety)

**Behaviour & rules.**
- **Approval policy** (per conversation, default from Options): *Ask for every model change* (default), *Ask for destructive changes only* (delete, purge, check-in, overwrite), *Allow all model changes this conversation*. Read-only and presentation commands never ask unless the user chose so (§1.30.1).
- `execute_lua` and full-scripting tools always ask, showing the full script.
- **Preview mode**: for commands that support preview, the card offers **Preview** (temporary graphics in the active view) before Approve.
- Each **agent turn** (one user message and the resulting tool calls) is wrapped in a transaction group so **Undo agent turn** reverts it in one step.
- Model content (element names, parameter values, text notes, server comments) supplied to the agent is marked as data in the prompt envelope; the pane never executes instructions found in model content without the user's approval under the policy above.
- Transcripts are stored per user in the data directory (retention configurable: never, 30 days, forever); never in the model and never checked in.
- The pane shows which external services a conversation has used.
- **Approval card.** A call that needs approval shows its tool name, a plain-language summary generated from the command descriptor ("Delete 3 elements: 2 walls, 1 door"), the arguments, and *Approve*, *Reject*, *Preview* (where supported) and *Approve all like this in this conversation*. The card does not time out; the agent waits. Reject takes an optional reason that is returned to the agent with the error.
- **Undo agent turn** is offered on the last message of every turn with committed changes. If later edits depend on the turn's changes, the shell reports the conflicting undo entries and offers *Undo back to here* (which also undoes the later entries) or *Cancel*.

**Commands.** `agent.approve(call_id) -> ()` and `agent.reject(call_id) -> ()` are UI-only: they are bound to the card buttons and are not callable from Lua or MCP, so no script or agent can approve a call. `agent.undo_turn(conversation_id, turn_id) -> ()` (Lua and the pane) undoes the turn's transaction group.

**Acceptance criteria.**
- [ ] Under the default policy, an agent's `element.delete` call waits for Approve; Reject returns an error result to the agent and changes nothing.
- [ ] "Undo agent turn" after a turn that created 12 tags removes all 12 in one step.
- [ ] With retention "never", closing the conversation leaves no transcript on disk.

### 1.31.4 Agent terminal (external CLI agents)
**ID:** UI-AGENT-004 · **Priority:** P1 · **Ref:** owner requirement (2026-10-20); Reviera's own design

**Purpose.** Run the command-line AI agents people already use (Claude Code, Gemini CLI, Qwen Code, Codex CLI and others) inside Reviera, in a real terminal, already connected to the model open in the window through Reviera's MCP server. *(Owner requirement, 2026-10-20: "Terminal panel that runs the CLI AI agents (Claude, Gemini, Qwen, etc,) started and connectioned to the MCP provider." Follow-up answers: a stdio bridge to the running window; model-edit with confirmation by default; a general terminal with agent presets; per-session temporary MCP configuration that never changes the user's own agent settings.)*

**UI.** A dockable **Terminal** pane (default: bottom dock, hidden; View › User Interface › Terminal, `Ctrl+Shift+T`) with a tab per session. The **+** button and its drop-down open a new tab: **Shell** (the user's login shell), one entry per **agent preset**, and *Manage Presets…*. Each tab's caption shows the preset and, for agent tabs, a coloured dot for the MCP session (connected, waiting, disconnected) and its permission level; the tab's context menu has *Permission ›* (Read-only, Edit with confirmation, Edit, Full scripting), *Restart*, *Close*, *Copy*, *Paste*, *Clear Scrollback*.

**Behaviour & rules.**
- **Terminal.** Each tab runs its process in a pseudo-terminal (POSIX `openpty` on Linux and macOS, ConPTY on Windows) with an xterm-compatible emulator (UTF-8, 256 colours and true colour, the alternate screen, bracketed paste, mouse reporting, cursor styles), so full-screen terminal programs such as the agents' own interfaces work. Scrollback is 10 000 lines (configurable). Copy and paste use the system clipboard; pasting several lines asks first unless the program enabled bracketed paste. The pane follows the UI theme's terminal palette and a monospace font of the UI font size. Resizing the pane resizes the terminal (the program receives the new size).
- **Agent presets.** A preset is: a name, an executable and arguments, a working directory (the model's folder by default), extra environment variables, and **how the agent takes MCP servers**: a configuration file named on its command line, a configuration file found through an environment variable, or a settings file in a working directory Reviera prepares for the session. Reviera ships presets for Claude Code, Gemini CLI, Qwen Code and Codex CLI, each written from that agent's own published documentation when it is built (recorded in the research log), and the user may add, edit, duplicate or delete presets (stored per user, not in the model). A preset whose executable is not found opens a tab that says so and offers *Edit Preset*; Reviera never installs agents.
- **Connection (stdio bridge).** Opening an agent tab creates an MCP session for it and writes a temporary configuration, in a directory only the user can read, naming Reviera's bridge as the agent's MCP server: `reviera mcp-bridge --session-file <file>`, where the session file (mode 0600, two lines: the endpoint and the session's secret) keeps the secret off every command line. The bridge is a small stdio process: it relays the agent's MCP messages to the running window over a private local endpoint (a Unix-domain socket in the user's runtime directory with mode 0600 on Linux and macOS; a named pipe restricted to the user on Windows) after presenting the session's one-time secret. No network port is opened. The window serves the session with the same MCP server, tool catalogue, queue, transactions and undo attribution as any other client (DM-MCP-001 … 006): the actor is the preset's name and the tab number.
- **Never the user's own settings.** Reviera does not change the agents' global or project settings, and removes the session's temporary files when the tab closes or Reviera exits (stale ones from a crash are removed at the next start).
- **Permission.** Each agent session starts at *Edit with confirmation* (Options › AI & MCP › Terminal agents changes the default): model edits run, and destructive operations (delete, purge, overwrite, check-in) and `execute_lua` wait for approval in a Reviera dialog naming the tab, the tool and a plain-language summary, since the agent's terminal cannot show Reviera's prompt. The level can be changed from the tab's menu; lowering it applies to the next call. Approval dialogs are UI-only; no MCP or Lua call can approve them (as UI-AGENT-003).
- **Scope.** A session sees the model of the active view when the tab was opened; *Follow active model* in the tab's menu switches it to whichever model is active.
- **Lifetime.** Closing a tab ends its process (after asking when it is still running) and revokes its session; a bridge whose secret is revoked or whose window has gone exits with an MCP error. Closing a model with agent sessions on it asks first and disconnects them. Sessions appear in the AI Agents panel (DM-MCP-002) with *Disconnect*.
- **Keys and accounts.** The agents' own logins and API keys stay the agents' business: Reviera passes no credentials to them and reads none from them.
- **Flatpak and sandboxes.** In the Flatpak the agents run on the host (`flatpak-spawn --host`), the bridge runs inside the sandbox through the same spawn, and the endpoint lives in the app's runtime directory; the Flatpak's permissions are documented in its notes. Where host programs cannot be started, the pane says so.

**Data.** Presets and the pane's settings are per-user application settings; sessions and their secrets are memory only. Nothing is written to the model or its RXD.

**Commands.** `terminal.open(preset_id?, cwd?) -> tab_id`, `terminal.list() -> tabs[]`, `terminal.close(tab_id)`, `terminal.send_text(tab_id, text)`, `agent_preset.list() -> presets[]`, `agent_preset.set(preset)`, `agent_preset.delete(preset_id)`. These are window commands for Lua and the UI only; they are **not** exposed to MCP, so an agent cannot start other agents or type into terminals. The bridge is `reviera mcp-bridge` (a command-line mode of the application; it takes its endpoint and secret from its environment and refuses to run without them).

*Built (2026-10-20):* View › User Interface › **Terminal** (Ctrl+Shift+T; Window menu) opens a dockable pane below the views with a preset list, *Open* and *Close Tab*, and a tab per session. Each tab runs its program in a pseudo-terminal (`portable-pty`, MIT) with the `vt100` emulator (MIT): 16, 256 and true colours, bold, inverse, the alternate screen, the cursor, application cursor keys, 10 000 lines of scrollback on the wheel, `TERM=xterm-256color`; keys sent as xterm sequences (arrows with modifiers, function keys, Ctrl and Alt combinations); Ctrl+Shift+C copies the screen, Ctrl+Shift+V pastes (bracketed when the program asked; several lines ask first otherwise); resizing resizes the program. A tab whose program ends keeps its output and is marked *(ended)*. Built-in presets: Shell (the user's `$SHELL`, `COMSPEC` on Windows), Claude Code (`claude --mcp-config <session>/mcp.json`), Gemini CLI (`GEMINI_CLI_SYSTEM_SETTINGS_PATH` naming the session's settings file), Qwen Code (started in the session folder with its `.qwen/settings.json`, as its documentation names no session override), Codex CLI (`-c mcp_servers.reviera.command=…` and `.args=…`), each from the agent's documentation (RESEARCH_LOG, S0835–S0839); the user's own presets in `terminal-presets.json` in the settings folder (same id replaces a built-in). An agent tab gets an MCP session on the window's endpoint (permission model-edit, toolsets core, modeling and documentation, named after the tab), a session folder in the user's runtime directory (`$XDG_RUNTIME_DIR/reviera`, mode 0700) with the session file (mode 0600: the endpoint and the secret) and the agent's configuration, all naming `reviera mcp-bridge --session-file <file>`; the secret appears in no command line or environment. Its calls run on the UI thread from the endpoint's queue (undo and attribution as the user's commands; the views, Properties, browser and Model Tree refresh after them); destructive calls show *"<tab> wants to run <command>. It would leave n added, n changed, n deleted. Allow it?"*, asked with the window free so the terminals keep drawing; *No* returns `approval.rejected`. The session is revoked when its program ends or its tab closes, and the folder is removed; stale folders from a crash are removed at start-up (Linux). In the Flatpak, tabs run on the host through `flatpak-spawn --host` (the Shell preset runs the host's login shell), agents start the bridge as `flatpak run --command=reviera <app id> mcp-bridge …`, and the endpoint and session folders live in the app's runtime directory (`$XDG_RUNTIME_DIR/app/<app id>/reviera`), which the host can reach; the manifest adds `--talk-name=org.freedesktop.Flatpak` for this. Closing Reviera with tabs open asks first and ends them. A program that cannot start says so in its tab. Not yet: the tab context menu (permission level, Restart, Clear Scrollback, *Follow active model*), selecting text with the mouse (Copy takes the whole screen), mouse reporting to programs, *Manage Presets* (presets are edited in the JSON file), the default-permission option, the AI Agents panel listing, `terminal.*` and `agent_preset.*` Lua commands, the endpoint on Windows tested beyond compiling (it cross-compiles).

**Acceptance criteria.**
- [ ] Opening a Claude Code tab with Claude Code installed starts it in the model's folder, and asking it to list the walls returns the open model's walls through Reviera's MCP tools.
- [x] The agent's `element.delete` call shows a Reviera approval dialog naming the tab; Reject returns an error to the agent and deletes nothing; the agent's edits undo in the window like any other. *(Built 2026-10-20; tested with a stand-in agent that reads the Claude Code configuration.)*
- [x] No file in the user's agent settings (for example `~/.claude.json` or `~/.gemini/settings.json`) changes when an agent tab is opened and closed. *(Only the session folder is written.)*
- [x] A process that connects to the endpoint without the session's secret is refused and logged; closing the tab revokes the secret.
- [ ] A Shell tab runs the user's shell; a full-screen program (for example `vim` or the agent's own interface) draws and resizes correctly.
- [x] A preset whose executable is missing opens a tab that says so and changes nothing else.

---

## 1.32 BIMserver-mode user interface

### 1.32.1 Connect and log in
**ID:** UI-SRV-001 · **Priority:** P1 · **Ref:** TECH_STACK §3.7, §3.7.1

**Workflow.**
1. Collaborate › Connect (or Home › Servers › Add server, or opening a recent server project).
2. **Login dialog**: Server (combo of saved connections plus "New…"), URL, user name, password, "Remember me" (stores the session token in the OS keychain; the password is never stored), Test connection, Log in. Shows server version, supported IFC schemas and deserialisers once reached.
3. On success the connection indicator turns to "Connected as <user>" and the Server Projects pane opens.

**Behaviour & rules.**
- TLS certificate problems show a clear dialog with certificate details and options Cancel / Trust this certificate for this server (pinned) — never a silent bypass.
- Expired tokens trigger the login dialog pre-filled, without losing local work.
- Credentials are never exposed to Lua or MCP (`bimserver.login` from scripts uses a saved connection id; the keychain lookup is internal).
- Logout (`bimserver.logout`) keeps the working copy and switches to offline.

**Commands.** `ui.server.login_dialog(connection_id?)`; protocol commands `bimserver.login`, `bimserver.logout` are owned by Ch. 6.

**Acceptance criteria.**
- [ ] Logging in with "Remember me" and restarting reconnects without asking for the password until the token expires.
- [ ] A self-signed certificate is refused until the user explicitly trusts it.

### 1.32.2 Server Projects pane and Home Servers page
**ID:** UI-SRV-002 · **Priority:** P1 · **Ref:** research/01 §2.2 (project browsing pattern) · `screenshots/ui/home_new_projects.png`

**UI.**
- A tree of **servers › projects › sub-projects**, with search; each project shows schema, last revision number, date and author.
- Selecting a project shows its **revisions list** (number, date, user, comment, size) in the lower half, with filter by user and date and search in comments.
- Actions: **Open latest**, **Open revision…**, **Link into current model** (Ch. 6), **New project…** (name, schema, parent), **Publish current model here** (`bimserver.publish_local`), **Properties** (project settings the user may edit), **Refresh**.
- Opening a project downloads the revision into the local working copy with progress and Cancel; subsequent opens use the cache and only download newer revisions.
- Home › **Servers** shows the same browser full-page, plus recently opened server projects.

**Commands.** `ui.server.browser(connection_id?)`; data commands (`bimserver.list_projects`, `bimserver.list_revisions`, `bimserver.open`, `bimserver.create_project`) are owned by Ch. 6.

**Acceptance criteria.**
- [ ] Opening revision 12 of a project with 40 revisions opens that revision read-write in the working copy, and the title shows `r12`.

### 1.32.3 Check In
**ID:** UI-SRV-003 · **Priority:** P1 · **Ref:** TECH_STACK §3.7 (check-in)

**Workflow.**
1. Collaborate › Check In (CI), or QAT Sync › Sync with Options.
2. **Check In dialog**: comment (required; template from Options), change summary (counts of added/modified/deleted elements by category, expandable to a list with "zoom to"), option "Get latest first" (default on), option "Relinquish my ownership after check-in", and Check In / Cancel.
3. Progress shows serialise → upload → server processing; the UI remains usable for navigation (editing is paused until upload completes, with a status explanation).
4. Result: new revision number toast; failures show the server message and keep local changes.

**Behaviour & rules.**
- **Comment validation.** 1–2 000 characters after trimming; the template's placeholders (`{user}`, `{date}`, `{changed_count}`) are filled in before the dialog opens. The last 10 comments are offered in a drop-down.
- **Change summary** comes from the live store's change sets since the last Get Latest or Check In (Ch. 2b). "Zoom to" activates a view containing the element and selects it without closing the dialog. RXD-only changes (for example a shared view bookmark) are listed under *Project data*.
- **Get latest first.** When on and newer revisions exist, Reviera runs Get Latest (UI-SRV-004) first; if that produces conflicts, Check In stops at the conflict dialog and resumes after the conflicts are resolved.
- **Open edit modes.** Check In is refused while an edit mode or tool session is active ("Finish or cancel <mode> first").
- **Save first.** The model is saved locally before upload so that a crash during upload leaves a consistent working copy.
- **Ownership.** With "Relinquish my ownership" ticked, ownership is released only after the server confirms the new revision; on failure the user keeps it.
- **Cancel** is possible during serialise and upload; it leaves the working copy and the server unchanged. During server processing Cancel is disabled with the explanation "The server is processing the revision".
- **Offline.** In offline mode Check In is disabled with "Not connected".

**Commands.** `ui.server.checkin_dialog(comment?) -> ()` opens the dialog (pre-filling the comment when given); OK submits `bimserver.check_in(comment)` (Ch. 6), preceded by `bimserver.get_latest()` when "Get latest first" is on and followed by `ownership.relinquish(scope: all)` when chosen.

**Acceptance criteria.**
- [ ] Check In is disabled with no local changes (tooltip "Nothing to check in").
- [ ] A failed upload leaves the working copy unchanged and offers Retry.
- [ ] An empty comment disables the Check In button.
- [ ] With a floor sketch active, Check In is refused with a message naming the mode.
- [ ] With "Relinquish" ticked and a failed upload, the user still owns the elements.

### 1.32.4 Get Latest and Sync
**ID:** UI-SRV-004 · **Priority:** P1 · **Ref:** TECH_STACK §3.7, §3.7.2

**Behaviour & rules.**
- **Get Latest** (RL) downloads newer revisions and merges them by GlobalId into the working copy. **Sync** = Get Latest, merge, then Check In (with the Check In dialog unless "Sync now" with a remembered comment is chosen).
- After merging, a **Merge report** notice lists elements added, changed and removed by others, with "Show in view" (temporarily highlights them; uses the Collaboration Display "Model Updates" mode) and "Select".
- **Conflicts** (an element changed locally and on the server) open the **Conflict resolution dialog**: per element or per category, choose Keep mine / Take theirs / Inspect (side-by-side property comparison and a highlight in the active view); bulk actions; the dialog can be left and resumed. Rules for what counts as a conflict are owned by Ch. 6.
- Merge results are one undo-able step only before any later edit; after that, the server revision is the baseline.

**Acceptance criteria.**
- [ ] After another user moves a wall and checks in, Get Latest updates the wall and the merge report lists it.
- [ ] A conflicting edit opens the conflict dialog; choosing Keep mine and checking in uploads the local value.

### 1.32.5 Connection indicator and notifications
**ID:** UI-SRV-005 · **Priority:** P1 · **Ref:** TECH_STACK §3.7.1 (notification interfaces)

**Behaviour & rules.**
- **Connection indicator** in the QAT strip and status bar: Offline (grey), Connected (accent), Syncing (animated, with reduce-motion fallback), Error (warning colour + icon). It shows counts of local changes not checked in and of new server revisions. Clicking opens the Server pane.
- **Notifications**: new revision events from the server (pushed; polling fallback at the configured interval) show an in-app toast "Revision 42 by Alex: <comment> — Get Latest / Dismiss" and, when Reviera is not focused, an OS notification (preference). Ownership requests, request responses and lock changes also notify.
- All notifications are kept in the **Notifications pane** (with filters, mark read, and actions), and are available via `ui.notifications.list`.
- Working offline: the indicator shows Offline; Save keeps working locally; Check In and Get Latest are disabled; on reconnect, a notice summarises what changed on the server.

**Commands.** `ui.notifications.list(filter?) -> [Notification]`, `ui.notifications.dismiss(id|all)`, `ui.notifications.act(id, action)`.

**Acceptance criteria.**
- [ ] A check-in by another user produces a toast within 5 seconds when push notifications are available.
- [ ] Disconnecting the network switches the indicator to Offline without an error dialog; local saves still succeed.

### 1.32.6 Revision history
**ID:** UI-SRV-006 · **Priority:** P1

**Behaviour & rules.**
- The **Revision History** pane lists the current project's revisions; actions: Open revision read-only in a new document, Compare with working copy / with another revision (Ch. 6 `bimserver.compare`; results shown as a colour-coded temporary view state plus a list), Download as `.ifc`, Copy revision URI.
- Comparisons use the mode-border convention (§1.17.2) with the label "Comparing r40 ↔ working copy".
- **List columns.** Revision number, date (UI locale), user, comment (first line, full text in a tooltip), size, and a marker for the revision the working copy is based on. Sort by any column; filter by user and date range; search in comments. The list loads 100 revisions at a time and fetches more on scroll.
- **Compare display.** Added elements in `status.success`, changed in `status.warning`, removed shown as ghost outlines in `status.error`, each also with a distinct line pattern so colour is not the only signal (§1.33.2). A legend sits in the view's top-left under the mode label. The list beside it groups changes by category with counts; selecting a row selects and zooms to the element. Compare state is temporary view state: Esc or *End comparison* clears it, and it is never saved.
- **Open read-only** opens the revision as a separate document whose title carries `r<n> (read-only)`; editing commands are disabled in it, and it can be closed without a save prompt.
- **Restore.** *Make this the latest revision…* (with a required reason) is offered to users with the server right to do so; it creates a new revision with the old content (Ch. 6) and never rewrites history.
- **Offline.** The pane shows the cached list with "Offline — list may be out of date" and disables actions that need the server.

**Commands.** `ui.server.revisions(project?) -> ()` opens or focuses the pane; actions call `bimserver.history()`, `bimserver.compare(roid_a, roid_b)`, `bimserver.download_revision(server_id, roid, path)` and `bimserver.restore(roid, reason)` (Ch. 6).

**Acceptance criteria.**
- [ ] Comparing two revisions highlights added, changed and removed elements in distinct, labelled styles.
- [ ] Opening r12 read-only leaves the working copy open and unchanged, and closing r12 asks nothing.
- [ ] Ending a comparison restores the view's normal display with no undo entry.

### 1.32.7 Ownership feedback
**ID:** UI-SRV-007 · **Priority:** P1 · **Ref:** research/01 §17 (Editing Requests, Editable Only), §15 (#12 worksharing display); TECH_STACK §3.7.2

**Behaviour & rules.**
- Elements owned by another user show the owner in tooltips and Properties (read-only reason "Owned by <user>"); attempting to edit offers **Request editing** (sends a request; status bar count updates for the owner).
- Editing Requests dialog: incoming and outgoing requests with Grant / Deny / Withdraw.
- Collaboration Display modes on the View Control Bar colour elements by ownership status, owner, or "changed since my last Get Latest", with a legend.
- **Edit attempt flow.** When a command would change elements the user does not own, the command is not run. A dialog lists the blocking elements with their owners and offers *Request editing* (sends one request per owner, with an optional message), *Select blocking elements*, and *Cancel*. Elements owned by nobody are taken automatically (`ownership.borrow`) before the command runs, when the server allows it.
- **Waiting for a grant.** Pending outgoing requests show in the status-bar Editing Requests count. When a request is granted, a toast offers *Retry the last action*; denied requests show the owner's message.
- **Incoming requests** raise a toast for the owner with *Grant*, *Deny* and *Open requests*. The Editing Requests dialog lists incoming and outgoing requests (requester, elements with "zoom to", message, time, status) with *Grant*, *Deny* (with optional message) and *Withdraw*.
- **Properties palette.** For elements owned by others, every editable row is read-only with the reason "Owned by <user>"; a banner at the top of the palette offers *Request editing*.
- **Relinquish.** Collaborate › Relinquish All Mine, and right-click › Relinquish on owned elements; refused for elements with local changes not checked in ("Check in first").
- **Offline.** Ownership information shows the last known state, marked "may be out of date"; editing elements the user does not hold is refused until connected.

**Commands.** Ownership and request commands are owned by Ch. 6: `ownership.borrow`, `ownership.relinquish`, `ownership.query`, `editrequest.create`, `editrequest.grant`, `editrequest.deny`, `editrequest.retract`, `editrequest.manage` (opens the Editing Requests dialog); Collaboration Display uses `view.set_worksharing_display(mode)`.

**Acceptance criteria.**
- [ ] Moving an element owned by another user is blocked with a message offering Request editing.
- [ ] After the owner grants the request, *Retry the last action* moves the element.
- [ ] Relinquishing an element with local changes not checked in is refused.

---

## 1.33 Accessibility, HiDPI and localisation

### 1.33.1 Keyboard operability
**ID:** UI-A11Y-001 · **Priority:** P0 · **Ref:** research/01 §22 (keytips), §23

**Behaviour & rules.**
- Every command is reachable without a pointer (sequences, keytips, Command Search). Every pane, dialog and the ribbon support full keyboard navigation with a visible focus ring (token `ui.focus_ring`, minimum 2 logical pixels).
- **F6/Shift+F6** cycles focus regions; within the canvas, Tab cycles candidates and arrow keys **nudge** the selection by the current snap increment (Shift+arrow: 10×) as a move command (P1).
- Canvas picks can be made from the keyboard: **coordinate entry** (type `x,y` or `@dx,dy` or `<distance<angle` when a tool asks for a point; P1) and `snap.query` for scripts.
- No feature depends on hover alone: hover information is also available for the selected element through Properties and the status bar.
- **Focus regions** in F6 order: ribbon (with QAT), left dock panes, view window tabs, canvas, View Control Bar, right and bottom dock panes, status bar. Hidden regions are skipped. Each region remembers its last focused control.
- **Canvas keyboard selection.** With the canvas focused and no tool active, Ctrl+Shift+Right and Ctrl+Shift+Left step through the elements visible in the view, nearest to the view centre first, pre-highlighting each (Tab keeps its meaning of cycling candidates under the pointer, UI-SEL-001); while an element is stepped to, Enter selects it and Ctrl+Enter adds it to the selection (otherwise Enter keeps its repeat-last meaning, §1.26.3). The status bar and the accessible summary name the current element.
- **Nudge.** Arrow keys move the selection by the current snap length increment for the zoom level (Shift+arrow: 10×). Consecutive nudges within 1 s merge into one undo entry "Nudge". Nudging is refused for pinned or non-movable elements with a status-bar message.
- **Coordinate entry** syntax: `x,y` (absolute, project coordinates), `@dx,dy` (relative to the last point), `<distance<angle` (polar from the last point, angle in degrees counter-clockwise from the view's X axis). Values accept unit suffixes as in the Properties palette. Invalid input is kept in the field and marked, with an explanation.
- **Dialogs.** Every dialog has a default button (Enter) and a cancel button (Esc), visible mnemonics on labelled controls, and opens with focus on the first editable field.
- **No timing traps.** No interaction requires a key to be pressed within a fixed time except sequence shortcuts, whose timeout is configurable (§1.26.1).

**Acceptance criteria.**
- [ ] A test user can create, dimension and tag a wall without a mouse.
- [ ] Every dialog in the shell passes an automated tab-order check (no traps, logical order).
- [ ] Typing `@3000,0` after the first wall point places the end point 3 000 mm along X.
- [ ] Three arrow nudges within one second produce one undo entry.

*Built (2026-10-19), coordinate entry:* **Coordinates** (`coordinates.edit`, sequence `XY`, on Modify › Measure and in every sketch tab) takes points as `x, y`, `@dx, dy` (from the point before, or the tool's last clicked point) and `@distance<angle` or `<distance<angle`, lengths in project units with suffixes. It edits what is in progress or selected: sketch mode lists each curve as `start; end` (`via x, y` for arcs) to correct sketched curves exactly or add more, then optionally finishes; plate, area, path and fabric sketches list their points; any other point tool (walls, beams, columns, grids, lines, railings, stairs, measuring, …) receives the points one by one exactly as clicks, through the tools' own snapping and point functions, optionally followed by Enter; a selected wall, beam, grid, plate or area, fabric or path system has its ends, contour, boundary or path changed in one step. Not yet: typing straight into the canvas without the dialog, the inline error marking, 3D picks for tools that pick surfaces (Place Component in 3D, hosts under the cursor).

### 1.33.2 Visual accessibility
**ID:** UI-A11Y-002 · **Priority:** P1

**Behaviour & rules.**
- UI text and essential icons meet a contrast ratio of at least 4.5:1 (text) and 3:1 (icons, focus rings, selection outlines against both canvas schemes) in the standard themes; the high-contrast themes meet 7:1 for text.
- State is never conveyed by colour alone: mode borders carry labels; toggles have pressed shapes; status uses icons plus text; selection is drawn with thicker lines as well as colour.
- A colour-vision-deficiency-safe canvas palette (preference) changes selection, pre-selection, alert and comparison colours to a palette distinguishable under protanopia, deuteranopia and tritanopia.
- UI font scaling 90–200 % without clipping; the ribbon re-lays out.
- **Reduce motion** disables animated transitions (Orientation Cube, view changes, toasts).
- Larger grips and hit targets preference (minimum 12 logical pixels).

**Acceptance criteria.**
- [ ] Automated contrast checks on all theme files pass the thresholds above.
- [ ] At 200 % UI font scale every ribbon label is readable or available in a tooltip, and no dialog clips text.

### 1.33.3 HiDPI and multiple scale factors
**ID:** UI-DPI-001 · **Priority:** P0

**Behaviour & rules.**
- All sizes are specified in logical (device-independent) pixels and converted with `FromDIP`/`ToDIP`; all icons are SVG via `wxBitmapBundle` (no raster-only icons).
- Windows: per-monitor DPI awareness v2 in the manifest; handle `wxEVT_DPI_CHANGED` in every top-level frame and dialog. macOS: native backing scale. Linux: GTK scale factor and fractional scaling where the compositor supports it.
- The canvas renders at native device resolution; line weights, text heights and grip sizes are scaled correctly so that printed-scale line weights look consistent across monitors; overlay hit-testing uses device pixels.
- Moving a window between monitors with different scale factors updates layout, icons and canvas resolution without restart or blurring.
- Supported scale range: 100 %–300 %.

**Acceptance criteria.**
- [ ] At 150 % and 200 % scale on Windows, Linux (Wayland and X11) and macOS, no icon is blurry and no control is clipped.
- [ ] Dragging a floating view from a 100 % monitor to a 200 % monitor re-renders it sharply within one second.

### 1.33.4 Assistive technology
**ID:** UI-A11Y-003 · **Priority:** P1

**Behaviour & rules.**
- Native wx controls expose accessibility through the platform (UI Automation/MSAA on Windows, AT-SPI on Linux, NSAccessibility on macOS). Custom-drawn controls (ribbon, QAT strip, status bar toggles, View Control Bar, Type Selector list, keytips) implement accessible names, roles, states and descriptions (from the command descriptor's label and tooltip) through `wxAccessible` on Windows and equivalent bridge code on the other platforms.
- The canvas exposes an **accessible summary**: active view name and kind, selection summary ("3 walls selected, 1 door"), active tool and prompt, and mode labels. Changes are announced through a live region at the verbosity chosen in Options.
- The Properties palette is fully navigable by screen reader (group, name, value, read-only reason).
- Agent pane transcripts and notifications are accessible text.
- **Verbosity levels** (Options › Accessibility): *Off* (no canvas announcements; controls still have names), *Brief* (selection count and category, mode changes), *Normal* (default; adds tool prompts and the hovered or cycled element), *Detailed* (adds snap names and temporary-dimension values while drawing). Announcements of the same kind are throttled to one per second; the newest replaces a pending one.
- **Roles.** Ribbon tabs are announced as tabs, panels as groups, split buttons as split buttons with their current member, toggles with pressed state, galleries as lists with the selected item. The Project Browser and System Browser are trees with level and expanded state. Toasts are announced politely without moving focus; error dialogs are announced assertively.
- **Canvas as a single object.** The canvas is one accessible object with role "drawing", whose name is the view name and whose description is the accessible summary; individual elements are not exposed as accessible objects (the selection summary and the Properties palette carry their information).
- **Testing.** Each platform build runs an automated accessibility tree dump of the main window and compares it with a stored reference, so missing names or roles fail the build.
- The accessible summary is also available to scripts and tests.

**Commands.** `ui.a11y.summary() -> {view, selection, tool, prompt, modes[]}` returns the same text the canvas exposes.

**Acceptance criteria.**
- [ ] With a screen reader running, tabbing through the Modify tab announces each tool's name and shortcut.
- [ ] Selecting elements announces the new selection summary.
- [ ] With verbosity *Off*, selecting elements produces no announcement while ribbon controls are still named.
- [ ] The accessibility tree dump shows no unnamed interactive control in the main window.

### 1.33.5 Localisation
**ID:** UI-L10N-001 · **Priority:** P1

**Behaviour & rules.**
- All user-visible strings are externalised (gettext catalogues via wxWidgets' translation support); command ids, Lua names and MCP tool names are never localised.
- Number, unit and date formatting follow the model's unit settings (for values) and the UI locale (for dates in Home, revisions and notifications).
- Right-to-left layouts are not required for v1.0 (noted in §1.35).
- Keytips are regenerated per language; default key sequences are language-independent.
- **Language choice.** Options › General › Language offers *Follow system* (default) and each installed catalogue; the key is `general.language` (a BCP 47 tag such as `de-DE`). A change takes effect after restart and is marked "requires restart" (UI-PREF-001).
- **Fallback.** A missing translation falls back to British English per string, never to the command id; the build reports catalogue coverage, and a language with less than 80 % coverage is listed as "partial".
- **What is translated.** Labels, tooltips, dialogs, messages, help index titles, built-in layout and scenario names (stored by id, so renaming in another language does not create a new layout). Not translated: command ids, setting keys, Lua and MCP names, IFC class names and property set names, file names, and user content (view names, parameter names defined in the model).
- **Model-level text.** Category and built-in parameter display names in the UI follow the UI language; values written into IFC follow the IFC rules of Ch. 2, independently of the UI language.
- **Layout robustness.** Dialogs use sizers so text up to 40 % longer than English fits without clipping; the ribbon uses the reduction rules of UI-RIB-002 for longer labels.
- **Search.** Command Search matches both the localised label and the English label, so users can follow English tutorials.
- **Sources.** Catalogues live in the source tree as `.po` files under the same licence; contributed translations follow the project's contribution rules.

**Acceptance criteria.**
- [ ] Switching the UI language changes ribbon labels and tooltips after restart while all shortcuts and scripts keep working.
- [ ] In a German UI, typing "Wall" in Command Search finds the wall tool.
- [ ] A string missing from a catalogue is shown in English, not as an id.

---

### 1.33.9 Crash recovery and live-store status
**ID:** UI-RECOV-001 · **Priority:** P0 · **Ref:** spec/02b_live_model_store.md §6.2 (owner directive, 2026-10-12)

**Purpose.** This is the user interface for the live SQLite store (Chapter 2b). After a crash it offers to recover unsaved work. While the user works, it shows that their work is being kept safe.

**Workflow.**
1. At startup, if `recovery.list` returns any project, the **Recover unsaved work** dialog opens before the Home screen. It shows one row per project: name or "Untitled", path, time of the last change, number of unsaved changes, and the last three undo labels.
2. Each row has **Restore** (the default button), **Restore as copy…**, **Show changes**, **Discard** and **Decide later**. Discard asks for confirmation inside the dialog itself.
3. A restored project opens with "(recovered)" in its title and is marked as unsaved. A notice bar in the view area says: "Recovered N changes from <time>. Save to keep them." It has **Save** and **Show changes** buttons.

**UI.**
- **Status bar indicator.** A small disk glyph shows the store's state:
  - *Up to date*: everything committed is on disk.
  - *Writing…*: change sets are pending, shown for more than 0.5 s only.
  - *Recovery data unavailable*: a write failed or the disk is full. Shown in red, and clicking it opens details and a **Retry** button.
- **Tooltip.** Shows the time of the last durable commit and the store's location.
- **Settings.** **Options › General › Recovery** has fields for durability mode (Normal/Full), group-commit window, store location, retention and disk quota (DM-LIVE-014).
- **Auto-save reminders.** The existing save reminder (§1.29) remains, because the live store doesn't replace saving the IFC file.
- **Encryption.** The indicator's tooltip shows "Recovery data encrypted" or "Recovery data not encrypted". Options › General › Recovery has the encryption switch and a **Set recovery passphrase…** button (DM-LIVE-016).
- **Resuming a sketch.** For a recovered sketch or tool session, the notice bar shows **Resume sketch** and **Discard sketch** (DM-LIVE-015).
- **History panel.** A dockable panel (View › Windows › History, Ctrl+Shift+H) with a timeline, origin filters, historical views and restore and revert actions, as specified in Chapter 2b §8.5 (DM-LIVE-017).

**Acceptance criteria.**
- [ ] After the process is killed, the next start shows the dialog listing the project. Restore reopens the project with the "(recovered)" title and the notice bar.
- [ ] Filling the disk turns the indicator red within 2 s, and editing continues.
- [ ] Screen readers announce the indicator's state changes, but no more than once every 10 s.

## 1.34 Command catalog

All commands defined by this chapter (owner UI). Commands of other chapters that the shell binds to are listed in the ribbon catalog (§1.8) and are defined by their owners. "Undoable" is **No** for presentation and session-state commands; such commands never open a model transaction. Exposure is UI + Lua + MCP unless the Notes column says otherwise.

| Command | Parameters | Returns | Undoable | Req ID |
|---|---|---|---|---|
| `ui.command.list` | `filter?` (text, owner, tab, has_shortcut) | `[CommandDescriptor]` | No | UI-ARCH-001 |
| `ui.command.describe` | `id` | `CommandDescriptor` | No | UI-ARCH-001 |
| `ui.command_search` | `query`, `limit?` | `[SearchResult]` | No | UI-SEARCH-001 |
| `ui.help.context` | `target?` (command id, dialog id) | `()` | No | UI-HELP-001 |
| `ui.user.get` | — | `{name, source: options\|server, server_uri?}` | No | UI-IDENT-001 |
| `app.exit` | `force?: bool` | `()` | No | UI-FILE-001 |
| `ui.home.show` / `ui.home.hide` / `ui.home.toggle` | — | `()` | No | UI-HOME-001 |
| `ui.recent.list` | `kind?` (model, family, template, server) | `[RecentItem]` | No | UI-HOME-001 |
| `ui.recent.pin` | `path_or_uri`, `pinned: bool` | `()` | No | UI-HOME-001 |
| `ui.recent.remove` | `path_or_uri` | `()` | No | UI-HOME-001 |
| `ui.recent.clear` | — | `()` | No | UI-HOME-001 |
| `ui.qat.get` | — | `[QatItem]` | No | UI-QAT-001 |
| `ui.qat.add` | `command_id`, `index?` | `()` | No | UI-QAT-001 |
| `ui.qat.remove` | `index` | `()` | No | UI-QAT-001 |
| `ui.qat.move` | `from`, `to` | `()` | No | UI-QAT-001 |
| `ui.qat.add_separator` | `index` | `()` | No | UI-QAT-001 |
| `ui.qat.set_position` | `above|below` | `()` | No | UI-QAT-001 |
| `ui.qat.reset` | — | `()` | No | UI-QAT-001 |
| `ui.ribbon.set_state` | `full|tabs|titles|buttons` | `()` | No | UI-RIB-004 |
| `ui.ribbon.cycle_state` | — | `()` | No | UI-RIB-004 |
| `ui.ribbon.pin_slideout` | `tab_id`, `panel_id`, `pinned: bool` | `()` | No | UI-RIB-003 |
| `ui.ribbon.get_layout` | — | `RibbonLayout` | No | UI-RIB-005 |
| `ui.ribbon.move_tab` | `tab_id`, `index` | `()` | No | UI-RIB-005 |
| `ui.ribbon.move_panel` | `tab_id`, `panel_id`, `index` | `()` | No | UI-RIB-005 |
| `ui.ribbon.float_panel` | `tab_id`, `panel_id`, `pos?` | `()` | No | UI-RIB-005 |
| `ui.ribbon.dock_panel` | `panel_id` | `()` | No | UI-RIB-005 |
| `ui.ribbon.set_visible` | `tab_id`, `panel_id?`, `visible` | `()` | No | UI-RIB-005 |
| `ui.ribbon.reset` | — | `()` | No | UI-RIB-005 |
| `ui.ribbon.activate_tab` | `tab_id` | `()` | No | UI-RIB-006 |
| `ui.ribbon.activate_contextual` | — | `()` | No | UI-RIB-006 |
| `ui.disciplines.get` | — | `{enabled[], locked: bool}` | No | UI-RIB-007 |
| `ui.disciplines.set` | `enabled[]` (discipline flags) | `()` (fails when locked or no modelling discipline remains) | No | UI-RIB-007 |
| `ui.ribbon.rebuild` | — | `()` | No | UI-RIB-008 |
| `ui.ribbon.get_contextual` | — | `{label, panels[]}` or null | No | UI-CTX-001 |
| `ui.ribbon.register_context_panel` | `selector`, `panel` (Lua add-ins only; not MCP) | `panel_id` | No | UI-CTX-001 |
| `tool.modify` | — | `()` | No | UI-RIBCAT-002 |
| `tool.start` | command_id, type_id?, preset_args? | session_id | No (the tool's submissions are) | UI-API-004 |
| `tool.get_active` | — | `{command_id, step, prompt, options}` or null | No | UI-API-002 |
| `tool.get_options` | — | `{key: value}` | No | UI-ARCH-004 |
| `tool.set_option` | tool_id? (default: active tool), key, value | () | No (session state) | UI-ARCH-004 |
| `tool.finish` / `tool.cancel` | — | () | No (the tool's submissions are; a session collapses to one undo step) | UI-API-004 |
| `tool.repeat_last` | — | session_id | No (session state) | UI-MENU-001 |
| `tool.recent` | — | `[command_id]` | No | UI-MENU-001 |
| `ui.properties.get` | `target?` (element ids, view id, type id) | `PropertyGridModel` | No | UI-PROP-001 |
| `ui.properties.set_filter` | `category_id|"view"|"common"` | `()` | No | UI-PROP-001 |
| `ui.properties.apply` | — | `()` (submits `element.set_param`, which is undoable) | No (delegates) | UI-PROP-001 |
| `ui.properties.revert` | — | `()` | No | UI-PROP-001 |
| `ui.properties.varies_settings` | `text?` | `()` | No | UI-PROP-001 |
| `ui.type_selector.list` | `category|tool` | `[TypeEntry]` | No | UI-TYPE-001 |
| `browser.search` | `text` | `[BrowserNode]` | No | UI-PB-001 |
| `browser.reveal` | `element_or_view_id` | `node_id` | No | UI-PB-001 |
| `browser.expand` | `node_id`, `recursive?` | `()` | No | UI-PB-001 |
| `browser.collapse` | `node_id|"all"` | `()` | No | UI-PB-001 |
| `browser.set_tab` | `tab_id` | `()` | No | UI-PB-001 |
| `browser.get_tree` | `tab_id?`, `depth?` | `BrowserTree` | No | UI-PB-001 |
| `browser.get_selected` | — | `[BrowserNode]` | No | UI-PB-001 |
| `browser.organization_dialog` | `target?` (views, sheets, schedules) | `()` | No (OK submits Ch. 5 commands) | UI-PB-002 |
| `sysbrowser.get_tree` | `view: systems|zones|analytical`, `discipline?` | `SystemTree` | No | UI-SB-001 |
| `sysbrowser.show_element` | `element_id` | `view_id` | No | UI-SB-001 |
| `ui.pane.list` | — | `[PaneState]` | No | UI-DOCK-001 |
| `ui.pane.show` / `ui.pane.hide` / `ui.pane.toggle` | `pane_id` | `()` | No | UI-DOCK-001 |
| `ui.pane.float` | `pane_id`, `rect?` | `()` | No | UI-DOCK-001 |
| `ui.pane.dock` | `pane_id`, `side`, `position?` | `()` | No | UI-DOCK-001 |
| `ui.pane.group` | `pane_id`, `with_pane_id` | `()` | No | UI-DOCK-001 |
| `ui.focus_mode.toggle` | — | `()` | No | UI-DOCK-004 |
| `ui.fullscreen.toggle` | — | `()` | No | UI-DOCK-004 |
| `ui.fullscreen.reveal` | `edge: top|bottom|none` | `()` | No | UI-DOCK-004 |
| `ui.fullscreen.get` | — | `{mode, revealed}` | No | UI-DOCK-004 |
| `ui.radial.open` | `x?`, `y?`, `target: element|background` | `()` | No | UI-SEL-007 |
| `ui.radial.tree` | `target` | `RadialTier` | No | UI-SEL-007 |
| `ui.radial.close` | — | `()` | No | UI-SEL-007 |
| `ui.chrome.get` | — | `{element_id: bool}` | No | UI-DOCK-004 |
| `ui.chrome.set_visible` | `element_id`, `visible: bool` | `()` | No | UI-DOCK-004 |
| `ui.layout.load` | `name` | `()` (short form of `ui.layout.apply`) | No | UI-LAYOUT-001 |
| `ui.layout.manage` | — | `()` (opens the Manage layouts dialog) | No | UI-LAYOUT-001 |
| `ui.layout.reset` | — | `()` | No | UI-LAYOUT-007 |
| `view.activate` | `view_id`, `{new_window?, float?, monitor?}` | `window_id` | No | UI-VIEWWIN-001 |
| `view.close` | `view_id` | `()` | No | UI-VIEWWIN-001 |
| `view.close_inactive` | — | `()` | No | UI-VIEWWIN-002 |
| `window.list` | — | `[WindowInfo]` | No | UI-VIEWWIN-001 |
| `window.switch` | `view_id` | `window_id` | No | UI-VIEWWIN-001 |
| `window.next` | `direction: forward|back` | `window_id` | No | UI-VIEWWIN-001 |
| `window.tile` / `window.tab_all` | — | `()` | No | UI-VIEWWIN-002 |
| `window.float` | `view_id`, `rect?`, `monitor?` | `frame_id` | No | UI-VIEWWIN-003 |
| `window.dock_to_main` | `view_id` | `()` | No | UI-VIEWWIN-003 |
| `ui.monitors` | — | `[MonitorInfo]` | No | UI-VIEWWIN-003 |
| `view.refresh` | `view_id?` (all if omitted) | `()` | No | UI-CANVAS-001 |
| `view.temp_isolate` | `view_id?`, `ids`, `by: element|category` | `()` | No | UI-VCB-001 |
| `view.temp_hide` | `view_id?`, `ids`, `by: element|category` | `()` | No | UI-VCB-001 |
| `view.temp_reset` | view_id? (default: active) | () | No | UI-VCB-001 |
| `view.temp_apply` | view_id? (default: active) | () | Yes | UI-VCB-001 |
| `view.reveal_hidden.toggle` | `view_id?` | `bool` | No | UI-VCB-001 |
| `view.reveal_constraints.toggle` | `view_id?` | `bool` | No | UI-VCB-001 |
| `view.get_temp_state` | `view_id?` | `{hidden[], isolated[], reveal, constraints}` | No | UI-VCB-001 |
| `view.zoom` | `view_id?`, `mode: in_region|out|fit|sheet_size|previous|next`, `rect?` | `()` | No | UI-NAV-001 |
| `view.zoom_all_to_fit` | — | `()` | No | UI-VIEWWIN-002 |
| `view.zoom_to_elements` | `view_id?`, `ids` | `()` | No | UI-NAV-001 |
| `view.pan` | `view_id?`, `dx`, `dy` | `()` | No | UI-NAV-001 |
| `view.orbit` | `view_id?`, `yaw_deg`, `pitch_deg`, `pivot?` | `()` | No | UI-NAV-001 |
| `view.set_camera` | view_id, eye, target, up, projection?: orthographic\|perspective, fov? | () | Yes (explicit calls; interactive navigation is not, §1.19) | UI-NAV-001 |
| `view.get_camera` | `view_id` | `Camera` | No | UI-NAV-001 |
| `view.set_pivot` | `view_id`, `point` (interactive pick when omitted) | `()` | No | UI-NAV-001 |
| `view.orient` | `view_id?`, `preset` | `()` | No | UI-NAV-002 |
| `view.orient_to_view` | `view_id?`, `source_view_id`, `add_section_box?` | `()` | No (section box creation is Ch. 5, undoable) | UI-NAV-002 |
| `view.orient_to_direction` | `view_id?`, `dir` | `()` | No | UI-NAV-002 |
| `view.set_home` | `view_id?` | `()` | No | UI-NAV-002 |
| `view.set_projection` | `view_id?`, `perspective|orthographic` | `()` | No | UI-NAV-002 |
| `ui.nav.orbit_mode` | — | `()` | No | UI-NAV-001 |
| `ui.nav.wheel` | `kind?` (2d, full, mini_full, view_object, tour) | `()` | No | UI-NAV-004 |
| `ui.nav.device_mode` | `2d|object|walk|fly` | `()` | No | UI-NAV-001 |
| `ui.navbar.get` | — | `{tools[], dock}` | No | UI-NAV-003 |
| `ui.navbar.set` | `patch` (tools[], dock: linked\|top\|bottom\|left\|right) | `()` | No | UI-NAV-003 |
| `ui.status.get` | — | `StatusState` | No | UI-STATUS-001 |
| `ui.background_tasks` | — | `[Task]` | No | UI-STATUS-001 |
| `ui.background_tasks.cancel` | `task_id` | `()` | No | UI-STATUS-001 |
| `selection.get` | — | {ids[], by_category} | No (query) | UI-SEL-002 |
| `selection.set` | ids[], mode?: replace\|add\|remove (default replace) | () | No (session state) | UI-SEL-002 |
| `selection.clear` | — | `()` | No | UI-SEL-002 |
| `selection.previous` | — | () | No (session state) | UI-SEL-002 |
| `selection.select_all_instances` | type_ids? (default: types of the current selection), scope: view\|project | element_ids[] | No (session state) | UI-SEL-002 |
| `selection.chain` | `element_id` | `[id]` | No | UI-SEL-002 |
| `selection.box` | `view_id?`, `rect`, `mode: window|crossing`, `combine` | `[id]` | No | UI-SEL-002 |
| `selection.select_all_visible` | `view_id?` | `[id]` | No | UI-SEL-002 |
| `selection.highlight` | `ids`, `duration_ms?` | `()` | No | UI-API-004 |
| `selection.filter_dialog` | — | `()` | No | UI-SEL-003 |
| `selection.filter` | category_ids[] | element_ids[] | No (session state) | UI-SEL-003 |
| `selection.set_save` | `name`, `ids?` | `set_id` | Yes | UI-SEL-003 |
| `selection.set_load` | `set_id`, `mode: replace|add` | `[id]` | No | UI-SEL-003 |
| `selection.set_edit` | `set_id` | `()` (edit mode; Finish is undoable) | Yes | UI-SEL-003 |
| `selection.set_delete` | `set_id` | `()` | Yes | UI-SEL-003 |
| `selection.set_list` | — | `[SelectionSet]` | No | UI-SEL-003 |
| `selection.options.get` | — | `SelectionOptions` | No | UI-SEL-004 |
| `selection.options.set` | `name`, `value: bool` | `()` | No | UI-SEL-004 |
| `selection.show_ids` | — | `[{id, global_id}]` (also shown in a dialog) | No | UI-RIBCAT-021 |
| `selection.by_id` | `ids` (internal ids or IFC GlobalIds) | `[id]` (selects and zooms) | No | UI-RIBCAT-021 |
| `tempdim.list` | `element_id` | `[TempDim]` | No | UI-CTRL-001 |
| `tempdim.set_value` | `element_id`, `index`, `value` | `()` (delegates to owner move/resize) | Yes | UI-CTRL-001 |
| `tempdim.make_permanent` | `element_id`, `index` | `dimension_id` | Yes | UI-CTRL-001 |
| `tempdim.edit_witness` | `element_id`, `index`, `reference?` | `()` | No | UI-CTRL-001 |
| `ui.controls.activate` | `on?: bool` | `bool` | No | UI-CTRL-001 |
| `ui.temp_dim.settings` | — (dialog) or `patch` | `()` | Yes (model setting) | UI-CTRL-001 |
| `ui.flip_selection` | — | `()` (delegates to owner flip/rotate) | Yes | UI-CTRL-003 |
| `ui.gizmo.toggle_orientation` | — | `global\|local` | No | UI-CTRL-005 |
| `snap.settings.get` | — | `SnapSettings` | No | UI-SNAP-001 |
| `snap.settings.set` | `patch` | `()` | No | UI-SNAP-001 |
| `snap.settings_dialog` | — | `()` | No | UI-SNAP-001 |
| `snap.override_next` | `kind` | `()` | No | UI-SNAP-001 |
| `snap.query` | `view_id`, `point`, `tolerance_px?` | `[SnapCandidate]` | No | UI-SNAP-001 |
| `measure.between_references` | `ref_a`, `ref_b` (interactive when omitted) | `{distance, delta}` | No | UI-RIBCAT-023 |
| `measure.along_element` | `element_id` (interactive when omitted) | `{length}` | No | UI-RIBCAT-023 |
| `ui.context_menu.items` | `target_descriptor` | `[MenuItem]` | No | UI-MENU-001 |
| `ui.keytips.show` / `ui.keytips.hide` | — | `()` | No | UI-TIP-003 |
| `ui.shortcuts.dialog` | `filter?` | `()` | No | UI-KEY-002 |
| `ui.shortcuts.list` | `filter?` | `[Binding]` | No | UI-KEY-002 |
| `ui.shortcuts.assign` | `command_id`, `keys`, `scope?` | `[conflict]` | No | UI-KEY-002 |
| `ui.shortcuts.remove` | `command_id`, `keys` | `()` | No | UI-KEY-002 |
| `ui.shortcuts.import` | `path`, `mode: merge|replace` | `report` | No | UI-KEY-002 |
| `ui.shortcuts.export` | `path` | `()` | No | UI-KEY-002 |
| `ui.shortcuts.reset` | — | `()` | No | UI-KEY-002 |
| `ui.shortcuts.resolve` | `keys`, `scope?` | `[{command_id, exact, available}]` | No | UI-KEY-004 |
| `ui.options.open` | `page?`, `setting?` | `()` | No | UI-PREF-001 |
| `ui.options.get` | `key` | `value` | No | UI-PREF-001 |
| `ui.options.set` | `key`, `value` | `()` | No | UI-PREF-001 |
| `ui.options.list` | `prefix?` | `[OptionInfo]` | No | UI-PREF-001 |
| `ui.options.reset` | `prefix?` | `()` | No | UI-PREF-001 |
| `ui.theme.get` | — | `{ui, canvas, effective_ui}` | No | UI-THEME-001 |
| `ui.theme.set` | `light|dark|system|<custom>` | `()` | No | UI-THEME-001 |
| `ui.theme.list` | — | `[name]` | No | UI-THEME-001 |
| `ui.canvas_theme.set` | `light|dark` | `()` | No | UI-THEME-001 |
| `ui.canvas_theme.toggle` | — | `light|dark` | No | UI-THEME-001 |
| `ui.thin_lines.toggle` | — | `bool` | No | UI-RIBCAT-020 |
| `ui.get_state` | — | `UiState` | No | UI-API-002 |
| `ui.get_active_view` | — | `{view_id, name, kind, document_id, window_id}` | No | UI-API-002 |
| `ui.get_active_document` | — | `{document_id, path|uri, schema, modified}` | No | UI-API-002 |
| `ui.screenshot` | `view_id?`, `width?`, `height?`, `overlays?` | `PNG` | No | UI-API-002 |
| `ui.notify` | `message`, `level`, `actions?` | `notification_id` | No | UI-API-004 |
| `ui.pick.point` | `prompt`, `view_id?`, `snap?` | `Point3` or cancelled | No | UI-API-005 |
| `ui.pick.elements` | `prompt`, `filter?`, `multiple?` | `[id]` or cancelled | No | UI-API-005 |
| `ui.pick.box` | `prompt` | `Rect` or cancelled | No | UI-API-005 |
| `ui.prompt.text` | `title`, `label`, `default?` | `string` or cancelled | No | UI-API-005 |
| `ui.prompt.choice` | `title`, `options[]` | `index` or cancelled | No | UI-API-005 |
| `ui.a11y.summary` | — | `{view, selection, tool, prompt, modes[]}` | No | UI-A11Y-003 |
| `ui.notifications.list` | `filter?` | `[Notification]` | No | UI-SRV-005 |
| `ui.notifications.dismiss` | `id|"all"` | `()` | No | UI-SRV-005 |
| `ui.notifications.act` | `id`, `action` | `()` | No | UI-SRV-005 |
| `ui.server.login_dialog` | `connection_id?` | `()` (never returns credentials) | No | UI-SRV-001 |
| `ui.server.checkin_dialog` | `comment?` | `()` (OK submits `bimserver.check_in`) | No | UI-SRV-003 |
| `ui.server.browser` | `connection_id?` | `()` | No | UI-SRV-002 |
| `ui.server.revisions` | `project?` | `()` | No | UI-SRV-006 |
| `agent.conversation.new` | `provider?`, `model?` (Lua only; not exposed to MCP) | `conversation_id` | No | UI-AGENT-002 |
| `agent.send` | `conversation_id`, `text`, `attachments?` (Lua only) | `()` | No | UI-AGENT-002 |
| `agent.stop` | `conversation_id` (Lua only) | `()` | No | UI-AGENT-002 |
| `agent.approve` / `agent.reject` | `call_id` (UI only) | `()` | No | UI-AGENT-003 |
| `terminal.open` | `preset_id?`, `cwd?` (Lua and UI only) | `tab_id` | No | UI-AGENT-004 |
| `terminal.list` | — (Lua and UI only) | `tabs[]` | No | UI-AGENT-004 |
| `terminal.close` | `tab_id` (Lua and UI only) | `()` | No | UI-AGENT-004 |
| `terminal.send_text` | `tab_id`, `text` (Lua and UI only) | `()` | No | UI-AGENT-004 |
| `agent_preset.list` / `agent_preset.set` / `agent_preset.delete` | `preset` / `preset_id` (Lua and UI only) | `presets[]` / `()` | No | UI-AGENT-004 |
| `agent.undo_turn` | `conversation_id`, `turn_id` (Lua only) | `()` | Yes (undoes the turn's group) | UI-AGENT-003 |
| `agent.history` | `filter?` (Lua only) | `[ConversationSummary]` | No | UI-AGENT-002 |
| `ui.layout.list` | `group?` (builtin, org, personal) | `[LayoutInfo]` | No | UI-LAYOUT-001/002 |
| `ui.layout.save` | name, parts[], slot?, icon_color? | layout_id | No | UI-LAYOUT-001 |
| `ui.layout.apply` | layout_id \| name \| slot, parts? (subset) | report (skipped panes, overridden parts) | No | UI-LAYOUT-001 |
| `ui.layout.update` | layout_id | status | No | UI-LAYOUT-001 |
| `ui.layout.switch_back` | — | status | No | UI-LAYOUT-001 |
| `ui.layout.rename` | layout_id, name | status | No | UI-LAYOUT-001 |
| `ui.layout.duplicate` | layout_id, name | layout_id | No | UI-LAYOUT-002 |
| `ui.layout.delete` | layout_id | status | No | UI-LAYOUT-001 |
| `ui.layout.set_startup` | `last_used` \| layout_id \| `ask` | status | No | UI-LAYOUT-002 |
| `ui.layout.save_display_variant` | layout_id | status | No | UI-LAYOUT-003 |
| `ui.layout.rules.list` / `ui.layout.rules.set` | — / rules[] (context, target: layout_id \| scenario_id, order, enabled) | rules[] / status | No | UI-LAYOUT-004 |
| `ui.layout.auto_switch` | enabled | status | No | UI-LAYOUT-004 |
| `ui.layout.export` / `ui.layout.import` | layout_id, path / path, on_clash | status / layout_id | No | UI-LAYOUT-005 |
| `ui.profile.export` | path, stores[] | status | No | UI-LAYOUT-005 |
| `ui.profile.import` | path, mode: `merge`/`replace`, preview? | ChangeSummary | No | UI-LAYOUT-005 |
| `ui.profile.set_sync_folder` | path \| null | status | No | UI-LAYOUT-005 |
| `ui.layout.recommend` | layout_id, remove? | status | Yes (RXD) | UI-LAYOUT-006 |
| `ui.layout.list_recommended` | — | `[LayoutInfo]` | No | UI-LAYOUT-006 |
| `ui.scenario.list` | `group?` | `[ScenarioInfo]` | No | UI-SCEN-001 |
| `ui.scenario.apply` | scenario_id \| name \| slot | report (applied, locked, skipped) | No | UI-SCEN-001 |
| `ui.scenario.switch_back` | — | status | No | UI-SCEN-001 |
| `ui.scenario.capture` | name, include: [layout, prefs, shortcuts, open_views] | ScenarioDraft (diff for review) | No | UI-SCEN-001 |
| `ui.scenario.save` | ScenarioDef | scenario_id | No | UI-SCEN-001/004 |
| `ui.scenario.update` | scenario_id, from_current? | status | No | UI-SCEN-001 |
| `ui.scenario.set_pref` | key, value, target: `scenario`/`base` | status | No | UI-SCEN-001 |
| `ui.scenario.manage` | scenario_id? | `()` (opens Manage workspaces) | No | UI-SCEN-004 |
| `ui.scenario.diff` | scenario_id | `[PrefDiff]` | No | UI-SCEN-004 |
| `ui.scenario.duplicate` | scenario_id, name | scenario_id | No | UI-SCEN-004 |
| `ui.scenario.delete` | scenario_id | status | No | UI-SCEN-004 |
| `ui.scenario.export` | scenario_id, path | status | No | UI-SCEN-004 |
| `ui.scenario.import` | path, on_clash | scenario_id | No | UI-SCEN-004 |
| `ui.scenario.set_project_default` | scenario_id \| null | status | No | UI-SCEN-003 |
| `ui.scenario.recommend` | scenario_id, remove? | status | Yes (RXD) | UI-SCEN-004 |
| `view.bookmark.save` | name, view_id?, include: [camera, section_box, temp_hide, visual_style, selection_set] | bookmark_id | No | UI-SCEN-002 |
| `view.bookmark.apply` | bookmark_id | status | No | UI-SCEN-002 |
| `view.bookmark.list` | view_id?, scope: `personal`/`shared`/`all` | `[ViewBookmark]` | No | UI-SCEN-002 |
| `view.bookmark.delete` | bookmark_id | status | No | UI-SCEN-002 |
| `view.bookmark.share` | bookmark_id | status | Yes (RXD) | UI-SCEN-002 |
| `view.bookmark.retarget` | bookmark_id, view_id | status | No | UI-SCEN-002 |
| `view.bookmark.export_bcf` | bookmark_id, path | status | No | UI-SCEN-002 |
| `view.bookmark.import_bcf` | path | bookmark_id | No | UI-SCEN-002 |

Notes:
- `agent.approve` / `agent.reject` are deliberately **UI-only**: an agent must never approve its own tool calls.
- `agent.*` commands are not exposed over MCP to prevent agents from driving other agents through Reviera.

---

## 1.35 Open questions / deviations from reference

### 1.35.1 Intentional deviations

| # | Reference behaviour | Reviera behaviour | Reason |
|---|---|---|---|
| D1 | Options Bar row below the ribbon (removed in the reference's 2027 release) | No Options Bar; tool options rendered as contextual ribbon panels and a Properties "Tool Options" group from one descriptor; optional strip rendering (P2) | Matches the reference's 2027 direction while keeping one data model; addresses users who prefer a strip |
| D2 | QAT embedded in a custom-drawn title bar | QAT in a slim strip above the ribbon; native window decorations kept | Cross-platform correctness (Wayland, macOS) and accessibility of native title bars |
| D3 | Title-bar online-help search box, vendor sign-in and app store | Command Search (commands, views, files, offline help, settings); local user name or server login; no store | Offline-first, no vendor accounts, faster command discovery |
| D4 | Vendor cloud items (cloud models, cloud rendering, cloud parameters, shared views, insights page, cloud documents on Home) | Not provided (P3). Open alternatives: BIMserver mode, local rendering, shared parameters files, BCF | Vendor-cloud features are out of scope (Ch. 0) |
| D5 | Proprietary AI assistant palette | Built-in **Agent pane**: MCP client to Reviera's own MCP server, user-chosen model provider (local or hosted), explicit approvals, per-turn undo | Open, vendor-neutral alternative; same command surface as every other client |
| D6 | Separate Precast tab plus reinforcement on Structure (later gathered on a Concrete Detailing tab) | One **Concrete** tab from the start | Follows the reference's newest grouping; avoids duplicated tools |
| D7 | Worksharing central/local files, sync with central | BIMserver connect / check in / get latest / sync; Collaboration Display instead of worksharing display | Collaboration model is BIMserver (TECH_STACK §3.7) |
| D8 | Some default key sequences bound to two commands | Default map has no global-scope conflicts (RC, RG, PC, SZ resolved; SE, ME, ST kept for snaps/MEP) | Predictable behaviour; users can still create duplicates |
| D9 | Shortcuts stored in a vendor XML file; UI state in a binary file | Human-readable TOML stores, independently resettable; no import of the reference's shortcut file format is specified | Clean-room rule (no reproduction of vendor files); transparency |
| D10 | Single-key-sequence execution only on final key | Explicit prefix rule with configurable timeout | Needed once users define overlapping sequences |
| D11 | Dark theme leaves some dialogs light (fixed late in the reference) | Every dialog follows the UI theme from the first release; high-contrast themes shipped | Consistency and accessibility |
| D12 | Selection-colour and view-mode states signalled mainly by colour | Mode borders always carry text labels; colour-vision-safe palette option | Accessibility (§1.33.2) |
| D13 | Macros and visual programming panels | Scripting panel (Script Manager, Lua Console, Script Security, Add-in Manager); node-based programming deferred (P3) | Lua is the scripting platform (TECH_STACK §3.5) |
| D14 | Project-scoped UI data inside the proprietary model file | Stored as Reviera Extension Data (embedded in the IFC by default, optional sidecar); presentational state kept per user in BIMserver mode | IFC is the source of truth; avoid users overwriting each other's layouts |
| D15 | Ctrl+A unused in the canvas | Ctrl+A selects all visible elements in the active view | Common platform expectation |
| D16 | No coordinate entry | Optional typed coordinate entry for picks (P1) | Keyboard accessibility |

### 1.35.2 Open questions

| # | Question | Proposed handling |
|---|---|---|
| Q1 | ~~Rust↔wxWidgets binding~~ | **Resolved** (owner, 2026-10-12): `wxDragon`, with any extensions kept as clean patches offered upstream (TECH_STACK §3.4). The UI bridge interface (§1.1.5) is kept. |
| Q2 | `wxRibbonBar` lacks native support for contextual-tab colouring beyond page highlight, slide-out pinning, panel floating and four minimise states | Prototype with a custom art provider; fall back to a Reviera-owned ribbon widget if `wxRibbonBar` cannot be extended cleanly |
| Q3 | ~~Command ids for tools owned by Ch. 2–6 in §1.8~~ | **Resolved** (2026-10-12): every ribbon and shortcut command id now matches the owning chapter's catalog; `tools/build_spec.py` checks this. |
| Q4 | ~~Owner of phasing and design-option commands~~ | **Resolved**: `phase.*` belongs to Ch. 2 (DM), `designoption.*` to Ch. 5 (DOC, §5.11), and `element.demolish` to Ch. 3 (ARC-MOD-024). |
| Q5 | ~~Clean IFC export and `file.locate_rxd`~~ | **Resolved**: both are defined in Ch. 2a (§4, §13); `export.ifc` has one aligned signature across chapters (Appendix A.2). |
| Q6 | ~~Whether navigation marks the model modified~~ | **Resolved**: no. Navigation is view state, saved with the next save; explicit `view.set_camera` is an undoable edit (§1.19.1, UI-NAV-001). |
| Q7 | Push notifications depend on the BIMserver notification interface being reachable through proxies | Polling fallback specified; the target server is the latest stable BIMserver, currently 1.5.188 (TECH_STACK §5, decision 7) |
| Q8 | Right-to-left UI support | Not required for v1.0; revisit for P2 *Deferred to the post-Alpha/POC review (owner, 2026-10-12).* |
| Q9 | Agent pane provider connectors: which chat APIs ship in-tree, and their licences (must be GPL-3.0-compatible) | Decide with the dependency allow-list (TECH_STACK §4.2) *Deferred to the post-Alpha/POC review (owner, 2026-10-12).* |
| Q10 | Whether to offer a one-time importer for users' own shortcut exports from other applications | Excluded from this specification pending a clean-room review *Deferred to the post-Alpha/POC review (owner, 2026-10-12).* |

---

- **Deviation: bookmarked layouts (owner directive, 2026-10-12).** The reference keeps one UI state per user. Reviera adds named, partial, display-aware layout bookmarks; built-in, organisation and personal layouts; context-based auto-switching; portable preference profiles; and recommended project layouts (§1.29.4, UI-LAYOUT-001…006).
- **Deviation: workflow scenarios and view bookmarks (owner directive, 2026-10-12).** Reviera treats the workspace as a high-level, user-owned customisation of how each person works. Scenarios bundle a layout with preference, shortcut and view-bookmark overlays for each work process, and switch on demand or by context (§1.29.5, UI-SCEN-001…004). The reference has no equivalent beyond one global set of options.

## 1.36 Traceability

| Research section (`research/01_ui_shell.md`) | Used in |
|---|---|
| §1 Overall layout | §1.2 (UI-LAYOUT-007), §1.1.5 |
| §2, §2.1, §2.2 Home | §1.3 (UI-HOME-001), §1.32.2 |
| §3 File tab | §1.4 (UI-FILE-001) |
| §4 Quick Access Toolbar | §1.5 (UI-QAT-001) |
| §5 Title bar, search, sign-in, help, assistant | §1.6 (UI-SEARCH-001, UI-HELP-001, UI-IDENT-001), §1.31 (UI-AGENT-001…003) |
| §6.1–6.7 Ribbon behaviours | §1.7 (UI-RIB-001…008) |
| §7.0–7.14 Ribbon tab by tab | §1.8 (UI-RIBCAT-001…023), §1.26.5 |
| §8 Contextual tabs | §1.9 (UI-CTX-001…004) |
| §9 Options Bar | §1.1.4 (UI-ARCH-004), §1.10 (UI-OPT-001), §1.35 D1 |
| §10, §10.1 Properties palette and Type Selector | §1.11 (UI-PROP-001, UI-PROP-002), §1.12 (UI-TYPE-001) |
| §11 Project Browser and Browser Organization | §1.13 (UI-PB-001, UI-PB-002) |
| §12 System Browser | §1.14 (UI-SB-001) |
| §13 Dockable windows, tabs, tiling, multi-monitor | §1.15 (UI-DOCK-001, UI-DOCK-004), §1.16 (UI-VIEWWIN-001…003) |
| §14 Drawing area | §1.17 (UI-CANVAS-001, UI-CANVAS-002) |
| §15 View Control Bar | §1.18 (UI-VCB-001) |
| §16.1–16.4 Navigation | §1.19 (UI-NAV-001…004) |
| §17 Status bar | §1.20 (UI-STATUS-001), §1.32.5 |
| §18.1–18.5 Selection | §1.21 (UI-SEL-001…005) |
| §19.1–19.5 In-canvas controls | §1.22 (UI-CTRL-001…005) |
| §20 Snaps and alignment | §1.23 (UI-SNAP-001) |
| §21 Context menus | §1.24 (UI-MENU-001) |
| §22 Tooltips and keytips | §1.25 (UI-TIP-001…003) |
| §23.1–23.5 Keyboard shortcuts | §1.26 (UI-KEY-001…005) |
| §24.1–24.12 Options dialog | §1.27 (UI-PREF-001, UI-PREF-002) |
| §25 Themes | §1.28 (UI-THEME-001), §1.17.2 |
| §26 UI persistence | §1.29 (UI-PERSIST-001…003, UI-LAYOUT-001…006, UI-SCEN-001…004) |
| §6.7, §24.2 Discipline check boxes and their lock | §1.7.7 (UI-RIB-007, `ui.disciplines`), §1.29.3 (UI-PERSIST-003) |
| Owner directive 2026-10-12 (workspaces) | §1.29.4, §1.29.5 (UI-LAYOUT-001…006, UI-SCEN-001…004) |
| Chapter 6 command catalog (`bimserver.*`, `ownership.*`, `editrequest.*`) | §1.32.3, §1.32.6, §1.32.7 (UI-SRV-003, UI-SRV-006, UI-SRV-007) |
| §27 Implications | §1.0.3, §1.1 (UI-ARCH-001…006) |
| §28 Image index | Screenshot citations throughout (internal reference only) |
| `TECH_STACK.md` §2, §3.3–3.7, §4.2, §5 | §1.1, §1.29, §1.30, §1.31, §1.32, §1.35 |

Screenshots cited (internal reference only, never shipped): `ui_overview_annotated.png`, `ui_insert_tab_floor_plan.png`, `home_classic.png`, `home_new_recent.png`, `home_new_projects.png`, `file_tab_menu.png`, `quick_access_toolbar.png`, `qat_customize_dropdown.png`, `infocenter_search.png`, `help_menu.png`, `revit2027_autodesk_assistant_button.png`, `revit2027_autodesk_assistant_panel.png`, `ribbon_contextual_tab.png`, `ribbon_insert_tab_legacy.png`, `ribbon_expanded_panel.png`, `ribbon_expanded_panel_pinned.png`, `ribbon_dialog_launcher.png`, `view_tab_windows_panel_canvas_theme.png`, `options_bar.png`, `revit2027_options_bar_removed.png`, `properties_palette_annotated.png`, `properties_filter_dropdown.png`, `type_selector_search.png`, `project_browser_search.gif`, `dockable_windows_grouped.png`, `tab_views.png`, `tile_views.gif`, `multi_monitor_views.png`, `view_control_bar.gif`, `temporary_hide_isolate.gif`, `viewcube.png`, `navigation_bar.png`, `steeringwheel_full_navigation.png`, `steeringwheel_2d.png`, `status_bar.png`, `tooltip_ribbon.png`, `keytips.png`, `options_colors_tab_theme.png`, `dark_theme_full_ui.png`, `dark_theme_black_canvas_2023.png` (all under `screenshots/ui/`).
