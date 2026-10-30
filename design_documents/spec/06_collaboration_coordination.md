# Chapter 6 — Collaboration, Linking and Coordination

**Area code:** COL · **Status:** Draft 1 (2026-10-12) · **Primary source:** `research/04_documentation_collaboration.md` (cited as `research/04 §n`)
**Related chapters:** Chapter 1 (UI shell, status bar, notifications), Chapter 2 (data model, file I/O), Chapter 2a (Reviera Extension Data, RXD: `02a_extension_data_format.md`), Chapter 5 (views, graphics resolver, output), TECH_STACK §3.7 (Local and BIMserver modes).
**Reference:** Autodesk Revit 2026/2027 worksharing, linking, positioning and coordination features, used as the functional reference only.

This chapter specifies how several people and several models work together in Reviera. It covers the two operating modes (Local and Open Source BIMserver client), the mapping of the reference product's worksharing concepts onto BIMserver, the merge algorithm, advisory ownership, worksets, offline working copies, linked models and underlays, project positioning and coordinate systems, coordination tools (copy/monitor, coordination review, clash detection, BCF issues) and model maintenance.

**Storage rule for this chapter.** As set out in Chapter 2a, everything IFC entities can express stays in IFC entities. Collaboration data that IFC cannot express (worksets, link registry, positioning extras, monitor pairs, clash tests, issues) is Reviera Extension Data (RXD), embedded in the IFC file by default, and named here as "RXD document `<name>`". The single exception is **live ownership and lock state**, which must change without creating model revisions; in BIMserver mode it travels as BIMserver extended data on the latest revision (COL-OWN-002). This chapter does not define the RXD carrier; Chapter 2a does.

---

## 6.0 Principles

| ID | Principle | Priority |
|----|-----------|----------|
| COL-CORE-001 | **One editing engine, two stores.** Editing, regeneration, undo, Lua and MCP behave identically in Local and BIMserver mode. Only open, save, synchronise, history and ownership differ, and they sit behind the `ModelStore` trait (TECH_STACK §3.7). No command has a mode-specific signature: a command that needs the server fails in Local mode with `mode.not_available` and changes nothing (COL-MODE-001). Undo history is local to the session in both modes; it is never shared through the server and is cleared by *Get latest* only for the steps that the merge invalidated (the user is told how many). | P0 |
| COL-CORE-002 | **GlobalId is identity.** Every rooted IFC object keeps its GlobalId for life. Copies get new GlobalIds. Merges, links, monitors, clashes and BCF issues all refer to elements by GlobalId; RXD objects are referred to by their stable ids. Moving an element to another storey, workset or type never changes its GlobalId. An imported file with duplicate or malformed GlobalIds is repaired by *Audit* (COL-MNT-002) before it can be published to BIMserver; publishing is refused with `model.duplicate_globalid` until then. References into links use `link:<linkId>/ifc:<GlobalId>` (Chapter 2a). | P0 |
| COL-CORE-003 | **Never lose work silently.** Every merge, conflict decision and ownership override is recorded in a sync report that the user can reopen (Collaborate › Show History › *Sync reports*; the last 50 are kept in the working copy). Changes that lose a conflict are kept in a recoverable *conflict stash* (COL-MRG-002) until the user discards them. Destructive actions on shared state (*Discard local changes*, *Force relinquish*, *Discard working copy*, *Unload for everyone*) need explicit confirmation naming what will be lost, and MCP agents may call them only with the model-edit permission and an explicit `confirm: true`. | P1 |
| COL-CORE-004 | **Offline first.** In BIMserver mode a local working copy is always kept, so users can open, edit and save without a connection (COL-OFF-001). Losing the connection never interrupts an edit; only *Synchronise*, *Get latest*, link reloads from the server and new borrows become queued or disabled. Reconnection is automatic and never merges without the user's go-ahead. | P1 |
| COL-CORE-005 | **Credentials never leak.** Server passwords and tokens (BIMserver, BCF API, OpenCDE and other server profiles, COL-API-001) are stored in the operating system's credential store. They are never written to RXD, IFC, the working-copy folder, logs, crash reports, sync reports, Lua-visible tables or MCP resources; log lines that would contain a token show `<redacted>`. MCP tools never accept a password or token as a parameter (COL-BIM-001). Signing out removes the stored token. | P0 |

**Acceptance criteria (principles).**
- [ ] The same Lua script that creates and tags 100 doors produces identical IFC and RXD content in Local mode and in BIMserver mode (apart from the binding of the file to its server).
- [ ] Searching the saved IFC file, the working-copy folder and the log files for the server password finds nothing.
- [ ] `bimserver.get_latest` called in Local mode returns `mode.not_available` and leaves the undo stack unchanged.
- [ ] Publishing a model that contains two objects with the same GlobalId is refused with `model.duplicate_globalid`.

---

## 6.1 Operating modes

### Model stores and mode selection
**ID:** COL-MODE-001 · **Priority:** P0 (Local), P1 (BIMserver) · **Ref:** TECH_STACK §3.7; research/04 §7.1, screenshots/documentation/worksharing_central_local_diagram.png

**Purpose.** Let the same application work on a plain IFC file or on a shared BIMserver project.

**Workflow.**
1. File › Open › IFC file opens a project in **Local** mode.
2. File › Open › From BIMserver… opens the server browser (COL-BIM-002) and opens a project in **BIMserver** mode.
3. The title bar and status bar show the mode: the file path in Local mode; the server, project name, base revision number and connection state in BIMserver mode.

**Behaviour & rules.**

| Capability | Local mode (`LocalStore`) | BIMserver mode (`BimServerStore`) |
|---|---|---|
| Storage | `.ifc` or `.ifczip` with embedded RXD | BIMserver project; the client keeps a working copy (COL-OFF-001) |
| Save (Ctrl+S) | Writes the file atomically (Chapter 2a), with rotating backups (COL-MNT-003) | Writes the local working copy only; nothing is sent to the server |
| Publish changes | — | *Synchronise* (COL-BIM-006) |
| Get others' changes | — | *Get latest* (COL-BIM-005) |
| History | Local backups | Server revision list (COL-BIM-008) |
| Ownership | Not used (single editor) | Advisory ownership (§6.4) |
| Worksets | Available (organisation, visibility, open/close) | Available, plus ownership by workset |
| Equivalent in the reference | A standalone, non-workshared model | A workshared central model with local copies |

- The `ModelStore` trait has the operations `open`, `save_working`, `get_latest`, `check_in`, `history`, `download_revision`, `ownership` (a sub-interface, null in Local mode), and `subscribe` (change notifications, null in Local mode).
- Commands that only make sense in BIMserver mode (for example `bimserver.sync`) fail in Local mode with `mode.not_available`, and their ribbon buttons are disabled.
- **Shared-folder protection (Local mode):** when a Local project is opened, Reviera creates a lock file `<name>.ifc.lock` next to it, holding user, host, process id and time. If another user opens the same file, they are told who has it open and can open it read-only or as a copy. Stale locks (the process no longer exists on the same host, or older than 24 hours on another host) can be broken after confirmation. This is protection only; real multi-user editing is BIMserver mode.

**Data (IFC/RXD).** In BIMserver mode, the server binding (server URL, project id `poid`, base revision id `roid`) is client state in the working-copy folder (COL-OFF-001), not project data; a project file moved to another machine does not carry a server binding.

**Commands.** `model.open_file(path, read_only?) -> model_id`; `model.mode() -> {mode, server?, poid?, base_roid?, connected?}` (query)

**Acceptance criteria.**
- [ ] A second user opening a Local project already open elsewhere is warned with the first user's name and can open it read-only.
- [ ] `bimserver.sync` in Local mode returns `mode.not_available` and changes nothing.

*Built (2026-10-29), in part:* a document is in BIMserver mode when its file is a working copy (`working.ifc` beside its `state.json`, COL-OFF-001); `model.mode(document_id?) -> {mode, path, working_copy?, connected}`; `bimserver.check_in` in Local mode fails with `mode.not_available`. Not yet: the title-bar and status-bar mode display, the `ModelStore` trait over both stores, the shared-folder lock file, `model.open_file`.

### Moving between modes
**ID:** COL-MODE-002 · **Priority:** P1 · **Ref:** TECH_STACK §3.7 (*Moving between modes*); research/04 §7.2, §7.4 (*Detach from central*)

**Purpose.** Start a project alone and share it later, or take a shared project out of the server.

**Workflow.**
1. **Publish to BIMserver** (Collaborate › Share › Publish to BIMserver): log in, choose or create a project (or sub-project), enter a comment; Reviera checks in the current model as the project's first revision and switches to BIMserver mode with that revision as base.
2. **Export local copy** (Collaborate › Share › Export Local Copy): writes the current working copy as a standalone `.ifc`. Options: *Keep worksets* or *Discard worksets* (all elements move to no workset and the workset definitions are removed); ownership data is never included. This is the counterpart of "detach from central" and is used for archives and consultant issues.

**Behaviour & rules.**
- Publishing to a project that already has revisions is refused unless the user chooses *Replace* (which checks in a new revision and keeps the history) after a confirmation naming the project.
- The schema of the server project must match the model schema (IFC4 by default; IFC4X3 only if the server offers a matching deserialiser, TECH_STACK §3.7.3). On mismatch Reviera offers to convert the model (Chapter 5, DOC-OUT-006 conversion) or to create a new project with the right schema.

**Commands.** `bimserver.publish_local(project: {poid}|{new_name, parent_poid?}, comment, replace?) -> {poid, roid}`; `model.export_local_copy(path, keep_worksets?) -> ()`

**Acceptance criteria.**
- [ ] Publishing a Local project creates a server project whose first revision, downloaded by another user, opens with all views, sheets and annotation.
- [ ] Exporting a local copy with *Discard worksets* produces a file that opens in Local mode with no worksets and no ownership data.

*Built (2026-10-29), Publish:* `bimserver.publish_local(server_id, poid? | new_name + parent_poid?, comment, replace?, document_id?) -> {poid, roid, revision, warnings, document_id}`: the model is saved as the project's working copy and checked in; a project with revisions needs `replace`; the schemas must match (IFC4 or IFC2X3; IFC4X3 is refused, no conversion offered yet). Not yet: *Export local copy* and the menu entries.

---

## 6.2 BIMserver client

All calls use the BIMserver JSON API as described in TECH_STACK §3.7.1: an HTTP `POST` to `https://<host>[:port]/<context>/json` with a body of the form `{"token": "...", "request": {"interface": "...", "method": "...", "parameters": {...}}}`, or several calls batched in a `requests` array. The client is asynchronous (`reqwest` + `serde` on `tokio`), so the UI never waits on the network. Interface and method names below are those of BIMserver 1.5 as published in its source; **every name marked (verify) must be checked against the minimum supported server version** (TECH_STACK §5, open decision 7) before implementation, and the client must look the methods up through the server's service metadata at connect time, failing with a clear message if one is missing.

The JSON bodies below are illustrative. Tokens, ids and values are placeholders.

### Server profiles and login
**ID:** COL-BIM-001 · **Priority:** P1 · **Ref:** TECH_STACK §3.7.1

**Purpose.** Connect to one or more BIMserver instances securely.

**Workflow.**
1. Collaborate › BIMserver › Servers…: add a server profile (name, URL including context path, optional client certificate, *Verify TLS certificates* on by default).
2. Log in: user name and password; *Remember me* stores the password, or the token if the server allows long-lived tokens, in the OS credential store (Windows Credential Manager, macOS Keychain, Secret Service on Linux).
3. The status bar shows the connection state (connected, reconnecting, offline).

**Behaviour & rules.**
- **Login flow:**
  ```json
  POST /json
  {"request": {"interface": "AuthInterface", "method": "login",
               "parameters": {"username": "anna@example.org", "password": "••••"}}}
  → {"response": {"result": "<token>"}}
  ```
  The token is attached to every later call. `AuthInterface.getLoggedInUser` returns the user's server identity (user id and name), which Reviera uses as the **owner identity** for ownership (§6.4). Logout calls `AuthInterface.logout`.
- When a call fails because the token expired, the client logs in again silently if credentials are stored, otherwise it prompts once and retries.
- Plain `http://` URLs are allowed only for `localhost` unless the user explicitly accepts the risk per profile.
- Each profile records the server version and the list of serialisers and deserialisers it offers (looked up at connect time; COL-BIM-003, COL-BIM-004).

**Data.** Server profiles are user preferences (never project data). Secrets live only in the OS credential store.

**Commands.** `bimserver.add_server(name, url, verify_tls?) -> server_id`; `bimserver.remove_server(server_id) -> ()`; `bimserver.login(server_id, username, password?, remember?) -> session_info`; `bimserver.logout(server_id) -> ()`

The MCP tool for `bimserver.login` never accepts a password from an agent; it can only use stored credentials or ask the user interactively (COL-CORE-005).

**Acceptance criteria.**
- [ ] After *Remember me*, restarting Reviera reconnects without a prompt, and the password is not present in any Reviera file.
- [x] An expired token is renewed transparently during a long session.
- [x] Calling the MCP login tool with a password parameter is rejected.

*Built (2026-10-29):* `clients/bimserver` (MIT, `reqwest` with rustls on the platform's certificates, blocking with a 60 s timeout rather than on `tokio`); profiles in the per-user `bimserver.json` (name, URL, user name, login kind, server version); the password or a **user token** (`AuthInterface.loginUserToken`, an addition: BIMserver shows one per user) in the OS keychain under `org.bimreviera.bimserver` (session memory where there is none); `bimserver.add_server(name, url, allow_http?)` asks the server for its version first; `bimserver.login(server_id, username?, password?, user_token?, remember?)`, refused from MCP when it carries a secret; `bimserver.logout(server_id, forget?)`; `bimserver.remove_server` (not offered to agents); `bimserver.list_servers`. A refused session is renewed once from the stored secret. BIMserver answers a wrong password with a `UserException`, taken as an authentication failure. Checked live against BIMserver 1.6.1-SNAPSHOT. Not yet: client certificates and *Verify TLS* off, the status-bar connection state, the Servers dialog, and the method lookup through the server's service metadata.

### Project and revision browser
**ID:** COL-BIM-002 · **Priority:** P1 · **Ref:** TECH_STACK §3.7 (*Open*)

**Purpose.** Find and open a project or a specific revision.

**Workflow.** File › Open › From BIMserver… opens a dialog with the server list on the left, a project tree (projects and sub-projects) in the middle, and the selected project's revisions on the right (number, date, author, comment, size). Buttons: Open latest, Open revision, New project, New sub-project, Refresh.

**Behaviour & rules.**
- **Project list:**
  ```json
  {"token": "<token>", "request": {"interface": "ServiceInterface", "method": "getAllProjects",
   "parameters": {"onlyTopLevel": false, "onlyActive": true}}}
  ```
  The result gives each project's object id (`oid`, used as `poid`), name, schema, parent project, sub-projects and last revision id. The tree is built from the parent links.
- **Revision list:** `ServiceInterface.getAllRevisionsOfProject {"poid": …}` returns each revision's object id (`roid`), sequence number, date, comment, user and size. User names are resolved with `ServiceInterface.getUserByUoid` (verify) and cached.
- **New project:** `ServiceInterface.addProject {"projectName": "...", "schema": "ifc4"}`; sub-project: `ServiceInterface.addProjectAsSubProject {"projectName": "...", "parentPoid": …, "schema": "ifc4"}`.
- Sub-projects are offered as a way to split a large job by discipline or building; each sub-project is a separate model that the others can link (COL-LNK-001).
- Revisions whose check-in comment starts with `[reviera:ownership]` or `[reviera:restore]` are labelled accordingly.

**Commands.** `bimserver.list_projects(server_id) -> project_summary[]` (query); `bimserver.list_revisions(server_id, poid) -> revision_summary[]` (query); `bimserver.create_project(server_id, name, schema?, parent_poid?) -> poid`

**Acceptance criteria.**
- [ ] The browser lists projects and sub-projects as a tree matching the server's structure.
- [ ] Creating a sub-project under "Hospital" shows it nested under that project immediately.

*Built (2026-10-29):* `bimserver.list_projects` (with `working_copy` marking projects that have one here), `bimserver.list_revisions` (user names resolved and cached per call), `bimserver.create_project`. *Since (2026-10-29):* File › BIMserver… (`ui.bimserver`): the server choice (with its connection state), the chosen server's projects (schema, revision count, working copy here) and the chosen project's revisions newest first (number, date, user, first line of the comment); Add Server… (then Log In…), Log In… (user name and password or user token, *Remember in the system keychain*), Log Out (optionally forgetting the secret), Remove Server, Open Latest, Open Revision, Publish Current Project…, Check In…; the title bar shows a working copy's project and base revision; help page *BIMserver projects*. Not yet: the project tree of sub-projects, New project and New sub-project buttons, and the `[reviera:…]` revision labels.

### Open and download
**ID:** COL-BIM-003 · **Priority:** P1 · **Ref:** TECH_STACK §3.7.1 (*Download*)

**Purpose.** Bring a server revision onto the client as a working copy.

**Workflow.** In the browser, choose *Open latest* or select a revision and *Open revision*. A revision that is not the latest opens read-only unless the user chooses *Open as new working copy based on this revision* (which will merge forward on the next sync).

**Behaviour & rules.**
1. **Find the serialiser** for the project's schema: `ServiceInterface.getSerializerByName` (for example the server's IFC4 streaming serialiser) or `ServiceInterface.getSerializerByContentType {"contentType": "application/ifc"}` (verify). Cache its `oid` per server.
2. **Start the download:**
   ```json
   {"token": "<token>", "request": {"interface": "ServiceInterface", "method": "download",
    "parameters": {"roids": [65539], "query": "{}", "serializerOid": 3014707, "sync": false}}}
   → {"response": {"result": <topicId>}}
   ```
3. **Wait for the result:** progress through `NotificationRegistryInterface.getProgress {"topicId": …}` (or WebSocket progress events, COL-BIM-007).
4. **Fetch the data:** HTTP `GET /download?token=…&topicId=…` streams the IFC file. Reviera streams it to disk in the working-copy folder, then parses it with IfcOpenShell. The embedded RXD arrives as part of the IFC (Chapter 2a).
5. **Clean up:** `ServiceInterface.cleanupLongAction {"topicId": …}`.
6. Store the downloaded file as both `base.ifc` (immutable, for three-way merges) and `working.ifc` (COL-OFF-001), and record the base `roid`.
7. Fetch the live ownership document (COL-OWN-002) and subscribe to notifications (COL-BIM-007).
- Downloads resume after a dropped connection when the server supports HTTP range requests; otherwise they restart.
- If the working copy for that project already exists with unsynchronised changes, *Open latest* opens the working copy and offers *Get latest*; it never overwrites local work.

**Commands.** `bimserver.open(server_id, poid, roid?, as_working_copy?) -> model_id`; `bimserver.download_revision(server_id, roid, path) -> ()`

**Acceptance criteria.**
- [ ] Opening a 200 MB project shows download progress, can be cancelled, and leaves no partial working copy after cancel.
- [ ] Re-opening a project with unsynchronised local changes opens the local working copy with those changes intact.

*Built (2026-10-29):* `bimserver.open(server_id, poid, roid?)`: an existing working copy is opened as it is (with `newer_on_server`); otherwise the revision is downloaded (`download` with `sync`, `GET /download`, `cleanupLongAction`) with the IFC4 (or IFC2X3) serialiser into `base.ifc` and `working.ifc` and opened; `bimserver.download_revision(server_id, roid, path, schema?)`. The Reviera recipes come back through the server: a sample published and reopened on a second machine had the same walls and windows, rebuilt from their recipes. Not yet: progress and cancel, resumed downloads, read-only opening of an older revision, the ownership document and notifications.

### Check in
**ID:** COL-BIM-004 · **Priority:** P1 · **Ref:** TECH_STACK §3.7.1 (*Check-in*)

**Purpose.** Upload the working copy as a new server revision. Users normally do this through *Synchronise* (COL-BIM-006), which merges first; a bare check-in is used by synchronise and by *Publish to BIMserver*.

**Behaviour & rules.**
1. Serialise the working copy (with embedded RXD) with IfcOpenShell to a temporary file.
2. **Find the deserialiser:** `ServiceInterface.getSuggestedDeserializerForExtension {"extension": "ifc", "poid": …}`; cache its `oid`.
3. **Upload.** Small models (below a configurable size, default 32 MB):
   ```json
   {"token": "<token>", "request": {"interface": "ServiceInterface", "method": "checkinSync",
    "parameters": {"poid": 131073, "comment": "Level 2 partitions", "deserializerOid": 2949171,
                   "fileSize": 1234567, "fileName": "model.ifc", "data": "<base64>", "merge": false}}}
   ```
   (`checkin` with `"sync": false` returns a topic id instead, and progress is followed as for downloads.) Large models use the server's streamed upload endpoint: start the check-in to obtain a topic id (`ServiceInterface.initiateCheckin`, verify), then `POST /upload` as multipart form data with the token, topic id, deserialiser id, project id and comment (verify the endpoint and its fields).
4. `merge` is always `false`: Reviera uploads a complete, already merged model, and does not use the server's own merge.
5. On success, read the new revision id from the result (or from `getProject`'s last revision id), record it as the new base, and copy `working.ifc` to `base.ifc`.
6. The comment is required (empty comments are refused) and is prefixed with nothing; Reviera appends a machine-readable trailer line with the Reviera version and the RXD format version.

**Commands.** `bimserver.check_in(comment) -> roid` (used internally by sync and publish; exposed for scripts that manage their own merging)

**Acceptance criteria.**
- [ ] A 300 MB model checks in through the streamed endpoint without loading the whole file into memory twice (peak memory below model size + 100 MB).
- [ ] After a successful check-in, the status bar shows the new revision number as base.

*Built (2026-10-29):* `bimserver.check_in(comment, document_id?) -> {poid, roid, revision, warnings, document_id}`: saves the working copy, uploads it with `checkinSync` (base64, `merge: false`) through the deserialiser the server suggests for `.ifc`, then makes it the base; refused while the server's latest revision is not the base (`bimserver.out_of_date`, until synchronise can merge). The comment gets a trailer line `[reviera] BIM Reviera <version>`. Not yet: the streamed upload for large models and the RXD format version in the trailer.

### Get latest
**ID:** COL-BIM-005 · **Priority:** P1 · **Ref:** research/04 §7.1 (*Reload Latest*); TECH_STACK §3.7.2

**Purpose.** Bring other users' changes into the working copy without publishing local changes (the counterpart of "reload latest").

**Workflow.** Collaborate › Synchronise › Get Latest (default shortcut RL).

**Behaviour & rules.**
1. Read the project's last revision id (`ServiceInterface.getProjectByPoid`). If it equals the base, report "Up to date" and stop.
2. Download the latest revision (COL-BIM-003 steps 1–5) to `remote.ifc`.
3. Three-way merge (§6.3) with base = `base.ifc`, local = working copy, remote = `remote.ifc`.
4. Resolve conflicts (COL-MRG-002), regenerate, show the sync report.
5. The new base becomes the downloaded revision; local unsynchronised changes remain local changes on top of it (the merged working copy is saved and `remote.ifc` becomes `base.ifc`).

**Commands.** `bimserver.get_latest() -> sync_report`

**Acceptance criteria.**
- [x] User B gets the 5 walls user A checked in, while B's own unsynchronised door edits stay in B's working copy and are not uploaded.

*Built (2026-10-29):* `bimserver.get_latest(document_id?, conflicts?: fail|local|remote) -> sync_report` (up_to_date, base revision, created/updated/deleted, remote changes, merged elements, conflicts, applied): the newest revision downloaded as `remote.ifc`, merged into the open working copy (COL-MRG-001) as one undoable *Get Latest (revision n)* step; the working copy is saved, `remote.ifc` becomes `base.ifc` and the state's base moves on. With conflicts under `fail` (the default) nothing changes. File › BIMserver… › Get Latest asks, on conflicts, whether to keep the local or take the server's side. Checked live: a revision re-serialised by BIMserver 1.6.1 merges as exactly the other user's change (no spurious updates).

### Synchronise
**ID:** COL-BIM-006 · **Priority:** P1 · **Ref:** research/04 §7.4 · screenshots/documentation/synchronize_with_central_dialog.png

**Purpose.** Publish local changes and receive others' changes in one step (the counterpart of "synchronise with central").

**Workflow.**
1. Collaborate › Synchronise › *Synchronise and modify settings*: dialog with server project (read-only), comment, relinquish options (worksets I own, elements I borrowed; standards, views and families are not special in Reviera, see deviations), *Save working copy before and after*; or *Synchronise now*, which uses the last settings and relinquishes borrowed elements.
2. Progress is shown in the status bar; the user can keep reading the model but edits are blocked during steps 3–7 (they would be lost from the upload).

**Behaviour & rules.**
1. Save the working copy.
2. **Acquire the sync lock** (COL-OWN-002): a short-lived lock entry in the live ownership document, with a time-to-live (default 10 minutes, renewed while the sync is running). If another client holds it, wait and show "Project is being synchronised by <user>"; retry automatically.
3. Get latest and merge as in COL-BIM-005 (skipped if the base is still the latest).
4. If conflicts need user decisions, stop and show the conflict dialog (COL-MRG-002). Cancelling releases the lock and leaves the working copy merged but not uploaded.
5. Regenerate and validate (COL-MRG-003).
6. Check in (COL-BIM-004) with the comment.
7. **Race check:** list revisions again. If a revision other than Reviera's own appeared between the merged remote revision and the new one (someone checked in without respecting the lock, for example another client application), merge again with base = the merged remote revision, local = Reviera's revision, remote = the intruding revision, and check in a corrective revision with comment `[reviera:merge] …`. Report this in the sync report.
8. Relinquish ownership as chosen and release the sync lock (COL-OWN-002).
9. Save the working copy (base = new revision).
- The **sync history** (Collaborate › Show History) lists revisions with time, user and comment (COL-BIM-008).
- **Reminders:** a user option reminds the user to synchronise every N minutes (default 60) when there are unsynchronised changes.
- **Closing the project** with unsynchronised changes or held ownership offers: *Synchronise and relinquish*, *Relinquish without publishing* (local changes stay in the working copy), *Keep ownership* (ownership stays until next session; others are told it is held), *Discard local changes*, *Cancel*.

**Commands.** `bimserver.sync(comment, relinquish?: {worksets?, elements?}, save_before?, save_after?) -> sync_report`; `bimserver.set_sync_reminder(minutes?) -> ()`

**Acceptance criteria.**
- [x] Two users each move different walls and synchronise in turn; afterwards both working copies contain both moves.
- [x] While user A is synchronising, user B's synchronise waits and completes after A's, with B's revision based on A's. *(Refused with the holder named; the user retries.)*
- [x] If a foreign client checks in during Reviera's synchronise, the sync report shows a corrective merge and the final latest revision contains all three users' changes.

*Built (2026-10-29), in part:* `bimserver.sync(comment, document_id?, conflicts?) -> sync_report` (with `checked_in`): save, get latest and merge when the server moved on (stopping at conflicts unless a side is chosen), check in; File › BIMserver… › Synchronise…. *Since (2026-10-29):* the **sync lock** (COL-OWN-002) and the **race check**: `bimserver.sync` takes the lock in the ownership document on the latest revision (10 minutes) and is refused with `bimserver.sync_locked`, naming the holder, while another session holds an unexpired one; after its check-in, revisions that appeared between the merged one and its own are merged in (the local side winning, or the remote under `conflicts: remote`) and checked in as `[reviera:merge] Corrective merge of revision n …`; the new revision starts a fresh, unlocked ownership chain. `bimserver.lock_status`. Where the server will not register the ownership schema the sync runs unlocked and says so (`lock: unavailable`). Not yet: waiting and retrying automatically while locked, renewing the lock during a long sync, relinquishing, *save before/after* options, reminders, and the close-project choices.

### Notifications
**ID:** COL-BIM-007 · **Priority:** P1 · **Ref:** TECH_STACK §3.7.1 (*Notification interfaces*); research/04 §7.5 (update frequency)

**Purpose.** Tell users promptly when the model or ownership changes on the server.

**Behaviour & rules.**
- Reviera opens a WebSocket to the server's notification endpoint (`/stream`, verify), receives an endpoint id, and registers for events on the open project:
  ```json
  {"token": "<token>", "request": {"interface": "NotificationRegistryInterface",
   "method": "registerNewRevisionOnSpecificProjectHandler",
   "parameters": {"endPointId": 7, "poid": 131073}}}
  ```
  and, for ownership changes, the handler for new extended data on the latest revision (`registerNewExtendedDataOnRevisionHandler`, verify).
- Incoming new-revision events (`NotificationInterface.newRevision` with `poid` and `roid`) produce a status-bar balloon "Model updated by <user>: <comment>" with *Get latest* and *Show changes*, and update the *Model updates* display mode (COL-WS-004).
- Incoming ownership events refresh the live ownership document (COL-OWN-002).
- **Fallback:** if the WebSocket cannot be opened or drops, Reviera polls the project's last revision id and the ownership document every 30 seconds (user setting: manual, 5 s, 15 s, 30 s, 60 s), and reconnects with exponential back-off.

**Commands.** `bimserver.subscribe(events[]?) -> ()`; `bimserver.unsubscribe() -> ()`; `bimserver.set_update_frequency(seconds_or_manual) -> ()`

**Acceptance criteria.**
- [ ] When user A checks in, user B sees the notification within 5 seconds with the WebSocket connected, and within the polling interval without it.
- [ ] Unplugging the network and plugging it back re-establishes notifications without user action.

### History, compare and restore

> **Updated 2026-10-12.** Server revisions, local history and shared snapshots now appear in one combined History timeline, and named snapshots can be published to BIMserver without creating a new latest revision. Both are specified in Chapter 2b §8.6 (DM-LIVE-019, DM-LIVE-020). Where the text below conflicts with that section, Chapter 2b takes precedence.
**ID:** COL-BIM-008 · **Priority:** P1 (history), P2 (compare) · **Ref:** research/04 §7.4 (*Show History*, *Restore Backup*)

**Purpose.** Inspect, compare and roll back server revisions.

**Workflow.** Collaborate › Show History lists revisions. Per revision: *Download as file*, *Open read-only*, *Compare with base / with another revision*, *Restore as latest*.

**Behaviour & rules.**
- **Compare** computes the change set between two revisions with the same fingerprinting used by the merge (§6.3): added, deleted, modified elements (with changed attributes and properties), and changed RXD objects (views, sheets, annotation). Results are shown in a changes palette and can be highlighted in views (colour by change kind) and exported as CSV or BCF topics. The server-side `ServiceInterface.compare` (verify) may be used as an accelerator when available, but Reviera's own comparison is authoritative.
- **Restore as latest** downloads the chosen revision and checks it in as a new revision with comment `[reviera:restore] Restored revision n: <reason>`; history is never rewritten. Users with unsynchronised changes are told on their next sync that the model was restored, and their changes are merged onto the restored revision as usual.

**Commands.** `bimserver.history() -> revision_summary[]` (query); `bimserver.compare(roid_a, roid_b) -> change_set` (query); `bimserver.restore(roid, reason) -> roid`

**Acceptance criteria.**
- [ ] Comparing two revisions lists exactly the elements changed between them, including a dimension text change in a view.
- [x] Restoring revision 12 creates revision 20 identical in content to revision 12, and revisions 13–19 remain in the history.

*Built (2026-10-29):* `bimserver.history(document_id?)` (newest first, with Reviera's `restore` label and the working copy's base marked); `bimserver.compare(roid_a?, roid_b?, document_id?) -> {added, deleted, modified}` by the merge's element-store diff (`reviera_core::merge::diff`): modified elements list the facets that changed (class, name, parameters); `roid_a` defaults to the base and `roid_b` to the working copy, so with neither it lists the local changes; `bimserver.restore(roid, reason, document_id?)`: refused unless the working copy is up to date with no local changes; the working copy becomes the revision (one undoable step) and is checked in with `[reviera:restore] Restored revision n: <reason>`. File › BIMserver… › Compare with Working Copy and Restore…. Not yet: the changes palette, highlighting in views, CSV and BCF export, *Open read-only*.

---

## 6.2a FileEngine models

FileEngine (Proxima File Vault) is a versioned document store with a REST door. Reviera keeps models there as files: every save is a new version, and FileEngine decides who may read and write. This is phase 1 of `FILEENGINE_INTEGRATION.md` (§4–5): connections, browsing, open, save and versions. Publishing, BCF, Component Libraries, documentation folders and product data come in later phases. Reviera talks to FileEngine (AGPL-3.0) over HTTP only; its client `clients/fileengine` is MIT.

### Connections and sign-in
**ID:** COL-FE-001 · **Priority:** P1 · **Ref:** FILEENGINE_INTEGRATION §4, §8

**Purpose.** Reach one or more FileEngine servers as the user, without passwords in files or agents handling credentials.

**Behaviour & rules.**
- A connection is a per-user setting (`fileengine.json`), never project data. It holds a name, the tenant's address (its REST door is under `/api/`), an optional tenant to work in (sent as `X-Tenant`), and who signed in last.
- **Sign-in with a service credential.** FileEngine generates a key id and secret (`fesk_…`, `fesks_…`) with the `api` scope for this app. `POST /api/v1/auth/token` with HTTP Basic exchanges it for a bearer token that lasts 900 s.
  - The client renews the token at `/v1/auth/refresh` a minute before it runs out.
  - It asks for a new token with the credential when the renewal is refused, or when a call is refused once (a server restart).
- The credential is kept in the OS keychain under `org.bimreviera.fileengine` when *remember* is on, otherwise only for the session. No command returns it, and no settings file, model or log holds it.
- Plain `http://` is allowed for localhost only, unless the connection is marked as a trusted local network.
- MCP agents may use every `fileengine.*` command, acting as the signed-in user, except two things:
  - giving a credential to `fileengine.sign_in`;
  - `fileengine.remove`.

**Commands.** `fileengine.connect(url, name?, tenant?, allow_http?) -> connection`; `fileengine.list_connections() -> connection[]`; `fileengine.sign_in(connection_id, key?, secret?, remember?) -> identity`; `fileengine.sign_out(connection_id, forget?) -> ()`; `fileengine.remove(connection_id) -> ()`; `fileengine.whoami(connection_id) -> identity`

**Acceptance criteria.**
- [x] A wrong secret is refused as not authorised; the right one returns the user, tenant and roles.
- [x] The credential appears in no command's answer and not in `fileengine.json`.
- [x] An agent's `fileengine.sign_in` with a credential, and its `fileengine.remove`, are refused; with the stored credential it signs in.
- [x] A token past its renewal time is renewed, and a revoked one replaced, without the caller seeing an error.

*Built (2026-10-30):*
- `clients/fileengine/rust` (MIT): `Client` with sign-in, renewal and re-issue, folders, files, content streamed both ways with `Range`, versions, restore, metadata and permissions.
- An in-memory FileEngine behind the `fake` feature, for tests.
- Checked live against Proxima File Vault (arcdigital tenant).
- Not yet: username and password (LDAP) sign-in, single sign-on, and the connection dialog in Options (connections are managed in the Open and Save dialogs).

### Browse and open
**ID:** COL-FE-002 · **Priority:** P1 · **Ref:** FILEENGINE_INTEGRATION §5

**Purpose.** Find a model in FileEngine's folders and work on it as if it were a local file.

**Behaviour & rules.**
- *File › Open From FileEngine…* lists a connection's folders and models (`GET /v1/dirs/{uid}`), folders first. Each entry shows its size, version count, who changed it last, and whether this computer has a working copy. Double-click enters a folder; *Up* goes back.
- **Open** streams the model into a working copy, `<data>/reviera/fileengine/<connection>/<uid>/<name>`, beside `fileengine.json`. That file records the connection, the uid, the folder, the version opened, who opened it, and the read-only, past-version and unsent states. The working copy is then opened like any local file, with the live store, recovery and undo.
- A model the user may not write (`GET /v1/nodes/{uid}/permissions?permission=w`) opens read-only.
- A working copy holding work not uploaded yet is opened as it is and never overwritten. The open reports whether the server has a newer version.
- A past version opens read-only from `<uid>@<version>/`.
- `file.open` accepts `fileengine://<connection>/<uid>`, and `?version=<v>` for a past version.
- The title bar names the model with *(FileEngine)*, *(FileEngine, not uploaded)* or *(FileEngine, version of …)*.
- `.ifc` models open; `.ifczip` is listed but not opened yet.

**Commands.** `fileengine.browse(connection_id, folder_uid?|path?, models_only?) -> {folder, entries[], writable}`; `fileengine.make_folder(connection_id, parent_uid, name) -> uid`; `fileengine.open(connection_id, uid, version?, read_only?) -> {document_id, working_copy, link, existing, newer_on_server}`; `fileengine.status(document_id?) -> {linked, link?, connected}`

**Acceptance criteria.**
- [x] A model in a folder the user may not write opens read-only, and saving it is refused.
- [x] Opening a model again while its working copy holds unsent work keeps that work.
- [x] `file.open` of `fileengine://<connection>/<uid>?version=<v>` opens that version read-only.

### Save with the version check
**ID:** COL-FE-003 · **Priority:** P1 · **Ref:** FILEENGINE_INTEGRATION §5

**Purpose.** Save a FileEngine model with Ctrl+S, never over someone else's newer save without asking.

**Behaviour & rules.**
- `file.save` of a FileEngine model first writes the working copy on this computer (atomic), then uploads it (`PUT /v1/files/{uid}/content`) as a new version.
- **The version check.** Just before uploading, Reviera reads the file's newest version (`GET /v1/nodes/{uid}`). If it is not the one opened or last uploaded, someone else has saved.
  - The save stays on this computer, marked *unsent*.
  - The error `fileengine.conflict` names who saved and when.
  - The window offers *Save as a Copy Beside It*, *Overwrite* (their version stays in the history) or *Open Theirs Read-Only*. It never merges silently.
  - FileEngine's REST door has no conditional upload or lock, so a save landing between the check and the upload is not caught (FILEENGINE_INTEGRATION §10, Q2).
- **A copy** is a new file `<name> (<user>'s copy).ifc` in the same folder. The document becomes that file, and the original's working copy stays as it was opened.
- An upload that fails (offline, server error) keeps the save *unsent*; the next save sends it, with the same check.
- **Save As to FileEngine** (*File › Save to FileEngine…* for a local project, `fileengine.save_as`, or `file.save_as` to `fileengine://<connection>/<folder uid>/<name>.ifc`) creates the file (`POST /v1/dirs/{uid}/files`) and uploads its first version. A name already in the folder is refused unless `replace` adds a new version to it. The document then is that model.
- *Save to FileEngine…* on a FileEngine model asks for a comment, kept with the version.

**Commands.** `fileengine.save(document_id?, on_conflict?: refuse|overwrite|copy, comment?) -> uploaded`; `fileengine.save_as(document_id?, connection_id, folder_uid, name, replace?, comment?) -> uploaded`; `file.save` and `file.save_as` as above, whose answer gains `uploaded`.

**Acceptance criteria.**
- [x] When another user saved since the model was opened, Ctrl+S leaves their version untouched and reports them by name.
- [x] *Copy* makes `<name> (<user>'s copy).ifc` beside the original, and the document becomes it; *Overwrite* keeps their version in the history.
- [x] A model saved to a folder, edited and saved again has two versions, and reopens from the server with both edits.

### Version history and restore
**ID:** COL-FE-004 · **Priority:** P1 · **Ref:** FILEENGINE_INTEGRATION §5

**Purpose.** See every saved version of a model, look at one, and bring one back without losing any.

**Behaviour & rules.**
- *File › FileEngine Version History…* lists the open model's versions, newest first (`GET /v1/files/{uid}/versions`). Each row shows when it was saved (UTC), who saved it, its save comment, and which version is open.
- *Open Read-Only* opens a version beside the model (COL-FE-002).
- *Restore…* makes the chosen version the newest again (`POST /v1/files/{uid}/restore`). FileEngine saves it as a new version, so nothing is lost.
  - The open model is then reopened at the new version.
  - Restoring is refused while the model has unsaved or unsent changes.

**Commands.** `fileengine.versions(document_id?|connection_id+uid) -> version[]`; `fileengine.restore_version(document_id?|connection_id+uid, version) -> {restored, version, document_id?}`

**Acceptance criteria.**
- [x] After three saves, the history lists three versions newest first, with the comment of the one saved with a comment.
- [x] Restoring the first version adds a fourth, and the model reopens with the first version's content.

### Model metadata
**ID:** COL-FE-005 · **Priority:** P2 · **Ref:** FILEENGINE_INTEGRATION §5

**Purpose.** Let FileEngine list and search models by what they are, and let a BCF topic name its model.

**Behaviour & rules.**
- Each upload sets string metadata on the model's file (`PUT /v1/nodes/{uid}/metadata/{key}`):
  - `ifc.schema`;
  - `ifc.project_guid` (the IfcProject's GlobalId);
  - `reviera.version`;
  - `reviera.levels` (a JSON array of the level names);
  - with a comment, `reviera.comment` and `reviera.comment.<version>`, which the version history reads.
- The metadata is best effort: a refused metadata write does not undo the upload.

**Acceptance criteria.**
- [x] After a save, the file's metadata holds the schema, the 22-character project GlobalId and the level names.

*Not yet (later phases):* a thumbnail with each save; the Documents pane; publishing exports to FileEngine; BCF over BCF-API; Component Libraries; product data from documents (§7c); live updates through the discussion service.

## 6.3 Merging

### Three-way merge by GlobalId and RXD id
**ID:** COL-MRG-001 · **Priority:** P1 · **Ref:** TECH_STACK §3.7.2 (*Get latest*, *conflicts found by comparing GlobalIds*); spec/02a (RXD merge rules)

**Purpose.** Combine local and remote changes reliably, at the finest practical granularity, so that people working on different things never conflict.

**Behaviour & rules.**

*Inputs:* **B** (base: the revision the working copy was last synchronised with, kept unmodified in `base.ifc`), **L** (local working copy), **R** (remote: the latest server revision). Each is an IFC model with embedded RXD.

*Step 1 — Index.* Parse the three models (in parallel). Index every rooted IFC entity (every `IfcRoot` subtype: objects, types, relationships, property sets) by GlobalId. Index every RXD object by its stable id (Chapter 2a).

*Step 2 — Fingerprint.* For each rooted entity, compute a canonical fingerprint of its **owned closure**: its own attributes plus every non-rooted entity reachable from it without passing through another rooted entity (placements, representations and their geometry items, property values, materials usage, and so on). In the canonical form:
- references to other rooted entities are replaced by their GlobalIds, and STEP instance numbers are ignored;
- real numbers are rounded to the model's length precision (default 1e-6 m) before hashing, so that a re-serialised but unchanged model has identical fingerprints;
- the closure is split into named **facets** that can be merged separately: `attributes` (each direct attribute separately), `placement`, `representation` (as a whole), and, for property sets, one facet per property name;
- hashing uses BLAKE3 and runs in parallel (rayon).

*Step 3 — Classify.* For each GlobalId, compare B, L and R:

| B | L | R | Result |
|---|---|---|---|
| present | same as B | same as B | unchanged |
| present | changed | same as B | take L |
| present | same as B | changed | take R |
| present | changed | changed, same result as L | take either (identical) |
| present | changed | changed differently | **facet merge**: facets changed on one side only are taken from that side; a facet changed on both sides differently is a **conflict** |
| present | deleted | same as B | delete |
| present | same as B | deleted | delete |
| present | deleted | deleted | delete |
| present | deleted | changed | **conflict** (delete/modify) |
| present | changed | deleted | **conflict** (modify/delete) |
| absent | added | absent | add L |
| absent | absent | added | add R |
| absent | added | added (same GlobalId) | identical → add once; different → **conflict** |

*Step 4 — Relationship sets.* Relationships whose meaning is a set of members are merged as **sets**, not as values, so that two users adding different elements to the same storey, group or assignment never conflict: `IfcRelContainedInSpatialStructure.RelatedElements`, `IfcRelAggregates.RelatedObjects`, `IfcRelNests.RelatedObjects` (keeping order: local insertions first, then remote), `IfcRelAssignsToGroup.RelatedObjects`, `IfcRelDefinesByType.RelatedObjects`, `IfcRelDefinesByProperties.RelatedObjects`, `IfcRelAssociatesMaterial.RelatedObjects`, `IfcRelAssociatesClassification.RelatedObjects`. The merged set is `(B ∪ added_L ∪ added_R) − removed_L − removed_R`. A relationship whose set becomes empty is deleted.

*Step 5 — Exclusivity rules.* After step 4, rules that IFC or Reviera require are checked: an element contained in exactly one spatial structure element; an occurrence typed by at most one type; an element in at most one workset (RXD). If the merged result breaks a rule (for example the element was moved to Level 2 locally and to Level 3 remotely), it becomes a conflict on that element.

*Step 6 — RXD.* RXD objects are merged per Chapter 2a: by stable id, three-way against B, field by field; same-field changes on both sides are conflicts; delete/modify is a conflict. Keyed collections inside an object (for example per-category overrides in a view, or rows in a schedule's fields) are merged per key.

*Step 7 — Ownership-aware automatic resolution.* Before a conflict is shown to the user, the live ownership document (§6.4) is consulted for the conflicting element or RXD object:
- if the **local user** held ownership (borrowed it, or owned its workset) for the whole period since B, the local side wins automatically;
- if **another user** held ownership, the remote side wins automatically, and the local change is put in the conflict stash (COL-CORE-003) and reported as "overridden: element was owned by <user>";
- otherwise the conflict goes to the user (COL-MRG-002).
Ownership is advisory, so step 7 is what gives it force at merge time.

*Step 8 — Referential integrity.* References to GlobalIds that no longer exist in the merged model are repaired: relationships lose the missing members (step 4 rule); a hosted element whose host was deleted (for example a door whose wall was deleted remotely while the door was edited locally) becomes a conflict; RXD references go through the reconciliation of Chapter 2a (annotation of deleted elements is orphaned or removed and listed).

*Step 9 — Write.* The merged model is written with IfcOpenShell (new STEP instance numbers; GlobalIds unchanged). The merge is deterministic: the same B, L, R and decisions always give byte-identical output.

**Commands.** `merge.preview(base_path?, remote_path?) -> merge_summary` (query; counts of changes and conflicts without applying); `merge.three_way(base_path, local_path, remote_path, out_path, policy?) -> merge_report` (for scripts and tests; uses files, not the open model)

**Acceptance criteria.**
- [ ] User A changes a wall's fire rating property; user B changes the same wall's height. After both synchronise, the wall has both changes and no conflict was raised.
- [x] Users A and B both add new walls to Level 1; the merged containment relationship contains all walls from both, with no conflict.
- [ ] A deletes a wall that B modified; the sync raises one delete/modify conflict.
- [x] Merging an unchanged model that was re-serialised by another tool produces zero changes.
- [ ] Merging 100 000 elements with 1 000 changes on each side completes in under 30 seconds on the reference machine.
- [ ] Running the same merge twice gives byte-identical output.

*Built (2026-10-29), by the element store:* `reviera_core::merge` merges Reviera's element store rather than the IFC entity closures: each element (by GlobalId) is its class, name and parameters (properties, IFC attributes, the Reviera recipe and RXD records), which is what both sides edit and what a server's re-serialisation reads back unchanged. Classification as the table above; facets are the class, the name and each parameter; a parameter changed on both sides merges key by key when it is a JSON object (recursively) and as a set when it is a list of strings (members, parts); numbers equal within 1e-9 relative; conflicts by `policy` (fail, local, remote). The plan is ordered by GlobalId (deterministic) and applied as one transaction. `merge.three_way(base_path, local_path, remote_path, out_path, policy?) -> {created, updated, deleted, remote_changes, merged, conflicts, written}`. Checked: two users moving different walls, each adding walls, and both moving the same wall (one conflict on the recipe's curve). Not yet: `merge.preview`, the exclusivity checks (step 5), ownership-aware resolution (step 7), host/hosted repair (step 8), conflicts decided one by one (COL-MRG-002), geometry of elements that exist only in IFC (imported, without a recipe) when added remotely, and regeneration of generated parts changed on both sides (an enclosure's parts).

### Conflict resolution
**ID:** COL-MRG-002 · **Priority:** P1 · **Ref:** research/04 §7.1 (*Editing request*, as the reference's way of avoiding conflicts)

**Purpose.** Let the user decide conflicts clearly and safely.

**Workflow.** When a merge has unresolved conflicts, the Conflicts dialog lists them, grouped by element and RXD object, with: what changed locally, what changed remotely (with the remote user and revision comment), and the base value. Per conflict: *Keep mine*, *Take theirs*, *Show* (zoom to the element in a suitable view, with local and remote versions shown as ghosts), and, for property facets, *Edit merged value*. Bulk actions: *Keep all mine*, *Take all theirs* for a filtered set. *Finish* applies the decisions; *Cancel* abandons the merge and restores the working copy to its state before the merge.

**Behaviour & rules.**
- Every decision is recorded in the sync report (user, time, element, choice).
- The losing side of every conflict is stored in the conflict stash: a small IFC+RXD fragment per conflict, kept in the working-copy folder. *Collaborate › Conflict Stash* lists them; *Re-apply* re-applies the stashed change as a new local edit; *Discard* deletes it. The stash is never uploaded.
- Scripts and agents can resolve conflicts through commands; MCP agents need the model-edit permission.

**Commands.** `merge.list_conflicts() -> conflict[]` (query); `merge.resolve(conflict_id, choice: mine|theirs|value, value?) -> ()`; `merge.resolve_all(filter?, choice: mine|theirs) -> count`; `merge.finish() -> sync_report`; `merge.cancel() -> ()`; `stash.list() -> stash_entry[]` (query); `stash.reapply(entry_id) -> ()`; `stash.discard(entry_ids[]) -> ()`

**Acceptance criteria.**
- [x] Choosing *Take theirs* for a conflict puts the local value in the stash; *Re-apply* from the stash restores the local value as an undoable edit.
- [x] *Cancel* in the Conflicts dialog leaves the working copy exactly as it was before the merge.

*Built (2026-10-30):* a merge that meets conflicts under the `fail` policy is kept **pending** for the document (the downloaded revision stays as `remote.ifc`); `merge.list_conflicts(document_id?)` (ids `c1` …, with the local and remote values and the decision so far), `merge.resolve(conflict_id, choice: mine|theirs|value, value?, element?)` (`element` decides every conflict on that element, so a curve's start and end never come from different sides), `merge.resolve_all(choice, class?, all?) -> count`, `merge.finish(rest?, document_id?) -> sync_report` (refused while any conflict is undecided; conflicts that appeared since the working copy changed join the list), `merge.cancel(document_id?)`; a pending synchronise checks in after finish. The losing side of each decided conflict goes to the **stash** (`stash.json` in the working-copy folder, never uploaded): `stash.list`, `stash.reapply(entry_id, element?)` (an undoable edit; `element` takes the element's other entries from the same side too), `stash.discard(entry_ids)`. File › BIMserver… › Get Latest and Synchronise open the **Conflicts** dialog: one row per element (what changed, the decision), Keep Mine, Take Theirs, Keep All Mine, Take All Theirs, Finish, Cancel Merge. Not yet: *Show* (zoom with ghosts), *Edit merged value* in the dialog (the command takes one), the remote user and revision comment per conflict, recording decisions with user and time in the report, and Collaborate › Conflict Stash.

### Post-merge regeneration and validation
**ID:** COL-MRG-003 · **Priority:** P1 · **Ref:** research/04 §9 (*Review Warnings*)

**Purpose.** Make sure a merged model is consistent before it is used or uploaded, even though each side was consistent on its own.

**Workflow.**
1. After the merge's step 9 (COL-MRG-001), Reviera regenerates and validates the merged working copy automatically; the status bar shows "Regenerating merged model (n of m)".
2. During *Get latest* the result is shown in the sync report. During *Synchronise* it runs before the check-in (COL-BIM-006 step 5): if validation finds **blocking** problems, the sync stops before uploading and shows them with *Fix*, *Upload anyway* (blocked for schema errors) and *Cancel*.

**Behaviour & rules.**
- After a merge, the regeneration graph is re-run for every changed element and its dependents (joins, hosted elements, rooms, schedules), so that, for example, two walls edited by different users still join correctly.
- **Seed set:** every element or RXD object whose fingerprint differs from L (the local copy before merging), plus everything that depends on it in the dependency graph (Chapter 2, regeneration). Unchanged parts of the model are not regenerated, so the cost is proportional to the change, not to the model.
- Regeneration runs in parallel on independent sub-graphs, like an ordinary edit (Chapter 2b).
- Model validation runs on the merged result: IFC schema rules for changed entities, RXD schema validation, Reviera model rules (overlapping walls, duplicate instances, room boundary problems). New warnings caused by the merge are listed in the sync report and in the warnings list (COL-MNT-004).
- **Severity of findings:**
  - *Blocking* (stop before check-in): IFC schema errors on changed entities (wrong types, missing required attributes), duplicate GlobalIds, RXD documents that fail their schema.
  - *Warning* (reported, check-in allowed): model rules such as overlapping walls, rooms not enclosed, elements that failed to regenerate.
- If regeneration fails for an element, the merge still completes; the element keeps its merged IFC geometry and is flagged *needs attention* (a warning of kind `col.merge.regen_failed` naming the element and the two revisions involved).
- **Merge-caused warnings** are tagged with the sync report id, so the warnings dialog can filter "caused by last sync" (COL-MNT-004).
- **Undo:** the merge and its regeneration form one entry in the sync report, not in the undo stack; Ctrl+Z after *Get latest* does not undo other users' changes. The conflict stash (COL-MRG-002) is the way back for local changes that lost.
- **Cancel:** cancelling during regeneration leaves the merged model in place but unvalidated; the next save or sync runs validation again.

**Data (IFC/RXD).** Warnings found here are stored like other warnings in RXD document `project/warnings` (Chapter 2, DM-FAIL-001), with an extra `syncReport` field. The *needs attention* flag is a warning, not a separate field on the element.

**Commands.** No new commands; validation runs inside `bimserver.get_latest`, `bimserver.sync` and `merge.finish`. Scripts can re-run it with `model.audit(repair?)` (COL-MNT-002).

**Acceptance criteria.**
- [ ] Two users each extend one of two walls that meet at a corner; after both synchronise, the corner join is correct.
- [ ] User A deletes a room separation line while user B moves a wall that bounds the same room; after both synchronise, the room area is recomputed and a "room not enclosed" warning, tagged with the sync, appears if the room is open.
- [ ] A merge that produces an IFC schema error on a changed entity stops *Synchronise* before check-in, and *Upload anyway* is disabled.
- [ ] Regenerating after a merge of 1 000 changed elements in a 100 000-element model touches only those elements and their dependents (checked by the regeneration log).

---

## 6.4 Worksets and ownership

### Worksets
**ID:** COL-WS-001 · **Priority:** P1 · **Ref:** research/04 §7.1–7.3 · screenshots/documentation/worksets_dialog.png

**Purpose.** Divide the model into named collections of elements, for visibility control, partial loading and ownership by area of work.

**Behaviour & rules.**
- A workset has a name, a kind (*user* or *view*), *visible by default in all views*, and *open by default*.
- Every element belongs to exactly one workset. Model elements are in user worksets. View-owned elements (annotation, detail items) belong to the *view workset* of their view, which is created automatically for each view and sheet and cannot be edited by hand. (Reviera does not need the reference's separate worksets for families and project standards: those are types and RXD standards objects, which follow ordinary per-object ownership, see deviations.)
- A new project has the user worksets *Shared Levels and Grids* (levels, grids, reference planes, scope boxes) and *Workset1* (everything else). Enabling worksets on a project without them (Collaborate › Worksets on first use) shows a dialog to rename these two.
- Worksets work in Local mode as well (visibility, open/close); ownership applies only in BIMserver mode.

**Data (IFC/RXD).** Workset definitions are RXD document `project/worksets` (kind `workset`). Each element's workset is element-scoped RXD on that element (Chapter 2a), so that moving one element to another workset changes only that element and merges cleanly. **IFC export:** with the export option *Worksets as groups* (default on for coordination setups), each user workset is written as an `IfcGroup` (`ObjectType = 'WORKSET'`, `Name` = workset name) with its elements assigned by `IfcRelAssignsToGroup`, so other tools can filter by workset. This export form is never read back as the native store.

**Commands.** `workset.create(name, visible_by_default?, open_by_default?) -> workset_id`; `workset.rename(workset_id, name) -> ()`; `workset.delete(workset_id, move_to_workset_id?|delete_elements?) -> ()`; `workset.move_elements(element_ids[], workset_id) -> ()`; `workset.list() -> workset_summary[]` (query)

**Acceptance criteria.**
- [ ] Deleting a workset requires choosing whether its elements move to another workset or are deleted.
- [ ] An IFC export with *Worksets as groups* contains one `IfcGroup` per user workset with the correct members.
- [ ] Two users each moving different elements to a new workset merge without conflict.

### Worksets dialog, active workset and element workset
**ID:** COL-WS-002 · **Priority:** P1 · **Ref:** research/04 §7.3 · screenshots/documentation/worksets_dialog.png

**Purpose.** One dialog to create, organise, open, close and take ownership of worksets, and to choose where new elements go.

**Workflow.** Collaborate › Manage Collaboration › Worksets, or the status-bar workset button (`workset.manage`). The dialog shows the active workset, *Grey inactive workset graphics*, and a table: Name, Editable (owned by me), Owner, Borrowers, Opened, Visible in all views. Buttons: New, Delete, Rename, Open, Close, Editable, Non-editable. Show filter: user worksets, view worksets.

**UI (Worksets dialog).**
- **Header:** *Active workset* drop-down (user worksets only, sorted by name) and the *Grey inactive workset graphics* check box.
- **Table** (one row per workset, multi-select, sortable by any column):

| Column | Content | Editable in the table |
|---|---|---|
| Name | Workset name | Yes, for user worksets (in-place rename) |
| Editable | *Yes* if the current user owns the whole workset | Yes (drop-down), BIMserver mode only |
| Owner | User name of the workset owner, blank if none | No |
| Borrowers | Users borrowing elements of the workset; a drop-down when more than one | No |
| Opened | *Open* or *Closed* | Through Open/Close buttons |
| Visible in all views | Default visibility in views | Yes (check box) |
| Elements | Element count (open worksets only; "–" when closed) | No |

- **Show filter:** check boxes *User worksets* (default on) and *View worksets* (default off). View worksets are listed read-only with the view or sheet name; only *Opened* and *Visible* are meaningful for them, and they cannot be renamed, deleted or made editable here (ownership of a view is taken by borrowing the view, COL-OWN-001).
- **Buttons:** *New…* (name, *Visible in all views* default on; the new workset is owned by its creator in BIMserver mode), *Delete* (asks where the elements go: another workset, or delete them with their count shown), *Rename*, *Open*, *Close*, *Editable*, *Non-editable*, and *OK*, *Cancel*, *Help*.
- In Local mode the Editable, Owner and Borrowers columns and the Editable/Non-editable buttons are hidden.
- The dialog refreshes live when ownership changes arrive (COL-BIM-007) while it is open.

**Validation.**
- Names are 1–128 characters, unique among worksets (case-insensitive), and must not start with the reserved prefix `View: `. Duplicates are refused inline with "A workset with this name already exists".
- *Delete* is refused for the last remaining user workset, for the active workset (choose another active one first), and in BIMserver mode for a workset owned or borrowed from by another user (the message names them).
- *Rename* and *Delete* in BIMserver mode need ownership of the workset definition; Reviera borrows the definition object automatically, as for any RXD object.
- *Close* is refused for the active workset (choose another active workset first). Closing a workset that contains elements the user has borrowed asks for confirmation, because those elements cannot be edited until it is reopened; the borrows are kept.
- *Cancel* discards all changes made in the dialog; ownership changes already written to the ownership document (Editable/Non-editable) are not undone by *Cancel*, and the dialog says so before closing.

**Behaviour & rules.**
- **Active workset:** new elements go into it. It is shown on the Collaborate tab and on the status bar, and can be a workset owned by someone else (new elements are then created as borrowed by the creator).
- *Grey inactive workset graphics* halftones all elements not in the active workset in every view; it is display-only and never printed (layer 6 of Chapter 5, DOC-GFX-001).
- Each element shows *Workset* (editable for model elements) and *Edited by* (read-only; the current borrower or the workset owner) in Properties.
- *Editable = Yes* takes ownership of the whole workset (COL-OWN-001). It is refused when the user's base is older than the latest revision that changed any element of the workset; the user is offered *Get latest* first.
- *Editable only* (status-bar toggle) restricts selection to elements the user may edit without requesting.
- Per-view workset visibility (use global setting / show / hide) is part of the view's graphics settings (Chapter 5, DOC-GFX-003, Worksets tab).

**Data (IFC/RXD).** Workset definitions, names and *visible by default* are in RXD document `project/worksets` (kind `workset`, COL-WS-001). The active workset, the grey-inactive setting and the open/closed choice are per-user client state (working-copy `state.json`), never project data. Editable/owner/borrower state is in the live ownership document (COL-OWN-002).

**Commands.** `workset.manage() -> ()` opens the Worksets dialog (UI only; each change made in it calls the commands below, so every change is scriptable and recorded); `workset.set_active(workset_id) -> ()`; `workset.set_editable(workset_ids[], editable) -> ()`; `workset.set_visible_by_default(workset_id, visible) -> ()`; `view.set_workset_visibility(view_id, workset_id, mode: global|show|hide) -> ()`; `view.grey_inactive_worksets(on) -> ()`

**Acceptance criteria.**
- [ ] With *Grey inactive workset graphics* on, elements in other worksets are halftoned on screen but print normally.
- [ ] Making a workset editable while another user borrows one of its elements fails and names the borrower.
- [ ] Renaming a workset to the name of an existing one (different case) is refused inline, and *Cancel* leaves all workset names unchanged.
- [ ] In Local mode the Worksets dialog shows no Editable, Owner or Borrowers columns.
- [ ] Deleting the active workset is refused until another workset is made active.

### Opening and closing worksets
**ID:** COL-WS-003 · **Priority:** P1 (closing for display), P2 (partial download) · **Ref:** research/04 §7.3 (*Opened*)

**Purpose.** Improve performance by not loading parts of the model.

**Behaviour & rules.**
- A closed workset's elements are not displayed, not regenerated, not selectable and not included in schedules (a banner on the schedule says so).
- At open time, *Specify…* lets the user choose which worksets to open; the choice is remembered per user and project (client state).
- In P1, closed worksets are still downloaded and kept in the working copy; they are just not loaded into the editing session. In P2, Reviera downloads only open worksets, using a BIMserver query restricted to the GlobalIds of the open worksets' elements (verify query support), and fetches the rest when a workset is opened.
- Elements of closed worksets still take part in merges (they are in the working copy file), so closing a workset never loses others' changes.

**Commands.** `workset.open(workset_ids[]) -> ()`; `workset.close(workset_ids[]) -> ()`

**Acceptance criteria.**
- [ ] Closing the *Furniture* workset removes furniture from views and schedules, and reopening restores it without a download in P1.

### Advisory ownership and borrowing
**ID:** COL-OWN-001 · **Priority:** P1 · **Ref:** research/04 §7.1 (*Workset ownership*, *Element borrowing*); TECH_STACK §3.7.2, §5 (open decision 8)

**Purpose.** Avoid conflicts by letting users claim what they are working on, without needing a server-side lock service.

**Behaviour & rules.**
- **Owner identity:** the BIMserver user (COL-BIM-001) plus a client session id (so the same user on two machines is two owners).
- **Ownership units:** an element (borrowing), a workset (taking it editable), or an RXD object (a view, a sheet, a schedule, a standards object; borrowed when edited, like an element). View-owned annotation is covered by its view: borrowing a view borrows its annotation.
- **Automatic borrowing:** when a user starts an edit on something nobody owns, Reviera borrows it in the same action (no prompt). The borrow is written to the live ownership document (COL-OWN-002) **before** the edit is committed. If the write fails because someone else claimed it first, the edit is cancelled and the user is offered an editing request (COL-OWN-003).
- **Up-to-date rule:** an element can be borrowed only if the user's base contains its latest server version. Reviera knows this from the notifications and the ownership document (each ownership entry carries the revision in which its holder last changed the element; each relinquish records it). If the element changed in a newer revision, the user is asked to *Get latest* first.
- **Owned by others:** attempting to edit an element or object owned by someone else shows "<user> is editing this element" with *Place request* and *Cancel* (COL-OWN-003). Viewing and selecting are always allowed.
- **Relinquishing:** happens at synchronise (by option), on *Relinquish All Mine* (Collaborate tab), on close (COL-BIM-006), or by an administrator's *Force relinquish* (server administrators only; logged and notified to the former owner).
- **Advisory nature:** ownership is enforced by cooperating Reviera clients and by the merge (COL-MRG-001 step 7). A client that ignores it (for example another IFC tool checking in directly) cannot be stopped without a server plugin (COL-OWN-004); its changes are merged like any remote change.
- **Offline:** while offline, edits to unowned things create *provisional borrows* that are queued and claimed on reconnection. If a provisional borrow is refused (someone else claimed the element meanwhile), the element is flagged and the merge will treat both edits as a conflict at the next sync.

**Commands.** `ownership.borrow(element_or_object_ids[]) -> result` (normally implicit); `ownership.relinquish(scope: all|elements|worksets, ids[]?) -> ()`; `ownership.query(ids[]) -> ownership_info[]` (query); `ownership.force_relinquish(owner, ids[]?) -> ()` (administrators)

**Acceptance criteria.**
- [x] User A moves a wall (borrowing it); user B trying to move it sees "A is editing this element" within the notification latency.
- [x] After A synchronises with *relinquish*, B can move the wall without a request. *(After Get Latest, by the up-to-date rule.)*
- [x] B cannot borrow a wall changed in a revision B has not got yet; B is offered *Get latest*.

*Built (2026-10-30), elements:* every edit of a working copy (any document command, not undo or redo) borrows the elements it modified or deleted in the ownership document (COL-OWN-002) on the project's latest revision; if another session owns one (`ownership.owned_by_other`, naming the holder) or one changed in a revision newer than the base (`ownership.not_latest`, *Get latest* first), the edit is undone and refused. The borrow is written after the edit's transaction and before the command returns (the spec's "before commit" with an undo on refusal); a concurrent claim of the same version is settled by the chain, retried once. Entries are `{target: ifc:<GlobalId>, owner, since, lastChangedRevision}`. Synchronise relinquishes this session's borrows (unless `keep_borrowed`) and records the revision they changed in; the next revision's root carries every entry. `ownership.borrow(element_ids)`, `ownership.relinquish(element_ids?) -> count`, `ownership.query(element_ids) -> [{guid, owner, mine, since, last_changed_revision}]`. A profile's `ownership: false` turns borrowing off; offline (server unreachable) edits stand unborrowed. Ownership documents are cached by their extended-data id. Not yet: worksets and RXD objects as ownership units, provisional borrows queued offline, *Force relinquish*, *Relinquish All Mine* in the UI, and showing the owner on selection (worksharing display, COL-WS-004).

### Live ownership document and sync lock
**ID:** COL-OWN-002 · **Priority:** P1 · **Ref:** TECH_STACK §3.7.1–3.7.2; spec/02a (BIMserver transport)

**Purpose.** Share ownership and lock state among clients immediately, without creating model revisions.

**Behaviour & rules.**
- **Document:** a small JSON document, the *ownership document*, with schema name `org.reviera.ownership/1.0`. Contents: `version` (integer), `parentVersion`, `baseRoid` (the revision it applies to), `entries[]` (`{target: ifc:<GlobalId> | rv:<id> | workset:<id>, owner: {userId, userName, session}, since, lastChangedRoid}`), `requests[]` (COL-OWN-003), and `syncLock` (`{owner, acquired, expires}` or null). It is **not** embedded in the IFC model and is never part of a check-in.
- **Transport (BIMserver extended data, call names from the public BIMserver service interface):**
  1. One-time per server: register the schema with `ServiceInterface.addExtendedDataSchema` (name `org.reviera.ownership/1.0`) if `getExtendedDataSchemaByName` does not find it.
  2. **Read:** `ServiceInterface.getLastExtendedDataOfRevisionAndSchema {"roid": <latest>, "schemaId": …}` gives the newest ownership item on the latest revision; `ServiceInterface.getFile {"fileId": …}` returns its content. To resolve concurrent writers, the client reads all ownership items of the revision (`getAllExtendedDataOfRevision`, verify) and builds the chain described below.
  3. **Write:** `ServiceInterface.uploadFile` with the new JSON (base64) returns a file id; `ServiceInterface.addExtendedDataToRevision {"roid": <latest>, "extendedData": {"schemaId": …, "fileId": …, "title": "ownership v<N>"}}` attaches it.
- **Optimistic concurrency with a deterministic chain:** each write declares `parentVersion` = the version it was based on, and `version = parentVersion + 1`. When two clients write children of the same parent, the one attached first (lowest extended-data object id) wins; the others are *rejected writes*. Every client computes the same current state by following, from the root, the earliest child of each version. A client that sees its own write rejected re-reads, re-applies its change to the winning state if still valid (for example the element is still unowned), and writes again; otherwise it reports the refusal to the user (COL-OWN-001).
- **New revisions:** the client that checks in a revision immediately writes the carried-forward ownership state (after its relinquishes) as the root (`version` 1, `parentVersion` null) on the new revision. Until a root exists on the newest revision, clients keep using the chain on the previous revision.
- **Refresh:** clients refresh on new-revision and new-extended-data notifications (COL-BIM-007), and otherwise poll at the update frequency (default 30 s). Before any borrow, the client re-reads the document (not the cache).
- **Sync lock:** the `syncLock` field implements the "central model in use" lock of COL-BIM-006: a client acquires it by a successful write with itself as holder and an expiry time; it renews by further writes; it releases it by writing null. An expired lock may be taken over by any client.
- **Growth:** each revision accumulates ownership items; Reviera never deletes them (history is useful for audit), and a new revision starts a fresh chain.
- **Local mode:** there is no ownership document; all ownership commands return `mode.not_available`.

**Acceptance criteria.**
- [ ] Two clients borrowing the same element within 100 ms of each other: exactly one succeeds, and both clients agree on which one.
- [x] A client that crashes while holding the sync lock blocks others for no longer than the lock's time-to-live.
- [x] Borrowing and relinquishing elements does not create any new model revision.
- [ ] After user A checks in, user B (who still has base = previous revision) still sees A's remaining borrows.

*Built (2026-10-29), the sync lock:* the ownership document `org.reviera.ownership/1.0` (version, parentVersion, baseRoid, entries, requests, syncLock with owner {userId, userName, session}, acquired, expires) in BIMserver extended data: the schema registered on first use (`getExtendedDataSchemaByName`, `addExtendedDataSchema`), written by `uploadFile` and `addExtendedDataToRevision`, read by `getAllExtendedDataOfRevision` and `getFile`, the current state the chain from the root through the earliest child of each version (oid order). All verified on BIMserver 1.6.1. Entries and requests are carried forward but not yet used (borrowing, COL-OWN-001/003).

### Editing requests
**ID:** COL-OWN-003 · **Priority:** P1 · **Ref:** research/04 §7.1, §7.5; web: Grant a Request to Borrow an Element; web: Granted, Denied, and Retracted Requests · screenshots/documentation/editing_request_received.png

**Purpose.** Ask the owner to hand over an element, a workset or an RXD object.

**Workflow.**
1. On a refused edit, the user chooses *Place request* (optionally with a message).
2. The owner receives a balloon "Editing request from <user>" with *Show*, *Grant*, *Deny*; the status bar shows a pending-requests count.
3. Collaborate › Editing Requests lists *My pending requests* (with *Retract*) and *Requests to me* (with *Grant*, *Deny*, *Show*).
4. When a request is granted, the requester is notified; if the owner had changed the element, the requester must *Get latest* (offered in the notification) before editing.

**UI (Editing Requests dialog).** Opened by Collaborate › Communicate › Editing Requests, by the status-bar *Editing Requests* counter, or from a request balloon (`editrequest.manage`). It is modeless, so the user can keep working and use *Show* while it is open.
- **Two expandable groups:** *My pending requests* (requests I placed) and *Requests to me* (requests for things I own). Each request row shows: requester or owner, targets (count, expandable to one row per element, workset or RXD object with category and name), message, time placed, state, and an indicator when a target has unsynchronised changes on the owner's side (`*`) or was deleted in a newer revision (`^`).
- **Buttons (enabled by selection):** *Grant* and *Deny* (requests to me; *Deny* asks for an optional reply); *Retract* (my requests); *Show* (selects the targets and zooms to them in a suitable open view, opening one if needed; for targets in a closed workset it first asks to open that workset); *Close*.
- **History** toggle: shows granted, denied, retracted and expired requests of the current revision chain with who decided and when; read-only.
- The status-bar counter shows the number of *Requests to me* that are pending; it turns to the alert colour when there is at least one.

**Behaviour & rules.**
- Requests are entries in the ownership document's `requests[]` (`{id, from, to, targets[], message, created, state: pending|granted|denied|retracted|expired, decided, reply}`), written with the same chain mechanism.
- **Request scope:** a request may name elements, worksets or RXD objects. A request for an element in a workset owned by someone else goes to the workset owner; a request for an element borrowed by someone goes to the borrower. If targets belong to several owners, one request is created per owner and the dialog groups them.
- **Grant** relinquishes the targets to the requester. If the owner has unsynchronised changes to the targets, granting first runs a synchronise of those changes (the owner is asked to confirm), because handing over an element whose latest state is only on the owner's machine would lose work. The *Grant* button shows "Synchronise and grant" in that case.
- **Partial grant:** the owner can expand a request and grant some targets; the request then stays pending for the rest.
- **Implicit grant:** if the owner relinquishes requested targets in any other way (synchronise with relinquish, *Relinquish All Mine*), the request is marked granted automatically and the requester is notified.
- **Deleted targets:** a target deleted in a newer revision is dropped from the request with a note; a request whose targets are all deleted becomes *expired*.
- **On grant**, the requester's client borrows the targets automatically on its next ownership refresh, provided its base contains their latest version; otherwise it offers *Get latest* first (COL-OWN-001 up-to-date rule). The requester does not need to place the request again.
- **Advisory nature:** a request is a message between cooperating clients; it gives no server-enforced right. Ignoring it is possible only by editing offline or with a foreign client, and the merge (COL-MRG-001 step 7) then settles the outcome.
- Requests expire after 7 days (configurable per project, 1–90 days) and are then shown as expired.
- **Local mode:** the dialog and every `editrequest.*` command return `mode.not_available`.
- **Offline:** requests placed offline are queued (COL-OFF-001) and sent on reconnection; the dialog shows them as *queued*.

**Data.** Ownership document `requests[]` only (COL-OWN-002); never IFC or RXD, and never part of a check-in.

**Commands.** `editrequest.manage() -> ()` opens the Editing Requests dialog; `editrequest.create(targets[], message?) -> request_id`; `editrequest.grant(request_id) -> ()`; `editrequest.deny(request_id, message?) -> ()`; `editrequest.retract(request_id) -> ()`; `editrequest.list() -> request[]` (query; includes history entries with their state)

**Acceptance criteria.**
- [ ] B requests a wall owned by A; A grants; A's unsynchronised change to the wall is synchronised first; B is then able to edit the wall after getting latest.
- [ ] A denied request shows the owner's message to the requester.
- [ ] A request naming two walls, one borrowed by A and one by C, produces two requests, one to each owner, shown grouped in B's dialog.
- [ ] A synchronising with *relinquish borrowed elements* marks B's pending request for one of those elements as granted, and B is notified.
- [ ] The status-bar counter shows 2 when two requests to the user are pending, and 0 after both are decided.

### Request editing from the selection
**ID:** COL-OWN-005 · **Priority:** P1 · **Ref:** research/04 §7.1 (*Editing request*, *Place Request*), §7.5; web: Grant a Request to Borrow an Element

**Purpose.** Let a user ask for editing rights on selected elements owned by others before starting an edit, and see who owns them, within the advisory ownership model.

**Workflow.**
1. Select one or more elements (or a view, sheet or schedule in the Project Browser) in BIMserver mode. When any of them is owned by someone else, the contextual ribbon shows the *Ownership* panel: *Request Editing* and *Show Owner* (Chapter 1).
2. *Request Editing* opens a small form: the list of selected items grouped by owner (items nobody owns are marked "free"), a message box (optional, up to 500 characters), and *Request* / *Cancel*.
3. On *Request*, items nobody owns are borrowed at once (as an automatic borrow, COL-OWN-001); for the others, one editing request per owner is placed (COL-OWN-003). The result is reported as "Borrowed 3, requested 5 from 2 users".
4. *Show Owner* shows a tooltip-style panel listing, per selected item, its workset, owner or borrower, since when, and whether the user's base is up to date for it; it uses `ownership.query`.

**Behaviour & rules.**
- **Mapping onto the advisory model:** a request is a message in the live ownership document; nothing is locked on the server (BIMserver has no locking, COL-OWN-004 is deferred). Requested items stay read-only in the requester's client until granted, exactly as for an edit refused by COL-OWN-001.
- Items the user already owns are skipped and reported as "already yours".
- Items whose latest version is newer than the user's base are still requested, but the result says *Get latest* will be needed before editing them.
- **Workset-level request:** if the user ticks *Request whole workset* (shown when all selected items are in one workset owned by another user), the request targets the workset instead of the elements.
- Linked elements cannot be requested (links are read-only); they are listed as "in link, not editable".
- **Local mode:** the panel is hidden and the command returns `mode.not_available`.
- **Offline:** the request is queued (COL-OFF-001) and the free items get provisional borrows.
- MCP agents can call `ownership.request`; requests from an agent carry the logged-in user's identity plus the marker `via agent` in the message.

**Data.** Borrows and requests are written to the ownership document (COL-OWN-002); no IFC or RXD change.

**Commands.** `ownership.request(element_or_object_ids[], message?, whole_workset?) -> {borrowed[], requests[], skipped[]}`. *Show Owner* uses `ownership.query(ids[]) -> ownership_info[]` (COL-OWN-001).

**Acceptance criteria.**
- [ ] Selecting five walls, two free and three borrowed by A, and choosing *Request Editing* borrows the two free walls and places one request to A for the other three.
- [ ] Selecting an element in a linked model and choosing *Request Editing* places no request and reports it as not editable.
- [ ] *Show Owner* on an element borrowed by A shows A's name and the time the borrow started.

### Enforced ownership through a server plugin (optional)
**ID:** COL-OWN-004 · **Priority:** P3 · **Ref:** TECH_STACK §5 (decision 8)

**Purpose.** For teams that need ownership to be binding even for other clients.

> **Deferred (owner, 2026-10-12).** Ownership follows what the target BIMserver release supports. Stock BIMserver has no locking, so Reviera's ownership is advisory (COL-OWN-001…003), and the Reviera project does **not** build or ship an enforcement plugin. The capability check below stays in P1 so that Reviera uses enforcement automatically if BIMserver itself, or a separately maintained plugin, ever provides it. The behaviour below describes what such a server-side facility must offer.

**Behaviour & rules.**
- An optional BIMserver service plugin (Java, distributed separately under a licence compatible with BIMserver's AGPL-3.0) provides: an atomic compare-and-set endpoint for the ownership document (replacing the optimistic chain), and a check-in validator that rejects revisions changing elements owned by someone other than the uploader (comparing the uploaded model with the previous revision by GlobalId).
- Reviera detects the plugin through the server's plugin list and uses it automatically; otherwise it falls back to COL-OWN-002.

**Commands.** No new commands; `bimserver.capabilities() -> capability_set` (query) reports whether enforcement is available.

**Acceptance criteria.**
- [ ] With the plugin installed, a check-in from a non-Reviera client that modifies an element owned by a Reviera user is rejected by the server.

### Worksharing display modes
**ID:** COL-WS-004 · **Priority:** P1 · **Ref:** research/04 §7.5 · screenshots/documentation/worksharing_display_worksets.png · screenshots/documentation/worksharing_display_owners.png

**Purpose.** Show, in any view, who owns what, which elements are out of date, and which workset elements belong to.

**Workflow.** View Control Bar › Worksharing Display: Off; *Checkout status*; *Owners*; *Model updates*; *Worksets*. *Worksharing Display Settings* sets colours per user, per workset and per status, and which ones are shown.

**Behaviour & rules.**
- **Checkout status:** owned by me / owned by others / not owned.
- **Owners:** one colour per owner (stable, chosen from a distinct palette).
- **Model updates:** up to date; changed in a newer server revision; deleted in a newer server revision. Reviera knows which elements changed from the notifications: after a new-revision event, the client downloads the revision in the background (or uses the server comparison where available) and compares fingerprints with its base (§6.3, steps 1–3), without merging.
- **Worksets:** one colour per workset.
- The mode is applied as layer 6a of the graphics model (Chapter 5, DOC-GFX-001): a temporary colour override that is not printed and not saved.
- Tooltips over an element show its workset, owner, borrowers and update status.

**Commands.** `view.set_worksharing_display(view_id, mode: off|checkout|owners|updates|worksets) -> ()`; `worksharing.set_display_colours(kind, key, colour) -> ()`

**Acceptance criteria.**
- [ ] In *Model updates* mode, elements changed in a revision B has not got yet are coloured within the notification latency, before B gets latest.
- [ ] Worksharing display colours never appear in PDF output.

---

## 6.5 Offline working copies

### Working copy layout and offline editing

> **Storage (updated 2026-10-12).** The offline working copy is the project's **live SQLite store** (Chapter 2b §8.2). The downloaded base revision is kept in its `base_entity` and `base_rxd` tables for the three-way merge, and local edits are kept in its live tables and transaction journal. Crash recovery and offline work therefore share one mechanism. Where the layout described below conflicts with Chapter 2b, Chapter 2b takes precedence.
**ID:** COL-OFF-001 · **Priority:** P1 · **Ref:** TECH_STACK §3.7 (*Save to a local working copy and cache (works offline)*)

**Purpose.** Keep working when the server is unreachable, and never lose local work.

**Behaviour & rules.**
- Each opened BIMserver project has a working-copy folder in the user's data directory: `<data>/reviera/bimserver/<server-id>/<poid>/` containing:
  - `working.ifc`: the working copy (IFC with embedded RXD), saved by Ctrl+S and by autosave;
  - `base.ifc`: the unmodified base revision, used for three-way merges;
  - `state.json`: client state (server profile id, `poid`, base `roid`, user, open/closed worksets, last sync time, queued ownership operations, queued editing requests);
  - `journal/`: an append-only log of committed commands since the last save, so that a crash loses nothing (the journal is replayed on reopen);
  - `stash/`: the conflict stash (COL-MRG-002);
  - `cache/`: downloaded linked models (COL-LNK-001) and ownership snapshots.
- **Offline detection:** the connection state is shown on the status bar. When offline, the user can open the working copy, edit, save and close. *Synchronise* and *Get latest* are disabled with a tooltip.
- **Queued actions:** borrows (provisional), relinquishes and editing requests made offline are queued and sent on reconnection, in order.
- **Reconnection:** the client refreshes the ownership document, processes the queue, and if the server has newer revisions, offers *Get latest* (it never merges automatically without the user's go-ahead, because merges may need decisions).
- **Several working copies:** a user can keep working copies for several projects and servers; the File › Open › From BIMserver dialog marks projects that have a local working copy and whether it has unsynchronised changes.
- **Discard:** *Discard working copy* (with confirmation) deletes the folder, after offering to export it as a standalone `.ifc`.

**Commands.** `workingcopy.status() -> {base_roid, unsynced_changes, queued_ops, last_sync}` (query); `workingcopy.list() -> working_copy_summary[]` (query); `workingcopy.discard(poid, export_path?) -> ()`

**Acceptance criteria.**
- [ ] With the network disconnected, a user opens a BIMserver project from the working copy, edits it, saves and closes; on reconnection, synchronise uploads the edits.
- [ ] Killing the Reviera process after 10 unsaved edits and reopening restores all 10 from the journal.
- [ ] A borrow made offline on an element that another user borrowed meanwhile is reported on reconnection, and the element is flagged for conflict review.

---

## 6.6 Linking

All links are registered in RXD document `project/links` (kinds `link.ifc`, `link.cad`, `link.pointcloud`, `link.pdf`, `link.image`). Each link has a **link type** (the source and how to load it) and one or more **link instances** (placement in the host). Linked content is read-only in the host.

Common link fields: `id`, `name`, `source` (see each type), `pathType` (relative, absolute, or server), `status` (loaded, unloaded, not found, error), `referenceType` (attachment or overlay, IFC links only), and per instance `transform` (4×3 matrix in host internal coordinates), `positioning` (how it was placed), `namedPosition` (COL-POS-004), `workset`, `pinned`.

### IFC links
**ID:** COL-LNK-001 · **Priority:** P1 · **Ref:** research/04 §8.1 (*Revit link*, *IFC link*); TECH_STACK §3.7.2 (*Linked models*)

**Purpose.** Reference other models (consultants' models, other buildings, other sub-projects) for coordination and documentation.

**Workflow.**
1. Insert › Link › Link IFC: choose the **source**:
   - *Local file*: an `.ifc` or `.ifczip` file;
   - *BIMserver project*: pick a server, project and either *latest* (follows new revisions) or a *specific revision* (pinned).
2. Choose positioning: *Auto – origin to origin* (internal origins aligned), *Auto – project base point to project base point*, *Auto – by shared coordinates* (requires both models to have map conversions, COL-POS-003), *Auto – centre to centre*, *Manual – origin*, *Manual – base point*, *Manual – centre* (the user clicks the insertion point).
3. Choose the workset for the link instance (BIMserver mode) and *Attachment* or *Overlay*.

**Behaviour & rules.**
- **Loading:** the linked file is parsed with IfcOpenShell into a read-only model in its own element space. If it carries Reviera Extension Data, its views and annotation are available for *By linked view* display (COL-LNK-007); other tools' `IfcAnnotation` content is shown as read-only drafting.
- **BIMserver sources:** the link is downloaded like a revision (COL-BIM-003) into the working-copy `cache/`. For *latest* links, a new revision of the linked project triggers a notification "Link <name> has a new revision" with *Reload*. Source syntax: `bimserver://<server-id>/<poid>` (latest) or `bimserver://<server-id>/<poid>/<roid>` (pinned).
- **Local file sources:** Reviera watches the file and offers *Reload* when it changes.
- **Selection and use:** linked elements can be selected with Tab, or directly when the *Select links* toggle is on; they can be tagged, dimensioned, scheduled (*Include elements in links*), used as references for snapping and hosting of view annotation, copied into the host (as new elements with new GlobalIds), monitored (COL-CM-001) and clash-checked (COL-CLASH-001). References use `link:<linkId>/ifc:<GlobalId>` (Chapter 2a).
- **Room bounding:** a link type option makes linked walls, floors and columns bound host rooms.
- **Phase mapping:** each link type maps the linked model's phases (from its RXD, or from IFC status properties) to host phases; unmapped phases map to the nearest earlier host phase.
- **Nested links:** links inside a linked project are shown if their reference type in that project is *Attachment*, and hidden if *Overlay*.
- **Multiple instances** of the same link type are allowed (for example a repeated building), each with its own transform and named position.
- **Linking across modes:** a Local project may link a BIMserver project (when the user is logged in), and a BIMserver project may link a local file; a server-sourced link is stored as a server reference, and a local file link as a path relative to the project when possible.
- **Performance:** linked models are tessellated and indexed in parallel on load; the graphics resolver treats them as additional element sources (Chapter 5, DOC-HLR-001).

**Commands.** `link.add_ifc(source, positioning, point?, workset_id?, reference_type?: attachment|overlay) -> {link_id, instance_id}`; `link.add_instance(link_id, positioning, point?) -> instance_id`; `link.set_phase_map(link_id, map{}) -> ()`; `link.set_room_bounding(link_id, on) -> ()`; `link.copy_elements(instance_id, element_refs[]) -> element_id[]`

**Acceptance criteria.**
- [ ] Linking a structural model from BIMserver (latest) shows it in the host; when the structural engineer checks in a new revision, the architect gets a notification and *Reload* shows the new geometry.
- [ ] A host plan tags a linked column, and the tag survives a reload.
- [ ] A linked model placed *by shared coordinates* lands at the correct position relative to the host within 1 mm.

### CAD links and imports
**ID:** COL-LNK-002 · **Priority:** P1 (DXF), P1 (DWG), P3 (DGN, SAT, SKP, 3DM) · **Ref:** research/04 §8.1 (*CAD Link vs Import*)

**Purpose.** Use 2D and 3D CAD drawings as underlays and backgrounds.

**Workflow.** Insert › Link › Link CAD (keeps a reference and reloads) or Insert › Import › Import CAD (copies the content into the project). Options: *Current view only* (the CAD becomes view-owned in that view) or all views; colours (black and white, preserve, invert); layers (all, visible only, specify); import units (auto-detect from the file header, or a chosen unit); *Correct lines slightly off axis* (snaps lines within 0.1° of the axes); positioning (as COL-LNK-001, plus *Auto – by shared coordinates* using the file's coordinates as shared coordinates); level to place at; orient to view.

**Behaviour & rules.**
- **Readers:** DXF and DWG through `acadrust` (MPL-2.0), the library CADCraft uses, so drawings from CADCraft (DM-IMP-013) read as CADCraft reads them; LibreDWG (GPL-3.0-or-later) is a fallback for DWG versions `acadrust` does not read. Unsupported entities (for example proxy objects) are skipped and listed.
- CAD layers become sub-categories of an *Imported Objects* category for that file (Chapter 5, DOC-GFX-002), with per-view visibility in the graphics dialog's Imported Categories tab.
- *Query* reports the layer and entity type of a picked CAD element; *Delete layers* removes layers from an import.
- **Explode** (imports only): *Partial* (one level of blocks) or *Full*, turning CAD geometry into detail lines, text and filled regions (view-specific imports) or model lines (all-view imports).
- Snapping to CAD geometry (end points, mid points, intersections, centres) is supported.
- Linked CAD is stored as a link (`link.cad`); imported CAD is stored as RXD drafting content of the view (current view only) or as an RXD `import.cad` object with its own geometry (all views). Imported CAD geometry is never written into IFC elements, except as export annotation when the user exports documentation (Chapter 5, DOC-IFC-002).

**Commands.** `link.add_cad(path, view_id?, colours?, layers?, units?, correct_off_axis?, positioning, level_id?) -> {link_id, instance_id}`; `import.cad(path, view_id?, options) -> import_id`; `import.explode(import_id, mode: partial|full) -> element_ids[]`; `import.delete_layers(import_id, layers[]) -> ()`; `import.query(import_id, pick_ref) -> cad_entity_info` (query)

**Acceptance criteria.**
- [ ] A DWG site survey linked by shared coordinates appears at its survey coordinates.
- [ ] Hiding a CAD layer in a view hides only that layer's geometry in that view.
- [ ] Fully exploding a view-specific import produces detail lines with line styles derived from the CAD layers.

### Point clouds
**ID:** COL-LNK-003 · **Priority:** P2 · **Ref:** research/04 §8.1 (*Point cloud*)

**Purpose.** Use laser scans of existing buildings as references.

**Workflow.** Insert › Link › Point Cloud: choose E57, LAS, LAZ, COPC (cloud-optimised LAZ) or PLY files; positioning (as COL-LNK-001); Reviera indexes the file on first use.

**Behaviour & rules.**
- **Readers:** libE57Format (Boost licence) for E57; `las`/`laz` Rust crates (MIT/Apache) for LAS/LAZ; Reviera's own PLY reader.
- **Indexing:** every cloud is converted once into an octree in the open COPC format (LAZ 1.4 with a hierarchical index), stored next to the source file or in the working-copy cache. Indexing runs in the background on all cores, with progress. Scans in a multi-scan E57 file become *scan regions*.
- **Display:** the GPU viewport streams octree nodes by screen-space error within a point budget (default 5 million points), with point size and eye-dome lighting for depth perception. Colour modes: normal (RGB), single colour, by elevation, by intensity, by normal.
- Point clouds are clipped by section boxes, crop regions and view ranges like model geometry; they appear in hidden-line views as points (not as linework).
- **Snapping:** to points, and to planes and edges detected locally around the cursor (RANSAC plane fit in a small neighbourhood).
- Per-view visibility per cloud and per scan region (graphics dialog, Point Clouds tab).

**Commands.** `link.add_pointcloud(path, positioning, point?) -> {link_id, instance_id}`; `pointcloud.reindex(link_id) -> job_id`; `view.set_pointcloud_display(view_id, link_id, visible?, colour_mode?, regions?) -> ()`

**Acceptance criteria.**
- [ ] A 500-million-point E57 scan is indexed in the background and then orbits at 30 fps or more on the reference GPU with the default budget.
- [ ] Snapping to a detected wall plane lets the user draw a wall aligned with the scan within 5 mm.

### PDF underlays
**ID:** COL-LNK-004 · **Priority:** P1 · **Ref:** research/04 §8.1 (*PDF / Image (underlay)*)

**Purpose.** Trace over and measure from PDF drawings (surveys, consultants' plans, old drawings) in a plan, section, elevation or drafting view.

**Workflow.**
1. Insert › Import › PDF: choose a file; a page picker shows thumbnails (multi-page files); choose the page, a resolution for raster content (72–1200 dpi, default 150), *Link* (reloadable, default) or *Import* (copied into the project assets), and the draw layer.
2. Click to place the page's centre in the current view (or press Enter to place it at the view centre). The page is placed at 1:1 paper size scaled by the view scale.
3. Optionally *Calibrate*: click two points on the underlay and type their real distance; the underlay is scaled about the first point.

**Behaviour & rules.**
- Pages are rendered with PDFium (BSD/Apache licence). Vector content is also extracted as geometry so that the user can **snap** to PDF lines and arcs, and **scale-calibrate** by picking two points and entering their real distance.
- *Draw layer*: background (below model) or foreground.
- A PDF underlay is view-owned; it can be hidden, moved, rotated, scaled, pinned.
- Only plan, ceiling, section, elevation, drafting and detail views accept a PDF underlay; 3D views and schedules disable the command. Sheets use image placement (COL-LNK-005).
- **Rendering cache:** the page is rasterised once per dpi and cached in the working-copy or project cache folder; zooming in beyond the cached resolution re-renders the visible tiles at higher resolution in the background, up to the chosen dpi × 4.
- **Vector extraction:** paths are flattened to lines and arcs in page coordinates; text is not extracted. Pages with more than 2 million path segments keep snapping off and show a note.
- **Calibration** keeps the aspect ratio. The real distance must be greater than zero and the two points at least 1 mm apart on paper; otherwise the command fails with `underlay.bad_calibration`. Calibration is undoable and can be repeated.
- **Linked PDFs:** a changed source file is detected on open and on *Reload* (COL-LNK-006); a missing file shows the last cached raster greyed with "not found", and the underlay keeps its position and calibration. If the reloaded page has different paper dimensions, Reviera keeps the calibration scale and warns.
- **Imported PDFs** are stored as RXD assets and do not depend on the source file.
- The underlay never prints when the view property *Show underlays in print* is off (default on); it is never exported to IFC.
- **Interactive placement and calibration** follow the micro-operation rule (conventions §2.1): micro-ops `pdf.place_point{xy}` and `underlay.calib_point{index, xy}`, `underlay.calib_distance{value}`; replay re-applies the recorded points and reopens the calibration prompt at the last missing step.

**Data (IFC/RXD).** Linked PDF: RXD document `project/links`, kind `link.pdf` (`source`, `page`, `dpi`, `pathType`, `status`) with one instance per placement holding `viewId`, transform, `layer` and calibration scale. Imported PDF: the same kind with `source` pointing at an RXD asset. The underlay is not an IFC entity.

**Commands.** `link.add_pdf(path, page, view_id, dpi?, link?, position, layer?: background|foreground) -> underlay_id`; `underlay.calibrate(underlay_id, point_a, point_b, real_distance) -> ()`. Reload, reload from, unload and remove use the link commands of COL-LNK-006.

**Acceptance criteria.**
- [ ] After calibration with a known 10 m dimension, a 5 m line in the PDF measures 5 m (±0.5 %).
- [ ] The cursor snaps to end points of PDF vector lines.
- [ ] Deleting the source of a linked PDF shows the underlay greyed as "not found" at the same position; restoring the file and reloading shows it normally with its calibration kept.
- [ ] *Link PDF* is disabled in a 3D view.
- [ ] Killing Reviera after the first calibration point and reopening resumes calibration at the second point.

### Image underlays
**ID:** COL-LNK-005 · **Priority:** P1 · **Ref:** research/04 §8.1 (*PDF / Image*)

**Purpose.** Place raster images (site photos, scanned drawings, logos) as underlays in views or as artwork on sheets.

**Behaviour & rules.**
- Insert › Import › Image: PNG, JPEG, TIFF (single page; multi-page TIFF uses the first page and warns), BMP, WebP; link or import; placed in the current view (or on a sheet, Chapter 5, DOC-SHT-001); draw layer background or foreground; calibrate scale by two points (as COL-LNK-004, same `underlay.calibrate` and micro-ops).
- **Placement:** two clicks for opposite corners, or one click with the image's own size (from its dpi metadata, 96 dpi if none) scaled to the view; *Lock proportions* is on by default. `width` sets the placed width in model units (or paper units on sheets).
- **Limits:** images up to 16 384 × 16 384 pixels; larger images are refused with `image.too_large` and the user is offered to downscale on import. Colour images with alpha keep transparency.
- **Linked images** reload when the source changes; a missing source shows a placeholder frame with "not found" and keeps size and position.
- **Imported images** are stored as RXD assets (Chapter 2a), de-duplicated by SHA-256 hash, so placing the same image twice stores it once.
- **Manage Images** (Insert › Import › Manage Images) lists every image asset and linked image: name, source path (for linked), pixel size, file size, number of placements and the views or sheets using them. Actions: *Delete* (an image in use asks to delete its placements too, or to replace it with another image via *replace_with*, which keeps every placement's size and position), *Reload*, *Show* (opens a view using it).
- In BIMserver mode, image assets travel with the model and merge by asset hash; two users importing the same file produce one asset.

**Data (IFC/RXD).** Kind `link.image` in RXD document `project/links` (linked) or RXD asset plus a placement record (imported); placements are view- or sheet-owned. Not written to IFC.

**Commands.** `link.add_image(path, view_id, link?, position, width?, layer?) -> underlay_id`; `image.manage_list() -> image_info[]` (query); `image.delete(image_ids[], replace_with?) -> ()`. The Manage Images dialog is shared with Chapter 2 (`image.manage`, DM-IMG-001); `image.manage_list` returns the same records as Chapter 2's `image.list` and is kept as an alias.

**Acceptance criteria.**
- [ ] An imported image still displays after the source file is deleted; a linked one shows "not found" in Manage Links.
- [ ] Importing the same PNG into two views stores one asset, and Manage Images shows two placements.
- [ ] Deleting an image in use with *replace_with* swaps the picture in every placement without changing their sizes or positions.

### Manage Links
**ID:** COL-LNK-006 · **Priority:** P1 · **Ref:** research/04 §8.2 · screenshots/documentation/manage_links_new_dialog_type_filter.png

**Purpose.** One place to see and control every link and import.

**Workflow.** Insert › Link › Manage Links, or Manage › Manage Project › Manage Links (`link.manage`). The dialog is a single tree of all links and imports (IFC, CAD, point clouds, PDF, images) with a type filter, search, and a collapsible details panel.

**UI (Manage Links dialog).** Modeless and resizable; it remembers its size, columns and filter per user.
- **Toolbar:** *Type* filter (All, IFC, CAD, Point clouds, PDF, Images), search box (matches name and source), *Show imported CAD* check box, column chooser (gear), and the action buttons below. Actions that do not apply to the selected rows are disabled with a tooltip giving the reason.
- **Tree:** one row per link (link type); its instances are child rows (name, placement summary, named position, workset). Nested links of an IFC link appear as grandchildren, read-only, marked *nested (attachment)*; nested overlays are not listed because they are never loaded.
- **Details panel** (collapsible, for the selected row): full source (copyable), resolved absolute path, file size and modification time, last load time and duration, element count, schema (IFC2X3, IFC4, IFC4X3), map conversion present yes/no, room bounding, phase map summary, and the error text for links in *error* state.
- **Context menu** per row: the same actions plus *Copy source*, *Open source folder* (local files), *Open in new window* (IFC links: opens the source read-only as its own model).

**Behaviour & rules.**
- **Columns** (choosable): name, type, status (loaded, unloaded, not found, error), source (path or `bimserver://` reference; copyable), path type (relative, absolute, server), reference type (attachment, overlay), revision (for BIMserver links: pinned revision or "latest", and whether a newer revision exists), positions not saved, size, instances.
- **Link kinds and what applies to each:**

| Kind | Sources | Reference type | Instances | Positions | Worksets | Bind |
|---|---|---|---|---|---|---|
| IFC (`link.ifc`) | local `.ifc`/`.ifczip`, `bimserver://` | Attachment / Overlay | Many | Yes (COL-POS-004) | Yes, if the link has worksets | Yes |
| CAD (`link.cad`) | local `.dxf`/`.dwg` | — | Many (one per view for *current view only*) | Yes (CAD shared coordinates) | — | No (use Import CAD, then Explode) |
| Point cloud (`link.pointcloud`) | local E57/LAS/LAZ/COPC/PLY | — | Many | No | — | No |
| PDF (`link.pdf`) | local `.pdf` | — | One per view placement | No | — | No (re-insert as *Import*) |
| Image (`link.image`) | local raster files | — | One per placement | No | — | No |
| Imported CAD (`import.cad`, shown with *Show imported CAD*) | none (content is in the project) | — | One | No | — | — |

- **Status values:**
  - *Loaded*: parsed and displayed.
  - *Unloaded*: deliberately not loaded; kept in the registry with all instances, tags and monitors, which reappear on reload.
  - *Not found*: the source cannot be resolved (missing file, unknown server profile, deleted server project or revision).
  - *Error*: the source was found but failed to load (parse error, unsupported schema, decryption failure, missing deserialiser); the details panel shows the message.
  - *Loading…*: transient, with progress; large links load in the background and the row shows a percentage.
  - BIMserver links also show *Newer revision available* (latest-following links) or *Pinned* in the revision column.
- **Actions** (on one or many rows): Reload, Reload From… (a different file, or a different server project or revision), Unload, Remove, Add, Bind (IFC links: copy the linked elements into the host as new elements with new GlobalIds, after a warning), Save Positions (write a moved instance's position back as a named position, COL-POS-004), Show (select the instance or underlay in a view), Manage Worksets (open or close the linked model's worksets per instance).
  - *Reload* re-reads the source; elements are re-matched by GlobalId so that tags, dimensions, monitors, clash results and issues keep their references; references whose element disappeared are orphaned and listed (COL-CM-003 for hosted elements). After reload, monitored pairs are re-checked (COL-CM-002).
  - *Reload From…* changes the source and reloads. Changing between a local file and a server source is allowed. If fewer than 50 % of referenced GlobalIds are found in the new source, Reviera warns before committing ("this looks like a different model").
  - *Unload* keeps the link and its instances; annotation that refers to unloaded elements is hidden, not deleted.
  - *Remove* deletes the link type, all its instances, and asks what to do with annotation and monitor pairs that refer to it (*Delete them* or *Keep as orphans*); the confirmation states the counts.
  - *Add* opens the matching insert command (Link IFC, Link CAD, Point Cloud, PDF, Image) for the chosen type filter.
  - *Bind* is refused while the link is unloaded or not found, and for links with more than 200 000 elements unless confirmed twice.
- **Path type:** *Relative* (to the project file's folder, or to the working-copy folder for BIMserver projects), *Absolute*, or *Server* (`bimserver://`, set automatically for server sources and not selectable for files). Changing to *Relative* is refused for a file on another drive or volume, and the row reports why. Local mode stores relative paths by default.
- **Reference type and nesting:** *Attachment* means the link is carried along when this project is itself linked into another model; *Overlay* means it is ignored there. Only IFC links have a reference type (default *Overlay*, so that consultant models are not duplicated downstream). Nesting depth shown and loaded is limited to 4; deeper attachments are listed with a warning and not loaded. A circular link (A attaches B, B attaches A) is detected on load; the repeat is not loaded and a warning names the cycle.
- **Batch edit:** change path type and reference type of many links at once.
- **Unload for me / for everyone:** in BIMserver mode, *Unload for me* is a client preference; *Unload for everyone* changes the shared RXD link status (requires ownership of the link object).
- *Show imported CAD* lists imports as well as links.
- Selection in the dialog and selection in the canvas stay in step.
- **Undo:** link registry edits (remove, path type, reference type, bind, open worksets) are one undo step per dialog action. Reload, Reload From and Unload change what is loaded, not project data (except the source of *Reload From* and the status of *Unload for everyone*), and are not undoable.

**Data (IFC/RXD).** Everything listed is in RXD document `project/links` (kinds above; common fields in §6.6 introduction). *Unload for me* and per-instance open linked worksets for the current user are client state; *Unload for everyone* and the shared default of open linked worksets are RXD.

**Commands.** `link.manage() -> ()` opens the Manage Links dialog (UI only); `link.show(link_or_instance_id) -> view_id` (selects the instance or underlay and zooms to it in a suitable view, opening one if needed); `link.list() -> link_summary[]` (query); `link.reload(link_ids[]) -> ()`; `link.reload_from(link_id, new_source) -> ()`; `link.unload(link_ids[], for_everyone?) -> ()`; `link.remove(link_ids[]) -> ()`; `link.bind(link_id) -> element_id[]`; `link.set_path_type(link_ids[], path_type) -> ()`; `link.set_reference_type(link_ids[], reference_type) -> ()`; `link.set_open_worksets(instance_id, workset_names[]) -> ()`

**Acceptance criteria.**
- [ ] Changing ten links from absolute to relative paths in one batch edit updates all ten in one undo step.
- [ ] A link whose file was moved shows *Not found*, and *Reload From* with the new path restores it without losing tags on its elements.
- [ ] A link set to *Overlay* in project B is not shown when B is linked into project C; switching it to *Attachment* and saving B makes it appear in C as a nested link after reload.
- [ ] Two projects that attach each other load without hanging, and a warning names the cycle.
- [ ] Unloading a link hides the tags on its elements; reloading shows them again unchanged.
- [ ] Changing a link on drive D: to a relative path from a project on drive C: is refused with a reason.

### Link visibility and display settings
**ID:** COL-LNK-007 · **Priority:** P1 · **Ref:** research/04 §3.3 (*Revit Links* tab), §8.2 (*Visibility of links*)

**Behaviour & rules.**
- In the graphics dialog's Links tab (Chapter 5, DOC-GFX-003), per link instance: visible, halftone, underlay, and *Display settings*:
  - **By host view**: linked elements go through the host view's graphics layers (categories, filters, phase, detail level);
  - **By linked view**: the link is drawn as a chosen view of the linked model shows it (requires the linked model's RXD; its view range, overrides, annotation categories and phase are used);
  - **Custom**: per tab, choose host, linked view or custom settings for model categories, annotation categories, filters, worksets, phase (with phase mapping), detail level and nested links.
- *Line weights from host or linked model*: whether linked elements use the host's pen tables or the link's.
- Host view filters can target linked elements when the link uses host or custom filter settings.
- Visibility of a linked model's worksets is per instance (*Manage Worksets*).

**Data (RXD).** In the host view's payload: `linkOverrides` keyed by link instance id.

**Commands.** `view.set_link_display(view_id, instance_id, mode: host|linked_view|custom, linked_view_id?, custom?, visible?, halftone?, underlay?, line_weights_from?: host|link) -> ()`

**Acceptance criteria.**
- [ ] With *By linked view* set to the structural model's Level 1 plan, the host plan shows the linked columns exactly as that structural plan does.
- [ ] Halftoning a link instance halftones all of its elements in that view only.

---

## 6.7 Positioning and coordinate systems

### Internal origin, project base point and survey point
**ID:** COL-POS-001 · **Priority:** P0 (internal origin, project base point), P1 (survey point, shared coordinates) · **Ref:** research/04 §8.3

**Purpose.** Locate the model in project terms and in the real world.

**Behaviour & rules.**
- **Internal origin:** the origin of the IFC model's world coordinate system (the `WorldCoordinateSystem` of the project's `IfcGeometricRepresentationContext`, normally at (0,0,0) with no rotation). All Reviera geometry is stored relative to it. It cannot be moved. It can be shown in views through the *Site › Internal Origin* sub-category.
- **Project base point (PBP):** the origin of the *project coordinate system* that users dimension from and report spot coordinates against (Chapter 5, DOC-ANN-006/007). Its properties are its position relative to the internal origin and the *angle to true north*. Moving the PBP changes the project coordinate system only; the model does not move. Pinning prevents accidental moves.
- **Survey point (SP):** a known point of the *shared coordinate system*, which is the real-world map grid (for example a national grid). Its properties are its shared coordinates (easting, northing, elevation). Moving the survey point graphically, or *Specify coordinates at a point* (COL-POS-004), redefines the shared coordinates of the whole model; the model does not move relative to the internal origin.
- **Distance warning:** if model geometry lies more than a set distance from the internal origin (default 20 km, user setting), Reviera warns that geometric operations may lose precision, and offers to move the geometry and adjust the project base point and shared coordinates so that real-world positions are preserved.
- **Plan orientation:** each plan has *Orientation: Project North / True North* (Chapter 5, DOC-GFX-015).
- **Rotate project north:** rotates the project coordinate system (and so the presentation of every plan set to project north), for example to square the building on sheets. It changes the PBP angle; the model does not rotate relative to the internal origin.
- **Rotate true north:** rotates the shared coordinate system relative to the project (changes the map conversion angle, COL-POS-003).

**Data (IFC/RXD).** Project base point position and angle, and survey point marker position: RXD document `project/settings` (section `positioning`). Shared coordinates and true north: IFC entities, see COL-POS-003.

**Commands.** `coord.set_project_base_point(position?, angle_to_true_north?) -> ()`; `coord.set_survey_point(internal_position?, shared_coordinates?) -> ()`; `coord.rotate_project_north(angle) -> ()`; `coord.rotate_true_north(angle) -> ()`; `coord.relocate_to_origin(shift?) -> ()`

**Acceptance criteria.**
- [ ] Moving the project base point changes spot coordinates relative to it but no element's internal position.
- [ ] Rotating project north by 30° rotates every plan set to project north on screen and on sheets, while views set to true north are unchanged.
- [ ] Geometry placed 25 km from the internal origin triggers the distance warning; accepting the relocation keeps every element's shared coordinates unchanged.

### Geographic location
**ID:** COL-POS-002 · **Priority:** P1 · **Ref:** research/04 §8.3 (*Location*, *Sites*)

**Purpose.** Record where on Earth the project is, for sun studies, climate data and georeferencing.

**Workflow.**
1. Manage › Project Location › Location opens the Location dialog with three tabs: *Location*, *Weather*, *Site*.
2. *Location* tab: choose *Define location by*: *Address* (search box, results list), *Coordinates* (latitude, longitude in decimal degrees or D°M′S″), or *Map* (click the site on an online map). Elevation and time zone are filled from the result where known and can be edited.
3. *Weather* tab: pick a nearby weather station or a climate file (EPW) reference for analysis (Chapter 4).
4. OK writes the location; Cancel discards.

**Behaviour & rules.**
- Manage › Project Location › Location: set the site by address search (optional online geocoding service; user-configurable, off by default for privacy), by latitude and longitude, or by picking on a map (optional online map tiles). Elevation above sea level, time zone and a weather station or climate file reference (Chapter 4 analysis) are stored too.
- **Privacy:** when the online service is off, the *Address* and *Map* options are disabled with a tooltip explaining how to enable them in Options; no request leaves the machine. The service URL is a user setting (any Nominatim-compatible geocoder and any XYZ tile server).
- **Validation:** latitude −90…90°, longitude −180…180°, elevation −500…9 000 m; out-of-range values are refused inline. The time zone is an IANA name (for example `Europe/London`); daylight saving is derived from it.
- Latitude, longitude and elevation are written to `IfcSite.RefLatitude`, `RefLongitude` and `RefElevation`.
- **IFC form:** latitude and longitude are written as `IfcCompoundPlaneAngleMeasure` (degrees, minutes, seconds, millionths of a second, with a common sign). Reviera converts from decimal degrees with rounding to the nearest millionth of a second (about 0.03 mm) and reads both four- and three-part forms.
- If the project has several `IfcSite` objects (several buildings), the location is written to the site chosen as *project site* (default: the first); the others keep their own values.
- The location does not move the model and does not change shared coordinates. If a CRS is set (COL-POS-003), Reviera compares the location with the position implied by the map conversion and warns when they differ by more than 1 km ("Location and shared coordinates disagree"), offering to update either one.
- The location drives sun position (Chapter 5, DOC-GFX-012, DOC-OUT-011) and, when a CRS is chosen, can be converted to and from map coordinates with PROJ (MIT licence).

**Data (IFC/RXD).** `IfcSite.RefLatitude`, `RefLongitude`, `RefElevation`, and `IfcPostalAddress` (on `IfcSite.SiteAddress`) for the address. Time zone, weather station and climate file reference: RXD document `project/location`.

**Commands.** `coord.set_location(latitude, longitude, elevation?, time_zone?, address?) -> ()`

**Acceptance criteria.**
- [ ] Setting latitude and longitude writes them to `IfcSite` in the IFC degree-minute-second-microsecond form and reads back the same values.
- [ ] With the online service off, no network request is made when the Location dialog is opened and used.
- [ ] Entering latitude 95° is refused inline.
- [ ] Setting a location 5 km from the position implied by the project's map conversion shows the disagreement warning.

### IFC georeferencing: IfcMapConversion and IfcProjectedCRS
**ID:** COL-POS-003 · **Priority:** P1 · **Ref:** research/04 §6.4 (*Geographic Reference*), §8.3

**Purpose.** Store shared coordinates in standard IFC, so that every IFC tool places the model correctly.

**Behaviour & rules.**
- **Coordinate reference system:** Manage › Project Location › Coordinate System: choose a projected CRS by EPSG code or name search (from the PROJ database), plus a vertical datum. Reviera writes an `IfcProjectedCRS` with `Name` (for example `EPSG:27700`), `Description`, `GeodeticDatum`, `VerticalDatum`, `MapProjection`, `MapZone` and `MapUnit` (an `IfcNamedUnit`) where known.
- **Map conversion:** shared coordinates are the image of internal coordinates under one similarity transform, written as an `IfcMapConversion` with `SourceCRS` = the project's 3D model `IfcGeometricRepresentationContext`, `TargetCRS` = the `IfcProjectedCRS`, and:
  - `Eastings`, `Northings`, `OrthogonalHeight`: the shared coordinates of the internal origin;
  - `XAxisAbscissa`, `XAxisOrdinate`: cosine and sine of the angle from the internal x axis to the map's easting axis (grid north rotation);
  - `Scale`: the ratio of map units to model length units, combined with the CRS's scale factor at the site if the user enables *Apply grid scale factor* (default off; IFC4X3 projects may use the scaled map-conversion entity where the schema provides one).
- **Deriving the conversion from the survey point:** with survey point internal position `s`, its shared coordinates `(E, N, H)`, and rotation `θ`, the translation is `(E, N, H) − R(θ)·s`, where `R(θ)` rotates about the vertical axis. Reviera recomputes and rewrites `IfcMapConversion` whenever the survey point, shared coordinates or true-north rotation change.
- **True north:** `IfcGeometricRepresentationContext.TrueNorth` is set to the direction of geodetic true north in internal coordinates. Grid north (from the map conversion) and true north differ by the meridian convergence at the site, which Reviera computes with PROJ; the UI shows both and lets the user say which one *Angle to true north* means (default: grid north, which is what surveys normally give).
- **Reading:** when a foreign IFC with `IfcMapConversion` is opened or linked, Reviera derives its shared coordinate system from it. IFC2X3 files with only the IFC2X3 property-set convention for georeferencing (`ePSet_MapConversion` / `ePSet_ProjectedCRS`) are read too.
- **Export coordinate base** (Chapter 5, DOC-OUT-006): *internal origin* writes geometry unchanged with the map conversion; *project base point* or *shared coordinates* write the IFC with geometry shifted so that the chosen origin becomes (0,0,0), and adjust the map conversion so that real-world positions are unchanged.

**Commands.** `coord.set_crs(epsg_or_wkt, vertical_datum?, apply_scale_factor?) -> ()`; `coord.get_georeference() -> {crs, map_conversion, true_north, grid_convergence}` (query)

**Acceptance criteria.**
- [ ] A model with CRS EPSG:27700 and survey point coordinates set opens in another IFC tool that supports georeferencing at the correct national-grid position (checked at three points, within 1 mm).
- [ ] Changing the survey point's shared coordinates updates `IfcMapConversion` and leaves internal geometry unchanged.
- [ ] Exporting with coordinate base *shared coordinates* produces geometry whose coordinates equal the survey-based shared coordinates.

### Acquire, publish, specify and report coordinates; named positions
**ID:** COL-POS-004 · **Priority:** P1 · **Ref:** research/04 §8.3 (*Coordinate tools*, *Positioning options*, *Reposition a link*)

**Purpose.** Agree on shared coordinates between linked models.

**Behaviour & rules.**
- **Acquire coordinates:** pick a link instance (IFC or CAD); the host adopts the shared coordinate system implied by the link's placement and the link's own shared coordinates (its map conversion, or for CAD the file's coordinates), so that the host's real-world position matches the link's.
- **Publish coordinates:** pick a link instance; the host's shared coordinate system, expressed in the link's internal coordinates, is recorded as a **named position** for that link. For a local IFC link, *Save positions* writes the new map conversion into the linked file (after confirmation, and only if the file is writable and not open elsewhere). For a BIMserver link, saving positions means checking in a new revision of the linked project containing only the updated georeference (requires ownership of the linked project's georeference, through its ownership document). Until saved, Manage Links shows *Positions not saved*.
- **Specify coordinates at a point:** click a point and enter its shared coordinates (easting, northing, elevation) and optionally the angle to true north; the survey point and map conversion are updated.
- **Report shared coordinates:** click a point to read its shared coordinates (and project coordinates); also available as `coord.report_point` for scripts.
- **Reset shared coordinates:** makes the shared coordinate system equal to the internal one (identity map conversion), after confirmation.
- **Named positions (sites):** a project can hold several named shared positions (for example, the same building placed at two sites). Each link instance records which named position of the linked model it uses, and *Auto – by shared coordinates* asks which named position to use when the link has more than one. Only the *current* named position is written to the project's `IfcMapConversion`; the others are RXD (`project/settings`, section `positioning.namedPositions`).
- **Moving a link instance:** after moving or rotating an instance placed by shared coordinates, Reviera asks on save whether to *Record the new position* in the link (as publish), *Keep the old shared position* (the instance becomes manually positioned), or *Cancel*.

**Commands.** `coord.acquire(instance_id) -> ()`; `coord.publish(instance_id, position_name?) -> ()`; `link.save_positions(link_ids[]) -> ()`; `coord.specify_at_point(point, easting, northing, elevation, angle?) -> ()`; `coord.report_point(point) -> {shared, project, internal}` (query); `coord.reset_shared() -> ()`; `coord.create_named_position(name) -> position_id`; `coord.set_current_position(position_id) -> ()`

**Acceptance criteria.**
- [ ] After acquiring coordinates from a survey DWG, a spot coordinate on a survey control point shows the survey's coordinates.
- [ ] After publishing coordinates to a linked IFC and saving positions, re-linking that file *by shared coordinates* in a new project places it at the same real-world position as in the original host.

---

## 6.8 Coordination

### Copy/Monitor
**ID:** COL-CM-001 · **Priority:** P1 (levels, grids), P2 (columns, walls, floors, openings, MEP fixtures) · **Ref:** research/04 §8.4

**Purpose.** Create host copies of key elements from a linked model (or pair existing elements) and watch the pairs for changes.

**Workflow.**
1. Collaborate › Coordinate › Copy/Monitor ▾ › *Select Link* (pick a link instance) or *Use Current Project* (monitor two elements in the host).
2. Contextual tab *Copy/Monitor*:
   - **Options**: per category, a mapping from linked types to host types (levels, grids, columns, walls, floors), plus offsets (for example a level offset), name prefixes or suffixes, *Copy hosted openings, doors and windows* for walls, *Split columns by level*, and which parameters to monitor.
   - **Copy**: pick elements (or *Multiple* with a window selection, then Finish). Each copy is created in the host with the mapped type and a new GlobalId, at the linked element's position, and paired with it.
   - **Monitor**: pick a host element, then the linked (or host) element to pair it with.
   - **Batch copy** (MEP fixtures, P2): copy all fixtures of chosen categories that match mapping rules.
   - **Finish**.
3. Monitored host elements show a monitor badge in views.

**Behaviour & rules.**
- A **monitor pair** records the host element, the source element (`link:<linkId>/ifc:<GlobalId>` or a host GlobalId), the monitored aspects (position, orientation, extent, name or mark, chosen parameters) and a **snapshot** of the source's values as last accepted.
- Pairs survive reloads, changes of link source (*Reload From*) when GlobalIds are kept, and BIMserver merges.
- Stopping monitoring (*Stop Monitoring* on the host element) removes the pair but keeps the element.
- **Starting the mode** (`copymonitor.start`): with a `link_instance_id`, the user picked a link instance (only loaded IFC link instances are pickable; CAD, point clouds and underlays are ignored by the pick filter); without it, the mode works inside the current project and *Copy* is disabled (only *Monitor* is available). Only one Copy/Monitor session can be active per model window. Starting it in a view where the link is hidden asks to show it temporarily.
- **Session behaviour:** every Copy or Monitor pick inside the session is added to a pending list shown in the status bar ("3 copied, 1 monitored"). *Finish* commits all of them as one undoable transaction; *Cancel* discards them. Options changed in the session are saved immediately (they are settings, not part of the transaction).
- **Validation on Copy:** an element whose category has no mapping or whose mapping is *Do not copy* is skipped with a note; an element already paired with a host element is not copied again (the existing pair is highlighted); a level or grid whose name would clash with an existing host name gets the mapping's prefix or suffix, then a numeric suffix if still not unique.
- **Validation on Monitor:** both elements must be of the same category (levels with levels, grids with grids, and so on), the host element must not already be monitoring another element, and in BIMserver mode the host element is borrowed when the pair is created (COL-OWN-001).
- **Ownership:** copied elements are created in the active workset and are borrowed by the creator; monitor pairs are RXD objects owned like any other.
- **Interactive tool rule** (conventions §2.1): the session declares micro-ops `cm.start{link_instance_id?}`, `cm.copy{source_ref, host_type_id, offsets}`, `cm.multiple{source_refs[]}`, `cm.monitor{host_element_id, source_ref}`, `cm.set_option{category, key, value}` and `cm.finish`. Replay after a crash rebuilds the pending list from the recorded micro-ops against the reloaded link, skips sources that no longer exist (listed), and leaves the session open for the user to *Finish* or *Cancel*.

**Data (RXD).** RXD document `coordination/monitors`, kind `monitor.pair`: `host`, `source`, `aspects[]`, `snapshot{}`, `tolerances{}`, `status`. Copy/Monitor options and coordination settings are kind `monitor.settings` in the same document (COL-CM-004). Copied elements are ordinary IFC elements with new GlobalIds.

**Commands.** `copymonitor.start(link_instance_id?) -> ()` enters the Copy/Monitor mode and shows its contextual tab (omitting the link means *Use Current Project*); `copymonitor.finish() -> pair_id[]`; `copymonitor.cancel() -> ()`; `copymonitor.set_options(link_instance_id?, category, mapping{}, options{}) -> ()`; `copymonitor.copy(link_instance_id, element_refs[]) -> element_id[]`; `copymonitor.monitor(host_element_id, source_ref) -> pair_id`; `copymonitor.stop(pair_ids[]) -> ()`; `copymonitor.batch_copy(link_instance_id, categories[], rules) -> element_id[]`

**Acceptance criteria.**
- [ ] Copying all levels from a structural link creates host levels with the mapped level type, offsets and name suffix, each paired with its source.
- [ ] Copying a wall with *Copy hosted openings* also copies its doors and windows, each monitored.
- [ ] *Cancel* after copying five grids leaves the host without those grids and without monitor pairs; *Finish* followed by one Undo gives the same result.
- [ ] Trying to monitor a host level against a linked grid is refused with a category-mismatch message.
- [ ] Killing Reviera after three copies inside a session and reopening shows the session with the three pending copies.

### Coordination review
**ID:** COL-CM-002 · **Priority:** P1 · **Ref:** research/04 §8.4 (*Coordination Review*); web: Actions for Coordination Review; web: Review Warnings for Monitored Elements

**Purpose.** Show and settle changes found on monitored pairs.

**Workflow.**
1. After a link reload (or when a host element is edited), Reviera checks every affected pair; if anything changed, a warning says "Coordination review needed for <link>".
2. Collaborate › Coordinate › Coordination Review ▾ › *Select Link* / *Use Current Project* opens the review dialog: a tree grouped by status and category, each item describing the change (moved by 120 mm; renamed from "Level 3" to "L03"; deleted; parameter X changed from a to b; type changed).
3. Per item: *Postpone* (leave for later), *Reject* (the change in the source is wrong; keep the host and record the rejection, which the source's author can see if they also run Reviera with this host linked), *Accept difference* (update the snapshot; keep the host as it is), *Modify host* (move, rename or edit the host element to match), and a comment. *Show* zooms to the pair. *Create report* writes an HTML or CSV report.

**UI (Coordination Review dialog).** Opened with `coordreview.show(link_instance_id?)`: with a link instance it reviews pairs whose source is in that link; without it, pairs inside the current project. Modeless, so *Show* can be used while it is open.
- **Header:** the scope (link instance name or "Current project"), a *Group by* choice (status → category → change kind, the default; or category → status → change kind), and a filter for status (default: pending and postponed).
- **Tree rows** (one per change): host element (category, name or mark), source element, change description, detected at, current action (drop-down), comment (editable cell), and last decided by.
- **Action drop-down**, offering only the actions valid for that change kind:

| Change kind detected | Actions offered (besides *Postpone*, *Reject*) |
|---|---|
| Moved or rotated | *Accept difference*, *Move host to match* |
| Renamed | *Accept difference*, *Rename host to match* |
| Extent or type parameter changed (height, length, level offset, type) | *Accept difference*, *Update host extents* / *Change host type* (via the type mapping) |
| Monitored parameter changed | *Accept difference*, *Copy value to host* |
| Source deleted | *Accept difference* (keep host, stop monitoring), *Delete host element* |
| Host changed (edited in the host so it no longer matches) | *Accept difference*, *Restore host from source* |
| New hosted element in a monitored source wall (opening, door, window) | *Copy new element* (copy and monitor it), *Ignore new element* |
| Opening sketch changed | *Accept difference*, *Copy sketch* |

  *Modify host* in the commands covers every "…to match" action; the change kind decides what it does.
- **Buttons:** *Apply* (carries out the chosen actions as one undoable transaction), *Show* (opens a suitable view and zooms to the host and source, with the source highlighted), *Create report…* (HTML or CSV of the listed items with their actions and comments), *Create issue* (BCF topic from the selected items, COL-BCF-001), *Settings…* (COL-CM-004), *Close*.

**Behaviour & rules.**
- Change detection compares the current source with the snapshot, using per-aspect tolerances (default: position 1 mm, angle 0.01°, extent 1 mm; user settings).
- *Modify host* is an ordinary undoable edit, subject to ownership (§6.4).
- Pairs with unresolved changes show a warning badge on the host element, and are listed in the warnings list (COL-MNT-004).
- In BIMserver mode, decisions are RXD objects and are shared with other users at the next sync.
- **When detection runs:** after each link load or reload, after *Get latest* (for pairs inside the host), and after any host edit to a monitored element (checked against the source in the same transaction, so that the warning appears at once). The trigger set can be narrowed in the settings (COL-CM-004).
- **Reject** leaves both elements unchanged and keeps the item visible with status *rejected* until the source changes again (a new item is then raised) or someone chooses another action. It does not change the linked model.
- **Accept difference** writes the current source values into the pair's snapshot.
- **Postponed** items stay in the default view and keep the warning badge; *rejected* and *accepted* items leave the default view.
- If a *Modify host* action fails (for example the target position collides with a constraint), the transaction rolls back that item only, marks it *failed* with the reason, and applies the others.
- **Unloaded or not found link:** the dialog opens read-only with a banner; no actions can be applied until the link is loaded.

**Data (RXD).** `coordination/monitors`: `review[]` entries per pair (`change`, `detectedAt`, `status: pending|postponed|rejected|accepted|modified`, `comment`, `user`).

**Commands.** `coordreview.show(link_instance_id?) -> ()` opens the dialog (UI only); `coordreview.list(link_instance_id?, status?) -> review_item[]` (query); `coordreview.resolve(item_ids[], action: postpone|reject|accept|modify, comment?) -> ()`; `coordreview.report(path, format: html|csv, filter?) -> ()`. *Create issue* uses `bcf.create_topic`.

**Acceptance criteria.**
- [ ] Moving a grid by 300 mm in the linked model and reloading lists one *moved* item; *Modify host* moves the host grid by 300 mm and clears the item.
- [ ] *Accept difference* clears the item and keeps the host grid where it was; a further 1 mm move of the source does not raise a new item if the tolerance is 1 mm.
- [ ] Adding a door to a monitored linked wall raises a *new hosted element* item; *Copy new element* creates a monitored host door in the copied host wall.
- [ ] The action drop-down for a *renamed* item does not offer *Move host to match*.
- [ ] Opening the dialog for an unloaded link shows the items read-only with the banner.

### Reconcile hosting
**ID:** COL-CM-003 · **Priority:** P2 · **Ref:** research/04 §8.4 (*Reconcile Hosting*); web: Orphaned elements and the Reconcile Hosting browser

**Purpose.** Find and repair host elements and annotation that lost the linked element or face they were hosted on.

**Workflow.**
1. After a link reload that orphans anything, a warning says "n elements lost their host in <link>" with *Reconcile*.
2. Collaborate › Coordinate › Reconcile Hosting (`hosting.reconcile`) opens the *Reconcile Hosting* palette, docked on the right by default.
3. The user selects orphans and chooses an action; the list updates as orphans are resolved.

**UI (Reconcile Hosting palette).**
- A tree grouped by link instance, then by category, then one row per orphan: category, family and type, mark, the lost host (category and GlobalId of the linked element), the reason, and the view (for view-owned annotation such as tags).
- *Show orphans of*: all links or one link instance (the `link_instance_id` parameter).
- **Actions** (toolbar and context menu): *Show* (opens a suitable view and zooms to the orphan), *Pick new host* (pick a face or element in the canvas), *Make unhosted*, *Delete*.
- **Graphics…:** *Highlight orphans in views* (on/off) with colour, line weight and fill pattern for the highlight (default: magenta, weight 5, no pattern). The highlight is display-only and never printed (Chapter 5 graphics layer 6a).

**Behaviour & rules.** Host elements hosted on linked elements (for example face-based fixtures on a linked wall) keep their hosting reference. When a reload loses the host (the linked element was deleted or its face changed), they are listed in the *Reconcile Hosting* palette (Collaborate › Coordinate), grouped by link; the user can *Pick new host*, *Make unhosted* (keep in place, no host), or *Delete*. Orphaned hosted elements keep their last position until reconciled.
- **Causes of orphaning:** the linked host element was deleted; its hosting face was removed or split so that the stored face reference no longer resolves; the linked element was recreated with a new GlobalId (for example by copy-paste or mirror in the source); the link instance was removed with *Keep as orphans* (COL-LNK-006).
- **What can be orphaned:** face-hosted and work-plane-hosted host elements on linked geometry, and annotation (tags, dimensions, spot elevations) referencing linked elements.
- **Tags** remember their lost host; if an element with that GlobalId reappears on a later reload, the tag re-attaches automatically and leaves the list.
- **Pick new host** accepts a face of a linked or host element of a category the element can host on; the element keeps its offset from the face along the face normal and its in-plane position projected onto the new face. A pick on an incompatible face is refused with the reason. The action is undoable.
- **Make unhosted** converts the element to a level- or work-plane-based placement at its current position (Chapter 3 placement rules); it is undoable.
- **Delete** uses ordinary element deletion (`element.delete`) and is undoable.
- An orphaned element is still displayed, scheduled and exported; it is flagged with a warning of kind `col.hosting.orphaned` (COL-MNT-004).
- In BIMserver mode, re-hosting needs ownership of the orphan (COL-OWN-001).
- **Interactive tool rule** (conventions §2.1): *Pick new host* records micro-ops `rehost.pick_element{element_id}` and `rehost.pick_face{host_ref, face_ref}`; replay re-runs the pick against the reloaded link or reports that the face no longer exists.

**Data (IFC/RXD).** The host reference and the *orphaned* flag are in the element's RXD `hosting` section (Chapter 2a). The orphan highlight settings are user settings. No IFC relationship exists for hosting on a linked element.

**Commands.** `hosting.reconcile(link_instance_id?) -> ()` opens the palette (UI only); `hosting.list_orphans(link_instance_id?) -> orphan[]` (query); `hosting.rehost(element_id, new_host_ref) -> ()`; `hosting.unhost(element_ids[]) -> ()`; `hosting.set_orphan_graphics(on, colour?, line_weight?, pattern_id?) -> ()`

**Acceptance criteria.**
- [ ] Deleting a linked wall that hosts three host fixtures lists the three fixtures as orphans after reload, and re-hosting one onto another linked wall keeps its offset from the wall face.
- [ ] A tag on a linked column that was deleted and restored (same GlobalId) re-attaches after reload and disappears from the palette.
- [ ] With orphan highlighting on, orphans show in the chosen colour on screen and in normal colours in PDF output.

### Copy/Monitor and coordination review settings
**ID:** COL-CM-004 · **Priority:** P1 (tolerances, triggers), P2 (MEP fixture copy behaviour) · **Ref:** research/04 §8.4 (*Options*, *Coordination Settings*); web: Specify Coordination Settings

**Purpose.** Control how elements are copied and mapped by Copy/Monitor and how sensitive the coordination review is.

**Workflow.** *Settings…* in the Coordination Review dialog, the *Options* button of the Copy/Monitor tab, or Collaborate › Coordinate › Coordination Settings (`coordreview.settings`) opens the Coordination Settings dialog.

**UI (Coordination Settings dialog).**
- **Apply to:** *Defaults for new links* or a chosen link instance (drop-down). A link instance without its own settings uses the defaults, shown greyed until *Override for this link* is ticked.
- **Categories tab:** one row per category (levels, grids, columns, walls, floors, openings, and the MEP fixture categories in P2) with:
  - *Copy behaviour*: *Allow batch copy* (default for fixture categories), *Copy individually* (default for the others) or *Ignore category*;
  - *Mapping behaviour*: *Copy original type* (bring the source type in, creating it in the host if needed) or *Specify type mapping*;
  - when mapping: a type-map grid, source type → host type, where each row can be *Copy original type*, *Do not copy this type* or a chosen host type;
  - category options: offset (levels: vertical offset; others: none), name prefix and suffix, *Copy hosted openings, doors and windows* (walls), *Split columns by level* (columns), *Reuse existing levels and grids with matching names* (default on).
- **Review tab:**
  - tolerances: position (0.1–1 000 mm, default 1 mm), angle (0.001–10°, default 0.01°), extent (0.1–1 000 mm, default 1 mm);
  - monitored aspects per category (position, orientation, extent, name or mark, type) and extra monitored parameters (picked from the category's parameters);
  - *Check on*: link reload (always), *Get latest* (default on), host edits (default on);
  - *Notify when review needed*: balloon, warning only, or none.
- **Buttons:** *OK*, *Cancel*, *Reset to defaults*, *Copy from link…* (copy another instance's settings).

**Behaviour & rules.**
- Settings apply to future copies and future checks; existing pairs keep their aspects unless *Apply to existing pairs* is ticked on OK, which also re-checks all pairs of that scope.
- **Validation:** tolerances outside their ranges are refused inline; a type map must not map a source type to a host type of another category; a host type deleted later turns its mapping rows into *Copy original type* with a warning.
- In BIMserver mode settings are an RXD object owned like any other (borrowed on edit).
- Settings changes are one undoable step per OK.

**Data (RXD).** RXD document `coordination/monitors`, kind `monitor.settings` (one object for the defaults and one per overriding link instance): `scope`, `categories{}` (copy behaviour, mapping behaviour, `typeMap{}`, options), `tolerances{}`, `aspects{}`, `triggers{}`, `notify`.

**Commands.** `coordreview.settings() -> ()` opens the dialog (UI only); `copymonitor.set_options(link_instance_id?, category, mapping{}, options{}) -> ()` (COL-CM-001) sets the Categories tab; `coordreview.set_tolerances(link_instance_id?, position?, angle?, extent?, triggers?, apply_to_existing?) -> ()` sets the Review tab.

**Acceptance criteria.**
- [ ] With *Ignore category* for columns, picking a linked column in Copy mode copies nothing and reports why.
- [ ] Mapping linked level type "Structural Level" to host type "Architectural Level" gives every copied level the host type.
- [ ] Raising the position tolerance to 5 mm and ticking *Apply to existing pairs* clears pending items for grids moved by 3 mm.
- [ ] A tolerance of 0 mm is refused inline.

### Interference check
**ID:** COL-CLASH-001 · **Priority:** P1 · **Ref:** research/04 §8.5

**Purpose.** Find elements that occupy the same space, inside the host or between the host and links.

**Workflow.**
1. Collaborate › Coordinate › Interference Check ▾ › Run Interference Check.
2. Dialog with two columns. Each column: *Categories from* (current project, current selection, or a link instance) and a category checklist.
3. OK runs the check (in the background with progress) and opens the report: grouped by category pair (either order); each row shows the two elements (category, family, type, GlobalId). *Show* opens a suitable view, zooms and highlights the pair; *Export* writes HTML, CSV or BCF (COL-BCF-001); *Refresh* re-checks the listed pairs; *Close*. *Show Last Report* reopens it.

**Behaviour & rules.**
- **Broad phase:** axis-aligned boxes of all candidate elements in a BVH; candidate pairs are found in parallel (rayon).
- **Narrow phase:** the volume overlap engine (ARC-MOD-030) on the bodies the 3D view draws, then an exact OCCT check (`BRepAlgoAPI_Common`, keeping pairs whose common volume exceeds a tiny tolerance, default 1 cm³, so that touching faces do not count).
- Pairs that the model treats as intended are excluded automatically: a host and its hosted elements (wall and door), joined elements (wall–wall joins, wall–floor joins), an element and its own parts, elements in different design options that are never shown together, and elements of the same element group instance that overlap by definition (for example curtain panels and mullions).
- Linked elements are tested in their placed position.

**Data (RXD).** The last report is stored in RXD document `coordination/clash` (kind `clash.result`, with test definition and pairs) so that it survives reopening and is shared in BIMserver mode.

**Commands.** `clash.run_basic(set_a{source, categories[]}, set_b{source, categories[]}) -> job_id`; `clash.last_report() -> clash_report` (query); `clash.refresh(report_id) -> job_id`; `clash.export_report(report_id, path, format: html|csv|bcf) -> ()`

*Built (2026-10-16):* View › Coordination › **Clash Detection** opens a dockable pane. *Run Test…* asks for set A and set B as IFC classes (none ticked: every element) and runs the test out of process as an `analysis.clash` compute job (DM-RPC-001; the runner measures on the overlap engine of ARC-MOD-030 rather than OCCT's `BRepAlgoAPI_Common`, so results match Check Overlaps exactly), with progress in the pane; when it finishes, `clash.collect` merges it into the model. The list (filterable by status) shows each clash's status, elements, category pair, volume and depth; picking one selects its elements and frames the clash's own box; Previous and Next step through them; Isolate shows only the pair and keeps isolating each clash shown; Section Box boxes it; *Resolve…* applies the overlap tools (accept, cut, join, move apart, ARC-MOD-032). *Export…* writes CSV or HTML. Commands: `clash.run_basic`, `clash.refresh`, `clash.collect` (new: collects a finished run), `clash.last_report`, `clash.export_report`. Links, BCF export and the category-column dialog are not built yet; the report is kept on the project (`rxd:clash_test`, `rxd:clash_results`) rather than RXD document `coordination/clash`.

**Acceptance criteria.**
- [ ] A duct passing through a beam is reported; a door in its host wall is not; two walls meeting at a joined corner are not.
- [ ] The check of 20 000 MEP elements against 5 000 structural elements completes in under one minute on the reference machine.

### Clash tests with tolerances and status (beyond the reference)
**ID:** COL-CLASH-002 · **Priority:** P2 · **Ref:** research/04 §8.5 (*Limitations*), §7.6 (*Model Coordination*, as the cloud alternative the reference relies on)

**Purpose.** Provide the clash workflow that the reference leaves to separate coordination products: saved tests, clearance checks, grouping and status tracking, all in open formats.

**Behaviour & rules.**
- **Saved clash tests:** name; set A and set B defined by sources (host, links, selection sets, view filters, search rules); test type *hard* (with penetration tolerance), *clearance* (elements closer than a distance, computed with OCCT `BRepExtrema_DistShapeShape`), or *duplicate* (identical geometry and position).
- **Results** are kept between runs and matched by GlobalId pair. Each result has a status *new*, *active*, *reviewed*, *approved* or *resolved* (resolved automatically when a re-run no longer finds it), an assignee, comments, and the run in which it was first and last found.
- **Grouping:** by element (all clashes of one element), by level, or by proximity (clustering within a distance), so that one duct run crossing ten beams can be handled as one item.
- **Issues:** any result or group can be turned into a BCF topic (COL-BCF-001) with a generated viewpoint.
- Tests can run automatically after *Get latest* or link reload (option per test).

**Data (RXD).** `coordination/clash`: kinds `clash.test` and `clash.result`.

**Commands.** `clash.create_test(name, set_a, set_b, type: hard|clearance|duplicate, tolerance?, auto_run?) -> test_id`; `clash.update_test(test_id, fields) -> ()`; `clash.run(test_ids[]) -> job_id`; `clash.set_status(result_ids[], status, assignee?, comment?) -> ()`; `clash.group(test_id, mode: element|level|proximity, distance?) -> group_id[]`; `clash.to_bcf(result_or_group_ids[]) -> topic_id[]`

*Built (2026-10-16), in part:* results are kept between runs and matched by GlobalId pair, with statuses *new*, *active*, *reviewed*, *approved* and *resolved* (resolved automatically when a run no longer finds a clash, and *active* again if it returns), comments with author and time, and the runs that first and last found each (`clash.record_run`, `clash.set_status`); the pane sets status and adds comments. Saved tests, clearance and duplicate tests, tolerances per test beyond touching, assignees, grouping by level or proximity, BCF issues and automatic runs are not built yet.

**Acceptance criteria.**
- [ ] A clearance test of 50 mm between pipes and beams reports a pipe 30 mm below a beam and does not report one 80 mm below.
- [ ] After the pipe is moved and the test re-run, the result's status becomes *resolved* automatically.

---

## 6.9 BCF issues

### BCF topics, viewpoints and files
**ID:** COL-BCF-001 · **Priority:** P1 · **Ref:** research/04 §6.7 (*Integrated Issues*, as the reference's cloud-based counterpart), §8.5

**Purpose.** Exchange and manage model-based issues using buildingSMART's open BIM Collaboration Format (BCF).

**Workflow.**
1. Collaborate › Issues opens the Issues palette (`bcf.issues_pane`, COL-BCF-004): a filterable list of topics (title, status, priority, type, assignee, labels, due date, stage, modified) with a detail pane (description, comments, viewpoints with snapshots, related elements, reference links).
2. *New issue* captures a viewpoint of the active view (with snapshot), the selected elements, and opens the editor.
3. Clicking a viewpoint restores it in a view (COL-BCF-003).
4. Import/Export: *Import BCF* (`.bcfzip`, versions 2.1 and 3.0) and *Export BCF* (selected or all topics, version chosen by the user).

**Behaviour & rules.**
- Topic fields follow BCF: GUID, title, description, type, status, priority, labels, assigned to, due date, stage, creation and modification author and date, related topics, document references, comments (with optional viewpoint), viewpoints.
- Topic types, statuses, priorities and labels are project lists (BCF *extensions*), editable by the project administrator and exported with the file.
- Issues are project data: they are stored as RXD (document `coordination/issues`, one object per topic, keyed by the topic GUID, with snapshots as RXD assets), so in BIMserver mode they travel with the model and merge per topic and per comment. Importing a BCF file merges by topic GUID (newer modification date wins per field; comments are merged by GUID).
- Issues whose elements are deleted keep their GlobalIds and show "element not found".
- Clash results (COL-CLASH-002) and coordination review items (COL-CM-002) can be turned into issues.

**Commands.** `bcf.create_topic(title, description?, fields?, view_id?, element_ids[]?) -> topic_id`; `bcf.update_topic(topic_id, fields) -> ()`; `bcf.add_comment(topic_id, text, viewpoint?: current|none) -> comment_id`; `bcf.add_viewpoint(topic_id, view_id?) -> viewpoint_id`; `bcf.delete_topic(topic_ids[]) -> ()`; `bcf.import(path) -> import_report`; `bcf.export(path, topic_ids[]?, version: 2.1|3.0) -> ()`; `bcf.list(filter?) -> topic_summary[]` (query)

**Acceptance criteria.**
- [ ] A `.bcfzip` exported from Reviera imports into at least two other BCF-capable tools with titles, comments, snapshots and camera positions intact.
- [ ] A BCF 2.1 file from another tool imports into Reviera and its viewpoints restore the camera and selection.
- [ ] Two users commenting on the same topic in BIMserver mode both see both comments after synchronising.

*Built (2026-10-26), topics and archives:* topics are kept with the model as RXD objects (`rxd:coordination/issues/<topic guid>`, one per topic; a topic's comments and viewpoints inside it), so they save, reopen and undo with the model. `bcf.create_topic` (from the active 3D view's viewpoint and the selection or `element_ids`; type Issue and status Open unless given), `bcf.update_topic` (recording the modification date and author), `bcf.add_comment` (optionally on one of its viewpoints), `bcf.add_viewpoint`, `bcf.apply_viewpoint` (COL-BCF-003), `bcf.delete_topic`, `bcf.list` (filters: status, type, priority, assignee, label, text) and `bcf.get`; beneath them `bcf.put_topic`, `bcf.put_viewpoint` and `bcf.merge_topics`. `bcf.export` writes a `.bcf` archive in BCF 3.0 (bcf.version, extensions.xml with the project's lists, per topic markup.bcf and `<guid>.bcfv`) or 2.1 (comments and viewpoints beside the topic, no aspect ratio); `bcf.import` reads either and merges by topic GUID: the fields of whichever changed last, comments and viewpoints joined by GUID (a comment changed later is taken). Exports validate against buildingSMART's BCF 3.0 and 2.1 XSDs. Authors are given or the system's user name (no user identity yet, UI-IDENT-001); the lists (`bcf.lists`) are Reviera's defaults and every value in use. View › Coordination › New Issue, Issues (a dialog: Show, Edit, Comment, Add View, Delete), Import BCF and Export BCF, also in the File menu. *(Owner, 2026-10-26: "commit and push work, continue feature development".)* *Built (2026-10-26), snapshots:* New Issue and Add View render the 3D view as it shows (camera, section box and slices, temporary hide and isolate; 1024 × 768 PNG, offscreen) and keep the picture with the model as its own RXD object per viewpoint (`rxd:coordination/snapshots/<viewpoint guid>`, at most 4 MB), in the same step as the topic or viewpoint; deleting a topic deletes its snapshots. Exports write each beside its viewpoint (`<guid>.png`, the markup's `Snapshot`); imports keep a file's PNG or JPEG snapshots for viewpoints that have none. `bcf.snapshot(viewpoint_id)` returns one (base64). Not yet: issues from clash results and review items, document references and project.bcfp, the lists as edited extensions, the docked Issues pane with filters (COL-BCF-004), BIMserver merge (third criterion), BCF API (COL-BCF-002).

*Built (2026-10-26), topics and archives:* topics are kept with the model as RXD objects (`rxd:coordination/issues/<topic guid>`, one per topic; a topic's comments and viewpoints inside it), so they save, reopen and undo with the model. `bcf.create_topic` (from the active 3D view's viewpoint and the selection or `element_ids`; type Issue and status Open unless given), `bcf.update_topic` (recording the modification date and author), `bcf.add_comment` (optionally on one of its viewpoints), `bcf.add_viewpoint`, `bcf.apply_viewpoint` (COL-BCF-003), `bcf.delete_topic`, `bcf.list` (filters: status, type, priority, assignee, label, text) and `bcf.get`; beneath them `bcf.put_topic`, `bcf.put_viewpoint` and `bcf.merge_topics`. `bcf.export` writes a `.bcf` archive in BCF 3.0 (bcf.version, extensions.xml with the project's lists, per topic markup.bcf and `<guid>.bcfv`) or 2.1 (comments and viewpoints beside the topic, no aspect ratio); `bcf.import` reads either and merges by topic GUID: the fields of whichever changed last, comments and viewpoints joined by GUID (a comment changed later is taken). Exports validate against buildingSMART's BCF 3.0 and 2.1 XSDs. The authors are given or the system's user name (no user identity yet, UI-IDENT-001); the lists (`bcf.lists`) are Reviera's defaults and every value in use. View › Coordination › New Issue, Issues (a dialog: Show, Edit, Comment, Add View, Delete), Import BCF and Export BCF, also in the File menu. *(Owner, 2026-10-26: \"commit and push work, continue feature development\".)* Not yet: snapshots (named on import, not kept), issues from clash results and review items, document references and project.bcfp, the lists as edited extensions, the docked Issues pane with filters (COL-BCF-004), BIMserver merge (third criterion), BCF API (COL-BCF-002).

### BCF issues pane
**ID:** COL-BCF-004 · **Priority:** P1 · **Ref:** research/04 §6.7 (*Integrated Issues*), §8.5; COL-BCF-001

**Purpose.** A dockable pane in which users browse, filter, create, edit and act on BCF issues next to the model.

**Workflow.**
1. Collaborate › Issues › BCF Issues (`bcf.issues_pane`) shows the pane, docked on the right by default with the Properties palette; running the command again brings it to the front.
2. The user filters the list, selects a topic, reads or edits it in the detail area, and clicks a viewpoint to go to it.
3. *New issue* captures the active view and selection (COL-BCF-001 workflow step 2).

**UI (Issues pane).**
- **Toolbar:** *New issue*, *Import BCF…*, *Export BCF…*, *Delete*, *Refresh*, *Manage lists…* (types, statuses, priorities, labels, stages, users), and, when a BCF API server is bound (COL-BCF-002), *Sync now* with the last sync time.
- **Filter bar:** text search (title, description, comments), and drop-down filters for status, type, priority, assignee (including *Me*), labels, stage, due date (overdue, this week, none), *Has elements in this view*, and *Source* (manual, clash test, coordination review, imported). Filters combine with AND; the active filter is remembered per user and project.
- **List:** sortable columns: index number, title, status, priority, type, assignee, due date, modified; the first viewpoint's snapshot as a thumbnail (togglable). Overdue topics show the due date in the alert colour.
- **Detail area** (for the selected topic): editable fields (title, description, type, status, priority, labels, assignee, due date, stage), comment thread (newest last; each comment may carry a viewpoint thumbnail; *Add comment* with *Attach current view*), viewpoints strip (click to restore, COL-BCF-003; *Add viewpoint*; *Update viewpoint* replaces one with the current view after confirmation), related elements (*Select*, *Isolate*, with "not found" markers), related topics, and document references (links open in the system browser after confirmation).
- **Context menu on a topic:** *Go to first viewpoint*, *Select related elements*, *Copy link* (`reviera://issue/<guid>`), *Export selected*, *Delete*.
- **Canvas markers:** optional pins at each open topic's first viewpoint target in 3D views (toggle in the pane); clicking a pin selects the topic.

**Behaviour & rules.**
- **Editing:** field edits commit when the field loses focus and are undoable (one step per field). Each edit updates the topic's modification author and date.
- **Validation:** the title is required (1–255 characters); status, type, priority and labels must come from the project lists (an imported value not in the lists is shown in italics and added to the list on the user's confirmation); due date must be a valid date, and a date in the past is allowed but shown as overdue.
- **Manage lists** edits the BCF extension lists (`bcf.set_extensions`); removing a value in use is refused unless the user chooses a replacement value for the topics that use it. While a BCF API project is bound, the lists are read-only and come from the server.
- **Deleting** a topic needs confirmation; in BIMserver mode it is a normal RXD deletion merged at the next sync.
- **Performance:** the list handles 10 000 topics with filtering under 200 ms; snapshots load lazily when visible.
- The pane updates live after *Get latest*, BCF import and BCF API sync; topics changed by others are marked with a dot until opened.
- **Permissions for agents:** MCP agents can read topics with `bcf.list`; creating, editing or deleting needs the model-edit permission.

**Data (IFC/RXD).** Topics, comments, viewpoints and extension lists: RXD document `coordination/issues` (kind `issue.topic` per topic and `issue.extensions` for the lists), snapshots as RXD assets (COL-BCF-001). Pane layout and filters are user settings.

**Commands.** `bcf.issues_pane() -> ()` shows the pane (UI only); `bcf.set_extensions(lists{types?, statuses?, priorities?, labels?, stages?, users?}, replacements?{}) -> ()`; the pane uses `bcf.list`, `bcf.create_topic`, `bcf.update_topic`, `bcf.add_comment`, `bcf.add_viewpoint`, `bcf.delete_topic`, `bcf.import`, `bcf.export` (COL-BCF-001) and `bcf.apply_viewpoint` (COL-BCF-003).

**Acceptance criteria.**
- [ ] Filtering by *Assignee = Me* and *Status = Open* shows exactly the matching topics, and the filter is restored after reopening the project.
- [ ] Editing a topic's status in the pane and pressing Ctrl+Z restores the previous status.
- [ ] Removing the status value "In review" while three topics use it asks for a replacement and applies it to the three.
- [ ] Clicking a viewpoint thumbnail restores that viewpoint in a view.
- [ ] Saving a topic with an empty title is refused.

### BCF API client (OpenCDE)
**ID:** COL-BCF-002 · **Priority:** P1 · **Ref:** research/04 §7.6 (multi-company issue exchange, as context); web: buildingSMART BCF-API (v3.0), OpenCDE Foundation API (v1.1), OpenCDE Documents API (v1.0) repositories (public, accessed 2026-10-12)

**Purpose.** Use any issue server that implements the buildingSMART **BCF API** as the shared issue tracker for a project, so issues move between Reviera and other BIM tools live, not only as files.

**Workflow.**
1. Collaborate › Issues › **Connect issue server…** (`bcf.api_connect`): enter the server URL. Reviera reads the server's supported versions and authentication methods through the OpenCDE Foundation API, then signs the user in (COL-API-001).
2. Choose a project from the server's project list (`bcf.api_bind_project`). The Issues pane (COL-BCF-004) now shows the server's topics. Lists (types, statuses, priorities, labels, stages, users) come from the server's project extensions and are read-only locally.
3. Work normally. Changes sync automatically (default every 2 minutes and on every local change, after 5 s of quiet) or with *Sync now*.

**Behaviour & rules.**
- **Versions:** BCF API **3.0** is the target; **2.1** is supported for older servers. The version is negotiated from the server's versions list. Behaviour that only exists in 3.0, such as events and some extension fields, is hidden on 2.1 servers.
- **Coverage:**
  - project details and extensions
  - topics: create, read, update and delete
  - comments
  - viewpoints, with snapshots, bitmaps, components, colouring, visibility and clipping planes (mapped through COL-BCF-003)
  - related topics
  - document references, both internal documents and external URLs
  - documents (upload and download)
  - files information, which links a topic to the IFC model file and revision it refers to: the Reviera model's file name, IFC project GUID and, in BIMserver mode, the revision
  - topic and comment events (audit history, shown in the topic's *History* tab)
- **Server permissions:** the per-project, per-topic and per-comment action flags the server returns (for example `update`, `createComment`, `updateRelatedTopics`) drive the UI. Disallowed actions are disabled, with the server's restriction as the tooltip, and are never attempted.
- **Paging and queries:** lists use the server's OData query options (`$filter`, `$orderby`, `$top`, `$skip`). Incremental sync filters on modification date. Paging continues until done, with a progress indicator for large projects.
- **Sync and conflicts:**
  - local edits are queued while offline and replayed in order when the connection returns
  - each local object records its server GUID and the server modification date it last saw
  - when a local change conflicts with a newer server value, the server value wins per field, the local value is kept in the topic's *Conflicts* list, and the user can re-apply it with one click
  - nothing is silently lost
- **Local copy:** server topics are cached in RXD (`coordination/issues`) for offline reading, so a model file carries a snapshot of its issues. When a project is bound, the server is the source of truth. Unbinding keeps the local copy as ordinary local issues.
- **Several servers:** one project can be bound to one BCF API project at a time. Topics can be copied between bindings, or to and from BCF files.
- **Agents:** MCP agents use the same `bcf.*` commands. Server credentials are never exposed to them (COL-CORE-005).

**Data (IFC/RXD).** Cached topics in `coordination/issues` (kinds `issue.topic`, `issue.extensions`), each with `server: {connection, guid, modified}`. Connection profiles and tokens are client state (COL-API-001), never stored in the model.

**Commands.** `bcf.api_connect(url, profile_name?) -> connection_id`; `bcf.api_list_projects(connection_id) -> project[]` (query); `bcf.api_bind_project(connection_id, project_id) -> ()`; `bcf.api_unbind(keep_local_copy=true) -> ()`; `bcf.api_sync(connection_id?) -> sync_report`; `bcf.api_status() -> {connection, last_sync, queued_changes, conflicts}` (query); `bcf.api_resolve_conflict(topic_id, field, use: server|local) -> ()`; `bcf.api_disconnect(connection_id) -> ()`.

**Acceptance criteria.**
- [ ] Against the open-source reference implementation of the OpenCDE APIs and at least one commercial BCF API server, a topic with two viewpoints, a snapshot and a comment created in Reviera appears on the server with its camera, components and image. A comment added on the server appears in Reviera after the next sync.
- [ ] Losing the connection mid-sync leaves both sides consistent, and the next sync completes the transfer without duplicates.
- [ ] Ten edits made offline are pushed in order when the connection returns.
- [ ] An action the server forbids (for example editing a closed topic) is disabled in the pane and never sent.
- [ ] A conflicting status change keeps both values visible: the server's in the topic, Reviera's in *Conflicts*.

### Issue tracking workflow
**ID:** COL-BCF-005 · **Priority:** P1 · **Ref:** research/04 §6.7 (*Integrated Issues*, as context), §8.5; web: buildingSMART BCF-API (v3.0); COL-BCF-001, COL-BCF-004

**Purpose.** Make BCF issues a working tracker for coordination and design review, not only an exchange format.

**Behaviour & rules.**
- **Workflow rules** (optional, per project, in the issue lists):
  - allowed status transitions, for example Open → In progress → Resolved → Closed, with Reopened allowed from Closed
  - which statuses need an assignee or a due date
  - which roles may close a topic

  Invalid transitions are refused with the rule shown. When bound to a server, the server's rules and permissions take precedence (COL-BCF-002).
- **Sources of issues:**
  - manual capture (COL-BCF-001)
  - clash results (COL-CLASH-002, one topic per clash group, with both elements as components)
  - coordination review items (COL-CM-002)
  - model warnings (COL-MNT-004)
  - walkthrough and VR captures (Ch 7 ADV-WALK-001)
  - view bookmarks (UI-SCEN-002)

  Each topic records its source, and the pane can filter by it.
- **Element linking:**
  - selecting a topic highlights its components in all open views
  - the Properties palette shows a count of open issues for the selected element, with a link
  - deleting or replacing an element that open issues refer to warns first
  - issues follow elements across Copy/Monitor by GlobalId
- **Assignment and notifications:**
  - assignees come from the project's user list
  - in-app notifications are raised when a topic is assigned to the user, commented on, changed, or becomes overdue
  - a daily summary is optional
  - on a BCF API server, notifications follow server events
  - e-mail and chat notifications are left to the server or to add-ins
- **Due dates and stages:** overdue and due-this-week badges; stages (for example design phases) can be used to filter and report.
- **Reports:**
  - `bcf.report` produces a PDF or HTML issue report (cover, summary counts by status, assignee and type, then one section per topic with fields, comments and snapshots), a CSV or XLSX list, or a BCF file
  - report templates are project settings
  - a schedule-like issue list can be placed on a sheet
- **Dashboard:** counts by status, assignee, type and age, with a trend of open issues over time (from topic events, or local history when offline), shown in the Issues pane's *Overview* tab.
- **Bulk edit:** change status, assignee, priority, labels or due date for several selected topics in one step; one undo step for local issues.

**Data (IFC/RXD).** Workflow rules and report templates in `coordination/issues` (kind `issue.extensions`, keys `workflow` and `reportTemplates`). Notification preferences are user settings.

**Commands.** `bcf.set_workflow(rules) -> ()`; `bcf.bulk_update(topic_ids[], fields) -> ()`; `bcf.report(topic_ids[]? | filter?, format: pdf|html|csv|xlsx|bcf, template_id?, path) -> job_id`; `bcf.from_clash(clash_result_ids[], group_by?) -> topic_ids[]`; `bcf.issues_for_element(element_id) -> topic_summary[]` (query); `bcf.notifications(since?) -> notification[]` (query).

**Acceptance criteria.**
- [ ] With a workflow that forbids Open → Closed, closing an open topic is refused and the allowed next statuses are shown.
- [ ] Creating topics from 12 clash results grouped by level produces one topic per level, each listing its clashing element pairs as components.
- [ ] A PDF report of 50 topics contains every topic's fields, comments and first snapshot, with summary counts that match the list.
- [ ] Selecting an element with two open issues shows "2 open issues" in Properties, and the link filters the pane to them.

### OpenCDE Documents API client
**ID:** COL-CDE-001 · **Priority:** P2 · **Ref:** web: buildingSMART OpenCDE Documents API (v1.0) and Foundation API (v1.1) repositories (public, accessed 2026-10-12)

**Purpose.** Open models from, and publish models and drawings to, any common data environment (CDE) that implements the OpenCDE Documents API, without a vendor-specific connector.

**Workflow.**
1. **Open from CDE:** File › Open › *From CDE…* (`cde.select_documents`). Reviera starts a selection session. The CDE's own document picker opens in the system browser, and the user picks one or more files there. Reviera then downloads the chosen versions: IFC opens as a model, other files as links or attachments.
2. **Publish to CDE:** File › Export › *Publish to CDE…* (`cde.upload_documents`). Reviera starts an upload session for the files to publish (the IFC model, a clean IFC export, PDF sheet sets, BCF). The user enters the CDE's metadata in its own web form, and Reviera uploads the binary data in the chunks the CDE specifies.
3. **Stay current:** a document opened from a CDE remembers its document and version. Reviera polls the CDE's document-versions endpoint (with ETags) and shows "A newer version is available" with *Open newer version*.

**Behaviour & rules.**
- Sign-in and server profiles use COL-API-001. A CDE that also offers the BCF API can be bound for issues with the same profile (COL-BCF-002).
- Downloads are verified against the CDE's reported size and hash where given. Partial downloads resume.
- Publishing a model that has local unsaved changes asks whether to save first. Clean-IFC publishing uses `export.ifc{strip_rxd=true}`.
- BCF *files information* and document references can point at CDE document versions, so issues link to the exact published file.
- Works in Local mode. In BIMserver mode, the CDE is an additional publication target, and the BIMserver remains the model's working store.

**Commands.** `cde.connect(url, profile_name?) -> connection_id`; `cde.select_documents(connection_id) -> selection_session_id`; `cde.download(selection_session_id, target_folder?) -> files[]`; `cde.upload_documents(connection_id, files[]) -> upload_session_id`; `cde.upload_status(upload_session_id) -> status` (query); `cde.check_versions(document_ids[]?) -> version_info[]` (query); `cde.disconnect(connection_id) -> ()`.

**Acceptance criteria.**
- [ ] Against the open-source reference implementation, a user can pick an IFC file in the CDE's browser UI and Reviera opens that exact version.
- [ ] Publishing an IFC and a PDF sheet set creates two documents in the CDE with the metadata entered in its form.
- [ ] After someone uploads a newer version in the CDE, Reviera shows the newer-version notice within the polling interval (default 5 minutes).

### Viewpoint mapping
**ID:** COL-BCF-003 · **Priority:** P1 · **Ref:** research/04 §1.5 (3D views, section box), §3.5 (hide/isolate)

**Behaviour & rules.**
- **Capture:** from a 3D view: perspective camera (eye, direction, up, field of view) or orthogonal camera (eye, direction, up, view-to-world scale or field width); section box faces as six clipping planes and each slice plane as one more (DOC-VIEW-014); visible, hidden and selected elements as BCF *components* by IFC GlobalId (using *default visibility* plus exceptions to keep files small); colour overrides as component colouring; a PNG snapshot. From a 2D view: an orthogonal camera looking along the view direction plus the view's crop as clipping planes, and a snapshot.
- **Restore:** opens or creates a temporary 3D view named after the topic, sets the camera, creates a section box from clipping planes that form a box and slice planes from the others (DOC-VIEW-014, which defines the plane mapping; BCF directions point to the hidden side), applies temporary hide/isolate (Chapter 5, DOC-GFX-008) and colours, and selects the selected components. Components that are not found are listed.
- **Linked elements:** components in linked models are matched by GlobalId across host and links.

**Commands.** `bcf.apply_viewpoint(viewpoint_id, target_view_id?) -> view_id`

**Acceptance criteria.**
- [ ] Restoring a viewpoint with a section box reproduces the box within 1 mm and the camera within 0.1°.
- [ ] A viewpoint that isolates three elements shows only those three when restored.

*Built (2026-10-26), viewpoint files:* `bcf.export_viewpoint(view_id?, path)` writes a 3D view as a BCF 3.0 visualization info file (`.bcfv`): a perspective camera with the vertical field of view in degrees (Reviera keeps the horizontal one at 16:10, written as the aspect ratio) or an orthogonal one with the view's height as `ViewToWorldScale`; the section box's six faces and the slice planes as clipping planes (`views::to_bcf`); the selection; and the view's temporary visibility as components (hidden elements as exceptions to a visible default, an isolated view as a hidden default with the isolated elements as exceptions), by IFC GlobalId. `bcf.import_viewpoint(path, view_id?)` reads BCF 2.1 or 3.0 and shows it in a 3D view (the active one when 3D, else the default 3D view): `view.apply_viewpoint` (also a command) sets the camera, a section box from planes that form one and slices from the rest (`views::from_bcf`), as one undoable step, the target put at the box's centre or 10 m ahead; then the visibility as temporary hide or isolate and the selection; GlobalIds not in the model are returned. View › Coordination › Export Viewpoint… and Show Viewpoint… (and the File menu). `view.get` reads a view's settings in full, section box, slices, scope box and virtual-volume mode among them. Both acceptance criteria above hold in `tests/bcf.rs` (on the views sample). *(Owner, 2026-10-26: "commit and push work, continue feature development"; the gap was found building the views sample.)* Not yet: topics and their `.bcf` archives (COL-BCF-001) with `bcf.apply_viewpoint` by viewpoint id, snapshots, component colouring and translucency, `ViewSetupHints`, a temporary view named after the topic, 2D views' viewpoints, linked models' components.


*Built (2026-10-26), issues from clash results:* `bcf.from_clash(clash_ids?, group_by?: clash|element, margin?, fields?)` makes a topic for each open clash result (new, active or reviewed), or for each element of set A with all its clashes: type Clash, status Open, label Clash, titled by the two elements (category and name), described by each clash's volume, depth, status and id, with a viewpoint looking at the clash from above the south-east, a section box round it with a margin (default 1 m) and both elements selected, and its snapshot rendered offscreen (`App::render_viewpoint`). A topic's GUID is a hash of its clashes' ids, so making issues again skips the clashes that have one (`already`). One undoable step. The Clash Detection pane's *Issues* button makes them for every open clash. *(Owner, 2026-10-26: \"commit and push work, continue feature development\".)* Not yet: workflow rules, a topic's recorded source and filtering by it, the other sources, element linking (Properties count, delete warnings), notifications, reports, the dashboard, bulk edit.
---

## 6.9a Integration services

### Shared REST and authentication client
**ID:** COL-API-001 · **Priority:** P1 · **Ref:** TECH_STACK §3.7.1 (async HTTP client); web: OpenCDE Foundation API (v1.1) authentication conventions; COL-CORE-005

**Purpose.** One network client for every server Reviera talks to (BIMserver JSON API, BCF API, OpenCDE Documents API, library and analysis services, add-ins), so authentication, proxies, security and reliability work the same everywhere.

**Behaviour & rules.**
- **Stack:** an asynchronous HTTP client (`reqwest` on `tokio`, TECH_STACK §3.7.1) running off the UI thread. The UI shows progress and allows cancelling any request that takes longer than 1 s.
- **Server profiles:** each server is a named profile (URL, kind, authentication method, proxy override, certificate settings) kept in client state. Profiles can be exported, without secrets, for a team.
- **Authentication:**
  - OAuth 2.0 authorisation code flow with PKCE in the system browser, using a loopback redirect. This is the default for OpenCDE servers and follows the server's Foundation API authentication description.
  - OAuth 2.0 device code flow, where offered, for headless and locked-down machines.
  - API tokens.
  - BIMserver session tokens (COL-BIM-001).
  - Tokens are refreshed automatically. Secrets live only in the OS credential store and are never written to logs, the model, the live store, profile exports or MCP resources (COL-CORE-005).
- **Network environment:** system proxy settings and PAC scripts are honoured, and a per-profile override is allowed. TLS uses the OS trust store; custom CA certificates can be added per profile. Disabling verification needs an explicit per-profile choice, with a red warning in the status bar.
- **Reliability:**
  - timeouts: 10 s to connect, 60 s to read, overridable per request
  - retries with exponential backoff and jitter for idempotent requests and for 429/503 responses, honouring `Retry-After`
  - ETag/`If-None-Match` caching for polling
  - resumable chunked transfer for large files
  - an offline queue for writes that can be replayed (used by COL-BCF-002)
- **Paging helpers:** OData `$top`/`$skip`/`$filter`/`$orderby` and link-header paging, shared by all clients.
- **Diagnostics:** a per-profile request log (method, URL path, status, timing; secrets and bodies redacted) under Help › Diagnostics, and `api.profile.test` checks reachability, TLS and authentication.
- **Scripting:**
  - Lua scripts get `reviera.http.request(...)` only at trust level `trusted` or `native` (DM-LUA-007), and only to hosts on the script's declared allow-list in its manifest.
  - Requests made with a stored profile use that profile's authentication without exposing the token to the script.
  - MCP agents have no raw HTTP access. They use the commands of the services above.

**Commands.** `api.profile.add(name, url, kind, auth_method, options?) -> profile_id`; `api.profile.list() -> profile[]` (query, no secrets); `api.profile.remove(profile_id) -> ()`; `api.profile.test(profile_id) -> report`; `api.signin(profile_id) -> status`; `api.signout(profile_id) -> ()`; `api.log(profile_id, since?) -> entry[]` (query, redacted); `http.request(method, url, headers?, body?, profile_id?) -> response` (Lua at trusted/native trust only; not exposed to MCP).

**Acceptance criteria.**
- [ ] Behind an authenticating corporate proxy configured by PAC, BIMserver, BCF API and CDE connections all work with no per-service setup.
- [ ] A server answering 429 with `Retry-After: 5` is retried after 5 s, and the user sees "waiting for server" rather than an error.
- [ ] No token or password appears in the request log, the model file, the live store, a profile export or any MCP resource (checked by scanning for a known token value).
- [ ] A Lua script at `standard` trust calling `reviera.http.request` is refused. At `trusted` trust, a call to a host not on its allow-list is refused.

---

## 6.10 Model maintenance

### Purge unused
**ID:** COL-MNT-001 · **Priority:** P1 · **Ref:** research/04 §9 (*Purge Unused*)

**Behaviour & rules.**
- Manage › Settings › Purge Unused (default shortcut PU) shows a tree of unused items by kind: family types and families, materials, styles (line, fill, text, dimension), line and fill patterns, view templates, filters, colour schemes, schedules not placed and not referenced, groups without instances, property sets without objects, unused IFC resources (orphaned representations, placements and styles left by other tools), and unused RXD objects. Check All, Check None, Purge.
- **Dependency-aware:** an item is unused if nothing reachable from the model uses it. Reviera computes this transitively, so one pass removes nested unused items (no need to purge several times).
- In BIMserver mode, purging items owned by others is not possible; they are shown greyed with the owner's name.
- Purge is one undoable command.

**Commands.** `model.purge_preview() -> purge_tree` (query); `model.purge(item_ids[]?|all?) -> count`

**Acceptance criteria.**
- [ ] A family type used only by an unused nested family is purged in the same pass as that nested family.
- [ ] Purge followed by Undo restores every purged item.

### Audit and repair
**ID:** COL-MNT-002 · **Priority:** P1 · **Ref:** research/04 §9 (*Audit*)

**Behaviour & rules.**
- File › Open › *Audit* option, or Manage › Inquiry › Audit Model at any time.
- Checks: IFC schema validation (IfcOpenShell validator: types, cardinalities, WHERE rules where available); duplicate GlobalIds; relationships referring to missing objects; elements contained in more than one spatial structure element; property sets attached to nothing; RXD schema validation and reconciliation (Chapter 2a: references to missing GlobalIds, stale recipes); geometry that fails to build in OCCT; elements far from the origin.
- *Repair* fixes what can be fixed safely (new GlobalIds for duplicates, keeping the one referenced by RXD; removing dangling references; detaching orphan property sets; rebuilding representations from RXD recipes) and lists what needs a user decision. The audit report can be saved as HTML.
- Audit of a BIMserver working copy repairs the working copy; repairs are uploaded at the next sync like other changes.

**Commands.** `model.audit(repair?) -> audit_report`

**Acceptance criteria.**
- [ ] Opening a file with a duplicated GlobalId with *Audit* on reports it and, with repair, gives one of the two objects a new GlobalId.
- [ ] Audit on a valid model reports no problems and changes nothing.

### Backups and restore
**ID:** COL-MNT-003 · **Priority:** P0 (Local backups), P1 (BIMserver restore) · **Ref:** research/04 §9 (*Backups*, *Compact*)

**Behaviour & rules.**
- **Local mode:** each save keeps the previous file as a numbered backup (`name.0001.ifc`, `name.0002.ifc`, …) up to *Maximum backups* (default 3; project setting). Backups keep their embedded RXD, so each is a complete project. *Restore backup* (File › Backups) lists them with date and size and opens one or replaces the current file with it (after confirmation, keeping the current file as a new backup).
- **Autosave:** every N minutes (default 10) to a recovery file in the user's data directory, removed after a successful save; offered on the next start after a crash.
- **BIMserver mode:** the server's revisions are the history; *Restore* is COL-BIM-008. The working copy has its own journal (COL-OFF-001).
- **Compact:** Reviera always writes files in full, so there is no separate compaction; *Save As* with *Compressed (.ifczip)* reduces size.

**Commands.** `model.set_backup_policy(max_backups, autosave_minutes?) -> ()`; `model.list_backups() -> backup_info[]` (query); `model.restore_backup(backup_id, replace?) -> ()`

**Acceptance criteria.**
- [ ] With maximum backups = 3, five saves leave exactly three backups, the oldest removed first.
- [ ] After killing Reviera, the next start offers the autosave recovery file.

### Warnings review
**ID:** COL-MNT-004 · **Priority:** P0 · **Ref:** research/04 §9 (*Review Warnings*); spec/02 §2.7.4 (DM-FAIL-001)

**Purpose.** One dialog to review, locate, fix, acknowledge and report every stored warning in the model, building on the failure handling of Chapter 2 (DM-FAIL-001).

**Workflow.**
1. Manage › Inquiry › Review Warnings (`warnings.review`), or a click on the status-bar warning count, opens the Warnings dialog.
2. The user expands a warning kind, ticks elements, and uses *Show*, a resolution, *Delete checked* or *Acknowledge*.
3. *Export…* writes a report.

**UI (Warnings dialog).** Modeless and resizable.
- **Filter bar:** severity (warnings only by default; errors never persist, DM-FAIL-001), kind (drop-down of the kinds present, with counts), discipline or chapter area (ARC, STR, MEP, DOC, COL, DM), *Only in active view*, *Only my elements* (BIMserver: elements I own or borrow), *Caused by last sync* (COL-MRG-003), *Show acknowledged* (default off), text search.
- **Tree:** warning kind (message and count) → one node per warning instance → the elements involved, each with a check box, category, family and type, mark, and GlobalId. Selecting an element row highlights it in the canvas.
- **Buttons:** *Show* (selects the checked or selected elements and zooms to them in a suitable view, cycling through views that show them on repeated presses), *Resolve…* (offers the resolutions the failure definition provides, for example *Unjoin*, *Delete newer*, *Disallow join*), *Delete checked* (enabled when elements are ticked; confirmation lists the count), *Acknowledge* (with an optional note), *Un-acknowledge*, *Export…* (HTML or CSV), *Close*.
- The status-bar warning count excludes acknowledged warnings and turns to the alert colour above a project threshold (default 500).

**Behaviour & rules.**
- Manage › Inquiry › Warnings lists every current warning: overlapping or duplicate elements, join problems, room and area boundary problems, orphaned annotation (Chapter 5), coordination review items (COL-CM-002), reconciliation results, regeneration failures, elements far from the origin. Warnings are grouped by kind, with the elements involved (GlobalIds).
- *Show* selects and zooms; *Delete checked* (for duplicate instances); *Export* to HTML or CSV.
- The warning count is shown on the status bar.
- **Storage:** warnings are stored in RXD document `project/warnings` and re-checked whenever the elements involved regenerate, as specified in Chapter 2 (DM-FAIL-001); a warning disappears when its condition is gone. Acknowledgement is the *dismissed* state of that store, plus who acknowledged it, when, and the note, so it is shared with other users in BIMserver mode and merged per warning.
- **Acknowledgement is not a fix:** acknowledged warnings remain in exports (marked) and in the statistics (COL-MNT-007). If the elements involved change, the acknowledgement is cleared and the warning reappears.
- **Delete checked** and resolutions are ordinary undoable edits, subject to ownership (COL-OWN-001): elements owned by others are skipped and listed.
- **Linked models:** warnings inside linked models are not listed; warnings about host elements related to links (orphans, review items) are.
- **Export** includes for each warning: kind, severity, message, element GlobalIds, categories, acknowledged state and note, first-seen time; HTML output is a single self-contained file.
- The list updates live while the dialog is open; a warning resolved by an edit disappears from the tree without closing it.

**Data (IFC/RXD).** RXD document `project/warnings` (Chapter 2, DM-FAIL-001), with fields `acknowledgedBy`, `acknowledgedAt`, `note` and, for merge-caused warnings, `syncReport`.

**Commands.** `warnings.review() -> ()` opens the Warnings dialog (UI only); `warnings.list(filter?: {kind?, severity?, element_ids?}) -> warning[]` (query); `warnings.acknowledge(warning_ids[], note?) -> ()`; `warnings.export(path, format: html|csv) -> ()`. *Show* uses `warnings.show(warning_id)` and *Delete checked* uses `element.delete` (Chapter 2). `warnings.acknowledge` sets the same stored state as Chapter 2's `warnings.dismiss`, with a note.

**Acceptance criteria.**
- [ ] Placing two identical doors at the same position raises one duplicate-instance warning listing both.
- [ ] Acknowledged warnings stay hidden from the default list after reopening the project.
- [ ] Moving one of two overlapping walls away while the dialog is open removes that warning from the tree.
- [ ] Editing an element of an acknowledged warning so that the warning still applies makes it reappear unacknowledged.
- [ ] *Delete checked* on a duplicate door removes it, and Undo restores it with its warning.
- [ ] The exported CSV contains one row per warning element with its GlobalId.

### Transfer project standards
**ID:** COL-MNT-005 · **Priority:** P1 · **Ref:** research/04 §9 (*Transfer Project Standards*)

**Behaviour & rules.**
- Manage › Settings › Transfer Project Standards: choose a source (another open project or an IFC file on disk, including template files) and the kinds to copy: system types (walls, floors, roofs, ceilings, stairs, railings and so on), materials, object styles, line weights, line styles, line patterns, fill patterns, text, dimension and tag styles, view templates, view filters, colour schemes, print, PDF, CAD and IFC export setups, view/sheet sets, revision numbering sequences, saved viewport positions, phase filters and settings, keynote settings, browser organisation schemes.
- Items whose names already exist in the target: *Overwrite*, *New only* (skip existing) or *Rename* (append a suffix).
- Dependencies come along (a view template brings its filters, a wall type its materials and fill patterns).
- The transfer is one undoable command.

**Commands.** `standards.transfer(source_document_id, selection[{kind, item_ids?}], on_conflict: overwrite|new_only|rename) -> transfer_report`

**Acceptance criteria.**
- [ ] Transferring view templates also transfers the filters and line patterns they use, and assigned views in the target are unaffected unless overwritten templates change them.

### Package project for transmittal
**ID:** COL-MNT-006 · **Priority:** P2 · **Ref:** research/04 §9 (*eTransmit*)

**Purpose.** Send a project to someone else as one self-contained package that opens without missing files.

**Workflow.**
1. File › Package Project opens a wizard: *Contents* (the reference tree, below), *Options*, *Destination* (folder or `.zip`), *Summary*.
2. The user reviews what will be included and clicks *Package*; progress is shown and the job runs in the background.
3. On completion the transmittal report opens.

**Behaviour & rules.** File › Package Project: collects the project file and everything it references (IFC links, CAD links, point cloud sources or their indexes, PDF and image underlays, keynote files, render plug-in assets) into a folder or ZIP, rewriting link paths as relative. Options: include BIMserver links as downloaded files (pinned to their current revision), purge unused, remove views and sheets not on a chosen set, export as a local copy (strip ownership and server binding, COL-MODE-002), include a transmittal report (file list with sizes and SHA-256 hashes).
- **Contents tree:** every referenced file with its kind, size and status; the user can untick a file (its link is then kept but will show *not found* at the destination). Nested IFC links that are attachments are followed recursively (depth limit 4, COL-LNK-006); overlays are not.
- **Point clouds:** *Source*, *COPC index only* (default, smaller and enough for display) or *Exclude*.
- **Layout:** the project file at the package root; referenced files in sub-folders by kind (`links/ifc`, `links/cad`, `pointclouds`, `underlays`, `keynotes`); duplicate file names get a numeric suffix. Link paths in the packaged copy are rewritten to these relative locations; the original project is not changed.
- **The packaged project is a copy:** all options (purge, removing views, stripping) are applied to the copy only.
- **Missing references** (*not found* links) are listed in the summary and in the report; packaging continues unless the user cancels.
- **Size check:** the summary shows the total size; packages over 4 GB are written as ZIP64.
- **BIMserver mode:** the package always contains a local copy (server binding and ownership data are never included); the transmittal report records the server project and base revision for reference.

**Data.** No project data changes. The transmittal report (`transmittal.html` and `transmittal.csv`) lists every file with relative path, size, SHA-256 and origin path, plus the Reviera version and date.

**Commands.** `model.package(path, zip?, options) -> transmittal_report` (`options`: `{include_bimserver_links?, pointclouds?: source|index|exclude, purge?, keep_view_set_id?, local_copy?, report?, exclude_paths?[]}`)

**Acceptance criteria.**
- [ ] A packaged project opens on another machine with all links loaded and no broken paths.
- [ ] The SHA-256 hashes in the transmittal report match the packaged files.
- [ ] Packaging with *purge unused* leaves the original project unchanged.
- [ ] A package of a BIMserver project contains no ownership data and no server binding.

### Model statistics
**ID:** COL-MNT-007 · **Priority:** P2 · **Ref:** research/04 §9 (*Model Analytics / Model Health*)

**Purpose.** Show the size and health of a model so that teams can find what makes it slow or large.

**Behaviour & rules.** Manage › Inquiry › Model Statistics: element counts by category and workset, file size and the size of IFC geometry and RXD by document, warnings by kind, largest families, number of views, sheets and unplaced views, link sizes, regeneration time of the slowest elements. Exportable as CSV or JSON, and available to agents as an MCP resource.
- **Dialog sections:** *Summary* (file size, element count, warning count, link count, last save and sync), *Elements* (by category, workset, level, phase), *Storage* (IFC entity count, size of geometry representations, size of each RXD document and of RXD assets), *Families and types* (by instance count and by size, unused types), *Views and sheets* (counts by view type, unplaced views, views without a template), *Links* (size and load time per link), *Performance* (the 20 slowest elements to regenerate, from the last full regeneration, and open time).
- Counts cover open worksets only; a note gives the number of elements in closed worksets, which are counted from the working copy index without loading them.
- Statistics are computed in the background in under 5 seconds for a 100 000-element model; the dialog shows *Refresh* and the time computed.
- **History (BIMserver mode):** the summary values are also computed for each sync and kept in the working copy, so the dialog can chart file size, element count and warning count over the last 50 syncs.
- Numbers are read-only; clicking a category or warning kind selects the elements (with confirmation above 10 000).
- The MCP resource is `reviera://model/statistics` (read-only, JSON with the same structure as `stats`).

**Data.** Derived; nothing is stored in IFC or RXD. Sync history values are client state.

**Commands.** `model.statistics() -> stats` (query)

**Acceptance criteria.**
- [ ] The statistics' element counts equal the counts in a multi-category schedule of the same model.
- [ ] The RXD document sizes add up to the total RXD size shown in the summary.
- [ ] The JSON export validates against the published `stats` schema.

### Element lookup by id
**ID:** COL-MNT-008 · **Priority:** P0 · **Ref:** research/04 §9 (*Select by ID / IDs of Selection*)

**Purpose.** Find elements from ids in reports, warnings, clash lists or messages, and give others the ids of a selection.

**Behaviour & rules.** Manage › Inquiry › *Select by ID* accepts GlobalIds or internal ids (comma or line separated) and selects them, opening a view that shows them if needed; *IDs of Selection* shows and copies the GlobalIds of the selection.
- **Input parsing:** separators are commas, semicolons, spaces and line breaks. A 22-character token in the IFC base-64 alphabet is a GlobalId; a 36-character UUID is converted to a GlobalId; a decimal number is an internal id; link references `link:<linkId>/ifc:<GlobalId>` select linked elements. Duplicates are ignored. Up to 100 000 ids per call.
- **Not found:** ids that match nothing are listed in a result panel ("12 of 50 not found") with *Copy not found*; found ids are still selected.
- **Hidden elements:** if an element is in a closed workset, an unloaded link, or hidden in every open view, Reviera offers to open the workset or a suitable view (a 3D view with *Reveal hidden* if no other view shows it).
- **IDs of Selection** shows a dialog with GlobalIds and internal ids side by side, *Copy GlobalIds*, *Copy internal ids*, *Copy both (CSV)*. Internal ids are session-stable but GlobalIds are the ones to share (COL-CORE-002).

**Data.** None; the commands only read the model and change the selection.

**Commands.** `select.by_ids(ids[]) -> element_id[]`; `select.ids_of_selection() -> {global_ids[], internal_ids[]}` (query)

**Acceptance criteria.**
- [ ] Pasting 50 GlobalIds from a clash report selects those 50 elements.
- [ ] A list with 3 unknown ids selects the others and lists the 3 as not found.
- [ ] Selecting an element in a closed workset by id offers to open the workset.

---

## 6.11 Data summary

| Data | Where it lives | Requirement |
|---|---|---|
| Elements, spatial structure, types, georeferencing (`IfcMapConversion`, `IfcProjectedCRS`, `TrueNorth`, `IfcSite` location) | IFC entities | COL-POS-002, COL-POS-003 |
| Worksets (definitions) | RXD document `project/worksets` | COL-WS-001 |
| Element workset membership | element-scoped RXD | COL-WS-001 |
| Link registry, link instances, display per view | RXD documents `project/links`, `views/<viewId>` (`linkOverrides`) | COL-LNK-001 … 007 |
| Project base point, survey point marker, named positions | RXD document `project/settings` (`positioning`) | COL-POS-001, COL-POS-004 |
| Monitor pairs, coordination review and coordination settings | RXD document `coordination/monitors` (kinds `monitor.pair`, `monitor.settings`) | COL-CM-001, COL-CM-002, COL-CM-004 |
| Clash tests and results | RXD document `coordination/clash` | COL-CLASH-001, COL-CLASH-002 |
| BCF issues | RXD document `coordination/issues` (kinds `issue.topic`, `issue.extensions`; snapshots as assets) | COL-BCF-001, COL-BCF-004 |
| Stored and acknowledged warnings | RXD document `project/warnings` (Chapter 2, DM-FAIL-001) | COL-MNT-004, COL-MRG-003 |
| Orphaned hosting flag | element RXD `hosting` section | COL-CM-003 |
| Live ownership, editing requests, sync lock | BIMserver extended data, schema `org.reviera.ownership/1.0`, on the latest revision (never in the IFC) | COL-OWN-002, COL-OWN-003 |
| Server profiles, credentials, working copy state, conflict stash, closed-workset choice, BCF API bindings | Client state (user data directory, OS credential store) — never project data | COL-BIM-001, COL-OFF-001 |
| Export-only forms: worksets as `IfcGroup`, documentation as `IfcAnnotation` | Written on IFC export only | COL-WS-001; Chapter 5 DOC-IFC-002 |

The new RXD documents introduced by this chapter (`coordination/monitors`, `coordination/clash`, `coordination/issues`) and the ownership extended-data schema must be added to the Chapter 2a catalogue, together with the kinds `monitor.settings`, `issue.topic` and `issue.extensions`, which are not yet in the Chapter 2a §6.5 kind registry.

## 6.12 Command catalog

Every command below is registered once and is available from the UI, Lua and MCP. "Undoable = No" marks queries, network operations against the server (which cannot be undone locally; the server keeps the history), live ownership operations, and session-only display state. BIMserver commands need the user to be logged in; MCP agents can never supply credentials (COL-CORE-005).

| Command | Parameters | Returns | Undoable | Req ID |
|---|---|---|---|---|
| `model.open_file` | path, read_only? | model_id | No | COL-MODE-001 |
| `model.mode` | — | {mode, server?, poid?, base_roid?, connected?} | No | COL-MODE-001 |
| `bimserver.publish_local` | project: {poid}\|{new_name, parent_poid?}, comment, replace? | {poid, roid} | No | COL-MODE-002 |
| `model.export_local_copy` | path, keep_worksets? | () | No | COL-MODE-002 |
| `fileengine.connect` | url, name?, tenant?, allow_http? | connection | No | COL-FE-001 |
| `fileengine.list_connections` | — | connection[] | No (query) | COL-FE-001 |
| `fileengine.sign_in` | connection_id, key?, secret?, remember? | identity | No | COL-FE-001 |
| `fileengine.sign_out` | connection_id, forget? | () | No | COL-FE-001 |
| `fileengine.remove` | connection_id | () | No | COL-FE-001 |
| `fileengine.whoami` | connection_id | identity | No (query) | COL-FE-001 |
| `fileengine.browse` | connection_id, folder_uid?, path?, models_only? | {folder_uid, folder_name, parent_uid, writable, entries[]} | No (query) | COL-FE-002 |
| `fileengine.make_folder` | connection_id, parent_uid, name | uid | No | COL-FE-002 |
| `fileengine.open` | connection_id, uid, version?, read_only? | {document_id, working_copy, link, existing, newer_on_server} | No | COL-FE-002 |
| `fileengine.status` | document_id? | {linked, link?, connected} | No (query) | COL-FE-002 |
| `fileengine.save` | document_id?, on_conflict?, comment? | uploaded | No | COL-FE-003 |
| `fileengine.save_as` | document_id?, connection_id, folder_uid, name, replace?, comment? | uploaded | No | COL-FE-003 |
| `fileengine.versions` | document_id? \| connection_id + uid | version[] | No (query) | COL-FE-004 |
| `fileengine.restore_version` | document_id? \| connection_id + uid, version | {restored, version, document_id?} | No | COL-FE-004 |
| `bimserver.add_server` | name, url, allow_http?, ownership? | server | No | COL-BIM-001 |
| `bimserver.remove_server` | server_id | () | No | COL-BIM-001 |
| `bimserver.login` | server_id, username?, password?, user_token?, remember? | session_info | No | COL-BIM-001 |
| `bimserver.logout` | server_id, forget? | () | No | COL-BIM-001 |
| `bimserver.list_servers` | — | server[] | No (query) | COL-BIM-001 |
| `bimserver.list_projects` | server_id | project_summary[] | No | COL-BIM-002 |
| `bimserver.list_revisions` | server_id, poid | revision_summary[] | No | COL-BIM-002 |
| `bimserver.create_project` | server_id, name, schema?, parent_poid? | poid | No | COL-BIM-002 |
| `bimserver.open` | server_id, poid, roid?, as_working_copy? | model_id | No | COL-BIM-003 |
| `bimserver.download_revision` | server_id, roid, path, schema? | bytes | No | COL-BIM-003 |
| `bimserver.check_in` | comment, document_id? | {poid, roid, revision, warnings, document_id} | No | COL-BIM-004 |
| `bimserver.get_latest` | document_id?, conflicts? | sync_report | No | COL-BIM-005 |
| `bimserver.sync` | comment, document_id?, conflicts?, keep_borrowed? | sync_report | No | COL-BIM-006 |
| `bimserver.set_sync_reminder` | minutes? | () | No | COL-BIM-006 |
| `bimserver.subscribe` | events[]? | () | No | COL-BIM-007 |
| `bimserver.unsubscribe` | — | () | No | COL-BIM-007 |
| `bimserver.set_update_frequency` | seconds_or_manual | () | No | COL-BIM-007 |
| `bimserver.lock_status` | document_id? | {available, locked, holder?, expires?, mine} | No (query) | COL-OWN-002 |
| `bimserver.history` | document_id? | revision_summary[] | No (query) | COL-BIM-008 |
| `bimserver.compare` | roid_a?, roid_b?, document_id? | change_set | No (query) | COL-BIM-008 |
| `bimserver.restore` | roid, reason, document_id? | {poid, roid, revision} | No | COL-BIM-008 |
| `merge.preview` | base_path?, remote_path? | merge_summary | No | COL-MRG-001 |
| `merge.three_way` | base_path, local_path, remote_path, out_path, policy? | merge_report | No | COL-MRG-001 |
| `merge.list_conflicts` | — | conflict[] | No | COL-MRG-002 |
| `merge.resolve` | conflict_id, choice: mine\|theirs\|value, value?, element?, document_id? | — | No | COL-MRG-002 |
| `merge.resolve_all` | choice: mine\|theirs, class?, all?, document_id? | count | No | COL-MRG-002 |
| `merge.finish` | rest?, document_id? | sync_report | No | COL-MRG-002 |
| `merge.cancel` | — | () | No | COL-MRG-002 |
| `stash.list` | — | stash_entry[] | No | COL-MRG-002 |
| `stash.reapply` | entry_id, element?, document_id? | — | No | COL-MRG-002 |
| `stash.discard` | entry_ids[] | () | No | COL-MRG-002 |
| `workset.create` | name, visible_by_default?, open_by_default? | workset_id | Yes | COL-WS-001 |
| `workset.rename` | workset_id, name | () | Yes | COL-WS-001 |
| `workset.delete` | workset_id, move_to_workset_id?\|delete_elements? | () | Yes | COL-WS-001 |
| `workset.move_elements` | element_ids[], workset_id | () | Yes | COL-WS-001 |
| `workset.list` | — | workset_summary[] | No | COL-WS-001 |
| `workset.set_active` | workset_id | () | Yes | COL-WS-002 |
| `workset.set_editable` | workset_ids[], editable | () | No | COL-WS-002 |
| `workset.set_visible_by_default` | workset_id, visible | () | Yes | COL-WS-002 |
| `view.set_workset_visibility` | view_id, workset_id, mode: global\|show\|hide | () | Yes | COL-WS-002 |
| `view.grey_inactive_worksets` | on | () | No | COL-WS-002 |
| `workset.open` | workset_ids[] | () | No | COL-WS-003 |
| `workset.close` | workset_ids[] | () | No | COL-WS-003 |
| `workset.manage` | — | () | No | COL-WS-002 |
| `ownership.borrow` | element_ids, document_id? | — | No | COL-OWN-001 |
| `ownership.relinquish` | element_ids?, document_id? | count | No | COL-OWN-001 |
| `ownership.query` | element_ids, document_id? | ownership_info[] | No (query) | COL-OWN-001 |
| `ownership.force_relinquish` | owner, ids[]? | () | No | COL-OWN-001 |
| `ownership.request` | element_or_object_ids[], message?, whole_workset? | {borrowed[], requests[], skipped[]} | No | COL-OWN-005 |
| `editrequest.create` | targets[], message? | request_id | No | COL-OWN-003 |
| `editrequest.grant` | request_id | () | No | COL-OWN-003 |
| `editrequest.deny` | request_id, message? | () | No | COL-OWN-003 |
| `editrequest.retract` | request_id | () | No | COL-OWN-003 |
| `editrequest.list` | — | request[] | No | COL-OWN-003 |
| `editrequest.manage` | — | () | No | COL-OWN-003 |
| `bimserver.capabilities` | — | capability_set | No | COL-OWN-004 |
| `view.set_worksharing_display` | view_id, mode: off\|checkout\|owners\|updates\|worksets | () | No | COL-WS-004 |
| `worksharing.set_display_colours` | kind, key, colour | () | No | COL-WS-004 |
| `workingcopy.status` | — | {base_roid, unsynced_changes, queued_ops, last_sync} | No | COL-OFF-001 |
| `workingcopy.list` | — | working_copy_summary[] | No | COL-OFF-001 |
| `workingcopy.discard` | poid, export_path? | () | No | COL-OFF-001 |
| `link.add_ifc` | source, positioning, point?, workset_id?, reference_type?: attachment\|overlay | {link_id, instance_id} | Yes | COL-LNK-001 |
| `link.add_instance` | link_id, positioning, point? | instance_id | Yes | COL-LNK-001 |
| `link.set_phase_map` | link_id, map{} | () | Yes | COL-LNK-001 |
| `link.set_room_bounding` | link_id, on | () | Yes | COL-LNK-001 |
| `link.copy_elements` | instance_id, element_refs[] | element_id[] | Yes | COL-LNK-001 |
| `link.add_cad` | path, view_id?, colours?, layers?, units?, correct_off_axis?, positioning, level_id? | {link_id, instance_id} | Yes | COL-LNK-002 |
| `import.cad` | path, view_id?, options | import_id | Yes | COL-LNK-002 |
| `import.explode` | import_id, mode: partial\|full | element_ids[] | Yes | COL-LNK-002 |
| `import.delete_layers` | import_id, layers[] | () | Yes | COL-LNK-002 |
| `import.query` | import_id, pick_ref | cad_entity_info | No | COL-LNK-002 |
| `link.add_pointcloud` | path, positioning, point? | {link_id, instance_id} | Yes | COL-LNK-003 |
| `pointcloud.reindex` | link_id | job_id | No | COL-LNK-003 |
| `view.set_pointcloud_display` | view_id, link_id, visible?, colour_mode?, regions? | () | Yes | COL-LNK-003 |
| `link.add_pdf` | path, page, view_id, dpi?, link?, position, layer?: background\|foreground | underlay_id | Yes | COL-LNK-004 |
| `underlay.calibrate` | underlay_id, point_a, point_b, real_distance | () | Yes | COL-LNK-004 |
| `link.add_image` | path, view_id, link?, position, width?, layer? | underlay_id | Yes | COL-LNK-005 |
| `image.manage_list` | — | image_info[] | No | COL-LNK-005 |
| `image.delete` | image_ids[], replace_with? | () | Yes | COL-LNK-005 |
| `link.list` | — | link_summary[] | No | COL-LNK-006 |
| `link.reload` | link_ids[] | () | No | COL-LNK-006 |
| `link.reload_from` | link_id, new_source | () | No | COL-LNK-006 |
| `link.unload` | link_ids[], for_everyone? | () | No | COL-LNK-006 |
| `link.remove` | link_ids[] | () | Yes | COL-LNK-006 |
| `link.bind` | link_id | element_id[] | Yes | COL-LNK-006 |
| `link.set_path_type` | link_ids[], path_type | () | Yes | COL-LNK-006 |
| `link.set_reference_type` | link_ids[], reference_type | () | Yes | COL-LNK-006 |
| `link.set_open_worksets` | instance_id, workset_names[] | () | Yes | COL-LNK-006 |
| `link.manage` | — | () | No | COL-LNK-006 |
| `link.show` | link_or_instance_id | view_id | No | COL-LNK-006 |
| `view.set_link_display` | view_id, instance_id, mode: host\|linked_view\|custom, linked_view_id?, custom?, visible?, halftone?, underlay?, line_weights_from?: host\|link | () | Yes | COL-LNK-007 |
| `coord.set_project_base_point` | position?, angle_to_true_north? | () | Yes | COL-POS-001 |
| `coord.set_survey_point` | internal_position?, shared_coordinates? | () | Yes | COL-POS-001 |
| `coord.rotate_project_north` | angle | () | Yes | COL-POS-001 |
| `coord.rotate_true_north` | angle | () | Yes | COL-POS-001 |
| `coord.relocate_to_origin` | shift? | () | Yes | COL-POS-001 |
| `coord.set_location` | latitude, longitude, elevation?, time_zone?, address? | () | Yes | COL-POS-002 |
| `coord.set_crs` | epsg_or_wkt, vertical_datum?, apply_scale_factor? | () | Yes | COL-POS-003 |
| `coord.get_georeference` | — | {crs, map_conversion, true_north, grid_convergence} | No | COL-POS-003 |
| `coord.acquire` | instance_id | () | Yes | COL-POS-004 |
| `coord.publish` | instance_id, position_name? | () | Yes | COL-POS-004 |
| `link.save_positions` | link_ids[] | () | Yes | COL-POS-004 |
| `coord.specify_at_point` | point, easting, northing, elevation, angle? | () | Yes | COL-POS-004 |
| `coord.report_point` | point | {shared, project, internal} | No | COL-POS-004 |
| `coord.reset_shared` | — | () | Yes | COL-POS-004 |
| `coord.create_named_position` | name | position_id | Yes | COL-POS-004 |
| `coord.set_current_position` | position_id | () | Yes | COL-POS-004 |
| `copymonitor.set_options` | link_instance_id?, category, mapping{}, options{} | () | Yes | COL-CM-001 |
| `copymonitor.copy` | link_instance_id, element_refs[] | element_id[] | Yes | COL-CM-001 |
| `copymonitor.monitor` | host_element_id, source_ref | pair_id | Yes | COL-CM-001 |
| `copymonitor.stop` | pair_ids[] | () | Yes | COL-CM-001 |
| `copymonitor.batch_copy` | link_instance_id, categories[], rules | element_id[] | Yes | COL-CM-001 |
| `copymonitor.start` | link_instance_id? (omitted: current project) | () | No | COL-CM-001 |
| `copymonitor.finish` | — | pair_id[] | Yes | COL-CM-001 |
| `copymonitor.cancel` | — | () | No | COL-CM-001 |
| `coordreview.list` | link_instance_id?, status? | review_item[] | No | COL-CM-002 |
| `coordreview.resolve` | item_ids[], action: postpone\|reject\|accept\|modify, comment? | () | No | COL-CM-002 |
| `coordreview.report` | path, format: html\|csv, filter? | () | No | COL-CM-002 |
| `coordreview.show` | link_instance_id? | () | No | COL-CM-002 |
| `coordreview.settings` | — | () | No | COL-CM-004 |
| `coordreview.set_tolerances` | link_instance_id?, position?, angle?, extent?, triggers?, apply_to_existing? | () | Yes | COL-CM-004 |
| `hosting.list_orphans` | link_instance_id? | orphan[] | No | COL-CM-003 |
| `hosting.rehost` | element_id, new_host_ref | () | Yes | COL-CM-003 |
| `hosting.unhost` | element_ids[] | () | Yes | COL-CM-003 |
| `hosting.reconcile` | link_instance_id? | () | No | COL-CM-003 |
| `hosting.set_orphan_graphics` | on, colour?, line_weight?, pattern_id? | () | No | COL-CM-003 |
| `clash.run_basic` | set_a{source, categories[]}, set_b{source, categories[]} | job_id | No | COL-CLASH-001 |
| `clash.last_report` | — | clash_report | No | COL-CLASH-001 |
| `clash.refresh` | report_id | job_id | No | COL-CLASH-001 |
| `clash.export_report` | report_id, path, format: html\|csv\|bcf | () | No | COL-CLASH-001 |
| `clash.create_test` | name, set_a, set_b, type: hard\|clearance\|duplicate, tolerance?, auto_run? | test_id | Yes | COL-CLASH-002 |
| `clash.update_test` | test_id, fields | () | Yes | COL-CLASH-002 |
| `clash.run` | test_ids[] | job_id | No | COL-CLASH-002 |
| `clash.set_status` | result_ids[], status, assignee?, comment? | () | Yes | COL-CLASH-002 |
| `clash.group` | test_id, mode: element\|level\|proximity, distance? | group_id[] | Yes | COL-CLASH-002 |
| `clash.to_bcf` | result_or_group_ids[] | topic_id[] | Yes | COL-CLASH-002 |
| `bcf.create_topic` | title, description?, fields?, view_id?, element_ids[]? | topic_id | Yes | COL-BCF-001 |
| `bcf.update_topic` | topic_id, fields | () | Yes | COL-BCF-001 |
| `bcf.add_comment` | topic_id, text, viewpoint?: current\|none | comment_id | Yes | COL-BCF-001 |
| `bcf.add_viewpoint` | topic_id, view_id? | viewpoint_id | Yes | COL-BCF-001 |
| `bcf.delete_topic` | topic_ids[] | () | Yes | COL-BCF-001 |
| `bcf.import` | path | import_report | Yes | COL-BCF-001 |
| `bcf.export` | path, topic_ids[]?, version: 2.1\|3.0 | () | No | COL-BCF-001 |
| `bcf.list` | filter? | topic_summary[] | No | COL-BCF-001 |
| `bcf.issues_pane` | — | () | No | COL-BCF-004 |
| `bcf.set_extensions` | lists{types?, statuses?, priorities?, labels?, stages?, users?}, replacements?{} | () | Yes | COL-BCF-004 |
| `bcf.api_connect` | url, profile_name? | connection_id | No | COL-BCF-002 |
| `bcf.api_list_projects` | connection_id | project[] | No (query) | COL-BCF-002 |
| `bcf.api_bind_project` | connection_id, project_id | () | No | COL-BCF-002 |
| `bcf.api_unbind` | keep_local_copy=true | () | No | COL-BCF-002 |
| `bcf.api_sync` | connection_id? | sync_report | No | COL-BCF-002 |
| `bcf.api_status` | — | {connection, last_sync, queued_changes, conflicts} | No (query) | COL-BCF-002 |
| `bcf.api_resolve_conflict` | topic_id, field, use: server\|local | () | Yes | COL-BCF-002 |
| `bcf.api_disconnect` | connection_id | () | No | COL-BCF-002 |
| `bcf.set_workflow` | rules | () | Yes | COL-BCF-005 |
| `bcf.bulk_update` | topic_ids[], fields | () | Yes | COL-BCF-005 |
| `bcf.report` | topic_ids[]? \| filter?, format: pdf\|html\|csv\|xlsx\|bcf, template_id?, path | job_id | No | COL-BCF-005 |
| `bcf.from_clash` | clash_result_ids[], group_by? | topic_ids[] | Yes | COL-BCF-005 |
| `bcf.issues_for_element` | element_id | topic_summary[] | No (query) | COL-BCF-005 |
| `bcf.notifications` | since? | notification[] | No (query) | COL-BCF-005 |
| `cde.connect` | url, profile_name? | connection_id | No | COL-CDE-001 |
| `cde.select_documents` | connection_id | selection_session_id | No | COL-CDE-001 |
| `cde.download` | selection_session_id, target_folder? | files[] | No | COL-CDE-001 |
| `cde.upload_documents` | connection_id, files[] | upload_session_id | No | COL-CDE-001 |
| `cde.upload_status` | upload_session_id | status | No (query) | COL-CDE-001 |
| `cde.check_versions` | document_ids[]? | version_info[] | No (query) | COL-CDE-001 |
| `cde.disconnect` | connection_id | () | No | COL-CDE-001 |
| `api.profile.add` | name, url, kind, auth_method, options? | profile_id | No | COL-API-001 |
| `api.profile.list` | — | profile[] (no secrets) | No (query) | COL-API-001 |
| `api.profile.remove` | profile_id | () | No | COL-API-001 |
| `api.profile.test` | profile_id | report | No | COL-API-001 |
| `api.signin` | profile_id | status | No | COL-API-001 |
| `api.signout` | profile_id | () | No | COL-API-001 |
| `api.log` | profile_id, since? | entry[] (redacted) | No (query) | COL-API-001 |
| `http.request` | method, url, headers?, body?, profile_id? | response (Lua at trusted/native trust only; not exposed to MCP) | No | COL-API-001 |
| `bcf.apply_viewpoint` | viewpoint_id, target_view_id? | view_id | No | COL-BCF-003 |
| `bcf.export_viewpoint` | view_id?, path | export_report | No | COL-BCF-003 |
| `bcf.get` | topic_id | topic | No (query) | COL-BCF-001 |
| `bcf.import_viewpoint` | path, view_id? | import_report | Yes | COL-BCF-003 |
| `bcf.lists` | — | lists | No (query) | COL-BCF-001 |
| `bcf.merge_topics` | topics[] | merge_report | Yes | COL-BCF-001 |
| `bcf.put_topic` | topic | element_id | Yes | COL-BCF-001 |
| `bcf.put_viewpoint` | topic_id, viewpoint, snapshot? | viewpoint_id | Yes | COL-BCF-003 |
| `bcf.snapshot` | viewpoint_id | png_base64 | No (query) | COL-BCF-001 |
| `view.apply_viewpoint` | view_id, viewpoint | view_id | Yes | COL-BCF-003 |
| `model.purge_preview` | — | purge_tree | No | COL-MNT-001 |
| `model.purge` | item_ids[]?\|all? | count | Yes | COL-MNT-001 |
| `model.audit` | repair? | audit_report | Yes | COL-MNT-002 |
| `model.set_backup_policy` | max_backups, autosave_minutes? | () | Yes | COL-MNT-003 |
| `model.list_backups` | — | backup_info[] | No | COL-MNT-003 |
| `model.restore_backup` | backup_id, replace? | () | No | COL-MNT-003 |
| `warnings.list` | filter?: {kind?, severity?, element_ids?} | warning[] | No (query) | COL-MNT-004 |
| `warnings.acknowledge` | warning_ids[], note? | () | Yes | COL-MNT-004 |
| `warnings.export` | path, format: html\|csv | () | No | COL-MNT-004 |
| `warnings.review` | — | () | No | COL-MNT-004 |
| `standards.transfer` | source_document_id, selection[{kind, item_ids?}], on_conflict: overwrite\|new_only\|rename | transfer_report | Yes | COL-MNT-005 |
| `model.package` | path, zip?, options | transmittal_report | No | COL-MNT-006 |
| `model.statistics` | — | stats | No | COL-MNT-007 |
| `select.by_ids` | ids[] | element_id[] | No | COL-MNT-008 |
| `select.ids_of_selection` | — | {global_ids[], internal_ids[]} | No | COL-MNT-008 |

## 6.13 Open questions and deviations from the reference

**Deviations (intentional).**

| # | Topic | Reviera behaviour | Reason |
|---|---|---|---|
| D7-1 | Central model | An Open Source BIMserver project replaces the file-based or vendor-cloud central model; *Synchronise* = get latest + merge + check in. | Open, self-hostable server; no vendor cloud (TECH_STACK §3.7). |
| D7-2 | Element ownership | Advisory ownership through a live extended-data document, given force by the merge's ownership rule. No enforcement plugin is shipped; ownership follows what BIMserver supports (COL-OWN-004 deferred). The reference enforces ownership in its central model. | BIMserver has no native locking (TECH_STACK §5, decision 8). |
| D7-3 | Conflicts | Reviera merges by GlobalId at facet level and asks the user about true conflicts, with a recoverable conflict stash. The reference avoids conflicts by strict ownership and has no merge UI. | Optimistic merging fits a revision server and offline work. |
| D7-4 | System worksets | No separate worksets for families, project standards and views as ownership units; types, standards objects and views are borrowed individually as objects, and each view's annotation belongs to an automatic view workset. | Finer-grained, fewer conflicts; matches the RXD object model. |
| D7-5 | Worksets in IFC | Worksets are RXD; they are written as `IfcGroup` only on export. | Project-owner storage decision (Chapter 2a); `IfcGroup` remains available to other tools on export. |
| D7-6 | Offline working | Full offline editing with queued ownership claims. | The reference requires a connection to the central model to borrow. |
| D7-7 | Link formats | IFC is the only model-link format (local or BIMserver). Proprietary model formats, coordination caches and vendor-cloud coordination models are not supported (P3). | Open formats only; IFC + BCF cover coordination. |
| D7-8 | Point clouds | Open formats (E57, LAS/LAZ, COPC, PLY) indexed to COPC; no proprietary scan formats. | Licensing and openness. |
| D7-9 | CAD formats | DXF and DWG (through LibreDWG) only; DGN, SAT, SKP, 3DM deferred (P3). | No GPL-compatible readers of sufficient quality for some formats. |
| D7-10 | Clash detection | Basic interference check (P1) plus saved tests with tolerances, clearance, status tracking and grouping (P2), with BCF output. The reference's in-product check is hard-clash only. | Avoids depending on separate proprietary coordination products. |
| D7-11 | Issues | BCF 2.1/3.0 files, the buildingSMART BCF API (3.0, with 2.1 fallback) and an issue-tracking workflow instead of a vendor issue service; OpenCDE Documents API for CDE exchange instead of a vendor cloud. | Open standards (buildingSMART OpenCDE APIs). |
| D7-12 | Positioning storage | Shared coordinates stored as standard `IfcMapConversion` / `IfcProjectedCRS`, with grid and true north distinguished explicitly. | Standard georeferencing that other IFC tools read. |
| D7-13 | Compaction | No compaction command: files are always written in full. | IFC files have no free-space fragmentation. |
| D7-14 | Model history | Restore creates a new revision; history is never rewritten. | Auditable history; nothing is lost. |

**Open questions.**

| # | Question |
|---|---|
| Q6-1 | ~~Minimum supported BIMserver version~~: **resolved**, the latest stable release, currently 1.5.188 (TECH_STACK §5, decision 7). Still open: confirmation on 1.5.188 of every method marked "(verify)": serialiser and deserialiser lookup, `checkinSync`/`initiateCheckin` and the `/upload` endpoint, the `/download` and `/stream` endpoints, `getAllExtendedDataOfRevision`, `registerNewExtendedDataOnRevisionHandler`, `compare`, and query support for partial downloads (TECH_STACK §5, open decision 7). |
| Q6-2 | Behaviour and performance of full-model check-ins for very large projects; whether `LowLevelInterface` object-level transactions should be used for small syncs (TECH_STACK §3.7.1 notes this as "to verify"). |
| Q6-3 | Ordering guarantee of extended-data object ids on concurrent writes (the deterministic chain of COL-OWN-002 assumes ids are allocated in attach order). If not guaranteed, use server timestamps plus a client tie-break. |
| Q6-4 | Whether IFC4X3 deserialisers are available on BIMserver 1.5.188, the target release (TECH_STACK §3.7.3). |
| Q6-5 | ~~Whether to ship the optional enforcement plugin~~ **Resolved** (owner, 2026-10-12): no. Ownership follows what BIMserver supports, which is advisory only; COL-OWN-004 is deferred (P3). |
| Q6-6 | Size limits for RXD-embedded issues with many snapshots; whether snapshots should move to BIMserver extended data or an external store when large. |
| Q6-7 | ~~Chapter numbering~~ **Resolved**: the file is now `06_collaboration_coordination.md`, matching the overview. |

## 6.14 Traceability

| Research section (`research/04_documentation_collaboration.md`) or other source | Requirements |
|---|---|
| §7.1 Worksharing concepts | COL-MODE-001, COL-OWN-001, COL-OWN-003, COL-OWN-005, COL-WS-001 |
| §7.2 Enabling worksharing | COL-MODE-002, COL-WS-001 |
| §7.3 Worksets dialog | COL-WS-001 … 003 |
| §7.4 Synchronise, history, detach | COL-BIM-005, COL-BIM-006, COL-BIM-008, COL-MODE-002 |
| §7.5 Worksharing display modes, editing requests | COL-WS-004, COL-OWN-003, COL-OWN-005, COL-BIM-007 |
| §7.6 Cloud worksharing (functional context; vendor cloud out of scope) | COL-CLASH-002, COL-BCF-002, deviations D7-1, D7-7 |
| §8.1 Link types | COL-LNK-001 … 005 |
| §8.2 Manage Links, link visibility | COL-LNK-006, COL-LNK-007 |
| §8.3 Positioning and coordinates | COL-POS-001 … 004 |
| §8.4 Copy/Monitor, coordination review, reconcile hosting | COL-CM-001 … 004 |
| §8.5 Interference check | COL-CLASH-001, COL-CLASH-002 |
| §6.4 IFC export (geographic reference) | COL-POS-003 |
| §6.7 Publishing, integrated issues | COL-BCF-001, COL-BCF-004 |
| §9 Coordination tools and model maintenance | COL-MNT-001 … 008 |
| §11 Data model summary (worksharing, links) | §6.11 |
| TECH_STACK §3.7, §3.7.1–3.7.3, §5 | COL-CORE-001, COL-MODE-001 … 002, COL-BIM-001 … 008, COL-MRG-001 … 003, COL-OWN-001 … 004, COL-OFF-001 |
| spec/02a (RXD format and merge rules) | COL-MRG-001, §6.11 |
| buildingSMART BCF-API (v3.0), OpenCDE Foundation API (v1.1) and Documents API (v1.0) public repositories (SOURCE_REGISTER S0699–S0701) | COL-BCF-002, COL-BCF-005, COL-CDE-001, COL-API-001 |
| spec/02 §2.7.4 (DM-FAIL-001, stored warnings) | COL-MRG-003, COL-MNT-004 |
| web: Specify Coordination Settings (Autodesk public help, 2025) | COL-CM-004 |
| web: Orphaned elements and the Reconcile Hosting browser (Autodesk public help, 2025) | COL-CM-003 |
| web: Actions for Coordination Review (Autodesk public help, 2023) | COL-CM-002 |
| web: Review Warnings for Monitored Elements (Autodesk public help, 2022) | COL-CM-002 |
| web: Grant a Request to Borrow an Element (Autodesk public help, 2025) | COL-OWN-003, COL-OWN-005 |
| web: Granted, Denied, and Retracted Requests (Autodesk public help, 2014) | COL-OWN-003 |

Screenshots consulted (internal reference only, never used as Reviera assets): `screenshots/documentation/worksharing_central_local_diagram.png`, `worksets_dialog.png`, `synchronize_with_central_dialog.png`, `worksharing_display_worksets.png`, `worksharing_display_owners.png`, `editing_request_received.png`, `manage_links_new_dialog_type_filter.png`.
