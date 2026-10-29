# Integration with the FileEngine project

**Status:** Draft 0.4 · 2026-10-29 · for the owner's review (open questions in §10, requests to FileEngine in §11).
**Update (2026-10-29):** §7c uses the conversion/search/AI service's planned **headless research
API** (`convert_search_ai/design_documents/HEADLESS_RESEARCH_API.md`, branch
`research/headless-research-api` at 0db3dd3) to fill product data from documentation folders;
the review of that design from Reviera's side is §11.7.
**Implementation deferred (owner, 2026-10-27):** the owner is making the FileEngine improvements
of §11 first; Reviera's side starts once they land. **Update (owner, 2026-10-28):** the
improvements in progress include strong generated credentials on the REST door, like
FileEngine's other doors (§4, §11.2); the owner will say when FileEngine is deployed with them.
**Related:**
- Chapter 2 DM-FILE-007 (WebDAV, kept for other servers);
- Chapter 6 COL-BCF-001 … 004 (BCF issues), COL-API-001 (shared REST client and credentials);
- Chapter 2b §8.6 (server timelines);
- DM-CLS-001 (classification, keynotes);
- DOC-OUT-015 (COBie);
- `interfaces-are-mit` policy.

## 1. What Reviera needs from FileEngine

FileEngine is the owner's document server: a versioned, multi-tenant virtual file system with
ACLs, threaded discussion, BCF issues, search and conversion
(<https://github.com/Rational-Boxes/>). Reviera uses it for three things:

1. **Model storage.** Browse a FileEngine server, open models from it, save to it (each save
   a new version), and see and restore a model's past versions.
2. **Issues.** Read and write the BCF issues that FileEngine users raise on a model (in its
   web viewer, or from other BIM tools through its BCF service), so an issue opens in Reviera
   with the same camera, cut and selection, and replies go back to the same thread.
3. **Publishing.** Save reports and exports (PDF sheets, schedules, COBie, clash and analysis
   reports, BCF archives, IFC copies) straight to a FileEngine folder, for simple sharing with
   people who never open Reviera (owner request, 2026-10-27).
4. **Component Libraries on FileEngine.** Host an organisation's Component Libraries in
   FileEngine folders, browsed, placed from and added to from Reviera's Component Library panel,
   with FileEngine's permissions and versions (owner idea, 2026-10-27).
5. **Product documentation.** Link materials, products and equipment in the model to FileEngine
   folders of support documents: technical specifications, sustainability certifications,
   installation and O&M manuals, warranties, safety data sheets. Each folder keeps the product's
   identifying metadata (product code or SKU, MasterFormat division, …), and Reviera embeds file
   management for it: list, preview, upload, new version, search.

**Decision (owner, 2026-10-27):** Reviera talks to FileEngine through its **REST API**, not
WebDAV. WebDAV (DM-FILE-007) remains the general route for other servers. Using the API gives
UIDs that survive renames and moves, versions, metadata, ACL checks, renditions and the
discussion and BCF services, none of which WebDAV carries.

## 2. FileEngine as it is today (surveyed 2026-10-27)

| Service (repository) | What Reviera uses it for | State |
|---|---|---|
| **REST door** (`fileengine_http_proxy`), `/v1`, OpenAPI 3.0.3 | Everything on files and folders: directories, files, streamed content (with `Range`), versions, metadata, ACLs, roles, sign-in | Feature-complete against its plan |
| Core (`file_engine_core`), gRPC | Not reached directly; the REST door is a thin pass-through | Active development |
| **Discussion** (`fileengine_threaded_communication`) | Threads anchored to a file (and version), with a `model-viewpoint` anchor holding a BCF viewpoint; mentions, review requests, live updates over WebSocket | Backend complete (M0–M6) |
| **BCF service** (`fileengine_bcf_services`) | BCF-API 2.1 (projects, topics, comments, viewpoints, snapshots) and `.bcfzip` import/export, projected onto discussion threads; a BCF *project* is a FileEngine folder | Endpoints built over an in-memory store; the persistent store is pending |
| Conversion, search and AI (`fileengine_convert_search_ai`) | Thumbnails and previews of documents (renditions), extracted text, permission-gated full-text search | Built |
| Folder actions (`fileengine_folder_actions`) | Automating document folders: auto-sort, review on upload, notify, webhooks | Built |
| Outside share links (`fileengine_external_share`) | Handing documents to people without an account | M0 |
| MCP (`fileengine_mcp`) | Not needed by Reviera; agents can use it alongside Reviera's own MCP | Feature-complete |

What the REST door gives, in the terms that matter here:

- **Addressing:** every file and folder has a UID, and the root is `root` or the all-zeros
  UUID. Names can change; UIDs do not.
- **Sign-in:**
  - LDAP username and password (HTTP Basic), exchanged at `POST /v1/auth/token` for a bearer
    token (default lifetime one hour);
  - or OAuth/OIDC through the door itself (Google, GitHub, Microsoft, LinkedIn, Atlassian, any
    OIDC provider), which returns the same bearer token.
- **Tenants:** chosen by the host's subdomain or an `X-Tenant` header.
- **Versions:** each content `PUT` makes a new version, named by a timestamp. Versions can be
  listed, downloaded and restored.
- **Metadata:** string key/value pairs on any node (`GET/PUT/DELETE /v1/nodes/{uid}/metadata/{key}`).
- **ACLs:** decided by the core. A refused write returns `403`.
- **Renditions:** hidden child files of a file (thumbnails, previews, PDFs), counted in its listing.
- **Not available:** locking or check-out, a conditional `PUT` (no ETag or `If-Match`), and
  search (search is the conversion service's `POST /search`).

**Licences:**
- the services are AGPL-3.0, and the Python and JavaScript clients are LGPL-3.0;
- Reviera talks to them over HTTP only, and never links or vendors their code;
- Reviera's FileEngine client is MIT, like its other interoperability components, in
  `clients/fileengine/` (§9).

## 3. What Reviera already has to build on

- **BCF:**
  - topics, comments and viewpoints are kept in the model (`coordination/issues`, COL-BCF-001);
  - `.bcf` 2.1 and 3.0 archives are read and written, and `.bcfv` viewpoints too (COL-BCF-003);
  - section boxes and slice planes map exactly to BCF clipping planes, and hide/isolate to
    component visibility;
  - snapshots are taken with each viewpoint;
  - issues can be raised from clash results.

  FileEngine's viewpoint anchor is "exactly what xeokit's `BCFViewpointsPlugin` emits", which
  is the BCF-API JSON form. Reviera's viewpoint code reads and writes the same fields.
- **Specified, not built:**
  - the BCF-API client (COL-BCF-002, crate `reviera-opencde`);
  - the shared REST and credentials crate (COL-API-001, `reviera-net`);
  - WebDAV (DM-FILE-007).

  The FileEngine client builds on `reviera-net` for TLS, retries and keyring storage.
- **Types and materials:**
  - type parameters Manufacturer, Model, Keynote, Description, Type Mark and others;
  - classification codes (Uniclass, OmniClass, MasterFormat tables) per type or element;
  - the materials library;
  - COBie export (whose *Document* sheet is still empty).
- **Credentials:** the remote-analysis client already keeps a secret in the OS keychain and
  exchanges it for a short-lived bearer token (DM-RPC-011). FileEngine sign-in follows the same
  pattern.

## 4. Connections

A **FileEngine connection** is a per-user setting, never stored in a model:

- the REST door's base URL;
- the tenant (sent as `X-Tenant` unless the URL's subdomain names it);
- a display name;
- the sign-in method:
  - **Username and password:** LDAP over HTTPS, exchanged for a bearer token. The password is
    kept in the OS keyring only if the user asks; otherwise Reviera asks again when the token
    expires.
  - **Single sign-on:** the door's OAuth flow in the system browser, returning to Reviera. This
    needs a native-app return path on the FileEngine side (§10, Q3).
  - **Generated credentials** (coming, owner 2026-10-28): a strong credential generated on the
    FileEngine side for this app, as FileEngine's other doors already take, pasted into
    Reviera's connection settings. Its form, per the headless research design (2026-10-29): a
    key and secret (`fesk_…:fesks_…`) holding the `api` scope, exchanged at the bridge's token
    route for a session bearer and renewed at its refresh route while the key is live; the same
    bearer reaches the REST door and the conversion/search/AI service's headless routes (§7c). It is kept in the OS keyring and exchanged for a short-lived
    bearer token as the remote-analysis subscriptions do (DM-RPC-011), so there is no hourly
    password prompt, and it can be revoked on the server. This is expected to be the default
    for desktop use; the exact form (key and secret, or one token) follows FileEngine's
    deployment.
- plain `http://` is refused unless the connection is marked as a trusted local network.

Commands (UI, Lua and MCP; credential commands are UI and Lua only, never offered to agents):

- `fileengine.connect(url, tenant?, name?, auth: password|sso) -> connection_id`
- `fileengine.list_connections() -> connection[]`
- `fileengine.sign_in(connection_id)` and `fileengine.sign_out(connection_id)`
- `fileengine.remove(connection_id)`
- `fileengine.whoami(connection_id) -> {user, tenant, roles}`

## 5. Model storage

- **Browse.** *File › Open › From FileEngine…* lists a connection's folders
  (`GET /v1/dirs/{uid}`), with each model's size, owner, version count and thumbnail where one
  exists. It filters to `.ifc` and `.ifczip` (and Reviera project folders, §10 Q5).
  Recent-files entries remember `fileengine://<connection>/<uid>`, so a renamed or moved model
  still opens.
- **Open.**
  - The model is streamed down to a local working copy in the session folder, recording its
    UID and version. The live store, crash recovery and undo work exactly as for local files
    (Ch 2b).
  - If the user cannot write it (the ACL, checked with `GET /v1/nodes/{uid}/permissions` or
    learnt from a refused write), it opens read-only, with a banner.
- **Save.**
  - Saving writes the working copy locally first (atomic), then `PUT`s it, which makes a new
    FileEngine version. A save comment is kept as the version's metadata (`reviera.comment`).
  - **Conflicts:** before uploading, Reviera reads the file's current version (`GET
    /v1/nodes/{uid}`). If it is not the version Reviera opened or last saved, someone else has
    saved. Reviera then offers:
    - *Save as a copy beside it*;
    - *Overwrite* (their version stays in the history);
    - *Open theirs read-only to compare*.

    It never merges silently. There is a race between this check and the `PUT`, which only a
    conditional upload or a lock on the server can close (§10, Q2).
- **Save As** to FileEngine creates the file (`POST /v1/dirs/{uid}/files`) and uploads.
  *Save As* to a local path keeps no link.
- **Model metadata.** On save, Reviera writes metadata on the model's file:

  | Key | Value |
  |---|---|
  | `ifc.schema` | the IFC schema |
  | `ifc.project_guid` | the IFC project's GlobalId (what BCF's *Header/Files* records) |
  | `reviera.version` | the Reviera version that saved it |
  | `reviera.levels` | the level names |

  FileEngine can then search and list models by them, and a BCF topic can name its model.
- **Versions.** *File › Version History* lists the file's versions (`GET
  /v1/files/{uid}/versions`) with their saver, comment and date. It can:
  - open one read-only;
  - compare it with the open model (Reviera's model diff, when built);
  - restore it (`POST …/restore`, a new version: nothing is lost).

  The History panel shows these versions on the same timeline as local undo history (as
  Ch 2b §8.6 does for BIMserver).
- **Offline.** If the server cannot be reached, the working copy can be opened explicitly with
  an *Offline* banner. Saves are kept locally and uploaded when the server returns, with the
  same conflict check.
- **Thumbnails.** Reviera uploads a small PNG of the model's default 3D view with each save (as a
  rendition, if FileEngine accepts client-made renditions, or as metadata; §10 Q6), so the
  browser and FileEngine's file list show it.

Commands:
- `fileengine.browse(connection_id, folder_uid?) -> entry[]`
- `fileengine.versions(connection_id, file_uid) -> version[]`
- `fileengine.restore_version(connection_id, file_uid, version)`
- `file.open` and `file.save_as`, which accept `fileengine://<connection>/<uid>` (and
  `…?version=<ts>` for a past version, read-only).

## 6. Issues through FileEngine

Two doors exist on the FileEngine side.

1. **BCF-API 2.1 through the BCF service (recommended first).** This is COL-BCF-002's client:
   the same code works against FileEngine and any other BCF server. The mapping:
   - A **BCF project** is a FileEngine folder, and every model in it is the project's.
     *Collaborate › Issues › Connect to FileEngine* binds the open model to the folder it was
     opened from (or another).
   - A **topic** is a discussion thread with an issue facet (type, status, priority, assignee,
     labels, due date, stage). These are the fields Reviera's topics already have, and the
     project's lists come from the server's extensions.
   - **Viewpoints:**
     - the thread's anchor viewpoint, plus any additional viewpoints;
     - camera, clipping planes, selection and visibility by IFC GlobalId, and snapshot;
     - read and written by Reviera's existing BCF viewpoint code, so section boxes, slices,
       hide/isolate and selection survive both ways.
   - **Comments:** BCF's flat list. FileEngine flattens nested replies on export. A comment
     pinned to a viewpoint keeps the pin.
   - **Header/Files:** the topic's file is the model's FileEngine UID, with its IFC project
     GUID and the version it was raised on. When the open model is a later version, the topic
     opens with a note: *Raised on version …*.
   - **Sync:** as COL-BCF-002 sets out. Topics are cached in the model for offline reading,
     local edits are queued while offline, the server wins per field on conflict, and the local
     value is kept in the topic's *Conflicts* list.
2. **The discussion service directly (later).** This gives what BCF does not carry:
   - live updates while the model is open (`WS /files/{uid}/live`), so a colleague's new
     issue or reply appears at once;
   - `@mentions`;
   - review requests (raise, acknowledge, complete);
   - the attention feed;
   - issues on a model version.

   Reviera would read and write the thread's `model-viewpoint` anchor directly. Same data,
   richer behaviour, but FileEngine-only.

Either way:
- issues raised from clash results (`bcf.from_clash`) can be pushed to the server in one step;
- opening a FileEngine issue link (a permalink to a thread) in Reviera opens the model at that
  version and shows the issue.

Commands:
- the BCF-API client's `bcf.api_connect`, `bcf.api_bind_project` and `bcf.api_sync`
  (COL-BCF-002), given a FileEngine connection instead of a URL;
- `fileengine.open_issue(connection_id, thread_id)`.

## 7. Product and material documentation

**What is linked.** A **type** (a door type, a window type, equipment, a product placed as a
component) or a **material**, not each placed instance. Every instance of a type shares the
type's documents.

- **Instance documents:** an element can have its own link as well, for documents that belong
  to one installed item, such as a commissioning certificate or a warranty registration for
  one serial number.
- **Packaging:** a type or material links to **one documentation folder** in FileEngine,
  identified by its UID, which survives renames and moves.
- **In the model:**
  - RXD on the type: `rxd:docs = {connection hint, tenant, folder_uid}`;
  - in IFC as `IfcDocumentReference` (`Location` = the folder's FileEngine URL,
    `Identification` = its UID, `Name` = the folder's name), associated with the type by
    `IfcRelAssociatesDocument`.

  Other IFC tools therefore see the documents too, and FileEngine's own IFC viewer can follow
  them.

**Folder metadata (the product record).** FileEngine metadata on the folder holds the product's
identity, under namespaced keys so other FileEngine tools can use them:

| Key | Meaning | Maps to in Reviera |
|---|---|---|
| `product.manufacturer` | Manufacturer | type parameter *Manufacturer* |
| `product.model` | Model or product name | *Model* |
| `product.code` | Product code / SKU | *Product Code* (new type parameter) |
| `product.gtin` | GTIN / EAN barcode | – |
| `class.masterformat` | CSI MasterFormat section (`08 11 13`) | MasterFormat classification; keynote |
| `class.uniclass` | Uniclass 2015 Pr code | Uniclass classification |
| `class.omniclass` | OmniClass Table 23 code | OmniClass classification |
| `product.url` | Manufacturer's product page | *URL* |
| `cert.*` | Certifications held (`cert.fsc`, `cert.c2c`, `cert.epd`, …), each with its expiry | – (shown in the Documents pane) |
| `epd.gwp_a1a3` | Embodied carbon per declared unit, from the EPD | embodied carbon (ANA-CARB-001) |
| `warranty.years` | Warranty duration | COBie *Type.WarrantyDuration…* |

- **Values:** strings, since that is what FileEngine metadata holds; numbers and dates are
  written in a fixed form.
- **Which side is the master:** proposed as the folder. On linking, and on request, Reviera
  shows a side-by-side comparison of the folder's values and the type's parameters, and the
  user picks which to keep, field by field (§10 Q8).

**Documents in the folder.** Each file carries a `doc.type` metadata key:
- *specification*
- *certification*
- *EPD*
- *installation*
- *O&M*
- *warranty*
- *SDS*
- *drawing*
- *other*

Each can also carry an issue date and an expiry date. Subfolders are allowed and listed as
groups.

**The Documents pane** (a dockable pane, for the selected element's type or material) embeds
file management:

- **Browsing:** the folder's documents, grouped by `doc.type`, with thumbnails (FileEngine's
  renditions), version count, dates, and a warning on anything expired or expiring soon.
- **Viewing:** open a document in the system's viewer (downloaded to a temporary copy), or in
  the pane's preview when FileEngine has a preview or PDF rendition.
- **Uploading:** add documents by drag and drop or *Add…*, asking for `doc.type` and dates.
  Upload a new version of a document. Rename, move within the folder, and edit metadata.
- **Creating and linking:**
  - *Create folder* for an unlinked type: from a folder template (the standard subfolders and
    metadata keys), in a configured library location such as
    `/Product Library/<MasterFormat division>/<Manufacturer> <Model>`;
  - *Link existing folder* (browse or search);
  - *Unlink*.
- **Searching:** the documents' extracted text across the library (the conversion service's
  `/search`, gated by the user's permissions). For example: "every product with an FSC
  certificate expiring this year".
- **Sharing:** *Share* makes an outside share link (the share service) for a document or the
  folder, for a contractor or client without an account.

**Handover.**
- **COBie:** the *Document* worksheet is filled from linked folders: one row per document, with
  its type, the sheet and row it describes, its FileEngine URL and version.
- **IFC:** export carries the `IfcDocumentReference`s.
- **Handover package (optional):** a zip of every linked document at its current version,
  beside the model (or FileEngine's zip download of the library folders).
- **Reports:** a *Documentation report* lists types with no folder, folders with missing
  `doc.type` classes (no O&M, no warranty), and expired certificates.

**Folder automation.** FileEngine's folder actions can be bound to the product library. For
example:
- a review raised when a certification is uploaded;
- an inbox folder that sorts uploaded documents into product folders by their text.

This is FileEngine's side. Reviera only shows the results.

Commands:
- `docs.link(target_ids, connection_id, folder_uid)`, `docs.unlink(target_ids)`;
- `docs.create_folder(target_id, connection_id, parent_uid?, template?)`;
- `docs.list(target_id) -> document[]`;
- `docs.upload(target_id, path, doc_type, issued?, expires?)`;
- `docs.set_metadata(...)`;
- `docs.compare(target_id)` and `docs.apply(target_id, fields, direction: from_folder|to_folder)`;
- `docs.search(connection_id, text)`;
- `docs.report()`;
- `docs.share(document_uid, recipient, expires)`.

## 7a. Publishing reports and exports

Anything Reviera writes to a file can be written to FileEngine instead, so sharing a set of
drawings or a report is one step: publish, and FileEngine's people, links and notifications do
the rest.

- **One destination for every output.** Every command that writes a file accepts a FileEngine
  destination, `fileengine://<connection>/<folder_uid>/<file name>`, wherever it accepts a path.
  That covers:
  - `export.pdf` (plans, sections, sheets);
  - `export.cobie`;
  - `schedule.export`;
  - `clash.export_report`;
  - `embodied_carbon.export`, `pressure_loss_report.export`, `results.export_package` and the
    other analysis reports;
  - `bcf.export`;
  - `gbxml.export`, `hbjson.export`, `steel_nc.export`;
  - `file.save_as` (an IFC copy).

  Reviera writes the file locally first, as it does now, then uploads it. A failed upload keeps
  the local file and says where it is, and the status bar retries it.
- **In the window.** Every export dialog's location has a *FileEngine…* place beside the local
  folders, remembering the last folder used per connection and model.
- **Same name, new version.** Publishing to a name that already exists in the folder adds a
  version of that file, not a second file, so a link to *Ground Floor.pdf* always shows the
  latest. FileEngine keeps every earlier issue in its history. *Publish as a new file* is the
  other choice.
- **What a published file says about itself.** Metadata written on each published file:

  | Key | Value |
  |---|---|
  | `reviera.source_uid` and `reviera.source_version` | the model it came from, when that model is on FileEngine |
  | `reviera.source_name` | the model's file name |
  | `reviera.output` | `pdf`, `cobie`, `schedule`, `clash_report`, `carbon_report`, `bcf`, `ifc`, … |
  | `reviera.views` | the sheets or views it contains |
  | `reviera.published_by` and `reviera.published_at` | who published it, and when |

  FileEngine can therefore list "everything published from this model", and a reader can get
  back to the model from a PDF.
- **Publish sets.** *File › Publish to FileEngine…* picks several outputs at once, publishes
  them together, and can save the choice as a *publish set* kept with the model, to run again
  with one click (*Publish again*). For example:
  - the issue sheets as one PDF;
  - the door and room schedules;
  - COBie;
  - the latest clash report.

  This is the simple form of an issue: the sheet revisions and transmittals of the
  documentation chapter can build on it later.
- **Public share links, made in Reviera (owner decision, 2026-10-27).** The Publish dialog has
  a *Share* step, and every FileEngine file and folder in Reviera has a *Share link…* command:
  in the FileEngine browser, the Documents pane, the list of published outputs, and
  *File › Share Link…* for a model opened from FileEngine. The *Share link…* dialog makes an
  outside share link through FileEngine's share service, for people without an account:
  - **What:** one file (pinned to the version being shared), a folder's files as one zip, or a
    folder with its subfolders. A publish set shares as one folder link.
  - **Who:** the recipient's email address. FileEngine sends a one-time code to that address,
    so the link alone is inert.
  - **Limits:** an expiry (default 14 days) and an optional download count.
  - **Result:** the link, shown with *Copy* and *Open in browser*, and kept in the model's
    *Shared links* list. That list shows each link's target, recipient, expiry and downloads so
    far, and has *Revoke*.
  - **Permissions:** a link conveys at most what its creator can read, now and at redemption,
    as FileEngine enforces.

  Besides the public link, the Share step can also:
  - copy the file's internal FileEngine link, for colleagues with accounts;
  - post a note to the file's discussion thread with `@mentions`, which FileEngine notifies.

  Each is chosen per publish; nothing is shared unless asked.
- **Folder automation.** A publish folder can carry FileEngine folder actions (a review raised
  on each new PDF, a notification to the client, a webhook). That is configured in FileEngine;
  Reviera only publishes into it.

Commands:
- every exporting command's `path` accepts `fileengine://…`;
- `publish.run(outputs[] | set_id, connection_id, folder_uid, as_new_file?, share?: {link?,
  outside?: {recipient, expires}, note?})` returns what was published, with each file's UID and
  version;
- `publish.save_set(name, outputs[], connection_id, folder_uid)`, `publish.list_sets()` and
  `publish.remove_set(set_id)`;
- `fileengine.share_link(connection_id, uid, recipient, expires?, max_downloads?, scope?:
  file|folder|subtree) -> {link, expires}`;
- `fileengine.list_links(connection_id, uid?)` and `fileengine.revoke_link(connection_id,
  link_id)`.

  Agents may list links. Making or revoking a public link is the user's decision, through the
  window or Lua, as with credentials.

## 7b. Component Libraries hosted on FileEngine

A Component Library is already a plain folder:
- mesh files (OBJ with MTL and textures, STL, PLY, glTF, GLB) and IFC model fragments;
- sub-folders as the library's folders;
- one JSON manifest, `reviera-library.json`, at its root (DM-LIB-001 … 004).

A FileEngine folder with the same layout **is** a library, so an organisation keeps one shared,
permissioned, versioned library on its FileEngine server instead of copies on every machine.
This is a fuller form of DM-LIB-001's *organisation libraries*, which the spec plans only as a
read-only static index over HTTPS or git.

- **Opening.**
  - In the Component Library panel, *Open…* gains *From FileEngine…*: pick a connection and a
    folder holding a `reviera-library.json`, or create one there (*New…*).
  - The library then appears under the panel's *Organisation* sources, with its state
    (indexed, indexing, offline, error).
  - An administrator can pre-configure FileEngine libraries for everyone, as DM-LIB-001's
    policy file allows.
- **Reading.**
  - The manifest and the folder listing are read through the REST door.
  - Items download on demand into the per-user library cache, keyed by their UID and version.
    A new version on the server replaces the cached one the next time the item is used.
  - Thumbnails come from the manifest's preview images, or from FileEngine's renditions when it
    makes them for 3D files.
  - Cached items can be placed offline; uncached ones show greyed.
  - The panel's background index (DM-LIB-001) covers FileEngine libraries like local ones, so
    search stays instant.
- **Placing.**
  - *Place Component* works as for a local library: the item is loaded into the project as a
    type the first time and embedded in the model, so the model opens anywhere without the
    server.
  - The type records where it came from: connection, library folder UID, item UID and version.
- **Updates.**
  - When the library has a newer version of an item a model uses, the panel and the type's
    Properties show *Update available*.
  - *Reload from library* brings it in, with the comparison DM-LIB-001 shows before reloading.
  - Nothing changes in a model until someone reloads.
- **Adding and editing (by permission).** Users FileEngine lets write to the library can:
  - *Import into Library* (files and their textures and buffers);
  - *Create Component* from a selection, straight into a library folder;
  - edit an item's manifest entry (name, category, tags, units, insertion point, product data);
  - move and rename items;
  - delete items into `.deleted` (FileEngine's own history keeps every version as well).

  Everyone else sees the library read-only.
- **The manifest and concurrent edits.** The manifest is one file that several people may edit
  at once. Its entries are keyed by item path and written with sorted keys, so Reviera saves it
  as a three-way merge:
  - it re-reads the server's current manifest;
  - applies its own changes entry by entry;
  - uploads only if the server's version is still the one it merged with, retrying otherwise.

  Two people adding different items never conflict. Two editing the same item's entry get the
  usual conflict choice. (§10 Q14 asks whether per-item FileEngine metadata should replace the
  manifest's item entries instead.)
- **Curation (FileEngine's side).** A library can have an *Incoming* folder bound to
  FileEngine's folder actions: a review raised on each new component, moved into the library
  when approved. Reviera shows the review state on incoming items.
- **Products and documents.** A library item's product data (manufacturer, model, code,
  classification) and its documentation folder (§7) live in its manifest entry. Placing the
  component therefore links the new type to the same documentation folder: a door placed from
  the library arrives with its specification sheet, certifications and warranty.
- **Sharing a library.**
  - A share link (§7a) for the library folder gives outsiders a zip, which opens as a local
    library.
  - Another tenant's library can be added read-only when FileEngine grants it.

Commands. The existing library commands (`library.open`, `library.create`,
`library.import_files`, `library.set_meta`, `library.delete_items`, `library.restore_items`,
`component.place`, and Create Component) accept a FileEngine library root,
`fileengine://<connection>/<folder_uid>`, wherever they take a folder. Two more:
- `library.updates(library_id?) -> [{type, item, used_version, latest_version}]`;
- `library.reload_item(type_id)`.

## 7c. Extracting product data from documents

The documentation folders of §7 hold the values a model's types need: manufacturer, model and
code, sizes, fire rating, U-value and g-value, EPD figures, certificate numbers and expiry
dates. The conversion/search/AI service's planned **headless research API** reads them out of
the documents (HEADLESS_RESEARCH_API.md §3.1), and Reviera puts what the user accepts into
the model. It is the same comparison as §7's folder metadata, with each value backed by a quote.

**Flow.**
1. *Fill from Documents…* in the Documents pane (or `docs.extract`) for the selected types or
   materials: their linked folders, or files picked in the pane. Optionally a search first
   (`POST /csai/search` with `folder_uids`, §3.10) to find the datasheets, then extraction by
   file id.
2. Reviera sends `POST /csai/v1/extract` with a **field set** as the JSON Schema and streams the
   result (`plan`, `item`, `error`, `usage`, `done`). Progress shows per file; cancelling closes
   the stream.
3. A review table: for each type and field, the current value beside the extracted one with
   its unit, the evidence quote and page, the document and version, and whether it came from
   the cache. Fields with `found: false` are listed as not found. Skipped files are listed with
   their reasons (no text, budget, not readable), so "no text" means *check by hand*.
4. The user accepts values field by field (or all found). Reviera writes them as parameters in
   one undo step, through the same commands a user would use (`element.set_param`,
   `glazing.set_type`, `frame.set_type`, the carbon factors). Optionally it writes the product
   record's folder metadata too (§7's `product.*`, `cert.*`, `epd.*` keys) through the REST
   door. Nothing is written without the user accepting it; the service never writes values
   anywhere.

**Field sets** ship as JSON Schemas (MIT, `schemas/extract/`), each field naming the Reviera
parameter it fills and its data type, so units are parsed by Reviera's own unit parser:

| Set | Fields | Fills |
|---|---|---|
| Product | manufacturer, model, product code, GTIN, URL | *Manufacturer*, *Model*, *Product Code*, *URL* |
| Dimensions | width, height, depth, weight | type sizes (shown, applied only on request: a size change moves geometry) |
| Fire | fire rating (EI/REI or hours), reaction to fire class | `Pset_*Common.FireRating`, … |
| Thermal | U-value, g-value, light transmission | `ThermalTransmittance`; glazing types' `Reviera_GlazingUnit` (ARC-GLZ-003) |
| Glazing (horticultural) | light and PAR transmission, haze, U, g, thickness, weight, least pitch | `glazing.set_type` |
| EPD | EPD number, programme operator, declared unit, GWP A1–A3, valid until | embodied carbon (ANA-CARB-001), `epd.*` folder keys |
| Certification | scheme (FSC, PEFC, C2C, …), certificate number, holder, valid until | `cert.*` folder keys, the Documents pane's expiry warnings |
| Warranty | duration, start condition | COBie *Type.WarrantyDuration…* |

Each schema field is a plain value (string, number, date); the service wraps it with `unit`,
`evidence` and `found` (§3.1), so Reviera's schemas stay small.

**Provenance in the model.** An accepted value records where it came from, in RXD beside the
parameter: `{file_uid, version, run_id, field, cached}`. The quote is not copied into the model
by default: the run record in FileEngine holds it, erasure reaches it there, and a model copied
outside the tenant does not carry document text (this follows the parked PII question, §6 of the
csai design). The Documents pane shows the quote by fetching the run. When a document gets a new
version, the pane marks the values taken from the old one as *from an earlier version* and
offers to extract again.

**Limits and usage.** Reviera reads the usage state the service returns (headers, the `usage`
event, `GET /csai/v1/usage`) and shows it in the Documents pane, warning at `warning` and
explaining a `429` or `skipped: budget` with its reset time. Re-asking the same question of the
same version hits the service's cache (§3.7), so *Fill from Documents* is cheap to repeat;
*Extract again* sends `cache: "refresh"`.

**Agents.** MCP agents can run `docs.extract` (it acts as the signed-in user and writes nothing)
and propose values with `docs.apply_extracted`, which writes like any other command and can be
undone. The service's own MCP tools are never needed: extraction answers from the documents only.

Commands:
- `docs.extract(target_ids | file_uids, sets, cache?: use|refresh) -> run` (streams progress to the
  window; returns the items);
- `docs.extract_status(run_id)`, `docs.usage(connection_id)`;
- `docs.apply_extracted(run_id, accepted: [{target_id, field}], to_folder?: bool)`.

## 8. Permissions, credentials and agents

- FileEngine decides every permission. Reviera shows read-only states and disables actions the
  user cannot take; it never tries to enforce ACLs itself.
- Tokens and any stored password are in the OS keyring, never in settings files, models, logs
  or crash reports.
- MCP agents can use every `fileengine.*`, `docs.*` and `bcf.*` command that does not handle
  credentials, acting as the signed-in user. Deleting on the server is not offered to agents,
  matching FileEngine's own MCP door.

## 9. Building it

- **Client:** `clients/fileengine/rust` (MIT), a typed client for the REST door's OpenAPI
  contract (auth, nodes, content streaming, versions, metadata, permissions), with retries and
  `Range` resume. It uses `reviera-net` for TLS and keyring storage. Later it gains the
  discussion and search endpoints.
- **Application:** commands in `reviera-app` (connections, storage, documents). The BCF-API
  client in `reviera-opencde` gains FileEngine connections.
- **Window:**
  - *File › Open › From FileEngine*, *Save to FileEngine* and *Version History*;
  - the Documents pane;
  - the connection dialog in *Options*.
- **Testing:** against a local FileEngine stack (its `scripts/start_backend_services.sh`), with
  `live`-marked tests skipped when no stack is configured. The client is also tested against
  the OpenAPI contract with a mock server.
- **Phases:**
  1. connections, browse, open, save and versions;
  2. publishing: FileEngine destinations for every export, publish sets, and public share links
     made in the window (once FileEngine's share door is live);
  3. BCF issues over BCF-API, once FileEngine's persistent BCF store lands;
  4. Component Libraries on FileEngine (read, place, cache, updates; then writing, with the
     manifest merge);
  5. documentation folders, the Documents pane and COBie *Document*; then extraction of product
     data (§7c), once the headless research API is deployed;
  6. the discussion service: live updates, mentions and reviews;
  7. search, sharing and automation views.

## 10. Questions for the owner

1. **Packaging.** DM-FILE-007 said server-specific behaviour belongs in an add-in. Should
   FileEngine support be built in (shipped, off until a connection is added), or an optional
   add-in?
2. **Concurrent saves** (§11, items 1 and 2). The REST door has no lock and no conditional `PUT`, so two people can
   overwrite each other between Reviera's version check and its upload. Can FileEngine add
   `If-Match` on `PUT /v1/files/{uid}/content` (or a check-out lock), or is check-then-put with
   FileEngine's versions as the safety net enough?
3. **Single sign-on for a desktop app** (§11, item 15). The door's OAuth flow returns the token in a URL
   fragment to an allow-listed web address; a native app's loopback listener never sees
   fragments. Options:
   - allow-list `http://127.0.0.1` with a query-string return;
   - add a device-code flow;
   - accept only username and password for desktop clients for now.

   Which do you prefer?
4. **One token for every service** (§11, item 17). Do the BCF and discussion services accept the REST door's
   bearer token (the discussion service says it does; the BCF service uses a shared HS256
   secret), so Reviera signs in once per connection?
5. **What a "model" is on FileEngine.** A single `.ifc`/`.ifczip` file, or a folder (the model
   plus its `.resources`, linked models and exports)?
6. **Thumbnails** (§11, item 10). Can a client upload a rendition for a file, or should Reviera keep its
   thumbnail as a separate hidden file or as metadata?
7. **The product library.** Is there one shared product library per tenant, or per project?
   Where should Reviera create new documentation folders by default, and should it ship a
   folder template?
8. **Master of product data.** Should the folder's metadata be the master for manufacturer,
   model, code and classification (Reviera offers to update the type), or the model (Reviera
   writes the folder), or neither, with a comparison either way?
9. **Product fields.** *Answered (owner, 2026-10-27): "CLI/MF division" is the **CSI
   MasterFormat** division and section (`08 11 13`), kept as `class.masterformat`.* Still open:
   should Uniclass and OmniClass codes be kept too, and which other fields does a folder need
   (GTIN, EPD values, certifications and their expiries, warranty)?
10. **Instance documents.** Should individual elements (one installed unit with a serial
    number) have their own folders as well as their type's, or only types and materials?
11. **Materials.** Should materials link to folders the same way as types (an EPD per
    material), and should an EPD's embodied-carbon value feed Reviera's carbon calculation?
12. **Issues route.** Start with BCF-API through the BCF service (standard, waits for its
    persistent store), or go straight to the discussion service (live, FileEngine-only)?
13. **Publishing.**
    - Should republishing to the same name add a version (the proposal) or keep each issue as
      its own file?
    - Is there a default publish folder per project (for example `<project>/Published`)?
    - Which outputs belong in a first publish set?
    - **Share links:** FileEngine's share service is at M0. Links can be minted and revoked,
      but there is no public door yet that redeems them. In-app links therefore depend on that
      door being deployed. Is a default expiry of 14 days right, and should a download limit be
      on by default?
14. **Component Libraries.**
    - Is there one organisation library per tenant, or several (per discipline or client)?
    - Should new components go through a review (an *Incoming* folder with folder actions)
      before they appear in the library?
    - Should an item's details stay in the shared `reviera-library.json`, merged on save as
      proposed, or move to per-file FileEngine metadata? Per-file metadata means no merging,
      but a library copied to a local folder would then need its manifest rebuilt.

## 11. Improvements on the FileEngine side

What FileEngine would need to change or add so the integration works well, found while
surveying its services (§2). Each item says what Reviera needs, a suggested shape for the API,
and which Reviera phase (§9) waits on it. *Needed* means the phase cannot ship without it;
*wanted* means Reviera can work around its absence, less well.

### 11.1 REST door (`fileengine_http_proxy`)

1. **Conditional writes** (*needed* for phase 1).
   - Return the file's version as an `ETag` on `GET …/content` and `GET /v1/nodes/{uid}`.
   - Accept `If-Match` on `PUT /v1/files/{uid}/content` and on metadata writes, answering
     `412 Precondition Failed` when the version has moved on.

   This closes the race between Reviera's version check and its upload (§5), and makes the
   Component Library manifest merge safe (§7b).
2. **Check-out locks** (*wanted*, phase 1).
   - `POST /v1/files/{uid}/lock` (owner, time to live, renewable), `DELETE …/lock` to release,
     and the lock's holder and expiry shown in `stat`.
   - A write by anyone else answers `423 Locked`.

   Reviera would take the lock while a model is open for editing and open a locked model
   read-only, as DM-FILE-007 does with WebDAV locks.
3. **Large and resumable uploads** (*needed* for phase 1). Models and publish packages often
   exceed the 100 MB default request cap. Needed:
   - chunked uploads that resume after a dropped connection, such as
     `PUT …/content` with `Content-Range` into an upload session that is committed as one
     version (`POST /v1/files/{uid}/uploads`, `PUT …/uploads/{id}`, `POST …/uploads/{id}/commit`);
   - or a per-route cap high enough for models, with resumable sessions later.
4. **Version details** (*needed* for phase 1, version history).
   - `GET /v1/files/{uid}/versions` returning each version's author, size, content hash and
     comment, not only its timestamp.
   - `PUT …/content` accepting a comment (an `X-Version-Comment` header, or a field on the
     upload session's commit).

   Today a save comment could only be file-level metadata, which the next save overwrites.
5. **Content hash in `stat`** (*wanted*, phases 1 and 4). The SHA-256 the core already keeps,
   so Reviera can check a cached model or library item without downloading it.
6. **Effective permissions for the caller** (*wanted*, phase 1). `GET /v1/nodes/{uid}/access`
   answering, for the signed-in user, `{read, write, delete, share, manage_acl}`. Today the
   permissions endpoint lists ACL entries, which a client would have to evaluate itself (roles,
   claims, DENY precedence), and Reviera must not second-guess the core.
7. **Batch and listed metadata** (*wanted*, phases 2, 4 and 5).
   - `PATCH /v1/nodes/{uid}/metadata` setting several keys in one call (publishing writes about
     eight).
   - A directory listing with chosen metadata keys included
     (`GET /v1/dirs/{uid}?metadata=doc.type,doc.expires`), so the Documents pane and the library
     browser don't make one call per file.
8. **Metadata queries** (*wanted*, phases 5 and 7). Finding nodes by metadata (`product.code =
   X`, `doc.expires < date`, `reviera.source_uid = Y`) within a subtree, either in the REST door
   or through the search service.
9. **Paging and change lists** (*wanted*, phase 4).
   - Paging on directory listings (`?limit=&cursor=`).
   - A subtree change list (`GET /v1/dirs/{uid}/changes?since=<cursor>`) listing what was
     added, changed, moved or deleted, so Reviera can index a large library incrementally
     instead of walking it.
10. **Client-made renditions** (*wanted*, phases 1 and 4). `PUT /v1/files/{uid}/renditions/{name}`
    for a thumbnail or preview Reviera renders (a model's 3D view, a component's preview).
    Today renditions can be listed but only the conversion service makes them.
11. **Folder downloads as zip** (*wanted*, phase 2). `GET /v1/dirs/{uid}/zip` (optionally
    recursive, at the latest versions) for handover packages and for copying a library to a
    local folder. The share service already builds such zips for outside links.
12. **Change events for clients** (*wanted*, phases 1 and 4). A server-sent events or WebSocket
    feed of the core's `fileengine:events` for chosen folders, filtered by what the user can
    read. Reviera could then say *"Someone saved a newer version of this model"* while it is
    open, and offer library updates without polling.
13. **Folder templates** (*wanted*, phases 2 and 5). Instantiating a stored folder tree with its
    metadata keys in one call, for product documentation folders, publish folders and new
    libraries. Otherwise Reviera creates them call by call.
14. **Service discovery** (*wanted*, all phases). `GET /v1/services` listing the base URLs (and
    versions) of the BCF, discussion, search and share services for the tenant, so a Reviera
    connection needs only the REST door's address.

### 11.2 Sign-in and tokens

15. **Native-app sign-in** (*needed* for single sign-on).
    - Today the OAuth flow returns the token in a URL fragment to an allow-listed web address,
      and a desktop app's loopback listener never receives fragments.
    - Needed: OAuth for native apps as RFC 8252 describes. The return address
      `http://127.0.0.1:<any port>/` is allowed, and a one-time authorization code (not the
      token) comes back in the query string. The app exchanges it, with its PKCE verifier, at a
      token endpoint.
    - Or a device-authorization flow (RFC 8628): the user signs in on any browser with a short
      code.
16. **Refresh** (*wanted*). A refresh token (or longer-lived app tokens that can be revoked), so
    Reviera does not ask for the password every hour. *In progress (owner, 2026-10-28):* the
    REST door is getting strong generated credentials like the other doors, which covers this
    (§4).
17. **One token for every service** (*needed* for phases 3 and 6). The BCF, discussion, search
    and share services all accept the REST door's bearer token (or one token exchange gives
    tokens for each), with an audience claim per service. The discussion service already
    accepts bridge tokens; the BCF service uses a shared HS256 secret.

### 11.3 BCF service (`fileengine_bcf_services`)

18. **Persistent store** (*needed* for phase 3): the pending swap from the in-memory store to
    the per-tenant projection over the discussion service, with BCF-XML imports persisted.
19. **BCF-API 3.0** (*wanted*). Reviera's client targets 3.0 with 2.1 as the fallback
    (COL-BCF-002); 3.0 adds topic and comment events, which make incremental sync exact.
20. **Incremental sync** (*needed* for phase 3). OData `$filter` on `modified_date` for topics,
    comments and viewpoints (or events), and server-assigned modification dates on every
    change.
21. **Files and versions** (*wanted*). A topic's *Header/Files* naming the model's FileEngine UID
    and the version it was raised on, so Reviera opens the right version (§6).
22. **Document references** (*wanted*), linking a topic to FileEngine documents (a
    specification sheet, a published report).

### 11.4 Discussion service (`fileengine_threaded_communication`)

23. **A published anchor schema** (*wanted*, phase 6). The `fileengine.anchor.v1`
    `model-viewpoint` shape as a versioned JSON Schema, under a permissive licence, so Reviera
    can validate what it reads and writes (see 11.6).
24. **Threads on a model version** (*wanted*). Anchoring a thread to the version of the model it
    was raised on, with a *"raised on an earlier version"* marker when the model moves on.
25. **Live feed per folder** (*wanted*, phase 6). The `WS /files/{uid}/live` feed for every model
    in a project folder at once, so Reviera does not open one socket per file.

### 11.5 Share service (`fileengine_external_share`)

26. **The public door** (*needed* for in-app share links): link redemption with the recipient's
    one-time code, deployed with the stack.
27. **Links per node** (*wanted*). Listing the links on a file or folder (target, recipient,
    expiry, downloads so far, state) and revoking them, for Reviera's *Shared links* list.
28. **Redemption notices** (*wanted*). Telling the creator when a link is first used, through the
    attention feed (already planned) and an event Reviera can show.

### 11.6 Licensing and contracts

29. **Permissive API descriptions** (*needed* before Reviera's client is published). The OpenAPI
    contract (`openapi.yaml`), the anchor schema and the BCF extensions sit in AGPL repositories.
    Publishing these interface descriptions under MIT or Apache-2.0, as Reviera does for its own
    interfaces, puts beyond doubt that Reviera's MIT client, written against them, is not a
    derivative of the AGPL services.
30. **A versioned contract** (*wanted*). The REST door's version in `/v1/version` or a header,
    and a change log of the contract, so Reviera can tell users when a server is too old for a
    feature.

### 11.7 Headless research API (`fileengine_convert_search_ai`), reviewed 2026-10-29

Reviewed against HEADLESS_RESEARCH_API.md (investigation, branch `research/headless-research-api`,
0db3dd3). The shape fits §7c well: extraction per document rather than RAG, the evidence rule,
version stamps, results that are never written by the service, the cache keyed by version, and
one key and scope shared with the REST door. Points from the desktop side:

31. **Fetch a run after the stream** (*needed* for §7c). A 200-file extraction can outlive the
    window, a laptop's sleep or a network change. Since runs are stored (§3.5), let the client
    read one back: `run_id` in the `plan` and `done` events and on every `item`, and
    `GET /csai/v1/runs/{run_id}` returning the request, the items so far and the state
    (running, done, cancelled). Reviera then resumes the review table instead of paying again
    (cache hits make a rerun cheaper, but not free of requests).
32. **Where the page comes from** (*needed*). `evidence.page` is read from `content_md`; does the
    stored Markdown keep page breaks (for example a marker per page from the PDF extractor)?
    If not, give `page: null` rather than a guess, and add a stable locator (heading path or
    table and row) Reviera can show. Reviera opens the document at the page when there is one.
33. **Units as printed, never converted** (*wanted*). `unit` is the unit the document prints
    (`W/m²K`, `Btu/h·ft²·°F`, `mm`), and `value` its number as printed (decimal comma kept as
    found, or normalised: say which). Reviera converts with its own unit parser and shows the
    original. A `x-unit-kind` hint in the schema (length, thermal transmittance, mass) could
    let the service reject a quote whose unit does not fit.
34. **Dates in ISO 8601** (*wanted*). Certificate and EPD validity as `YYYY-MM-DD` beside the
    quote, so expiry warnings need no parsing of `31.12.2027` or `Dec 2027`. When only a month
    or year is printed, say so (`precision: month`).
35. **Datasheets that cover a range** (*needed*). One datasheet often lists several models or
    sizes. The `instructions` field can name the product, but the answer needs to say when the
    document holds several candidates: `ambiguous: true` with the alternatives (each with its
    own quote), not a pick. Reviera then asks the user which row is theirs.
36. **The schema contract** (*wanted*). Which JSON Schema dialect (2020-12 suggested), which
    keywords are honoured (`type`, `enum`, `format: date`, `description`, nested objects,
    arrays), the largest schema accepted, and how a field's `description` reaches the prompt.
    Reviera's field sets are small and flat, so a documented subset is enough.
37. **A permissive contract for the headless routes** (*needed* before Reviera's client is
    published; as item 29). The OpenAPI description of §3.10, extended to `/csai/v1/*`, under
    MIT or Apache-2.0, including the SSE event schemas.
38. **Token route path** (*check*). The design names `POST /api/v1/auth/token` and
    `/api/v1/auth/refresh` at the bridge with Basic `fesk_…:fesks_…`; this document (§4) assumed
    `/v1/auth/token` at the REST door. Reviera will take the token URL from the connection's
    server (a discovery document or `/v1/version`, item 30) rather than hard-coding either.
39. **Usage per key in the client's terms** (*wanted*). The `usage.message` text is helpful;
    please also keep `state` and the limit names stable (they are what Reviera keys its
    warnings on), and include `resets_at` on the 429 body as on the event.
40. **Cancel means stop** (*wanted*, already a §5 task). Closing the stream stops after the
    current file and the run is stored as `cancelled` with its items, so item 31 can resume it.
41. **Consent stays off for Reviera** (*note*). Reviera will call `extract` and, later, `research`
    with `tools.mcp: "deny"`; it does not need the consent relay (§3.9) for §7c. If a desktop
    research panel is added later, it would show the consent request in a dialog as §3.9
    proposes.

### 11.8 Summary by Reviera phase

| Reviera phase (§9) | Needs | Wants |
|---|---|---|
| 1. Storage | 1, 3, 4, 29 | 2, 5, 6, 10, 12, 14, 16 |
| 2. Publishing | 26 (share links only) | 7, 11, 13, 27, 28 |
| 3. Issues over BCF-API | 17, 18, 20 | 19, 21, 22 |
| 4. Component Libraries | 1 | 5, 7, 9, 10, 12 |
| 5. Documentation folders | — | 7, 8, 13 |
| 6. Discussion service | 17 | 23, 24, 25 |
| Single sign-on | 15 | 16 |
| 5b. Extraction (§7c) | 31, 32, 35, 37 | 33, 34, 36, 38–40 |

## Sources

FileEngine repositories read on 2026-10-27:
- the READMEs of `file_engine_core`, `fileengine_http_proxy` (and its `openapi.yaml` and
  `OAUTH_SETUP.md`), `fileengine_bcf_services`, `fileengine_threaded_communication`,
  `fileengine_folder_actions`, `fileengine_mcp`, `file_engine_python`,
  `fileengine_external_share` and `fileengine_convert_search_ai`;
- `file_engine_frontend/design_documents/XEOKIT_UPGRADE_MARKUP_BCF_PLAN.md` (§4 viewpoint
  anchor, §10 BCF mapping).

Read on 2026-10-29: `fileengine_convert_search_ai/design_documents/HEADLESS_RESEARCH_API.md`
(branch `research/headless-research-api`, 0db3dd3), the owner's own design, read as a
consumer's interface; nothing copied.
