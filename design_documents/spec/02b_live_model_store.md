# Chapter 2b — Live Model Store and Crash Recovery

**ID prefix:** `DM-LIVE` · **Status:** Draft 0.2 · 2026-10-12 (open questions OD1–OD5 accepted by the owner: sketch-session recovery, encryption at rest, time travel, unified BIMserver timeline, shared snapshots)
**Origin:** Project-owner feature directive (2026-10-12): *"Maintain the model live on disk as a SQLite database; on application crash, work can be restored from a near real-time on-disk representation."* This is a Reviera-originated feature. It has no counterpart in the functional reference, which offers only interval-based autosave reminders and backups.

**Related:** Chapter 2 §2.6–2.7 (regeneration, transactions, undo), Chapter 2a (RXD), Chapter 6 (BIMserver working copies), `TECH_STACK.md` §3.3.

---

## 1. Concept

| Layer | Role | Durability |
|-------|------|------------|
| **In-memory model** (Rust element DB, IfcOpenShell instance graph, RXD objects) | Where editing and regeneration happen | Volatile |
| **Live store**: `*.rvlive`, a SQLite database in WAL mode | A near real-time mirror of the model, updated after **every committed transaction** | Survives crashes, power loss bounded by `synchronous` mode (§5) |
| **Project file**: `.ifc` with embedded RXD (Chapter 2a) | Canonical, interoperable deliverable, written on **Save** | Durable and portable |

The live store **does not replace** IFC as the file format. Compatibility remains the first priority. The live store is a local, private working representation, comparable to a database's write-ahead log combined with a materialised state. When work is saved normally the live store is simply in step. When Reviera exits abnormally, it is the source of recovery.

```
 command ──► Transaction Manager ──commit──► in-memory model (regeneration done)
                                   │
                                   └─► ChangeSet (forward + inverse, dirty entities, RXD deltas)
                                            │  bounded channel (non-blocking for UI)
                                            ▼
                                   Live-store writer thread ──► SQLite (WAL)
                                            │   group commit ≤ 250 ms
                                            ▼
                                     *.rvlive  ──(on Save)──► project.ifc (atomic write)
```

## 2. File Location and Ownership

| Item | Rule |
|------|------|
| **Default location** | The per-user data directory: `<data_dir>/Reviera/live/<project-key>.rvlive`. On Linux `<data_dir>` is `~/.local/share`; on Windows it is `%LOCALAPPDATA%`; on macOS it is `~/Library/Application Support`. `<project-key>` is a hash of the project's canonical path plus the `Reviera_Manifest.DataId`. |
| **Why not beside the project file?** | SQLite locking is unreliable on network file systems (SMB/NFS), and projects often live on shared drives. Keeping the live store on local disk avoids corruption and keeps an unsaved working state out of shared folders. |
| **Optional location** | A preference may place the store beside the project as `.<name>.rvlive`. Reviera warns if that path is on a network file system. |
| **Companion files** | SQLite's `-wal` and `-shm` files, plus `<key>.lock`, which holds the PID, hostname, start time and a heartbeat timestamp. |
| **Single writer** | A live store is opened by exactly one Reviera process. A second instance opening the same project is offered **read-only** mode or **Open a copy**. A lock is stale when its PID no longer exists, or its heartbeat is older than 30 s and the host matches. |
| **New/untitled projects** | Get a live store immediately, keyed by a session UUID. Crash recovery therefore also covers work that was never saved. |

**Commands.** `live.status() -> {path, head_seq, saved_seq, pending, last_commit_at, size}` reports where the active project's store is, how far it has been written and how many change sets are still queued; the status-bar recovery indicator reads the same values (DM-LIVE-001).

## 3. Database Schema (version 1)

All tables are `STRICT`, the encoding is UTF-8, and SQLite must be 3.37 or later.

```sql
-- identity and state of this live store
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;
-- keys: schema_version, app_version, project_path, ifc_schema, data_id,
--       base_ifc_sha256 (IFC the session was opened from), saved_seq (last txn persisted to IFC),
--       head_seq (last txn applied here), clean_shutdown ('0'/'1'), session_id, opened_at, mode ('local'|'bimserver')

-- materialised IFC instance graph (everything except Reviera_* carrier Psets, see §3.1)
CREATE TABLE ifc_entity (
  sid        INTEGER PRIMARY KEY,         -- stable store id (= IfcOpenShell instance id at load; new ids allocated monotonically)
  ifc_type   TEXT    NOT NULL,            -- e.g. IFCWALL
  global_id  TEXT,                        -- rooted entities only
  step       TEXT    NOT NULL,            -- canonical STEP attribute list, e.g. ('2O2Fr$t4X7Zf8NOew3FLOH',#5,'Wall-001',$,...)
  seq        INTEGER NOT NULL             -- txn that last wrote this row
) STRICT;
CREATE UNIQUE INDEX ifc_entity_gid ON ifc_entity(global_id) WHERE global_id IS NOT NULL;
CREATE INDEX ifc_entity_type ON ifc_entity(ifc_type);

-- decoded RXD (Chapter 2a): project documents and element/type data, stored as JSON for direct access
CREATE TABLE rxd_document (
  path  TEXT PRIMARY KEY,                 -- canonical path, e.g. views/0192a3f5-...
  kind  TEXT NOT NULL,
  json  TEXT NOT NULL,                    -- canonical JSON
  seq   INTEGER NOT NULL
) STRICT;
CREATE TABLE rxd_element (
  global_id TEXT PRIMARY KEY,             -- element or type GlobalId
  scope     TEXT NOT NULL,                -- 'element' | 'type'
  json      TEXT NOT NULL,
  geom_hash TEXT,
  seq       INTEGER NOT NULL
) STRICT;

-- transaction journal: every committed transaction, in order
CREATE TABLE txn (
  seq        INTEGER PRIMARY KEY,         -- monotonically increasing
  at         TEXT    NOT NULL,            -- ISO-8601 UTC
  origin     TEXT    NOT NULL,            -- ui | lua | addin:<ns> | mcp:<client> | sync | system
  name       TEXT    NOT NULL,            -- user-visible undo label, e.g. "Create Wall"
  command    TEXT,                        -- registry command name(s), JSON array
  params     TEXT,                        -- JSON (credentials never stored)
  group_id   INTEGER,                     -- transaction group (one undo step)
  forward    BLOB    NOT NULL,            -- zstd(changeset: entity/RXD upserts+deletes)
  inverse    BLOB    NOT NULL,            -- zstd(inverse changeset) — undo after recovery
  undone     INTEGER NOT NULL DEFAULT 0   -- 1 if currently undone (redo stack)
) STRICT;

-- optional derived caches (rebuildable; may be dropped at any time)
CREATE TABLE cache_tessellation (
  global_id TEXT, lod TEXT, geom_hash TEXT, mesh BLOB,
  PRIMARY KEY (global_id, lod)
) STRICT;

-- interactive tool / sketch sessions in progress (§6.5): journaled micro-operations, not yet model transactions
CREATE TABLE session (
  id         INTEGER PRIMARY KEY,
  kind       TEXT NOT NULL,               -- sketch | tool | family_editor | group_edit | in_place
  owner      TEXT,                        -- element GlobalId being edited, or new-element placeholder
  tool       TEXT NOT NULL,               -- registry tool id, e.g. floor.create
  context    TEXT NOT NULL,               -- JSON: active view, work plane, type, options, selection
  started_at TEXT NOT NULL,
  state      TEXT NOT NULL                -- open | committed | cancelled
) STRICT;
CREATE TABLE session_op (
  session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  n          INTEGER NOT NULL,            -- order within session
  at         TEXT NOT NULL,
  op         TEXT NOT NULL,               -- JSON micro-op: add_curve, move_vertex, delete_curve, set_option, pick_point, ...
  PRIMARY KEY (session_id, n)
) STRICT;

-- time-travel keyframes (§8.5): compressed full-state snapshots bounding reconstruction cost
CREATE TABLE snapshot (
  seq        INTEGER PRIMARY KEY,         -- state after this txn
  at         TEXT NOT NULL,
  reason     TEXT NOT NULL,               -- save | interval | session_start | manual
  label      TEXT,                        -- user-named snapshot (optional)
  data       BLOB NOT NULL                -- zstd(canonical dump of ifc_entity + rxd_* at seq)
) STRICT;

-- BIMserver mode: base revision for three-way merge (Chapter 6)
CREATE TABLE base_entity (
  sid INTEGER PRIMARY KEY, ifc_type TEXT NOT NULL, global_id TEXT, step TEXT NOT NULL
) STRICT;
CREATE TABLE base_rxd (
  key TEXT PRIMARY KEY, json TEXT NOT NULL
) STRICT;
```

### 3.1 What is stored

| Stored | Not stored |
|--------|------------|
| **IFC entities**, as canonical STEP attribute text per instance. This is lossless: it is exactly what IfcOpenShell would write. | **RXD carrier Psets** (`Reviera_*`). RXD lives decoded in `rxd_*` and is re-encoded only when writing IFC. |
| **RXD objects**, as canonical JSON | Regenerated **B-rep geometry**. It can be derived from IFC plus RXD; at most an optional tessellation cache is kept. |
| The committed transaction journal, with forward and inverse change sets | — |
| **Interactive sessions in progress** (sketches, multi-click tools, family-editor and group-edit sessions), as journaled micro-operations (§6.5) | — |
| **Time-travel keyframe snapshots** (§8.5) | — |
| **Session metadata** | **Credentials**, API tokens and MCP session secrets |

## 4. Write Path

1. **Commit.** When a model transaction commits (Chapter 2 §2.7), the Transaction Manager emits a `ChangeSet`. It contains:
   - the upserted and deleted IFC instances (by `sid`), as STEP text
   - the RXD documents and element data that changed
   - the forward and inverse deltas
2. **Enqueue.** The `ChangeSet` goes onto a bounded channel to the **live-store writer thread**. The UI thread never waits on disk I/O.
   - If the channel is full, after 64 pending change sets or 64 MB, the next commit applies back-pressure, briefly blocking the issuer. The status bar shows "Writing recovery data…".
3. **Group commit.** The writer batches change sets that arrive within **250 ms** (configurable from 50 to 2,000 ms) into one SQLite transaction. In it, the writer upserts and deletes rows, appends the `txn` rows, and updates `meta.head_seq`.
4. **Serialisation runs in parallel.** STEP text for the dirty entities and canonical JSON for RXD are produced in parallel with rayon at commit time, then handed to the writer.
5. **Session micro-operations.** Each step of an interactive session, such as adding a sketch line or picking a point, is appended to `session_op` through the same writer and group commit. When the session finishes, its operations become one normal model transaction and the session row is marked `committed`. Cancelling a session marks it `cancelled`.
6. **Undo and redo** are ordinary change sets. They update the state tables and toggle `txn.undone`.
7. **Checkpointing.** WAL auto-checkpoint runs at about 4 MB. An explicit `PRAGMA wal_checkpoint(TRUNCATE)` follows each Save and each idle period of more than 30 s.

**Large operations.**
- Opening a project or importing a large IFC fills `ifc_entity` in a single transaction, using prepared statements and batched inserts.
- Loading a 200 MB IFC4 file must not add more than 25 % to open time (NFR-PERF-001). If the user starts editing first, the bulk insert continues in the background, and recovery during that window falls back to the source IFC plus the journal.

## 5. Durability Settings

| Setting | Default | Notes |
|---------|---------|-------|
| `journal_mode` | `WAL` | Readers never block the writer, which allows read-only access for diagnostics |
| `synchronous` | `NORMAL` | With WAL, committed data survives an **application crash**. On **power loss or an OS crash**, at most the last group-commit window can be lost. `FULL` is available as a "maximum safety" preference, at a throughput cost. |
| Group-commit window | 250 ms | Default upper bound on committed work at risk |
| `foreign_keys` | ON | — |
| `page_size` | 8192 | — |
| `cache_size` | −65536 (64 MiB) | — |

**Recovery-point objective (RPO):**
- **Application crash:** everything committed up to the last completed group commit, so normally less than 0.25 s of committed work at risk.
- **Power loss** with `NORMAL`: at most about 1 s.
- **Interactive sessions in progress** are covered too, to the same bound: their micro-operations are journaled (§6.5).

**Commands.** `live.flush() -> head_seq` forces the pending group commit and returns once that sequence number is durable; scripts call it before an external step that must not lose work (DM-LIVE-003). `live.settings.set(key, value) -> status` changes one preference: `synchronous` (`normal`|`full`), `group_commit_ms` (50–2,000), `location` (`user_data`|`beside_project`), `retention_days`, `retention_gb`, `quota_gb`, `undo_depth`. Out-of-range values are refused with `live.setting_out_of_range`, and a new location takes effect at the next project open (DM-LIVE-014).

## 6. Recovery

### 6.1 Detecting an unclean shutdown
At startup, and whenever a project is opened, Reviera scans the live-store directory. A store needs recovery when all of these are true:
- `meta.clean_shutdown = '0'`
- its lock is stale (§2)
- `head_seq > saved_seq`, or the store belongs to an untitled project that was never saved

### 6.2 Recovery dialog (UI)

**"Recover unsaved work"** lists each recoverable project with:
- the project name and path (or "Untitled")
- the time of the last change
- the number of unsaved changes
- the last few undo labels, for example "Create Wall · Move · Edit Type"

Actions per project:

| Action | Result |
|--------|--------|
| **Restore** (default) | Opens the project from the live store. Its title shows "(recovered)". |
| **Restore as copy…** | Asks for a new file path, then restores into it |
| **Show changes** | Lists the unsaved transactions, with origin and time |
| **Discard** | Asks for confirmation inside the dialog, then moves the store to the recovery bin (kept 7 days) |
| **Decide later** | Leaves the store untouched |

**Commands.** `recovery.list() -> [RecoverableProject]` returns the rows of this dialog (store key, project name or "Untitled", path, last change time, unsaved-change count, last undo labels, open sessions). `recovery.show_changes(store_key) -> [TxnSummary]` lists the unsaved transactions with origin and time. `recovery.discard(store_key) -> status` moves the store to the recovery bin. Restore is `recovery.restore` (§6.3) (DM-LIVE-004).

### 6.3 Restore procedure
1. Open the database read/write and run `PRAGMA integrity_check` (or `quick_check` above 1 GB).
2. **If it passes,** load state directly from `ifc_entity` and `rxd_*` into IfcOpenShell and the element DB. No replay is needed, because the tables are the materialised state. Then regenerate geometry in parallel, using `cache_tessellation` where `geom_hash` still matches.
3. **If it fails:**
   - Fall back to the **base IFC** (`meta.base_ifc_sha256`, the file the session was opened from, if it is still present and its hash matches).
   - **Replay** the `txn` forward change sets from `saved_seq + 1` up to the last readable row.
   - Report how far replay got, with the label and time of the last transaction recovered.
4. **Restore the undo history.** Undo and redo stacks are rebuilt from `txn`, so **Undo still works after recovery**, back to the configured depth.
5. **Mark the project dirty.** The title bar shows that it is unsaved; the user saves to write the IFC.
6. **Keep the old store.** The pre-recovery store is moved to the recovery bin, and a new session store is created.

**Commands.** `recovery.restore(store_key, as_copy_path?) -> {model_id, report}` runs this procedure. With `as_copy_path` the restored project is keyed to that new path. The report states whether the materialised state or the journal replay was used, and the last transaction recovered (DM-LIVE-005). `live.history(since_seq?, origin?) -> [TxnSummary]` lists the journal entries behind the rebuilt undo stack (DM-LIVE-006).

### 6.4 What is not recovered
- **Changes after the last group commit, after power loss.** With `NORMAL`, that is about 1 s at most.
- **Open modal dialogs and unconfirmed input:** for example a half-typed value in an Edit Type dialog that was never applied. This UI state is not model data.

### 6.5 Sketch and tool session recovery
1. **Recording a session.** Entering sketch mode, or starting a multi-step tool, creates a `session` row. Its context records the active view, work plane, element type and tool options.
   - Every step is journaled as a micro-operation in `session_op`: adding a curve, moving a vertex, setting a slope arrow, picking a point, changing an option.
   - Micro-operations are in the tool's own vocabulary. They are **not** model change sets, because nothing has been committed to the model yet.
2. **Recovering a session.**
   - The recovery dialog shows open sessions under their project, for example "Sketch for 'Floor 3' — 14 edits, last at 14:31:52".
   - After the model is restored, Reviera offers **Resume sketch** or **Discard sketch**.
   - **Resume** reopens the same view and re-enters the same tool. It replays the micro-operations to rebuild the sketch exactly, including the undo stack inside the sketch, and leaves the user in sketch mode, ready to finish or cancel.
3. **Sessions that can be resumed.** These are sketch mode (floors, roofs, ceilings, openings, railings, stairs by sketch, filled and masking regions, crop boundaries), multi-click placement tools (wall chains, beam chains, duct and pipe runs), group-edit mode, in-place family editing, and Family Editor documents opened from a project.
   - A Family Editor document has its **own live store** and is recovered in the same way.
4. **When the context is missing.** If the view or host element the session needs no longer exists in the restored model, the session can't be resumed. Its curves can still be **exported as detail lines** into a drafting view, so the geometry isn't lost.
5. **Tools must support replay.** Each interactive tool declares a micro-operation schema and a replay function. This is a requirement on every tool in Chapters 3–5 that has a multi-step session (DM-LIVE-015).

**Commands.** `recovery.resume_session(store_key, session_id) -> status` re-enters the tool and replays the session; it fails with `recovery.context_missing` when the view or host is gone. `recovery.discard_session(store_key, session_id) -> status` marks the session `cancelled`. `recovery.export_session_curves(session_id, drafting_view_id?) -> view_id` writes the session's curves as detail lines into the given drafting view, or into a new one named after the session; it is the only undoable command here (DM-LIVE-015).

## 7. Save, Close and Housekeeping
- **Save** writes IFC from memory, using the atomic save from Chapter 2a §9, then sets `meta.saved_seq = head_seq` and checkpoints.
- **Clean close** sets `clean_shutdown = '1'`. The store is kept, as the history for time travel and as a fast-reopen cache (§8.1), until it falls outside the retention policy. A clean store whose project file no longer exists is deleted after 14 days.
- **Journal retention.**
  - The journal is kept for **time travel** (§8.5), subject to a retention policy: by default **30 days or 5 GB per project**, whichever comes first, and always at least the configured undo depth (default 200 transactions).
  - Older journal rows are removed in whole keyframe intervals, so every retained seq can still be reconstructed.
  - Snapshots the user has named are kept until the user deletes them.
  - `VACUUM` runs in the background when free pages exceed 25 %.
- **Disk quota.** By default all live stores together may use up to 20 GB. The oldest clean stores are removed first, and stores that need recovery are never removed automatically.
- **Saving elsewhere.** Save As or Save a copy re-keys the store to the new path; the old key is removed.

## 8. Additional Uses
1. **Fast reopen.** If a clean store exists and its `base_ifc_sha256` and `saved_seq` match the project file, Reviera can open from the store without parsing STEP. The aim is at least 3× faster for large models. The file hash is always checked, so the IFC file stays authoritative.
2. **BIMserver working copies** (Chapter 6). The live store is the offline working copy: `base_entity` and `base_rxd` hold the downloaded base revision for the three-way merge, and the live tables hold local edits. Offline work, crash recovery and sync therefore share one mechanism, and an offline session that crashes recovers with its merge base intact.
3. **Audit and diagnostics.** `txn.origin` records who made each change (user, Lua script, add-in, MCP agent). This supports the "undo agent turn" feature (Chapter 1) and reports such as "what did the agent change?". Support bundles may include a redacted copy of the journal, with the user's consent.
4. **Read-only queries for tools.** The store is opened read-only alongside the WAL, so external analysis tools and Lua scripts can query model data with SQL without blocking editing (§10).

**Commands.** `live.query(sql, params?) -> rows` runs one read-only SQL statement on a separate read connection. Statements that write (`INSERT`, `UPDATE`, `DELETE`, `ATTACH`, `PRAGMA` with a value) are refused with `live.query.read_only`. It needs the `script`/`admin` trust level (§10) and is not exposed over MCP by default.

### 8.5 Time travel (history browser)

**Purpose.** Lets the user browse the model's past states, compare them with the present, and bring back anything that was lost. This includes the ability to review and undo work done by scripts and AI agents.

**Reconstruction.**
- The state at any retained seq *N* is rebuilt by taking the nearest **keyframe** (a `snapshot` row) and applying the forward change sets from the journal. Starting from the current head and applying inverse change sets backwards is used instead when that path is shorter.
- **Keyframes are written:**
  - at every Save
  - at session start
  - every 500 transactions or 30 minutes of editing
  - when the user asks (*Mark snapshot*, optionally named)
- **Building keyframes:** they are compressed with zstd and written by the writer thread when it is idle.
- **Reconstruction runs** in a background task, in parallel by `sid` range. It fills a **read-only historical model**: a separate in-memory model instance that never touches the live model.

**UI: History panel** (dockable; Ribbon › View › Windows › History; shortcut Ctrl+Shift+H).
- **Timeline.** The timeline lists transactions newest first, grouped by session and by save point.
  - Each row shows time, undo label, origin badge (User, Lua, Add-in, Agent, Sync) and the number of elements affected.
  - Filters: origin, command, element (all changes to the selected element), time range, and named snapshots only.
  - A scrubber along the top marks saves, syncs and named snapshots.
- **Preview.** Selecting a row opens a **historical view window** of the active view as it was after that transaction. It has a banner, "Viewing history: 2026-10-12 14:31 · read-only", and the elements that differ from the current model are highlighted: added in green, changed in amber, deleted shown as ghosts. The view also offers **Prev**, **Next** and **Return to present**.
- **Actions:**

  | Action | Result |
  |--------|--------|
  | **Compare with current** | A difference report listing elements added, changed or deleted and RXD documents changed, with parameter-level detail and a jump-to-element option |
  | **Restore selected elements** | Copies the historical state of the chosen elements (and their dependent RXD, such as their tags) into the current model as one new undoable transaction, "Restore from history". Hosting and joins are re-resolved; conflicts are reported. |
  | **Revert to this point** | Applies the inverse of every later transaction as one new undoable transaction. Nothing is lost: the reverted transactions stay in the journal. |
  | **Revert a single transaction or agent turn** | Applies one transaction's inverse, or one agent turn's inverses, if later transactions don't depend on it. Otherwise it lists the dependent transactions and offers to revert them all together. |
  | **Save as copy at this point** | Writes the historical model to a new `.ifc` file |
  | **Mark snapshot** | Names the current or selected point |
- **Scope.** Time travel covers what the live store retains (§7). Older history comes from saved files and backups. In BIMserver mode, the server's revisions and shared snapshots join the same timeline (§8.6).
- **Performance target.** Reconstructing a state up to 500 transactions from a keyframe on a 200 MB model shows the historical view in **3 s or less** on an 8-core workstation.

**Commands.** `history.list(since?, until?, origin?, command?, element?) -> [TxnSummary]` fills the timeline with its filters. `history.open_view(seq, view_id) -> historical_view_id` opens the read-only historical view. `history.compare(seq_a, seq_b?) -> DiffReport` compares two points, or one point with the present when `seq_b` is omitted. `history.restore_elements(seq, element_ids[]) -> report`, `history.revert_to(seq) -> report` and `history.revert_txn(seq | agent_turn_id, include_dependents?) -> report` each make one new undoable transaction. `history.save_copy_at(seq, path) -> status` writes the historical model as `.ifc` (DM-LIVE-017). `history.mark_snapshot(label?, seq?) -> seq` writes a keyframe at the head, or at `seq`, and names it; named snapshots are exempt from retention (DM-LIVE-018).

### 8.6 BIMserver integration: unified timeline and shared snapshots
*Accepted by the owner, 2026-10-12 (formerly OD4 and OD5).*

#### 8.6.1 Unified timeline
- **What it shows.** In BIMserver mode, the History panel shows **one combined timeline**, ordered by time, of:
  - local transactions
  - local saves
  - sync points (get latest, check in)
  - **server revisions** by every user
  - **shared snapshots** (§8.6.2)
- **Lanes.** Each entry has a lane marker: **Local**, **Server** or **Snapshot**. Lane filters show any combination; *Server only* gives the separate-track view.
- **Server revision entries.** Each shows the revision number, author, date and check-in comment. The list comes from `ServiceInterface.getAllRevisionsOfProject(poid)`, which is confirmed in the public BIMserver source. It is refreshed when a new-revision notification arrives.
- **Where local work sits.** Local transactions appear between the server revision they were based on (`meta.base_roid`) and the next sync. Sync points link local work to the revisions it produced, so it is visible which local changes went into which server revision.
- **Actions on a server revision:**
  - **View**: a read-only historical view. The revision is downloaded on demand, with its embedded RXD, and cached as a keyframe (`snapshot.reason = 'server_revision'`, with `roid` stored in `label`), so viewing it again is instant.
  - **Compare** with the current model, or with any other timeline entry
  - **Restore selected elements** into the current model, as one new undoable transaction
  - **Save as copy**
  - **Show changes**: a summary, revision against its predecessor, by element
- **Reverting to a server revision.** This is done locally as one transaction: the difference between the current model and that revision is applied. It reaches the server at the next sync, as an ordinary new revision. **History is never rewritten on the server.**
- **Permissions.** Users only see revisions their BIMserver account can read.

**Commands.** `history.list_server_revisions(since?, author?) -> [RevisionSummary]` lists the Server lane. `history.open_server_revision(roid, view_id?) -> historical_view_id` downloads (or reuses the cached keyframe of) a revision and opens it read-only. `history.revert_to_server_revision(roid) -> report` applies the difference as one local transaction (DM-LIVE-019).

#### 8.6.2 Shared snapshots on BIMserver
- **Purpose.** Lets a user publish a named snapshot, for example "Option B before client review", so that teammates can see it, compare against it and restore from it. Unlike a check-in, publishing a snapshot **doesn't change the project's latest revision**.
- **Default transport: Extended Data.** The snapshot is attached to the server revision it was based on. The calls are confirmed in BIMserver's public `ServiceInterface`:
  1. Register the extended-data schema `org.reviera.snapshot/1.0` once (`addExtendedDataSchema`, looked up by `getExtendedDataSchemaByName`).
  2. Upload the snapshot, a `.ifczip` with embedded RXD, written from the historical model at that seq (`uploadFile`).
  3. Attach it with `addExtendedDataToRevision(base_roid, {schemaId, fileId, title})`. The `title` is the snapshot label.
  4. Teammates list snapshots with `getAllExtendedDataOfRevisionAndSchema(roid, schemaId)` for each revision in the timeline, and download one with `getFile` when they open it.
- **Snapshot metadata.** A small JSON manifest, stored in the IFC's `Reviera_Manifest` and repeated in the extended-data title and description: label, description, author, created time, local seq, base `roid`, and the Reviera version that wrote it.
- **Optional: publish as a browsable project.** For snapshots that should open in BIMserver's own viewers and query tools, Reviera can check the snapshot in as a revision of a **companion project**, by default named `<Project> [Snapshots]` (created on demand with project-creation rights). It deliberately does **not** use a sub-project, because BIMserver merges sub-projects into the parent's latest model. The companion project is recorded in the main project's RXD document `project/links` with the role `snapshots`.
- **Snapshots from other users** appear in the Snapshot lane, showing their author. The actions are View, Compare, Restore selected elements, and Save as copy. A snapshot shared by someone else can't be edited in place; it is opened as a copy.
- **Deleting.** Authors (and project administrators) can withdraw a shared snapshot. The public `ServiceInterface` has no method for deleting extended data. In the extended-data transport, withdrawing therefore attaches a **tombstone** extended-data item (schema `org.reviera.snapshot-withdrawn/1.0`, holding the withdrawn snapshot's file id) to the same revision. Clients hide snapshots that have a tombstone. Administrators can purge withdrawn files on the server, outside Reviera. In the companion project, the revision is kept, because BIMserver revisions can't be deleted, but it is tagged withdrawn in its comment.
- **Size guard.** Snapshots larger than a configurable limit (default 500 MB) ask the user for confirmation, and suggest the companion project, where BIMserver stores objects instead of whole files.

**Commands.** `snapshot.publish(seq | snapshot_label, label, description?, target: extended_data|companion_project) -> {roid?, extended_data_oid?}` publishes a point of the timeline. `snapshot.list_shared(roid?) -> [SharedSnapshot]` lists the Snapshot lane, hiding tombstoned items. `snapshot.open_shared(snapshot_ref, mode: view|copy) -> historical_view_id | model_id` opens one read-only, or as an editable copy. `snapshot.withdraw(snapshot_ref) -> status` attaches the tombstone; only the author or a project administrator may call it (DM-LIVE-020).

## 9. Concurrency and Multi-core Notes
- **One writer thread per open project.** SQLite access is single-writer, and the writer owns its connection.
- **Parallel work happens before the writer.** Change-set serialisation, compression and hashing run in parallel with rayon, so the writer does only inserts.
- **Read-only connections** for diagnostics and SQL queries use separate connections in WAL mode and never take the write lock.
- **Recovery** loads table data in parallel chunks by `sid` range, then regenerates in parallel as described in Chapter 2 §2.6.

## 10. Security and Privacy
- The live store contains unsaved project data in plain form. It inherits the user's file permissions: `0600` on POSIX, and the user's ACL on Windows.
- The `params` column never holds credentials. Commands mark secret parameters in their schema, and those values are written as `"<redacted>"`.
- Lua and MCP may read the live store only through the commands in §12. Raw SQL is available only to callers holding the `script` or `admin` command permission class (Lua at trust level `trusted` or `native`, DM-LUA-007; MCP tokens granted those classes), and **only read-only**.
- **Encryption at rest:** see §10.1.

### 10.1 Encryption at rest
- **Engine.** SQLCipher (AES-256 page encryption, HMAC-SHA512 page authentication, PBKDF2 key derivation), through `rusqlite` with the `bundled-sqlcipher-vendored-openssl` feature. The live store, its WAL file, the session journal and the snapshots are all encrypted. The `-shm` file contains no row data.
- **Default: on**, whenever an OS credential store is available.
- **Keys.**
  - Each live store gets a random 256-bit key at creation.
  - The key is held in the **OS credential store**, accessed with the `keyring` crate: Windows Credential Manager, macOS Keychain, or Secret Service/KWallet on Linux. It is stored under the store's key name.
  - Recovery and time travel unlock the store with it, without asking the user.
- **Fallbacks.**
  - If no credential store is available (for example a headless Linux session), the user can set a **recovery passphrase**. The key is then derived with PBKDF2 at SQLCipher's default iteration count. The user can also turn encryption off for that machine; the status bar indicator then shows "Recovery data not encrypted".
  - Organisations can require encryption by policy setting. Reviera then refuses to create a store without a usable key: editing continues, and the indicator shows the store as unavailable.
- **Losing the key.** If the key is lost (credential store reset, or passphrase forgotten), the live store can't be read. The recovery dialog says so and offers to **Discard**. The project's saved IFC file is unaffected.
- **Scope.** Encryption applies to the live store only. Encrypting `.ifc` deliverables is out of scope, and remains for OS or file-system encryption or for document-management systems.
- **Performance.** The overhead must stay within the targets in §4. SQLCipher typically adds 5–15 % to I/O-bound work, which the writer thread absorbs.

**Commands.** `live.encryption.status() -> {enabled, key_source, policy}` reports the current state for the status-bar indicator. `live.encryption.set(enabled, key_source: os_store|passphrase, passphrase?) -> status` re-keys the store (SQLCipher `rekey`) on the writer thread; the passphrase is a secret parameter and is never journaled. Turning encryption off is refused with `live.encryption.required_by_policy` when policy requires it (DM-LIVE-016).

## 11. Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| DM-LIVE-001 | Reviera shall keep a live SQLite store (schema §3) for every open project, including untitled ones, and update it after every committed transaction. | P0 |
| DM-LIVE-002 | Live-store writes shall run on a dedicated writer thread with group commit (default 250 ms) and never block the UI thread except under back-pressure (§4). | P0 |
| DM-LIVE-003 | With default settings, an application crash shall lose no transaction committed more than 250 ms before the crash. | P0 |
| DM-LIVE-004 | On startup or project open, Reviera shall detect unclean shutdowns (§6.1) and offer the recovery dialog (§6.2). | P0 |
| DM-LIVE-005 | Restore shall rebuild the model from the materialised state; if the database is damaged, from the base IFC plus journal replay, with a report of how far recovery got (§6.3). | P0 |
| DM-LIVE-006 | Undo and redo history shall be available after recovery, up to the configured depth. | P1 |
| DM-LIVE-007 | The live store shall default to a local per-user directory. A store placed on a network file system shall trigger a warning. | P0 |
| DM-LIVE-008 | Only one process shall open a live store for writing; others are offered read-only or open-a-copy (§2). | P0 |
| DM-LIVE-009 | Save shall write the IFC atomically and then mark the store saved (`saved_seq`). The IFC file remains the canonical deliverable. | P0 |
| DM-LIVE-010 | Housekeeping (§7) shall prune the journal, enforce the disk quota, and never delete a store that needs recovery. | P1 |
| DM-LIVE-011 | Fast reopen from a matching clean store (§8.1). | P2 |
| DM-LIVE-012 | In BIMserver mode, the live store shall hold the base revision for three-way merge, so offline work and crash recovery share one store (§8.2). | P1 |
| DM-LIVE-013 | Secrets shall never be written to the store (§10). | P0 |
| DM-LIVE-014 | Durability, group-commit window, location, retention policy, quota and encryption shall be user preferences. | P1 |
| DM-LIVE-015 | Interactive sessions (sketch, multi-click tools, group edit, in-place and Family Editor editing) shall be journaled as micro-operations and be resumable after a crash (§6.5). Every multi-step tool shall declare a micro-operation schema and a replay function. | P1 |
| DM-LIVE-016 | The live store shall be encrypted at rest with SQLCipher by default, with its key held in the OS credential store and a passphrase fallback; organisations shall be able to require encryption (§10.1). | P1 |
| DM-LIVE-017 | Reviera shall provide a History panel for time travel: a timeline with filters, read-only historical views with difference highlighting, compare, restore selected elements, revert to a point, revert a transaction or agent turn, save a copy at a point, and named snapshots (§8.5). | P2 |
| DM-LIVE-018 | Keyframe snapshots and the retention policy (§7, §8.5) shall keep every retained point reconstructable within the performance target. | P2 |
| DM-LIVE-019 | In BIMserver mode, the History panel shall show one combined timeline of local transactions, saves, sync points, server revisions and shared snapshots, with lane filters (Local, Server, Snapshot), on-demand historical views of server revisions, compare and restore. Reverting to a server revision is a new local transaction and never rewrites server history (§8.6.1). | P2 |
| DM-LIVE-020 | Users shall be able to publish named snapshots to BIMserver without changing the project's latest revision: by default as Extended Data on the base revision, optionally as revisions of a companion `[Snapshots]` project. Teammates shall be able to list, view, compare, restore from and copy them, and authors to withdraw them (§8.6.2). | P2 |

**Acceptance criteria**
- [ ] Kill the process (`SIGKILL` / Task Manager) 1 s after creating a wall. On restart the recovery dialog lists the project, and Restore shows the wall.
- [ ] After recovery, Undo removes the wall.
- [ ] An untitled project with 20 edits, killed, is recoverable and can be saved as a new file.
- [ ] Corrupt the `.rvlive` file (truncate it to half its size). Recovery falls back to base IFC plus journal replay and reports the last recovered change.
- [ ] Opening the same project in a second instance offers read-only mode or a copy.
- [ ] The UI frame time is not affected by live-store writes during a 10,000-element array operation: the 95th-percentile frame time stays within 10 % of the time with the live store disabled.
- [ ] The `params` column contains no BIMserver password after a login command.
- [ ] Kill the process halfway through sketching a floor boundary (6 lines drawn). After restoring and choosing **Resume sketch**, the 6 lines are back in sketch mode on the same view, and Undo inside the sketch removes the 6th.
- [ ] Open the `.rvlive` file with plain `sqlite3`: it fails with "file is not a database". Recovery inside Reviera works with no prompt.
- [ ] With the credential store entry deleted, the recovery dialog explains that the data can't be decrypted and offers Discard. The saved IFC file opens normally.
- [ ] Let an MCP agent make 12 changes in one turn, then the user makes 3 more. In History, *Revert agent turn* undoes the agent's changes as one new transaction and keeps the user's 3 changes, or lists the dependencies if they conflict.
- [ ] Select a wall deleted 2 hours ago and choose *Restore selected elements*. It comes back with its hosted door and tag, as one undoable step.
- [ ] In BIMserver mode with 3 server revisions by two users, the History panel shows them interleaved with local transactions. *Server only* shows just the 3. Viewing revision 2 opens a read-only historical view, and viewing it again needs no download.
- [ ] User A publishes snapshot "Option B". The project's latest revision is unchanged. User B sees "Option B (A)" in the Snapshot lane after refreshing, and can compare it with B's current model.
- [ ] Publishing to a companion project creates `<Project> [Snapshots]` with the snapshot as a revision. The main project's latest revision and its sub-project structure are unchanged.
- [ ] After pulling power on a test VM with `synchronous=NORMAL`, at most the last 1 s of committed changes is missing, and the store passes `integrity_check`.

## 12. Commands

| Command | Parameters | Returns | Undoable | Req ID |
|---------|------------|---------|----------|--------|
| `recovery.list` | — | `[RecoverableProject]` | No | DM-LIVE-004 |
| `recovery.restore` | store_key, as_copy_path? | model_id, report | No | DM-LIVE-005 |
| `recovery.show_changes` | store_key | `[TxnSummary]` | No | DM-LIVE-004 |
| `recovery.discard` | store_key | status | No | DM-LIVE-004 |
| `live.status` | — | {path, head_seq, saved_seq, pending, last_commit_at, size} | No | DM-LIVE-001 |
| `live.flush` | — | head_seq (blocks until durable) | No | DM-LIVE-003 |
| `live.history` | since_seq?, origin? | `[TxnSummary]` | No | DM-LIVE-006 |
| `live.query` | sql (read-only), params? | rows | No | §10 (`script`/`admin` permission class only) |
| `live.settings.set` | key, value | status | No | DM-LIVE-014 |
| `recovery.resume_session` | store_key, session_id | status | No | DM-LIVE-015 |
| `recovery.discard_session` | store_key, session_id | status | No | DM-LIVE-015 |
| `recovery.export_session_curves` | session_id, drafting_view_id? | view_id | Yes | DM-LIVE-015 |
| `live.encryption.status` | — | {enabled, key_source, policy} | No | DM-LIVE-016 |
| `live.encryption.set` | enabled, key_source: `os_store`/`passphrase`, passphrase? (secret) | status | No | DM-LIVE-016 |
| `history.list` | since?, until?, origin?, command?, element? | `[TxnSummary]` | No | DM-LIVE-017 |
| `history.open_view` | seq, view_id | historical_view_id | No | DM-LIVE-017 |
| `history.compare` | seq_a, seq_b? (default head) | DiffReport | No | DM-LIVE-017 |
| `history.restore_elements` | seq, element_ids[] | report | Yes | DM-LIVE-017 |
| `history.revert_to` | seq | report | Yes | DM-LIVE-017 |
| `history.revert_txn` | seq \| agent_turn_id, include_dependents? | report | Yes | DM-LIVE-017 |
| `history.save_copy_at` | seq, path | status | No | DM-LIVE-017 |
| `history.mark_snapshot` | label?, seq? | seq | No | DM-LIVE-018 |
| `history.list_server_revisions` | since?, author? | `[RevisionSummary]` | No | DM-LIVE-019 |
| `history.open_server_revision` | roid, view_id? | historical_view_id | No | DM-LIVE-019 |
| `history.revert_to_server_revision` | roid | report | Yes | DM-LIVE-019 |
| `snapshot.publish` | seq \| snapshot_label, label, description?, target: `extended_data`/`companion_project` | {roid?, extended_data_oid?} | No | DM-LIVE-020 |
| `snapshot.list_shared` | roid? (default all in timeline) | `[SharedSnapshot]` | No | DM-LIVE-020 |
| `snapshot.open_shared` | snapshot_ref, mode: `view`/`copy` | historical_view_id \| model_id | No | DM-LIVE-020 |
| `snapshot.withdraw` | snapshot_ref | status | No | DM-LIVE-020 |

All of these are exposed to Lua and MCP. `live.query` is **not** exposed over MCP by default; it requires the `admin` permission class.

## 13. Technology and Licensing

| Component | Licence | Notes |
|-----------|---------|-------|
| SQLite (bundled through `rusqlite`'s `bundled` feature) | Public domain | Compatible with GPL-3.0-or-later; version pinned at 3.45 or later |
| `rusqlite` | MIT | — |
| `zstd` crate | MIT / BSD | Change-set and snapshot compression |
| SQLCipher (community edition, through `rusqlite` `bundled-sqlcipher-vendored-openssl`) | BSD-3-Clause-style | Compatible with GPL-3.0-or-later |
| OpenSSL 3.x (vendored, used by SQLCipher) | Apache-2.0 | Compatible with GPL-**3** |
| `keyring` crate | MIT / Apache-2.0 | OS credential store access |

## 14. Open Questions and Deviations
- ~~DM-LIVE-OD1~~ **Accepted (owner, 2026-10-12):** sketch and tool session recovery, specified in §6.5 (DM-LIVE-015).
- ~~DM-LIVE-OD2~~ **Accepted (owner, 2026-10-12):** encryption at rest with SQLCipher, on by default, specified in §10.1 (DM-LIVE-016).
- ~~DM-LIVE-OD3~~ **Accepted (owner, 2026-10-12):** time travel through the History panel, specified in §8.5 (DM-LIVE-017/018).
- ~~DM-LIVE-OD4~~ **Accepted (owner, 2026-10-12):** one combined timeline that includes BIMserver revisions, with lane filters that also give the separate-track view (§8.6.1, DM-LIVE-019).
- ~~DM-LIVE-OD5~~ **Accepted (owner, 2026-10-12):** shared snapshots on BIMserver (§8.6.2, DM-LIVE-020).
- **DM-LIVE-OD6:** if a future BIMserver version adds a way to delete extended data, `snapshot.withdraw` should use it instead of the tombstone. For now, withdrawing works through tombstones (§8.6.2).
- **Deviation from the reference.** The reference offers save reminders and rotating backups. Reviera adds continuous transactional persistence, and keeps the reference's numbered backups (`Chapter 6`, maintenance) as an independent safety net.

## 15. Traceability
- The owner's directive of 2026-10-12. This feature does not come from the reference product. Reference context: research/04 §9 (backups, maintenance) and research/05 §3 (transactions, undo).
- `TECH_STACK.md` §3.3 (Rust core, transactions), §3.7 (BIMserver working copy).
