# Chapter 8 — Remote Computation and Analysis (Overview)

**Status:** Draft 0.1 · 2026-10-15 · **Kind:** overview brief. This chapter adds no requirements. It explains how the remote computation subsystem (Chapter 2 §2.17, DM-RPC-001 … 010) and the analysis plugin layer (Chapter 4 §4.0.2a, ANA-PLUG-001 … 009) fit together, and why. The requirements in those sections are normative; where this chapter and they differ, they win. Research and reasoning: `research/07_analysis_engines_remote_compute.md`.

## 8.0 Scope and provenance

This is Reviera's own design, from owner directives given on 2026-10-15. It has no counterpart in the reference product, which links to a proprietary structural solver and vendor cloud services. The only outside inputs are the scopes and licences of open-source engines (sources S0711–S0719) and the names of published standards and libraries.

## 8.1 Goals

1. **Nothing resident when idle.** No analysis engine, or any other heavy calculation, holds memory or CPU when it is not in use.
2. **Run work where it makes sense.** Quick work on the workstation; heavy work on a more powerful machine on the LAN; the most rigorous runs on cloud compute; and, in time, a family of cloud-hosted calculation services.
3. **Any eventual use case.** One foundational subsystem abstracts calculation workloads from where and how they run. Analysis is its first user, not its only one.
4. **Send only what is needed.** A job carries a distillation of the model, never the model.
5. **Future-forward.** Compression and encryption are negotiated, so new techniques arrive without changing the contract.
6. **Contained failures.** A crash or runaway fails its job, never Reviera or the machine.
7. **Disconnect-safe.** A long job outlives its client: a durable job registry on every server and a record of submitted jobs in the model let Reviera reconnect, follow the job from where it left off and collect its results, even after quitting or moving to another machine.
8. **Licence freedom at the boundary.** Runners and hosted services are separate programs behind a published, arm's-length contract, so Reviera can connect to any of them regardless of licence, and their authors can implement the contract without taking on Reviera's licence (DM-RPC-002, TECH_STACK §4.2).

## 8.2 Architecture at a glance

```
 Reviera (workstation)                                     Where jobs run
 ┌──────────────────────────────────────────┐
 │ Live calculations (in-process, ≤ 50 ms   │
 │ per edit): duct sizing, voltage drop,    │
 │ overlap and clearance checks …           │
 │                                          │   local socket   ┌───────────────────────┐
 │ Batch job ─► Distiller ─► Negotiate,     │ ───────────────► │ Runner process        │
 │ (any kind)   (IFC filter  compress,      │                  │ (on demand, OS limits)│
 │              profile +    encrypt        │                  └───────────────────────┘
 │              transforms)     │           │   TLS (LAN)      ┌───────────────────────┐
 │                              └─ blobs ──►│ ───────────────► │ reviera-compute-host  │
 │                                          │                  │ (paired, on demand)   │
 │ Result applier ◄─ results (signed) ◄─────│   TLS (cloud)    ┌───────────────────────┐
 │ (parameters, packages, display styles)   │ ───────────────► │ Cloud cluster: gateway│
 │                                          │                  │ + workers, scale to 0 │
 └──────────────────────────────────────────┘                  └───────────────────────┘
                      contract: reviera.compute.v1 (gRPC; HTTPS/JSON binding)
```

## 8.3 The two layers

| Layer | Where specified | What it does |
|-------|-----------------|--------------|
| **Remote computation subsystem** | Chapter 2 §2.17 | The contract, job kinds, distillations, runner lifecycle, LAN hosts, cloud compute, job sets and the Compute Jobs panel. Knows nothing about analysis. |
| **Analysis plugin layer** | Chapter 4 §4.0.2a | Analysis families and providers; the split between live (in-process) and batch (compute job) calculations; each family's distiller and result handling; the Analysis Providers panel. |

Other users of the subsystem are rendering (Chapter 7), tessellation of large models and links, batch export (PDF sets, IFC, COBie, DWG), point-cloud indexing, project-wide booleans and clash runs, and design-space exploration (§2.15.3). Add-ins can register new job kinds.

## 8.4 A job, end to end

1. **Choose.** The user (or a script, or an MCP agent) starts a batch run: a job kind, a scope, options and a detail level. Its location (local, a host, *Best available*, or cloud) comes from the kind's routing, which can differ per detail level.
2. **Distil.** The kind's distiller filters the IFC model down to what the job needs (DM-RPC-003), applies any transforms, splits the result into deterministic chunks and, for remote destinations, anonymises it. The dialog shows the size and destination, and the first remote job asks for consent.
3. **Negotiate.** Client and server agree on compression, encryption, key encapsulation, signature and hash techniques (DM-RPC-001).
4. **Upload what is missing.** `HasBlobs` reports which chunks the server holds; only the rest are sent, compressed and encrypted.
5. **Run.** The server records the job in its registry, starts a runner on demand under memory and CPU limits, and streams numbered progress events. A cloud cluster sizes a worker to the job. Reviera can disconnect, close or crash; the job continues, and Reviera reattaches later from its own record of the job, resuming events and downloads where they stopped (DM-RPC-009).
6. **Return.** Signed results come back, keyed to the distillation; the result applier attaches them to the right elements, as parameters, result packages or display styles, with their provenance.
7. **Idle.** The runner exits after its idle timeout; a cloud cluster scales back to zero.

## 8.4a Real-time sessions

Some simulations are better explored than run: a heavy simulation runs on a cloud cluster or LAN host while Reviera, a game engine or a VR headset shows it live, and the user changes scenario data and sees the result respond at once (DM-RPC-010).

```
 Reviera (controller)          ── scenario inputs, model patches ──►   Cloud / LAN host
 viewport + scenario panel     ◄── keyframes + delta frames ───────    simulation session
        │  Render Scene (simulation layer)                              (workers reserved
        ▼                                                                while open)
 RSP field messages ──► Godot translation layer ──► desktop or VR (OpenXR)
                         (or an engine bridge attached directly as an observer)
```

- Inputs are reliable and ordered; frames are latest-wins, progressively refined and adapted to the viewer's bandwidth and frame rate, over a negotiated real-time transport (QUIC datagrams or WebTransport where available, otherwise the gRPC stream).
- Several viewers can watch one session; one controls it.
- A session survives disconnects, suspends to a checkpoint when nobody is attached, and can be recorded and committed as a result package.
- Targets: input-to-visible change within 150 ms on a LAN host and 300 ms on a cloud host in the same region, at 30 frames per second for the reference session.

## 8.5 Analysis families

| Family | Typical batch run | Distillation (mostly an IFC filter) | Suggested engines (§4.15) |
|--------|-------------------|--------------------------------------|---------------------------|
| Structural | Linear static, modal, non-linear | IFC4 Structural Analysis View | Built-in frame solver (proposed); PyNite or Frame3DD; CalculiX; Code_Aster; Kratos |
| Seismic | Response spectrum, time history, pushover | Structural view plus masses | xara; Code_Aster |
| Energy and thermal loads | Annual simulation, heating and cooling loads | Thermal zones, space boundaries, constructions, glazing, gains | EnergyPlus |
| Daylight, sun and shadow | Daylight factor, annual metrics, shadow studies | Simplified surfaces with optical properties, glazing, sensor grids, site | Radiance; pvlib |
| Plumbing and hydraulic | Pressure and flow | Pipe network graph | EPANET |
| Stormwater | Runoff and drainage | Catchments, gutters, downspouts, drainage runs | EPA SWMM |
| Interior airflow | Ventilation, comfort | Watertight interior fluid domain | OpenFOAM |
| External wind | Wind flow and facade pressures | Simplified envelope with site context | OpenFOAM |
| Clash | Project-wide clash test | Bodies of chosen categories | Built-in overlap engine |

Live calculations that update while the user models stay in Reviera's process (ANA-PLUG-008).

## 8.6 Negotiation, compression and encryption

- **Negotiated, never downgraded.** A registry of technique identifiers that only grows; `Negotiate` picks the strongest option both sides allow, honouring minimums and organisation policy; each blob records what produced it.
- **Compression.** zstd with trained per-profile dictionaries, content-defined chunking, deltas against what the server holds, mesh codecs and byte shuffling; format-aware compressors to evaluate.
- **Encryption.** TLS 1.3 with hybrid post-quantum key exchange preferred; end-to-end encryption of blobs and results with per-job keys, so relays and storage cannot read them; signed result manifests.

## 8.7 Suggested build order

| Step | Delivers | Requirements |
|------|----------|--------------|
| 1 | The contract with local runners: `Negotiate`, blobs, jobs, results; the job registry and reconnection; runner lifecycle and OS limits; the Compute Jobs panel | DM-RPC-001, 004, 008, 009 |
| 2 | Job kinds and filter-profile distillations; the structural profile | DM-RPC-002, 003; ANA-PLUG-009 |
| 3 | The analysis layer with live and batch providers; the built-in batch runner and a built-in frame solver | ANA-PLUG-004 … 008 |
| 4 | LAN compute hosts with pairing, reattachment and *Best available* | DM-RPC-005 |
| 5 | Cloud clusters: scale to zero, estimates, budgets, end-to-end encryption; the hosted-services catalogue | DM-RPC-006 |
| 6 | Job sets and studies; further engine runners and job kinds (rendering, export, tessellation) | DM-RPC-007; ANA-PLUG-001 families |
| 7 | Real-time sessions: the session stream, latest-wins frames, the real-time transport, the scenario panel, the simulation layer in RSP and the Godot/VR layer | DM-RPC-010; ADV-RSP-001; ADV-XR-001 |

Structural analysis needs the analytical model first (STR-AM-001 … 004, STR-LOAD-001, STR-LC-001, STR-BC-001), which follows the structural physical elements in the structure build order.

## 8.8 Requirement map

| Requirement | Topic | Section |
|-------------|-------|---------|
| DM-RPC-001 | Contract and protocol, negotiation, compression, encryption, provenance | §2.17.1 |
| DM-RPC-002 | Job kinds and their extension | §2.17.2 |
| DM-RPC-003 | Model distillations (IFC filter profiles and transforms) | §2.17.3 |
| DM-RPC-004 | Runner lifecycle and isolation | §2.17.4 |
| DM-RPC-005 | Compute hosts on the LAN | §2.17.5 |
| DM-RPC-006 | Cloud compute and hosted services | §2.17.6 |
| DM-RPC-007 | Job sets and studies | §2.17.7 |
| DM-RPC-008 | Compute Jobs panel | §2.17.8 |
| DM-RPC-009 | Job registry and reconnection | §2.17.9 |
| DM-RPC-010 | Real-time simulation sessions | §2.17.10 |
| ANA-PLUG-001 … 003 | Provider interfaces, in-process and out-of-process providers, built-in providers | §4.0.2 |
| ANA-PLUG-004 | Analysis transports | §4.0.2a |
| ANA-PLUG-005 | Provider lifecycle | §4.0.2a |
| ANA-PLUG-006 | Analysis results | §4.0.2a |
| ANA-PLUG-007 | Analysis Providers panel | §4.0.2a |
| ANA-PLUG-008 | Live and batch calculations | §4.0.2a |
| ANA-PLUG-009 | Analysis distillers | §4.0.2a |
| ANA-FEA-001 | Structural analysis provider interface | §4.3.11 |

## 8.9 Open questions

See `research/07_analysis_engines_remote_compute.md` §13: the built-in frame solver, feature requirements for the seismic, stormwater, airflow and wind families, library licence checks, who operates the first hosted services, and the training corpus for compression dictionaries.
