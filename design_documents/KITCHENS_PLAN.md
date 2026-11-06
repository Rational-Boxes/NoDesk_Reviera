# Kitchens, foodservice and retail refrigeration

**Status:** Draft 0.2 · 2026-11-06 · four of the owner's answers in §11a; nothing built.
**Owner's request (2026-10-31):** "launch a research session on kitchens. include residential and
commercial, including grocery store refrigeration hardware."

**See also:** `INTERIORS_PLAN.md` (2026-11-06), for furniture, retail fixtures, checkouts and
gyms. It shares this plan's rules schema (§3.1) and its cabinet runs and countertops (§4).

FileEngine is optional (standing rule). Every tool here works on a local project. Product
reference data held in FileEngine is a later, optional workflow, as for bathrooms.
**Research:** `research/14_kitchens_refrigeration.md` uses public sources only. It covers 172
sources, S1182–S1353, logged in `RESEARCH_LOG.md` and `SOURCE_REGISTER.md`. Its checkable rules
are numbered F1–F240 in Part F. Values are marked [V] verified, [S] secondary or [U] unverified.
Many key texts are paywalled or refused automated fetches: the ICC codes, NFPA 70 (NEC), NFPA 96,
ASHRAE 15 and 62.2, DW/172, EN 378 and the full F-gas Annex IV. Rules taken from them are
*advisory* until someone checks them against the text.

---

## 1. What the toolset is for

**Rooms where food is stored, prepared and cooked, and the shop floor where chilled food is
sold.** In practice:

- **Home kitchens (US and UK/EU):** one-wall, galley, L, U, island and peninsula kitchens,
  kitchen-diners, and accessible kitchens (ADA §804, ICC A117.1 Type A/B, England M4(2)/M4(3),
  DIN 18040-2).
- **Commercial and foodservice kitchens:** restaurants, cafés, hotel and institutional kitchens.
  Cook lines under hoods, prep areas, ware-washing, walk-ins, dry stores, hand and mop sinks.
- **Retail refrigeration:** supermarket and convenience-store display cases in line-ups, walk-in
  coolers and freezers, and the plant behind them: racks, condensing units, condensers and gas
  coolers, machinery rooms, and the refrigerant piping between.

**Out of scope, at least at first:**
- hospital (FGI) and school (USDA) kitchen rules, which were not researched (research/14, *Gaps*);
- food-processing plants, cold-storage warehouses and ammonia systems;
- cabinet-shop production (cut lists, CNC nesting), pricing and quoting;
- beverage and beer dispensing systems, and food trucks;
- Canada's NBC and CSA B651 kitchen rules, and Ontario food rules (not reached).

**The opening.** In the research, kitchen software splits three ways (research/14 E5):

| Tool | What it has |
|---|---|
| Chief Architect, Vectorworks | Good cabinet runs; Vectorworks 2025 cuts countertop holes automatically |
| Revit, Archicad | Casework families or library parts; no countertop generator |
| KCL, AutoQuotes, Specifi | Foodservice blocks and utility schedules, proprietary, tied to quotes |
| Hussmann, Hillphoenix | Case data sheets; some Revit and DWG; no IFC |

Missing everywhere: an open, IFC-native foodservice library with ports; a vendor-neutral
equipment-to-rough-in workflow; and display-case line-ups that sum refrigeration load per
circuit or rack (research/14 E5). Riviera can do all three, on the bathroom pattern: **rules as
data, placeholders with ports, layouts checked as you draw, products swapped in later.**

## 2. What Riviera already has

| Need | Existing tool | Status |
|---|---|---|
| Rule packs with jurisdiction, edition, source, confidence | Bathroom rule packs (ARC-BATH-001), `schemas/bath/` | Built 2026-10-31 |
| Clearance zones as virtual volumes, checked while drawing | Fixture spaces (ARC-BATH-002), ARC-MOD-031, overlaps ARC-MOD-030 | Built |
| Generated placeholder components with IFC classes and connectors | `bath_components` (ARC-BATH-004), custom objects (DM-IMP-015), connectors (MEP-CONN-001) | Built |
| Layout templates fitted to a room, with a shortfall report | `bath.layout` (ARC-BATH-005) | Built |
| Runs laid out along a wall from counts | Toilet cores (ARC-BATH-008) | Built |
| Counts from occupant load | `bath.fixture_count` (ARC-BATH-007) | Built |
| Product from a mesh; swap keeping GlobalId; documents on the type | ARC-BATH-006, COL-FE-006/007 | Built |
| Plumbing fixtures, sloped pipes, sizing | MEP-PIPE-002, -005, -006, MEP-CALC-001 | Built |
| Ducts, systems, auto-routing | MEP-DUCT-001 … 008, MEP-SYS-002 | Built |
| Electrical equipment, circuits, panel schedules | MEP-ELEC-001, -002, -013 | Built |
| Sprinklers | MEP-PIPE-007 | Built |
| Spaces and the energy model | MEP-SPACE-001, ANA-EM-003 | Built |
| Path of travel | ANA-ROUTE-001/002 | Built |
| Riviera property sets, published MIT | `schemas/psets/riviera-psets-1.json` | Built |
| Interior elevations, schedules, COBie | 5.4, schedules, DOC-OUT-015 | Built |
| Repeated units with connection points and shared spines | Modular units (ARC-GRP-004) | Specified, not built |

Each new tool produces walls, furniture, appliances, sanitary terminals, ducts, pipes and
spaces that every other tool already understands.

## 3. Rule packs

### 3.1 One rules schema, not two (proposal)

The bath schema (`riviera.bath-rules-1`) has zones, distances, bars and counts, keyed to six
sanitary fixture kinds. Kitchens need those four kinds of rule and several new ones
(research/14 E7.1):

| New rule kind | Examples |
|---|---|
| **Run lengths along a counter** | Landing areas: 24 in and 18 in beside a sink, 12 in and 15 in beside a cooktop, longer + 12 in where they combine (A3, F18–F26) |
| **Group geometry** | Work triangle: legs 4–9 ft, sum ≤ 26 ft, ≤ 12 in through an island (A1, F5–F8) |
| **Path distance** | Hand sink within 25 ft with no door between (Maricopa) or 15 ft in the same room (CUPHD); Class K extinguisher ≤ 30 ft travel; pull station 10–20 ft (B2, B9, F104, F179–F180) |
| **Coverage** | Hood overhang ≥ 6 in past every cooking surface below (B6, F150) |
| **Tables and formulas** | Exhaust cfm per foot by hood style and duty class (B6); DOE MDEC from TDA (C4); interceptor size (B10) |
| **Line-up sums** | Case lengths plus ends; load per rack or circuit (C12, F218) |
| **Dated rules** | GWP limit by equipment type and install date (C8, F224–F231) |
| **Product values** | Hood height, OTR microwave clearances, oven cut-outs, rear air gaps, A2L room area: read from the product, else the pack default (E7.5) |

**Proposal: generalise to `schemas/rules/riviera.rules-1.schema.json` (MIT).** Each pack names
its domain (`bath`, `kitchen`, `foodservice`, `refrigeration`). Element kinds become an open
list mapped from IFC class and predefined type, not a fixed enum. Existing bath packs stay valid
and are read unchanged; the `bath.*` commands keep working. Reasons:
- one loader, one check engine, one settings page and one way to add a project pack;
- the overlap between domains is real: a kitchen sink, a hand sink in a toilet room, a mop sink,
  an accessible clear floor space (ADA 305.3) and NKBA guidance appear in both;
- the new kinds (path distance, dated rules, product values) will be wanted by bathrooms too
  (a hand-dryer reach range, a sauna heater's distances from its product).

The alternative, a separate `riviera.kitchen-rules-1`, is quicker to start but doubles the code
that reads, merges and reports packs. Question 1 (§11).

### 3.2 Packs to ship first (proposed)

| Pack | Layer | Covers | Confidence |
|---|---|---|---|
| `us_nkba_kitchen` | recommended | Triangle, aisles, landings, seating, storage (A1–A3) | verified (2015-based PDF) |
| `us_irc_kitchen` | minimum | 30 in above the cooktop, hood ducting, makeup air, smoke alarms (A5–A9) | secondary (adoptions) |
| `us_nec_kitchen` | minimum | Counter receptacles, islands by edition, GFCI, circuits (A8) | **advisory**: NEC not read |
| `ada_2010_kitchen`, `a117_2017_kitchen` | accessible | ADA 804; A117.1 Type A/B (A10) | verified; secondary |
| `england_kitchen` | minimum + accessible | AD M M4(2)/M4(3), AD F, AD G, AD B (A13) | verified |
| `us_foodcode_2022` + local overlays | minimum | Food Code; Maricopa, CUPHD, SNHD numbers (B2–B4, B17) | verified |
| `us_imc_nfpa96` | minimum | Hoods, exhaust tables, grease ducts, suppression (B6–B9) | IMC [V], NFPA 96 [S] |
| `gb_852_2004`, `uk_dw172` | minimum; recommended | Hygiene layout; canopy and extract practice (B15) | verified; advisory |
| `us_refrigeration`, `eu_gb_refrigeration` | minimum | DOE and ecodesign energy, GWP dates, charge and leak rules (C4–C10) | dated; EU secondary |

German and Austrian practice (120 cm fronts, frontage minimums), Australia (AS 4674, AS 1668.2)
and British Columbia follow as advisory packs. Which ship first is Question 2.

### 3.3 Why per-jurisdiction packs are necessary

The FDA Food Code sets outcomes ("convenient", "adequate"). Health departments set the numbers,
and they disagree (research/14 B17 and *Gaps*):

| Topic | Values found |
|---|---|
| Hand sink to workstation | ≤ 25 ft, no door between (Maricopa); ≤ 15 ft, same room (CUPHD); "convenient" (Food Code) |
| Splash guard trigger | 24 in (Maricopa); 12 in (CUPHD) |
| 3-comp sink drainboards | one each end; ≥ largest compartment; ≥ 36 in; ≥ 18 in |
| Canopy overhang | 6 in (IMC); 250 mm, 600 mm at combis (DW/172 summaries); 150 mm (AU) |
| Grease filter height | 2 ft / 3.5 ft (IMC) against 18 in / 4 ft (NFPA 96 summary) |
| Island receptacles | required (NEC ≤ 2020) against optional with provision (NEC 2023) |
| Makeup air trigger | > 400 cfm with a fuel appliance (IRC 2018+) against any > 400 cfm hood (IRC 2015) |
| Accessible U-kitchen | 60 in (ADA) against 40 in with an island (A117.1-2017) |
| US refrigerant GWP | 150/300 from 2026–27 (2023 rule) against an interim 1,400 until 2032 (2026 revision) |

A finding names the rule, its pack, edition and confidence, as bathroom findings do. A
`recommended` pack or an `advisory` rule gives a **note**, never an error. Where the research
found two readings, both are shown until an expert decides. Defaults from judgement (toe-kick
size, commercial aisles, walk-in doors, placeholder sizes marked [U] in D5) go to
`EXPERT_REVIEW.md` (Q14), from E-68 on.

## 4. Cabinets and counters

### 4.1 Cabinet runs (`cabinet.run`)
- **Drawing a run.** Pick a wall face, or sketch a path in plan, or type coordinates (standing
  rule). Fill it with base, wall or tall units. Widths snap to the module: 3 in steps from 9 to
  48 in (US), or 300–1200 mm in 50 mm steps on 600 mm modules (EU, [U]) (research/14 A4).
- **Unit types:** door, drawer, sink and cooktop bases; blind, lazy-susan and diagonal corners;
  wall units 12 in deep (24 in over a refrigerator); tall pantry and oven towers 84/90/96 in;
  end panels and appliance openings.
- **Heights.** US base 34½ in plus a 1½ in top gives about 36 in; EU worktop 850, 900 or 950 mm
  (EN 1116, [S]). Toe kick about 4 × 3 in ([U], to `EXPERT_REVIEW.md`). Wall units 15–18 in
  above the counter (US) or 50–65 cm (DE/AT); the sources conflict (*Gaps* 16).
- **Fit.** The run fills from one end. The remainder becomes **fillers**, at the ends or at a
  corner where a door would hit the return. Where two runs meet, a corner unit is made. A run
  that cannot fit reports the shortfall, as bathroom layouts do. Grips change widths.
- **Appliances in cabinets.** A wall oven, microwave or built-in refrigerator in a tall or wall
  unit is checked against the product's cut-out table. A misfit is a finding, never a silent
  resize (research/14 E6.8).

### 4.2 Countertops and backsplashes (`countertop.from_runs`)
- **One top per run** (or across joined runs), from the base units' footprints. It has an
  overhang (1 in front by default), thickness (1¼–1½ in), joints placed by the user or at
  corners, edge profile, and waterfall ends.
- **Islands and peninsulas** get seating overhangs from the pack (12/15/18 in at 42/36/30 in
  counter height, NKBA KPG 9) and the 9 in behind a flush cooktop (A3).
- **Backsplashes** follow the top: 4 in, or full height to the wall units. They stop at
  fillers, tall units and windows, which other tools get wrong (E5).
- **Cut-outs are automatic.** A sink, cooktop or pop-up outlet hosted in the top cuts it from
  the type's cut-out data. Swapping the product re-cuts it and reports the change.

### 4.3 IFC mapping

IFC has no countertop or backsplash enumeration, no kitchen-cabinet value, and TOPPING in IFC4.3
is a levelling layer, not a countertop (research/14 D1, D3). The proposal follows Uniclass's
published mappings:

| Element | IFC (IFC4 and IFC4.3) | Notes |
|---|---|---|
| Base, wall, tall unit | `IfcFurniture` USERDEFINED ("BaseCabinet", "WallCabinet", "TallCabinet"), IsBuiltIn TRUE | Linked to its wall by `IfcRelConnectsElements` (D1) |
| Carcass, door, drawer front | `IfcSystemFurnitureElement` nested in the unit | Only at detailed level; off by default |
| Countertop | `IfcSystemFurnitureElement` WORKSURFACE | Uniclass Pr_40_30_30_46; fallbacks `IfcFurniture` USERDEFINED or `IfcCovering` USERDEFINED (Question 4) |
| Backsplash | `IfcCovering` USERDEFINED "Splashback" | Uniclass Pr_40_30_30_45 |
| Island | An `IfcElementAssembly` of its units and top | |

The IFC4.3 PSD lists panel values for the work surface's SupportType, apparently a defect; the
writer leaves that property out in IFC4.3 (D2, to `EXPERT_REVIEW.md`).

## 5. Appliances and equipment

### 5.1 Generic placeholders
A *Kitchens* and a *Foodservice* component library, generated as `bath_components` are, from the
catalogue in research/14 D5. Each placeholder carries what D6 lists:
- body box, door swing, service and cleaning clearances as zones, handing;
- cut-out data for built-ins, and line-up end widths for cases;
- ports at the element (never in nested parts);
- the IFC class and predefined type, Uniclass (with table version), MasterFormat and ETIM codes.

| Residential | Foodservice |
|---|---|
| Ranges 20–48 in (electric, gas, dual fuel), cooktops, wall ovens, OTR and drawer microwaves, dishwasher 24/18 in, refrigerators (French door, side-by-side, top-freezer, column), disposer, sinks (single, double, prep), faucets, range hoods | Ranges, fryers, griddles, charbroilers, convection and combi ovens, tilting kettles and braising pans, reach-ins, prep tables, work and dish tables, 3-comp, hand, prep and mop sinks, floor sinks, door-type dish machine with booster, ice machine on bin, shelving, grease interceptor, hoods, Class K extinguisher |

**IFC gaps and proposed workarounds** (D1, E7.4; each to `EXPERT_REVIEW.md`):
- **Gas cookers have no class in IFC4 or IFC4.3.** Proposal: `IfcElectricAppliance`
  USERDEFINED "GasRange" (or "GasCooktop") with a Gas port and `Riviera_Utilities` fuel GAS.
  The alternative is ELECTRICCOOKER with a gas port, which misleads any tool reading the enum.
- **Hoods have no class.** Proposal: `IfcAirTerminal` USERDEFINED "KitchenHood", as BDNS maps
  them.
- **Sinks have only a drainage port in IFC** (water comes through a nested `IfcValve` FAUCET).
  Riviera's sinks keep their cold and hot connectors (MEP-PIPE-006) and export a nested faucet
  carrying those ports.

### 5.2 A utility property set (`Riviera_Utilities`)
`Pset_ElectricApplianceTypeCommon` holds only Reference and Status: no voltage, power, gas,
water, drain or exhaust (research/14 D2). Without that data there is no rough-in schedule. The
proposal adds one type pset to `schemas/psets/riviera-psets-1.json` (MIT), from agent D's field
set (D6, E2, E3):

| Group | Properties |
|---|---|
| Electrical | Voltage, Phase, Frequency, Current (A), Power (kW), PlugType (NEMA/IEC) or HardWired, DedicatedCircuit, MCA, MOPD |
| Gas | FuelType (natural, LP), InputRating (Btu/h or kW), ConnectionSize, InletPressure |
| Water | ColdWater, HotWater, TreatedWater; sizes; flow; hot-water demand (gph) |
| Drain | Size, DrainType (direct, indirect, to grease interceptor), AirGap |
| Exhaust | HoodType (I, II, none), DutyClass, ExhaustRate, MakeupAirRate, HeatGain (sensible, latent) |
| Refrigeration | System (self-contained, remote, glycol), Load, EvaporatorTemperature, Refrigerant, Charge, LineSizes |
| Listing and handling | Listings (NSF, UL, ETL), Weight, TransportOpening |

Each value lives on the type; ports carry the same data where the MEP tools need it, so a
dishwasher's hot-water connector and its pset agree. Display cases add `Riviera_DisplayCase`
(§8).

### 5.3 Products and documents
As ARC-BATH-006, generalised so kitchens do not need their own copies of the commands:
- `component.make_product` (with `bath.make_product` kept as an alias): a manufacturer's mesh
  as a product of a generic's kind, with its ports and `Pset_ManufacturerTypeInformation`;
- `component.swap_product`: swap keeping GlobalId, reporting size changes, moved ports,
  cut-out misfits and new findings. A swapped range that needs 50 A where the circuit has 40 A
  is a finding;
- documents on the type (COL-FE-006): spec sheet, installation guide, cut-out guide, O&M,
  warranty, NSF/UL listings, EPD and HPD. Cabinet and quartz countertop EPDs exist (E4).

Riviera ships placeholders only. Manufacturers publish Revit and DWG (Hillphoenix) or data
sheets (Hussmann), not IFC (E3); import stays through meshes and IFC types.

## 6. Layout tools

### 6.1 Residential kitchen layouts (`kitchen.layout`)
- **Templates:** one-wall, galley, L, U, island, peninsula, in US and EU versions, each handed.
  Accessible versions: ADA 804, A117.1 Type A and B, England M4(2) and M4(3)(2)(a)/(b).
- **Fitting the room.** Pick a room. The tool places cabinet runs, sink, cooktop or range,
  refrigerator and dishwasher on the walls the template names. It places the three primary
  centres to satisfy the triangle and landings, then fills the rest with storage. When the room
  is too small it says by how much, in each direction.
- **Checks while drawing and in `kitchen.check`:**
  - triangle (F5–F8), landings and the dishwasher's 36 in to the sink (F18–F27);
  - aisles 42/48 in, or 1200/1500 mm in England (F9–F13); door conflicts (F17);
  - 30 in above the cooktop, hood size from the product, no OTR microwave on an island
    (F44–F50);
  - countertop receptacles by NEC edition, advisory (F66–F70);
  - ADA and A117.1 spaces, knee and toe, 34 in surfaces (F86–F99); frontage, storage and M4(3)
    worktop lengths (F28–F32).
- **Living layout.** The result is a group tagged with its template. Changing the room or the
  pack re-runs the fit, keeping what the user moved.

### 6.2 Commercial line-ups (`foodservice.lineup`)
- **Cook line.** Along a wall or as an island, from a list of appliances. The tool orders them
  by duty class, heaviest toward the middle of each hood section (B5, S1227). It keeps fryers
  16 in from open flames or places a baffle (F181), puts each on legs, and offers the hood
  (§7).
- **Ware-washing.** Soiled table, pre-rinse, dish machine with booster, clean table at least as
  large as the soiled one, 3-comp sink, hand sink and floor sinks (B2, B11, F184–F188).
- **Prep and storage.** Prep tables, prep sinks, reach-ins, shelving with the 6 in bottom shelf.
- **Checks:**
  - hand-sink coverage as a reach map from each workstation, by path with doors counted, not
    straight line (F103–F106);
  - a floor sink at each indirect drain, ≥ 1 in air gap (F113–F117);
  - aisles, with oven doors and kettle pour arcs as zones (B5, F142); cleaning gaps (F134);
  - flow from receiving to service, soiled return and refuse kept apart: a note only, since the
    sequence is common practice, not code (B5).
- **Sizing helpers**, each naming its method: dish-machine racks per hour (Georgia), hot-water
  demand (California CCDEH), grease interceptors (PDI G-101, UPC), walk-ins from seats
  (Berkshire, advisory) (B5, B10, B11).

## 7. Ventilation and fire

### 7.1 Hoods (`hood.place`, `hood.from_line`)
- **A hood is an element** with a style (wall canopy, island, eyebrow, backshelf), a type (I or
  II), filters, lights, and exhaust and makeup air connectors.
- `hood.from_line` sizes it from the appliances below: overhang on every open side from the pack
  (6 in IMC; 250 mm DW/172), lip at most 4 ft above the cooking surface, filters at least 2 ft
  above flame and 3.5 ft above charcoal (IMC; NFPA 96 reads differently, *Gaps* 5).
- **Exhaust rate** comes from the pack's table: style × heaviest duty class × length (CMC/IMC,
  B6). A listed hood uses its listing rate from the product. Makeup air is set at 80–100 % by
  the pack, interlocked (B7).
- Residential hoods are the same element at a smaller scale: width at least the cooktop, height
  from the product (default 24–30 in), ducted outdoors, makeup air above 400 cfm where the pack
  says so (A5, A7).
- Checks: Type I over every grease-producing appliance (F148); Type II over a high-temperature
  dish machine (F149); overhang coverage (F150); extra-heavy duty not under an eyebrow (F154);
  flashing to the ceiling within 12 in (F152).

### 7.2 Grease ducts (`grease_duct.route`)
- A duct type with the rules of research/14 B8, routed with MEP-DUCT-008 and sloped as pipes are
  (MEP-PIPE-002). Checks (F160–F167):
  - slope ≥ ¼ in/ft toward the hood, ≥ 1 in/ft over 75 ft;
  - velocity ≥ 500 fpm, with 1,500–1,800 fpm shown as a recommendation;
  - cleanouts at each turn and along runs (12 ft or 20 ft; the sources conflict);
  - termination ≥ 40 in above the roof and 10 ft from intakes and property lines.
- Clearances (18 in to combustible, 3 in limited-combustible, 0 noncombustible) and the rated
  shaft (1 h under four storeys, 2 h above) are zones, checked by the overlap detector.

### 7.3 Suppression and safety
- A suppression system per hood (UL 300 or EN 17446), with nozzles over each appliance and a list
  of what it shuts down: gas, power and lights under the hood, and makeup air, while exhaust
  runs on (F171, F178).
- Pull station 10–20 ft from the hood, 42–48 in high; Class K extinguisher within 30 ft of travel;
  both checked by path (F179–F180). UK: gas–ventilation interlock, CO and CO2 alarms (F173–F175).
- Residential: smoke-alarm distances from cooking appliances by type (F80).

## 8. Retail refrigeration

### 8.1 Display cases
- **Case types** by DOE family: open multideck (VOP), semi-vertical (SVO), island (HZO),
  glass-door reach-in (VCT), chest (HCT), solid door, service counter (SOC), ice cream and
  self-contained (research/14 C1). Each has lengths (8 and 12 ft; doors about 30.7 in), ends,
  rear air gap, door swing (26½ in), temperatures, load, defrost, refrigerant and charge (C2).
- **`Riviera_DisplayCase` pset** (MIT): DOE class (e.g. VOP.RC.M), TDA, volume, temperature class
  (DOE M/L/I; ISO 23953 M0–H2, L1–L3), load, defrost, CDEC against MDEC, A2L minimum room area.
- IFC: `IfcElectricAppliance` REFRIGERATOR (MT) or FREEZER (LT, ice cream), as agent C proposed
  (D3).

### 8.2 Line-ups (`refrig.lineup`)
- Along a path, sketched or typed. Length = cases + ends (1⅛–2⅜ in) + partitions + joints (⅛ in)
  (C12, F218). Wedges turn corners. Rear gap and door-swing zones are virtual volumes, so an
  aisle that the doors eat is reported (F216–F217).
- **Built on modular units (ARC-GRP-004)** where possible: a line-up is a unit array whose bays
  are cases, whose connection points are refrigerant suction and liquid, drains and circuits,
  and whose spine is a header in a trench or overhead. Repeated apartment kitchens use the same
  framework. ARC-GRP-004 is specified but not built; whether to build it first is Question 5.

### 8.3 Walk-ins (`refrig.walk_in`)
- From an outline or a room: 4 in panel walls and ceiling as wall and slab types, 2 in clear of
  the building; an insulated floor or slab; heated doors with closers; a cooler vestibule before
  a freezer, with a relief port (B13, C5). An `IfcSpace` "ColdRoom" bounded by them (D3).
- A unit cooler ≥ 3½ in below the ceiling, never over the door; its condensate to a floor sink
  outside; the condensing unit indoors, outdoors or on the roof.
- Checks: R-25 / R-32 / R-28 (10 CFR 431.306), lighting, no floor drain inside (F197–F208).

### 8.4 Plant and piping
- **Racks and condensing units** as equipment with MT and LT suction groups. Cases are assigned
  to a circuit and a group; the tool sums load by length or by door and compares it with
  capacity. Defrost groups and the LT cases' 208 V defrost circuits go to panels (MEP-ELEC).
- **Systems:** direct expansion, distributed, secondary, cascade, transcritical CO2 (C6). Piping
  is ordinary pipe in a REFRIGERATION system, sloped back to the machine room (1 in per 20 ft),
  in closed trenches or overhead.
- **Condensers and gas coolers** on the roof with clearance zones: one unit width to walls,
  4 ft end to end, 2B in a pit (C7, F240).
- **Machinery room:** detectors, emergency exhaust Q = 100·√G (advisory), CO2 detectors about
  12 in above the floor (C6, C7).

### 8.5 Refrigerant data and checks
- A refrigerant table as data: ASHRAE 34 class, GWP (AR4, with the alternative values the
  sources give), source and as-of date (C8).
- Dated checks, all naming their date: GWP limits by equipment type and install date (US TT
  2026 revision, EU 2024/573, GB); R-290 charge (about 500 g open and 300 g closed in the US,
  13 × LFL ≈ 494 g IEC); A2L minimum room area from the product; leak detection at ≥ 1,500 lb;
  GB leak-check intervals by tonnes CO2e (F221–F235).
- These rules change often (twice in two years in the US). They are shipped as advisory data
  with an as-of date, and the report says how old the data is (Question 7).

### 8.6 Loads to the MEP and energy tools
- Case loads sum per rack and circuit; the store's total charge and CO2e are reported.
- Open cases take sensible and latent heat from the sales floor. These credits go to the space
  for the energy model (ANA-EM-003); self-contained cases reject their heat to it.
- Each case's MDEC from its DOE class and TDA (10 CFR 431.66, 2017 levels), and EU EEI (C4).

## 9. Schedules, sheets and exports

- **Equipment schedule** in the foodservice layout (NY OGS 11 40 00, E2): item number, quantity,
  manufacturer, model, then electrical, plumbing, gas and exhaust columns with rough-in heights,
  all from `Riviera_Utilities` and ports. A **rough-in plan** tags each connection for plan
  review (B1).
- **Kitchen elevations:** one action makes an elevation of each wall, cabinets tagged and
  dimensioned.
- **Cabinet, hood and refrigeration schedules:** units and tops; hoods with duty class and
  airflows; cases by line-up with class, load, circuit, rack, refrigerant and MDEC; a
  refrigerant inventory.
- **Compliance report** (`kitchen.check`, `refrig.check`): HTML and BCF, as `bath.check`.
- **COBie:** foodservice equipment and cases are managed assets (Components and Types); residential
  cabinets are left out by default (E1). The Connection sheet is filled from the ports, which
  Riviera's COBie export (DOC-OUT-015) does not yet do.

## 10. Build order (proposed)

1. Generalised rules schema (or the bath schema extended, per Question 1) with the new rule
   kinds; first packs `us_nkba_kitchen`, `ada_2010_kitchen`, `england_kitchen`; `kitchen.check`.
2. `Riviera_Utilities` and the residential placeholder library with ports and IFC mapping.
3. Cabinet runs, countertops with automatic cut-outs, appliances in cabinets (§4).
4. Residential layout templates with triangle, landing and aisle checks; kitchen elevations (§6.1).
5. Generalised product swap and documents, with cut-out fit checks (§5.3).
6. Foodservice library, `us_foodcode_2022` and two local overlays, path-distance rules and the
   hand-sink reach map (§6.2).
7. Hoods, grease ducts and suppression with `us_imc_nfpa96` (§7).
8. Commercial line-ups, sizing helpers, equipment schedule and rough-in plan (§6.2, §9).
9. Walk-ins (§8.3).
10. Display cases and line-ups, on ARC-GRP-004 if built by then (§8.1–8.2).
11. Racks, circuits, piping and loads; refrigerant and energy checks; refrigeration schedules
    and the COBie Connection sheet (§8.4–8.6, §9).

Each step gets its spec section first, its samples and a help walkthrough. Every tool is a
registered command (`kitchen.*`, `cabinet.*`, `countertop.*`, `foodservice.*`, `hood.*`,
`refrig.*`), exposed to Lua and MCP, and every tool that takes points, paths or outlines
(cabinet runs, line-ups, walk-in outlines, hood extents, duct routes) takes sketched or typed
points (standing rules).

## 11. Questions for the owner

1. **Rules schema:** generalise the bath schema into one `riviera.rules-1` for all domains
   (proposed, §3.1), or keep a separate kitchen schema?
2. **Packs and jurisdiction:** which packs ship by default? For commercial kitchens, ship the
   Food Code plus a few local overlays (Maricopa, CUPHD, SNHD) as worked examples, or the Food
   Code only, with an empty template for the user's health department?
3. **Priority:** residential first (§10), or commercial kitchens or grocery refrigeration
   earlier, as commercial greenhouses came early?
4. **IFC classes for the gaps:** countertop as `IfcSystemFurnitureElement` WORKSURFACE
   (Uniclass), gas cookers as `IfcElectricAppliance` USERDEFINED with a gas port, hoods as
   `IfcAirTerminal` USERDEFINED (BDNS). Accept these, or prefer proxies?
5. **Modular units:** build ARC-GRP-004 before display-case line-ups and repeated apartment
   kitchens, or start with a simple line-up generator and move it onto unit arrays later?
6. **The NEC:** its kitchen receptacle rules are known only from city handouts and NKBA
   citations. Ship them as advisory, or leave them out until a reviewer checks the text?
7. **Refrigerant and energy rules:** they change often (the US GWP limits changed twice since
   2023). Ship them as dated, advisory data that Riviera updates, or leave them to user packs?
8. **Cabinet depth:** stop at design (runs, tops, elevations, schedules), or plan for shop cut
   lists and CNC export later, as KCD does?

## 11a. The owner's answers (2026-11-06)

Given with `INTERIORS_PLAN.md`:
- **Q1, rules schema:** one shared schema, `riviera.rules-1`, for every domain (§3.1). The bath
  packs keep working.
- **Q3, priority:** the shared steps first (the rules schema, then the counter tool), then
  interiors. Kitchens follow, reusing the counters.
- **Q4, IFC classes:** as proposed:
  - countertops are `IfcSystemFurnitureElement` WORKSURFACE;
  - gas cookers are `IfcElectricAppliance` USERDEFINED with a gas port;
  - hoods are `IfcAirTerminal` USERDEFINED.
- **Q5, modular units:** start line-ups and repeated kitchens on path arrays, and move them onto
  ARC-GRP-004 when it is built.

Still open: Q2 (packs), Q6 (NEC as advisory), Q7 (refrigerant and energy data) and Q8 (cabinet
depth).

