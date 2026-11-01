# Bathrooms, toilet rooms, locker rooms and spas

**Status:** Draft 0.2 · 2026-10-31 · building started at the owner's request ("focus on implementing
bathroom related features"), on the defaults proposed in §11 until the owner says otherwise.
Built so far (2026-10-31): rule packs and fixture clearance zones (ARC-BATH-001/002), grab bars by
rule (ARC-BATH-003), the generic component catalogue (ARC-BATH-004) and layout templates
(ARC-BATH-005), products and their documents (ARC-BATH-006), fixture counts (ARC-BATH-007),
toilet cores (ARC-BATH-008), locker rooms (ARC-BATH-009), and step 9's wet areas: electrical
zones (ARC-BATH-010), shower falls (ARC-BATH-011) and waterproofing extents (ARC-BATH-012), and
the first of §7: hot tubs (ARC-BATH-013). Next: the rest of §7 (whirlpool baths, saunas, steam
rooms), then schedules and elevations (§9).
**Owner's requests (2026-10-31):**
- "deeper research into bathroom features, standards, and common components";
- "review clearance rules, accessibility rules, rails, etc.";
- "common residential bathroom as well as bathroom like facilities like locker rooms";
- "include hot-tub, sauna, and similar spa facilities under the umbrella of bathroom type features";
- "identify common equipment for bathrooms and include common equipment placeholder items and
  support for specific realized components with provided models and documents".

FileEngine is optional (owner, 2026-10-31). Every tool here works on a local project. Product
reference data held in FileEngine is a later, optional workflow.
**Research:** `research/13_bathrooms.md` uses public sources only. It covers 312 sources,
S0869–S1180, all logged in `RESEARCH_LOG.md` and `SOURCE_REGISTER.md`. Rules are numbered
G1–G150 in its Part G. Paywalled standards (ICC A117.1, BS 8300, ISO 21542, EN and DIN
standards, NEC) are known only from free texts and secondary summaries. Rules taken from them
are marked *advisory* until someone checks them against the standard itself.

---

## 1. What the toolset is for

**Rooms where people wash, use the toilet, change and bathe.** In practice:

- **Homes:** full and three-quarter baths, powder rooms, en-suites, Jack-and-Jill baths, wet
  rooms, and accessible (wheelchair) bathrooms.
- **Public and workplace toilets:** single-user and all-gender rooms, multi-stall toilet cores
  with urinals, accessible and ambulant compartments, Changing Places toilets.
- **Locker rooms and changing facilities:** gyms, pools, schools, sports pavilions, workplaces
  (end-of-trip) and healthcare staff areas. They have benches, lockers, gang and individual
  showers, and accessible changing rooms.
- **Spas:** hot tubs, whirlpool baths, saunas, steam rooms, cold plunges and thermal suites.

In the research, no mainstream BIM tool has a complete bathroom toolset (research 13, *Software
landscape*). The pieces are scattered:

| Tool | What it has |
|---|---|
| Vectorworks | Parametric toilet stalls |
| Archicad | Accessible WC parts (users report the grab bars are placed wrongly) |
| Chief Architect | Residential vanities and dimensions |
| Solibri | After-the-fact clearance checks |
| Manufacturer planners | Installation walls only |

Clearance zones are usually drawn by hand into each user's own content. Tile set-out is weak
everywhere.

Reviera's opening is to **treat the rules as data** and use them while drawing:

- fixtures that carry their own clearance zones, taken from a chosen rule pack;
- grab bars placed by rule;
- rooms laid out and checked as you go.

## 2. What Reviera already has

| Need | Existing tool | Status |
|---|---|---|
| Fixtures with connectors, hosted on walls | Plumbing fixtures (MEP-PIPE-006) | Built 2026-10-21 |
| Fixture units, pipe sizing | MEP-PIPE-005 | Built (user tables) |
| Sanitary and water systems, layout | MEP-SYS-002, MEP-DUCT-008 | Built |
| Rails | Railings (3.7.7, 3.7.8b) | Built |
| Overlap, clash and clearance conflicts | Volume overlaps (3.12.2a), clash tests | Built |
| Reusable types and stamps | Component Libraries (2.8.16, 3.4.4) | Built |
| Imported product geometry | Custom objects from meshes (2.11.12) | Built 2026-10-14 |
| Rooms, room boundaries, spaces | 3.9.2, MEP-SPACE-001 | Built |
| Walls by room | 3.3.19 | Built |
| Product documents on a material, local or FileEngine | COL-FE-006, -007, -010 | Built 2026-10-31 |
| Schedules and COBie | Schedules, DOC-OUT-015 | Built |
| Path of travel | ANA-ROUTE-001/002 | Built |

The new work builds on these. Each new tool produces walls, fixtures, railings, rooms and
components that every other tool already understands, as the greenhouse enclosure does.

## 3. Rule packs

Every dimension in research 13 Part G belongs to one jurisdiction and edition. Reviera will ship
rules as **rule packs**, not as constants in code.

- **What a rule pack is.** A pack is a versioned JSON file, published MIT like the other
  schemas. Each rule carries:
  - an ID and a description;
  - the edition it comes from and its source ID;
  - a confidence level: *verified*, *secondary* or *advisory*;
  - the value, as a dimension, count, range or formula of other values.

  A project chooses one or more active packs. Users can copy a pack and change values, for a
  local amendment or an office standard.
- **Packs to ship first (proposed):**
  - **US residential:** IRC/IPC minimums plus NKBA recommendations as a "comfort" layer.
  - **US accessible:** 2010 ADA Standards plus ICC A117.1, with A117.1 marked advisory where it
    is only known from secondary sources.
  - **England:**
    - Approved Document M, volumes 1 and 2;
    - Approved Document T (2024): toilet types A–D;
    - Approved Documents F and G: ventilation and hot water;
    - BS 7671 Section 701 bathroom zones.
  - **Canada:** NBC 2020 / BCBC accessibility.
  - **Germany:** DIN 18040, advisory.
- **What the checks report.** Each finding names the rule, its value, the measured value and
  the pack's edition. A rule marked *advisory* reports a note, never an error. Values the
  research found in conflict (research 13, *Gaps*) are shown with both readings until an expert
  decides. Example: the UK ambulant cubicle, 800 mm wide before 2024 against 920 × 1465 mm now.
- Defaults that come from judgement go into `EXPERT_REVIEW.md`, per the standing rule. These
  include placeholder sizes, locker aisle widths and gang-shower spacing.

## 4. Fixtures carry their clearance zones

- Every sanitary type carries **named zones** in its own local frame:
  - front use;
  - side transfer;
  - knee and toe;
  - door exclusion;
  - turning space;
  - the bather's or heater's safety distance (spas).

  Their sizes come from the active rule pack. Some products override a size; for example, a
  sauna heater's safety distances come from its manual.
- **How zones behave.** Zones are shown in plan and 3D as hatched outlines, with their own
  visibility switch. They are not selectable by default and never drive geometry, avoiding the
  problems users report with clearance zones in Revit. They are stored as data and exported as a
  Reviera property set, plus an `IfcVirtualElement` when the owner wants them in IFC (§11).
- **Checking while drawing.** The checks reuse the overlap detector:
  - a zone against walls, other fixtures and door swings;
  - a zone against another zone, where the pack forbids sharing.

  A failing placement shows the zone in red with the rule's name as you place. The same checks
  run as a report (`bath.check`).

## 5. Grab bars and other rails

- **Grab bars are parametric railings** (`IfcRailing`, HANDRAIL, as NBS maps them):
  - straight, L-shaped, U-shaped, vertical, drop-down (folding) and swing-up bars;
  - diameter, wall gap, height and length are checked properties.
- **`bath.place_grab_bars`** places the bars a fixture needs under the active pack. Examples:
  - ADA water closet: a 42 in side bar and a 36 in rear bar, 33–36 in high, starting at set
    distances from the rear wall;
  - AD M: a drop-down rail on the transfer side;
  - showers and tubs: the bars their rules require.

  It also places the **backing (blocking) in the wall** as a hidden wall layer region. Users
  move a bar with grips, and the check reports any rule it then breaks.
- **Other rails and accessories:**
  - towel rails, heated towel rails, shower curtain rails and fold-down shower seats;
  - alarm pull cords, which must reach the floor in AD M rooms;
  - mirrors, dispensers, hand dryers and baby-change units.

  Each has its mounting height and reach range from the pack.

## 6. Room tools

### 6.1 Bathroom layout (`bath.layout`)
- **Templates.** You draw or pick a room and choose a template. Each template comes in left- and
  right-handed versions:
  - residential: full bath 5 × 8 ft, three-quarter, powder 3 × 6 / 4 × 5, Jack-and-Jill, UK
    family bath, en-suite, cloakroom, wet room;
  - accessible: ADA single-user, AD M unisex, AD T types A–D, Changing Places.
- **Fitting the room.** The tool fits the fixtures along the walls you choose (the **wet wall**).
  It keeps centreline spacing and front clearances to the pack, and offers back-to-back pairing
  with a neighbouring room. When the room is too small, it reports the shortfall in each
  direction.
- **Living layout.** The result is a group of ordinary fixtures, rails and partitions that
  remembers its template. Changing the room or the pack re-runs the fit, keeping anything the
  user has moved.

### 6.2 Toilet cores and partitions (`bath.toilet_core`)
- **Laying out a run.** Along a wall, the tool works out the stall count and widths from the run
  length and the fixture count needed (§6.4). It places standard, accessible and ambulant
  compartments, urinals with screens, and lavatory counters.
- **Partitions** are a new lightweight element, exported as `IfcWall` with PredefinedType
  `PARTITIONING` (or `IfcBuildingElementProxy`, see §11). They come in floor-mounted,
  ceiling-hung, floor-to-ceiling and overhead-braced types. Each has a door with swing, handing
  and in- or out-opening, checked against the compartment's zones.
- **Changing layouts.** Partition systems are types, so a manufacturer's system can replace the
  generic one (§8).

### 6.3 Locker rooms (`bath.locker_room`)
- **What it lays out:**
  - banks of lockers in tiers, with sizes as types;
  - benches with aisle widths;
  - gang showers or individual cubicles;
  - drying areas, toilets and an accessible changing room with an accessible bench (ADA:
    42 × 20–24 in, 17–19 in high, against a wall).
- **Sizing.** Locker counts come from users per shift, or from a given count. Aisle and bench
  clearances come from the pack. Lockers have no IFC class, so they are Reviera furniture types
  with a mapped IFC fallback (§11).

### 6.4 Fixture counts (`bath.fixture_count`)
- **Methods.** Required WCs, urinals, lavatories, showers and drinking fountains are calculated
  from occupant load and occupancy, by IPC Table 403.1 / IBC 2902.1, NBC 3.7.2.2, OSHA 1910.141,
  or HSE L24 / INDG293. The method follows the pack and has per-jurisdiction override tables.
  The IPC 2021 all-gender and single-user rules are included.
- **Comparison.** The result is compared with what is modelled per room, level or building, with
  the shortfall listed. It feeds §6.2's run layout.

### 6.5 Wet areas
- **Showers and wet rooms:**
  - floor falls to the drain, drawn as a sloped finish layer, with the fall checked against
    the pack;
  - drain capacity against shower flow;
  - level-access thresholds.
- **Waterproofing.** Extents are marked on walls and floors, with exposure classes as data (DIN
  18534 classes where the pack has them). They appear in elevations and a quantity schedule.
- **Electrical zones.** BS 7671 / IEC 60364-7-701 zones 0, 1 and 2 are drawn as volumes from the
  bath and shower positions, and NEC rules where the pack is American. The checks are:
  - the IP rating of a fixture or device in a zone;
  - socket distances, flagged *conflicting* where sources disagree (2.5 m against 3 m);
  - GFCI/RCD needs.
- **Ventilation.** The extract rate required by the pack (for example AD F intermittent l/s, IRC
  50/20 cfm) is set on the room's space, ready for HVAC sizing.

### 6.6 Tile set-out (P2)
- Tile layouts on walls and floors start from a datum the user chooses: centred, or a full tile
  at a corner or edge. Cuts are shown in elevation and plan.
- A schedule counts full and cut tiles and waste. Tile and grout are types with the material's
  documents (COL-FE-006).

## 7. Spas

- **Hot tubs and spas:**
  - a type with filled weight;
  - pad and floor load, checked against the structure where a slab exists;
  - service and access clearances, cover and barrier rules;
  - NEC 680 Part IV electrical distances (advisory, since the sources conflict);
  - the deck and steps.

  Indoor halls get ventilation and humidity notes.
- **Whirlpool baths:** a bath type with its access panel and motor location kept clear.
- **Saunas:**
  - a room tool that sizes the heater from the room volume and the product's sizing table;
  - benches in tiers;
  - the heater's safety distances, taken from the product as §4 zones;
  - vent positions by heater type (the pattern is stored per product, because makers disagree);
  - the door opening outward with no lock;
  - IRC M1902 rules.
- **Steam rooms and steam showers:**
  - a room tool that sizes the generator from volume and wall material factors (per product);
  - the ceiling sloped to shed condensate, checked against the pack;
  - steam head and sensor positions;
  - a vapour-proof assembly as a wall type.
- **Cold plunges, foot spas and thermal suites** are components with zones and documents, with
  no special tool at first.
- **Accessibility:**
  - ADA §242 transfer walls (16–19 in high) and transfer systems;
  - the ADA §1009 means of entry, required for spas per the rules;
  - AD M guidance for UK spas.

## 8. Placeholder equipment and specific products

The owner asked for both, so every item in research 13's equipment catalogue (Part E3) ships
twice over.

- **Generic placeholders** go in a *Bathrooms* component library: WC, urinal, lavatory, vanity,
  bath, shower tray, shower, bidet, sink, mirror, grab bar, accessories, partition systems,
  lockers, benches, hand dryer, baby change, water heater, hot tub, sauna heater, steam
  generator and more. Each one has:
  - nominal sizes at schematic detail;
  - connectors with fixture units;
  - mounting height;
  - clearance zones from the pack;
  - handing;
  - the IFC class and PredefinedType for both IFC4 and IFC4.3, mapped on export (for example
    WCSEAT, which IFC4.3 deprecates, and the BathType conflict, §11);
  - Uniclass and MasterFormat codes.
- **Specific products** are component types made from a manufacturer's data. Each is a local
  folder holding:
  - **Geometry:** an IFC type, or a mesh imported with 2.11.12 from OBJ/STL/glTF. A DWG import
    comes later. RFA and GSM cannot be read.
  - **Properties:** `Pset_ManufacturerTypeInformation`, `Pset_Warranty`, `Pset_ServiceLife` and
    the type's own psets. Property names are the same as the generic type's, so schedules don't
    break (the NBS convention).
  - **Documents:** data sheet, installation guide, O&M, warranty, EPD/HPD, certificates. They use
    the same document link as material documents (COL-FE-006): a local folder by default, typed
    by kind. Values can be entered by hand or, when FileEngine is connected, read from the
    documents.
- **Swapping generic for specific (`component.swap_type`).** The swap is an IFC type swap. It
  keeps each occurrence's GlobalId, tag and instance data, and reports:
  - changed dimensions;
  - connector offsets that moved;
  - rule-pack results that changed (a larger pan projection now breaks a clearance, for
    example).

  Candidates are matched by classification and PredefinedType.
- **FileEngine (optional, later).** Publishing a product folder uses COL-FE-010 as it stands. A
  shared product library across projects can come later: types and documents in FileEngine,
  values read by extraction. No step requires it.

## 9. Schedules, sheets and exports

- **Schedules:**
  - a fixture schedule with DFU/WSFU totals per room, group and stack;
  - a fixture count compared with the requirement (§6.4);
  - lockers, partitions and accessories;
  - a waterproofing area schedule.
- **Wall-elevation sheets:** one action makes an elevation of each wall of a bathroom, with tile,
  rails and mounting heights dimensioned.
- **Compliance report** (`bath.check`) per room. It lists each rule, its pack and edition, the
  value against what was measured, and its confidence. It writes HTML and BCF topics, so
  findings can travel to FileEngine as issues when it is connected.
- **COBie:** the fixture and product fields map from the same data.

## 10. Build order (proposed)

1. Rule-pack format and the first packs: US residential, US accessible and England, with
   `bath.check`.
2. Clearance zones on sanitary types, with live overlap checking while placing.
3. The generic *Bathrooms* component library: fixtures, accessories, both IFC versions.
4. Grab bars and rails by rule, with wall backing.
5. Bathroom layout templates and the wet-wall fit (§6.1).
6. Product types with documents and the generic-to-specific swap (§8).
7. Toilet cores and partitions (§6.2), and fixture counts (§6.4).
8. Locker rooms (§6.3).
9. Wet areas: falls, waterproofing, electrical zones (§6.5).
10. Spas: hot tubs, saunas, steam rooms (§7).
11. Schedules, elevation sheets and tile set-out (§9, §6.6).

Each step gets its spec section first, its samples and a help walkthrough. Every tool is a
registered command, exposed to Lua and MCP, and takes sketched or typed points (standing rules).

## 11. Questions for the owner

1. **Which rule packs** ship by default, and which jurisdiction is the default for a new
   project: the US, England, or chosen from the project location?
2. **Build order:** residential bathrooms and accessibility first (§10)? Or should toilet cores
   and locker rooms come earlier for commercial work, as commercial greenhouses did?
3. **Clearance zones in IFC:** export them as `IfcVirtualElement`s other tools can see, or only
   as Reviera property data?
4. **IFC classes Reviera must invent:** lockers, benches, partitions, towel rails and sauna
   heaters have no good IFC class. Reviera-defined types in its own namespace, with an IFC
   fallback (proxy or furniture), as agreed for greenhouses (2026-10-28)?
5. ~~The IFC4 BathType conflict.~~ *Settled 2026-10-31* from buildingSMART's official pages
   (S1181): IFC4 ADD2 TC1 and IFC 4.3 both list DOMESTIC, FOOT, POOL, PLUNGE, SITZ, SPA
   ("Whirlpool") and TREATMENT; neither has WHIRLPOOL or JACUZZI. WCSEAT is deprecated in both.
   No per-schema mapping is needed: a hot tub or whirlpool bath is BATH with BathType SPA.
6. **Spa depth:** stop at hot tubs, saunas and steam rooms? Or include pools in the bath context
   (plunge pools and hydrotherapy pools, with their barriers and entrapment rules)?
7. **Manufacturer content:** should Reviera ship any real product examples? (Licensing usually
   forbids redistribution, so the proposal is placeholders plus an import path.)
