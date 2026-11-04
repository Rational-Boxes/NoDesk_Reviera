# Vertical and people transport: elevators, lifts, escalators, moving walks, stairlifts, people movers, funiculars and ropeways

**Status:** Draft 0.1 · 2026-11-03 · for the owner's review; nothing built.
**Owner's requests:**
- 2026-11-01: "Elevators, both passenger and freight." and "For elevators include ornamental
  version like glass enclosed."
- 2026-11-03, while the second research round ran: "Escalators should be included in elevator
  type equipment research", "Include moving walks in airports and transit stations too",
  "Include people movers and stairlifts too", "Include funiculars and inclined elevators too",
  "Include cable cars and gondolas too".

FileEngine is optional (standing rule). Every tool here works on a local project. Maker data
held in FileEngine is a later, optional workflow, as for bathrooms and kitchens.

**Research:** `research/16_elevators.md` uses public sources only, about 350 of them, logged in
`RESEARCH_LOG.md` and `SOURCE_REGISTER.md`. Most come from two rounds: S1505–S1684 (2026-11-01)
and the resume round's S2383–S2538 (2026-11-03); a few earlier ones are reused.
- Parts A–N cover elevators and lifts. Their checkable rules are numbered R1–R200 (Part K), their
  conflicts M1–M47 (Part M), and Part L proposes the packs, components, clear zones and
  generators this plan builds on.
- Part O holds the resume round's elevator findings.
- Part P covers escalators and moving walks, including airports and transit.
- Part Q covers stairlifts, inclined platform lifts, automated people movers (APMs), inclined
  elevators and funiculars.
- Part R covers cable cars, gondolas and aerial ropeways.

Values are marked [V] verified, [S] secondary, [U] unverified, [mfr] maker data or
[judgement] proposed defaults.

**Advisory rules.** The governing standards are paywalled. They were read only through state and
city adoptions, inspection checklists, regulators' guides, makers' planning guides and free
previews. Rules taken from them are *advisory* until someone checks them against the text. The
standards are:
- ASME A17.1/CSA B44, A18.1 and B20.1;
- EN 81-20/-50/-70/-72/-40 and EN 115-1;
- ISO 8100 and ISO 4190;
- ANSI B77.1, the EN 12929 family, ASCE 21 and NFPA 130.

---

## 1. What the toolset is for

**Moving people and goods between levels, along a building, and across a site.** That splits
into four families:

| Family | Devices | Where it lives |
|---|---|---|
| **Shaft devices** | Passenger, hospital, service, freight and vehicle elevators; LULA and residence elevators; vertical platform lifts; dumbwaiters, material lifts and VRCs; panoramic and glass-shaft lifts | In a hoistway through the floors |
| **Floor-to-floor conveyors** | Escalators; horizontal and inclined moving walks, including airport and transit units | In a floor opening (wellway) or along a floor |
| **Stair devices** | Seated stairlifts; inclined platform (wheelchair) stairlifts | On a stair flight |
| **Guideway and rope systems** | Automated people movers; inclined elevators; funiculars; aerial ropeways (gondolas, 3S, reversible trams, funitels, chairlifts, briefly); street cable cars (briefly) | Along a route across a site or between buildings |

What the tools do:
- **Size from capacity:** for example "give me a 3500 lb MRL car with a front entrance at every
  level" or "a 1000 kg stretcher lift to EN 81-70 type 3". The hoistway, pit, overhead, machine
  or control room, entrances and hall fixtures follow from the car.
- **Check as you draw:** the device carries its clear zones (car, pit refuge, landing, escalator
  run-off, stair width beside a folded stairlift, ropeway envelope). Rule packs check them as for
  bathrooms.
- **Hand over to other trades:** structural reactions, power, heat, sump and drains, fire-alarm
  recall, shunt trip and sprinkler interlocks are carried on ports and in property sets for the
  structural, MEP and energy tools.
- **Plan the traffic:** the elevator up-peak calculation, escalator and walk unit counts from
  flows, APM and ropeway line capacity, and egress credit by code.

**Out of scope, at least at first:**
- machine design: rope, sheave and safety-gear selection, controller logic and dispatching
  algorithms;
- stamped structural design of hoistways, glass shafts, APM guideways, funicular viaducts and
  ropeway towers. Reviera carries maker-style reactions and flags; an engineer designs.
- time-step traffic simulation inside the editor. If built, it runs as a compute job (§9).
- ropeway rope dynamics, wind-tunnel and evacuation-team planning beyond the rough checks in §8;
- APM vehicle and train control engineering, and rail operations;
- maintenance and inspection records (LOLER, A17.1 periodic tests). These are a later COBie and
  digital-twin link, not modelling.

## 2. What Reviera already has

| Need | Existing tool | Status |
|---|---|---|
| A void through every floor between two levels, kept up to date | Shaft opening (ARC-OPEN-004) | Built |
| Stairs completed to the level above, with openings and headroom checks | ARC-STAIR-007 | Built |
| Rule packs with jurisdiction, edition, source and confidence | Bathroom rule packs (ARC-BATH-001), `schemas/bath/`; a general `reviera.rules-1` is proposed in KITCHENS_PLAN §3.1 | Built; generalisation awaits the kitchens answer |
| Clear zones as virtual volumes, checked while drawing | ARC-MOD-030/031, fixture spaces (ARC-BATH-002) | Built |
| Generated placeholder components with IFC classes and ports | `bath_components` (ARC-BATH-004), connectors (MEP-CONN-001) | Built |
| Products from a mesh; swap keeping GlobalId; documents on the type | DM-IMP-015, ARC-BATH-006, COL-FE-006/007 | Built |
| Railings and guards | ARC-RAIL-* | Built |
| Curtain walls, for glass shafts | ARC-CW-* | Built |
| Electrical equipment, circuits, sprinklers, ducts | MEP-ELEC-*, MEP-PIPE-007, MEP-DUCT-* | Built |
| Path of travel | ANA-ROUTE-001/002 | Built (one path) |
| Heavy analysis out of process | Compute jobs (DM-RPC-001…, ANA-PLUG-003/008) | Built |
| Repeated units with shared service spines | Modular units (ARC-GRP-004) | Specified, not built |
| Site and terrain | Toposolids, property lines, site components (ARC-SITE-001) | Built |
| **Alignments (IfcAlignment), guideways, rope lines** | — | **Not in Reviera.** Roads (research/17) need them too |

Each device produces walls, slabs, openings, spaces, doors, railings, equipment and ports that
the other tools already understand.

## 3. Rule packs

### 3.1 Schema

Use the same schema as kitchens: one general `reviera.rules-1`, or the bath schema extended,
depending on the owner's answer to KITCHENS_PLAN Question 1. Vertical transport needs these rule
kinds beyond the bathroom four:

| Rule kind | Examples (research/16) |
|---|---|
| **Tables with interpolation** | A17.1 maximum inside area per rated load (R4, A3); EN 81-20 car area per load, + 0.16 m² per 100 kg (D2); pit and overhead by speed and drive (A8, D4) |
| **Geometric fit tests** | Stretcher 610 × 2134 mm with 127 mm corner radii, tried straight and diagonally through the door (Z2, R20); 1525 mm turning circle (Z3) |
| **Swept envelopes** | Escalator headroom along the step line (Z30); stairlift running envelope (Q5); ropeway cabin envelope swung by α′ (R3) |
| **Along-path checks** | Ropeway clearance to ground, roads and buildings along every span for each sag case; APM guideway envelope |
| **Counts from flows** | Escalators = peak-minute flow ÷ 100 with one unit out (P4); elevator handling capacity (I1) |
| **Edition flags** | A17.1 edition (overhead increments, residence 15/18 ft², dumbwaiter 9/10.75 ft²); EN 81-70 2003/2021; IBC 2021+ text/video communication |
| **Product values** | Pit, overhead, reactions, heat and power: read from the chosen product; otherwise the pack's default, marked as such |

### 3.2 Packs to ship first (proposed)

| Pack | Layer | Covers | Confidence |
|---|---|---|---|
| `us_ibc_ada` | minimum + accessible | ADA §206.6/206.7, §407–§410; IBC Ch. 30 (CBC text) | ADA [V]; IBC [S] |
| `us_a17_checklists` | minimum | A17.1 through state checklists: area table, pits, refuge, machine rooms, escalator Part 6 | **advisory** |
| `us_a18` | minimum | Platform lifts and stairlifts (A18.1 §2–§4; IBC 1011.2 ex. 3) | **advisory** |
| `en81` | minimum + accessible | EN 81-20/-70/-72 and EN 115-1 values through makers and previews | **advisory** |
| `england_adm_adb_adk` | minimum + accessible | Lift triggers, car and landing sizes, firefighting shafts, refuges, glazing | [V] |
| `practice_elevators` | recommended | Maker envelopes, lobby depths, traffic targets, panoramic allowances | notes only |
| `transit_escalators` | recommended | Transit duty, run-off, unit counts, NFPA 130 egress mode (P3.6) | [S] |
| `fr_strmtg_rm2` | minimum | Ropeway envelope, safety distances, overflight heights, fire distances, rescue times (R2.2) | [V] |

Later, as advisory packs: `us_ca`, `us_wa`, the local and owner packs (`us_phoenix`, `us_sf`,
`owner_va`, `owner_dod`), `scotland_tech_handbook`, `germany_mbo`, `france_erp_2017`,
`australia_ncc_e3`, `canada_nbc` and `hk_bd_lifts`. They are listed in research/16 L1 and O.
The US ropeway pack (B77.1 as restated by Colorado and Vermont) follows. Which packs ship first
is Question 2.

### 3.3 Why per-jurisdiction packs are necessary

| Topic | Values found |
|---|---|
| Escalator headroom | 2130 mm (US) · 2300 mm (EN, HK, TfL) · 3000 mm (TfL atrium) · 2100 mm (JP) |
| Escalator clear landing area | (CL distance + 200) × 2 × CL (US) · (outer + 160) × 2500 (EN) · CL × 2500 (HK) |
| Inclination | ≤ 30° (US) · 35° allowed at rise ≤ 6 m and ≤ 0.5 m/s (EN, JP) |
| Stretcher | 24 × 84 in at ≥ 4 stories (IBC) · 76 in at 3+ stories (FL) · 2010 × 610 (CA) · 600 × 2000 (AU) · 1100 × 2100 car (DE) |
| Lift required | All storeys of new non-dwellings (England) · > 13 m (DE) · ≥ 50 public on other floors (FR) |
| Stair beside a folded stairlift | ≥ 508 mm clear (US R-3, R-2 units) · full required width (other US) · none in an exit enclosure (NFPA 101) · carriage parked clear (England) |
| Wheelchair stairlift platform | 800 × 1250 (England) against a maker's "ADA" 800 × 1220 (N2) |
| Ropeway swing angle | 0.3 rad ≈ 17° (FR) · 15° (US) · 20° (DE 2003) · 10° (PA) |
| Partially enclosed well | 3.5 m landing side / 2.5 m elsewhere / 1.1 m at ≥ 2 m from moving parts (EN, [S]) · 8 ft where reach ≤ 4 ft (CA) |

A finding names the rule, its pack, edition and confidence, as bathroom findings do. A
`recommended` pack or an `advisory` rule gives a **note**, never an error. Where two readings
were found, both are shown until an expert decides. Judgement defaults go to `EXPERT_REVIEW.md`
from E-83. These include the research/16 L5 list and the conflicts flagged across research/16
(71 **Conflict** marks, including the resume round's).

## 4. Elevators and shaft devices

### 4.1 The elevator object

`IfcTransportElement` ELEVATOR, with `Pset_TransportElementCommon`,
`Pset_TransportElementElevator` and a Reviera set (`Reviera_Pset_Elevator`, published MIT). The
Reviera set holds:
- drive type: MRL traction, traction with a machine room, hydraulic holed, holeless, roped
  hydraulic;
- rated load and speed, and capacity in persons;
- clear car W × D × H;
- door type, width and height;
- entrances per level;
- loading class (A, B, C1–C3) for freight;
- flags for firefighting, FSAE/OEE, stretcher and panoramic use;
- edition flags.

The car, hoistway space, pit, overhead zone and machine or control room form one assembly (an
`IfcElementAssembly` aggregate). Moving the elevator moves them all, and editing levels re-runs
the hoistway.

The hoistway reuses the **shaft opening** (ARC-OPEN-004), so it cuts every slab it passes and
any slab added later. Shaft walls are ordinary walls, with Function Core-Shaft and a rating
from the pack.

### 4.2 Generators

| Command | What it makes | Research |
|---|---|---|
| `elevator.from_capacity` | Passenger car from load, car type (ADA, side door, stretcher, hospital, EN 81-70 type 1–5), speed, drive, entrances and door. Makes the hoistway, pit (ladder, sump), overhead and hoist beam, machine or control room, entrances with frames and sills, hall fixtures at code heights, landing zones, and FSAE/OEE or firefighting lobbies if flagged | L4 G-1, A1–A8, D1–D7, O |
| `elevator.freight` | Freight, service, hospital and vehicle cars: class, door type (bi-parting, vertical, pass-type), truck approach zone, jack hole for in-ground hydraulics, pump room | L4 G-2, B, O |
| `lift.small` | LULA, residence elevator, vertical platform lift, dumbwaiter, material lift, VRC | C, L2 |
| `elevator.panoramic` | Glass car (rear, rear + sides, 270°) and glass shaft as a curtain wall: posts, transoms at floors and bracket spacing, laminated panels, counterweight screen, observation landing; the partially enclosed well check | F, L4 G-4, O (Pilkington, IGV, SCI P103) |
| `elevator.bank` | 2–8 cars in line or facing, pitched by hoistway width + divider beam, with a shared lobby and machine or control room; on ARC-GRP-004 when built | L4 |

Car sizes come from the pack's tables, choosing the nearest maker-style car (A2, D2, Hong Kong
table O). Hoistway = car + side and front allowances by door type. Pit and overhead come from
the table for drive and speed, or the chosen product (A8, D4, O). Every generator takes typed
values and a sketched or picked location, and reports what fell short.

### 4.3 Clear zones

Clear zones Z1–Z28 from research/16 L3 are virtual volumes on the assembly:
- the car and the stretcher test;
- the pit, the pit refuge (US 610 × 1219 × 610) and the EN refuge volumes;
- the overhead;
- the counterweight guard;
- the pit's wet zone and ladder;
- the landing call floor space, the UK 1500 × 1500 landing and the button reach bands;
- lobby depth, and the FSAE and OEE lobbies;
- machine-room working space;
- the freight door operating band and truck approach;
- the platform-lift runway;
- the VRC guards;
- the dumbwaiter overhead.

The hoistway envelope (Z4) reports anything foreign inside it: ducts, pipes and conduit not
serving the elevator (R25; ADB 17.9).

### 4.4 Building interfaces

- **Structure:** rail bracket reactions and spacing, pit loads (buffer, car and counterweight
  impact), hoist-beam load and machine loads. They come from the product or the pack's [mfr]
  defaults (H1, O: SCI P103, Hydro-Lift, Canton), carried as point loads for the structural
  tools.
- **Electrical:** mainline disconnect, car-light and pit circuits, GFCI and sump receptacles,
  shunt trip, and standby power for FSAE/OEE cars (H3, R43–R47, R75–R77). Per-product kVA, FLA
  and heat are still a research gap (O4), so defaults are [U].
- **Mechanical and plumbing:** machine-room temperature band (13–32 °C US; packs vary), heat
  output to the energy model, sump 3000 gal/h, oil interceptor outside the pit for hydraulics,
  and hoistway venting where a pack wants it.
- **Fire:** shaft rating, entrance rating, recall inputs from the fire alarm, sprinkler and
  shunt-trip interlock, firefighting and evacuation lift lobbies, and UK refuges (E, R36–R42,
  R144).

### 4.5 IFC choices to settle (M43)

- Platform lifts: USERDEFINED "PLATFORMLIFT" or LIFTINGGEAR.
- VRCs: LIFTINGGEAR.
- Stairlifts: USERDEFINED "STAIRLIFT".
- Home lifts: ELEVATOR with ObjectType "ResidenceElevator".

These choices are proposed in research/16 L2 and Q2. Question 4.

## 5. Stairlifts and inclined platform lifts

`stairlift.place` and `platform_stairlift.place` pick a stair (ARC-STAIR-*) and side, and follow
the flight. Straight rails ride the run; curved rails follow landings and turns. Parameters come
from research/16 Q5:
- **Rail:** side, intrusion (160 mm), end option (none, hinge, slide track, park round the corner).
- **Seated unit:** folded and unfolded depth (320 / 530–659 mm), seat width and swivel.
- **Inclined platform lift:** platform size (800 × 1250 … 675 × 1000), load (250–300 kg), speed
  (≤ 0.15 m/s), mount (direct or tower), and the maker's head-box and run-out tables.
- **Wall loads:** F1 1.1–1.25 kN, F2 2.4–2.9 kN, M ≤ 3.6 kN·m. They are flagged on the wall's
  build-up.

**Envelopes:** the parked box, the running swept volume, the code head box (1524 mm US /
1500 mm Canada), the bottom run-out, and the wheelchair spaces at the landings.

**Stair checks** (Z26a–h) are added to the stair's own checks:
- the width left beside the folded unit, by occupancy and jurisdiction;
- exit-enclosure prohibitions;
- headroom 2032 mm above the nosings;
- landing run-out;
- inside-bend width on curved rails;
- platform size by jurisdiction.

## 6. Escalators and moving walks

`escalator.place` and `moving_walk.place` (G-3, research/16 P4) take two levels, or a start
point and a length for walks.

**Parameters:**

| Parameter | Values |
|---|---|
| Inclination | 27.3, 30 or 35°, limited by pack |
| Step or pallet width | 600, 800 or 1000 mm (walks 800–1600 mm) |
| Speed | 0.5 m/s by default; up to 0.75 m/s by pack and context |
| Flat steps | 2, 3 or 4, by rise and speed |
| Transition radii | by pack and context |
| Duty class | commercial, heavy or transit; sets truss depth, pit depth, step load and deflection limit |
| Balustrade | glass or metal; 900, 1000 or 1100 mm |
| Context | building, airport or transit |
| Outdoor flag | adds a cover, drains, heaters, IP rating and a forced intermediate support |

**Geometry:**
- The truss length at 30° is 1.732 H + 4700 mm with two flat steps; the 3- and 4-flat constants
  are in P4.
- The truss is the step width + 530 mm wide (+ 630 mm for transit); the pit is 60–100 mm wider.
- The pit is 1150 mm deep (1220 mm transit, US).
- Intermediate supports go in beyond about 16.4 m, or from the maker's table.
- Support reactions use R ≈ k·L + c and go to the structure.
- The floor void is an opening sized to the wellway.

**Clear zones and checks:**
- the clear landing area (by pack) and headroom along the step line;
- side clearance;
- spacing between adjacent units;
- an automatic intersection guard where the handrail passes close to a slab edge or ceiling;
- the deck barricade (US) and landing void barrier;
- lighting levels (50 lx on the step band and landings);
- US wellway protection: opening ≤ 2 × projected area and ≤ 4 storeys unless B or M, or a draft
  curtain ≥ 457 mm with sprinklers, or a 1½ h shutter, else a §713 shaft.

**Arrangements** (parallel, crisscross and scissor) are placed as pairs that share the void and
run-off zones, on ARC-GRP-004 when built.

**Airport and transit context (P3.6):**
- a station run-off zone, 6 m by default and 8–12 m to gatelines;
- a walk-series checker that keeps each walk's safety zone and an entry queue of 4.6–9.1 m;
- unit counts from the peak-minute flow, assuming one unit is out, with a stair beside every
  bank of two or more;
- egress mode: IBC (escalators count 0) or NFPA 130/SCDF (up to 50 %, worst unit out).

Accelerating walks are a separate product type, outside the generic walk rules.

## 7. Guideways: people movers, inclined elevators and funiculars

These run along a route, not through a floor, so they need an **alignment**. Reviera has none
yet. Research/17 (roads, drives and lots) needs the same thing. Proposal: one shared foundation,
specified once and used by both:
- `IfcAlignment` with horizontal, vertical and cant layouts;
- chainage stationing;
- sketched or typed points along the route;
- elements placed by chainage.

Question 5.

On that foundation:
- **Inclined elevator** (`inclined_elevator.place`): an ELEVATOR with ObjectType
  "InclinedElevator" on a straight track up a slope. It covers angle (< 70°), track width
  (914 / 1219 mm), car (3 × 3 to 5 × 8 ft), wall height, gates, drive type and landing
  platforms, and the A17.1 §5.1 (commercial) or §5.4 (residential) pack checks (Q4 C2).
- **Funicular** (`funicular.place`): a two-car, three- or four-rail or Abt passing-loop line.
  Classes are IfcRailway "FUNICULAR", IfcRail, an Abt turnout assembly, IfcVehicle ×2, and the
  drive as a HAULINGGEAR transport element at the top station. It carries gauge, grade, level
  or stepped car floor, and stepped or level platforms. Reference installations are in Q4 C3.
- **Automated people mover** (`apm.guideway`, `apm.station`): a single- or dual-lane guideway.
  - Parameters: track separation 4.6–4.9 m (6.7 m at crossovers), right-of-way, a 12 ft dynamic
    width, curves ≥ 300 ft (150 ft absolute) and grades ≤ 6 % (0 % in stations). These come from
    ACRP Report 37 (Q3 B2).
  - Elevated spans of 15–37 m. Vehicles about 12.2 × 2.7–3.0 m; consists up to 4.
  - Stations with side, centre or triple platforms and platform screen doors.
  - Checks: NFPA 130 platform clearing in ≤ 4 min and to a point of safety in ≤ 6 min, and line
    capacity.

ASCE 21's own numbers were not reachable, so APM rules are advisory throughout.

## 8. Aerial ropeways

`ropeway.line` lays out a gondola, 3S, tram or funitel line between stations on an alignment
over the terrain (research/16 R3):
- **System type** sets the ropes per side, the default gauge (MGD 6.4 m [S]), cabin box and
  spacing.
- **Towers:**
  - placement and heights, with pile-cap or pad foundations;
  - checks on centripetal acceleration (≤ 2.5 m/s²), slope change (≤ 0.15 rad) and deviation
    (≤ 10 % of vertical load);
  - Pinellas defaults: 18.3 m high, 152 m spacing.
- **Stations:** terminal, return, intermediate and angle stations, with boarding level and
  passage widths. The cabin garage is 16.7 m² per cabin [S], and stations are kept 8 m from
  buildings [V, FR].
- **Rope curve per span:** a parabola with point loads, with cases for loaded-low, empty-high,
  stopped-empty and ice. The formulas are the research agent's own mechanics, so they are marked
  for expert review before use.

**Checks:** the jurisdiction's clearance envelope, checked along every span and sag case:
- ground and obstacles: 4 m, or 2.5 m on spans < 200 m, or 1.5 m fenced (FR);
- roads and other lines: 1.5 m;
- the US restatement: 5 ft lateral at 15° swing, and 8 ft over pedestrian areas;
- overflight height limits by system class: 15/25 m, 60 m or none;
- fire distances over buildings: 20 m below the rope, 16 m below the cabin;
- the road-class table;
- evacuation feasibility: ≤ 3 h 30, with recovery in 1 h 30 (FR) and a 30-minute decision time
  (AT);
- a privacy list of residential windows overflown or passed close;
- line capacity, as 3600 × cabin capacity ÷ headway, or the tram round-trip formula.

**IFC [judgement]:**
- the line as `IfcFacility` "Ropeway" with an `IfcBuiltSystem` TRANSPORT;
- towers as `IfcElementAssembly` PYLON;
- cabins as `IfcVehicle` USERDEFINED;
- ropes as `IfcMember`;
- the alignment as `IfcAlignment`.

IFC 4.3 has no ropeway entity (R2.5).

**Compute jobs.** Clearance along every span over a large terrain, overflight inventories and
evacuation estimates run as compute jobs (advanced-features rule). Moving one tower re-checks
only its two spans, in process.

Street cable cars (grip cars and an underground cable slot) are left out. They are recorded in
R2 for completeness only.

## 9. Traffic, capacity and egress

- **Elevator up-peak** (`elevator.traffic`): round-trip time, interval and handling capacity per
  5 minutes, against the building-type targets in research/16 I1. Examples: office intervals of
  25–30 s at 12–15 %; apartments 50–80 s at 5–8 %; hospitals < 35 s at 15 % plus migration.
  It covers persons per car by mass and by area (I2) and groups needing ≥ 120 % when they
  overlap. It is a light calculation, in process. The RTT formula's coefficients are [S] until a
  primary source is read (O4).
- **Simulation** of lunchtime and two-way traffic and destination dispatch, if wanted, is a
  compute job for a later provider. It is not built in. Question 7.
- **Escalators and walks:** practical capacity by width and speed (EN table), transit unit
  counts and one-out redundancy (§6).
- **APMs and ropeways:** pphpd from headway, consist and cabin size; fleet = peak + standby +
  20 % spares (APM).
- **Egress:** evacuation and firefighting lifts and OEE cars count where a pack says so.
  Escalators count 0 under the IBC and up to 50 % under NFPA 130 or SCDF. The path of travel
  (ANA-ROUTE) learns to pass through lifts and escalators in the egress mode the pack sets.

## 10. Schedules, sheets and exports

- **Elevator schedule:** car, load, speed, drive, stops, entrances, door, hoistway, pit,
  overhead, machine room, rating, power and heat (product or default, marked).
- **Hoistway section and plan sheets:** pit, overhead, car and hoistway dimensions, and the
  entrance elevation, dimensioned automatically. These are the drawings a maker asks for.
- **Escalator and walk schedule:** rise, width, inclination, speed, flat steps, duty, supports
  and reactions.
- **Compliance report** (`vt.check`): HTML and BCF, as `bath.check`, listing each finding's
  rule, pack, edition and confidence.
- **COBie:** elevators, escalators, walks, platform lifts and stairlifts are managed assets
  (Components and Types). Machine-room equipment goes in Components, and the Connection sheet is
  filled from the ports.
- **Products:** a maker's model or mesh is swapped in keeping the GlobalId (ARC-BATH-006 pattern).
  Its pit, overhead, reactions and heat then override the pack defaults, and the checks re-run.

## 11. Build order (proposed)

1. Rule kinds (tables with interpolation, fit tests, swept envelopes, counts, edition flags) in
   the shared rules schema. First packs: `us_ibc_ada`, `us_a17_checklists`, `en81` and
   `england_adm_adb_adk`. Then `vt.check`.
2. The elevator assembly, `Reviera_Pset_Elevator`, and `elevator.from_capacity` on the shaft
   opening, with clear zones Z1–Z22 and hall fixtures (§4.1–4.3).
3. Freight, service, hospital and vehicle elevators; LULA, residence, platform lifts,
   dumbwaiters and VRCs (§4.2).
4. Building interfaces: reactions, power, heat, sump, fire interlocks; elevator schedule and
   hoistway sheets (§4.4, §10).
5. Escalators and moving walks with their zones, wellway checks and schedule (§6).
6. Stairlifts and inclined platform lifts with the stair checks (§5).
7. Panoramic cars and glass shafts (§4.2). Banks and escalator pairs move onto ARC-GRP-004
   when it is built.
8. Elevator up-peak, escalator and walk capacity, and the egress modes (§9). Then the airport
   and transit context and `transit_escalators` (§6).
9. The shared alignment foundation, with roads (Question 5). Then inclined elevators and
   funiculars (§7).
10. Automated people movers (§7).
11. Aerial ropeways, with `fr_strmtg_rm2` first and the span checks as compute jobs (§8).

Each step gets its spec section first (a new §3.x in chapter 03, IDs `ARC-VT-001` on), its
samples and a help walkthrough. Every tool is a registered command (`elevator.*`, `lift.*`,
`escalator.*`, `moving_walk.*`, `stairlift.*`, `apm.*`, `funicular.*`, `ropeway.*`, `vt.*`),
exposed to Lua and MCP. Every tool that takes points, paths or outlines takes sketched or typed
points (standing rules): hoistway locations, wellways, stair picks, alignments and tower
positions. The README gains the goals, marked as not yet built.

## 12. Questions for the owner

1. **Scope of the first build:** shaft devices, escalators and stairlifts (steps 1–8) first, with
   guideways and ropeways (steps 9–11) after? Or bring any of the latter forward?
2. **Packs and jurisdiction:** ship the US (ADA and IBC, A17.1 through checklists) and EU/England
   packs first? Or add Hong Kong (its public lift table is the best-sourced metric table found)
   or another jurisdiction you work in?
3. **Advisory A17.1, A18.1 and EN 81 rules:** they are known only through adoptions, checklists
   and makers. Ship them as advisory notes, which is proposed, or leave them out until someone
   checks the standards?
4. **IFC classes for the gaps:** platform lifts as USERDEFINED "PLATFORMLIFT", VRCs as
   LIFTINGGEAR, stairlifts as USERDEFINED "STAIRLIFT", ropeways as `IfcFacility` with
   `IfcElementAssembly` PYLON towers and `IfcVehicle` cabins. Accept these, or prefer proxies?
5. **Alignments:** build one shared `IfcAlignment` foundation for roads (research/17), APMs,
   funiculars and ropeways before any of them? Or give ropeways a simpler station-to-station
   polyline first?
6. **Maker data:** keep the built-in defaults generic ("maker-style") with sources? Or also ship
   named maker catalogues (KONE, Otis, Schindler, TKE, Garaventa, Doppelmayr) where their public
   planning guides allow? They change, and their terms vary.
7. **Traffic simulation:** is the up-peak calculation enough? Or should a simulation compute-job
   provider (lunchtime, two-way, destination dispatch) be planned now?
8. **Glass shafts:** the structural guidance is thin (post sizes, fixings at ≤ 1500 mm, pit
   loads from one quotation; SCI P103 for rail brackets). Ship the panoramic generator with these
   as flagged defaults, or wait for more research?
