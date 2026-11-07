# Interiors workbench, retail fixtures, checkouts and gyms

**Status:** Draft 0.2 · 2026-11-06 · four of the owner's answers in §11a; nothing built.
**Owner's requests (2026-10-31):** "Additional research items: gym and excercise equipment.
Retail hardware for grocery, department store, display cases, etc."; "We'll also need and
interiors workbench section: Common furnature items, counters, ornimental lighting, bedrooms,
hotel rooms, etc."; and "For retail environments research standard checkout configurations,
both manned and self-service." On 2026-11-06 the owner chose this plan as the next epic.

FileEngine is optional (standing rule). Every tool here works on a local project. Shared
product reference data in FileEngine is a later, optional workflow, as for bathrooms and
kitchens.

**Research:** `research/15_gyms_retail_interiors.md` uses public sources only. It covers 150
sources, S1354–S1503, logged in `RESEARCH_LOG.md` and `SOURCE_REGISTER.md`. Its checkable rules
are numbered G1–G182 in Part G:

| Rules | Area |
|---|---|
| G1–G21 | Shared accessibility |
| G22–G31 | Counters |
| G32–G65 | Gyms |
| G66–G99 | Retail fixtures and stores |
| G100–G137 | Checkouts |
| G138–G182 | Interiors, bedrooms and hotels |

Values are marked [V] verified, [S] secondary, [U] unverified, [judgement] or [guide].

Many key texts were not read. They are paywalled or refused automated fetches:
- ASTM F2115 and F2276, ACSM and ISO 20957 (gym equipment);
- the model IBC and IFC (fire code) text, NFPA 13 and 101, and the NEC;
- IES RP-2-20 and EN 12464-1, BS 8300-2, and BIFMA.

Rules taken from them are *advisory* until someone checks them against the text. Grocery
refrigeration (cases, walk-ins, racks) is in `KITCHENS_PLAN.md` §8, not here.

---

## 1. What the toolset is for

**Furnishing and fitting out rooms people live, stay, shop and exercise in,** with each item's
use and access space checked against the adopted rules as it is placed. Four areas share one
engine:

- **Interiors workbench:** furniture, counters and casework, and decorative lighting. Room
  layouts cover bedrooms, living and dining rooms, home offices, hotel guest rooms (king,
  double-queen, accessible, roll-in shower, communication features, junior suite), private
  offices, meeting rooms, and lobbies with reception desks.
- **Retail fixtures (not refrigerated):** gondolas and end caps, and wall shelving, slatwall and
  pegboard. Also pallet racking in public areas, produce, bakery and bulk fixtures, and apparel
  fixtures and mannequins. Also showcases and pharmacy fixtures, fitting rooms, cart corrals,
  EAS pedestals, signs, and department-store specifics.
- **Checkouts:** staffed checkstands of each type, cash wraps and service counters,
  self-checkout units and pods, and the front end: lanes at a pitch, queues, the power aisle, and
  sightlines.
- **Gyms:** cardio, selectorised and free-weight equipment, racks and platforms, mirrors, gym
  circuits and studios, and their building integration: ceiling height, floor loads,
  circuits, heat gain and outdoor air.

**Out of scope, at least at first:**
- planogram content (product facings per shelf). Riviera exports the macro plan and leaves the
  planogram to the tools built for it (§5.6);
- sports halls, pools, courts and rehab or physio gyms, which were not researched;
- FF&E procurement, pricing, and brand-standard libraries (Marriott, Hilton, IHG);
- shop drawings for millwork, which is the same question as kitchen Question 8.

**The opening.** In the research, existing tools split this way (research/15 F1):

| Tool | What it has |
|---|---|
| Revit and similar | Furniture and casework families; FF&E schedules across categories; hotel typicals as groups or links, with "which typical goes where" a known pain |
| Blue Yonder, DotActiv, Spaceman, CADS StoreSpace | Macro floor plans linked to planograms; rarely joined to the building model |
| Madix, BIMobject retail content | Fixture families with shelf parameters; no common retail property standard |
| Checkout and gym makers | Site-preparation guides and footprint PDFs; no planning tool |

Missing everywhere: clearance zones as real geometry on every type, checked against the
adopted pack; count rules checked from what is placed (one of each exercise machine type
accessible, accessible checkout aisles per function, accessible guest rooms by Table 224.2);
repeated rooms and lanes that carry their aisles and queues; and an open planogram exchange
(research/15 F2). Riviera can do all of them on the bathroom pattern: **rules as data,
placeholders with zones and ports, layouts checked as you draw, products swapped in later.**

## 2. What Riviera already has

| Need | Existing tool | Status |
|---|---|---|
| Rule packs with jurisdiction, edition, source and confidence | Bathroom packs (ARC-BATH-001), `schemas/bath/`; the generalised `riviera.rules-1` proposed in `KITCHENS_PLAN.md` §3.1 | Built; generalisation proposed |
| Clearance zones as virtual volumes, checked while drawing | Fixture spaces (ARC-BATH-002), clearance zones (ARC-MOD-031), overlaps (ARC-MOD-030) | Built |
| Placeholder components with IFC classes and connectors | `bath_components` (ARC-BATH-004), custom objects from meshes (DM-IMP-015), components (ARC-COMP-001) | Built |
| Layout templates fitted to a room, with a shortfall report | `bath.layout` (ARC-BATH-005) | Built |
| Counts from occupant load and from placed fixtures | `bath.fixture_count` (ARC-BATH-007) | Built |
| Product swapped for a placeholder, keeping its GlobalId; documents on the type | ARC-BATH-006, COL-FE-006/007 | Built |
| Components repeated along a path | Array along a path (ARC-MOD-033) | Built |
| Repeated units with bays, pitch and shared service spines | Modular units (ARC-GRP-004) | Specified, not built |
| Rooms and spaces with names and areas | Rooms (ARC-ROOM-001), MEP spaces (MEP-SPACE-001) | Built |
| Lighting fixtures, circuits and panel schedules | MEP-ELEC-001, -002, -013 | Built |
| Lighting analysis | ANA-LIGHT-001 | Built |
| Energy model loads | ANA-EM-003 | Built |
| Path of travel and egress routes | ANA-ROUTE-001…007 | Built |
| Sprinklers | MEP-PIPE-007 | Built |
| Schedules, sheets, COBie | Schedules (5.x), DOC-OUT-015 | Built |
| Library browser | DM-LIB-001…005 | Built |
| Cabinet runs and countertops | `KITCHENS_PLAN.md` §4 | Planned |

Each new tool produces furniture, lighting fixtures, appliances, walls, spaces and zones that
every other tool already understands.

## 3. Rule packs

### 3.1 On the shared rules schema

These areas need the rule kinds the bath schema has (zones, distances, counts) and those
kitchen Question 1 proposes adding (path distance, product values). They also need three more:

| New rule kind | Examples |
|---|---|
| **Type coverage** | One of each exercise machine *type* accessible, strength machines grouped by function and each cardio machine its own type (G32, ADA §236.1) |
| **Array counts** | Accessible checkout aisles per function by Table 227.2, with the under-5,000 ft² exception and an alterations mode (G100–G103). Accessible guest rooms by Tables 224.2 and 224.4, with dispersion and the 10 % overlap cap (G141–G143). Fitting rooms at 5 % per cluster (G66). UK one wheelchair-accessible bedroom per 20 (G147) |
| **Integration** | Top of storage against the sprinkler deflector, and gondola anchoring from height and leveller spacing. Ceiling height against the tallest equipment plus the user's rise. Box weight under chandeliers and fans. Each rule reads another discipline's model (F2 item 5) |

This plan assumes the generalised `riviera.rules-1`, so a kitchen's island, a hotel room's
bed and a checkout's counter are checked by one engine. If the owner keeps separate schemas
(kitchen Question 1), this plan needs one too. That is Question 1 below.

### 3.2 Packs to ship first (proposed)

| Pack | Layer | Covers | Confidence |
|---|---|---|---|
| `ada_2010_interiors` | accessible | Routes, reach, counters, protruding objects, storage, beds, guest rooms, fitting rooms, exercise machines, checkouts (G1–G21, G32–G34, G66–G70, G100–G108, G138–G146) | verified |
| `a117_2017_interiors` | accessible | The edition differences: 30 × 52 in clear floor space, 67 in turning circle, the new bed item (*Gaps* 15) | secondary |
| `uk_adm_m4` | accessible | M4(2)/M4(3) bed and room spaces in dwellings (ADM vol. 1) | verified |
| `uk_adm_vol2` | accessible | Hotel bedrooms, counters, fitting rooms (G147, G148, …) | verified |
| `us_ibc_mercantile` | minimum | Occupant load factors, Group M aisles and the merchandise-pad accessway (G71–G74) | secondary (adoptions) |
| `us_ifc_highpiled` | minimum | High-piled storage and rack flags (fire code ch. 32) | secondary |
| `eu_en301549_sco` | accessible | Self-checkout terminals as ICT (EN 301 549, read directly) | verified |
| `practice_interiors`, `practice_retail`, `practice_gym`, `practice_checkout` | recommended | Clearances around furniture, aisles beyond code, lane pitch and queue depth, treadmill zones, and other practice and [judgement] values | advisory |

Gym packs from Sport England, the IFI and sportscotland, ASHRAE 62.1 (as Washington adopts it)
and DGUV follow as advisory packs. Which ship first is Question 2.

### 3.3 Where the research corrected the brief

Four common beliefs are wrong in the primary text (research/15 *Gaps* 1–4). The packs follow
the text:

| Belief | The text |
|---|---|
| Store shelves must be within reach (ADA §308) | Self-service shelves must be on an accessible route; they need not meet §308. "50 % of each type" is §227.4, for food-service lines only |
| Accessible counters are 28–34 in | Sales and service counters have an accessible portion ≤ 36 in (§904.4). 28–34 in is §902, dining and work surfaces |
| Beds need 36 in each side | 30 × 48 in parallel-approach space on both sides of one bed, or one between two beds (§806.2.3). 36 in is older California wording |
| Self-checkouts have ADA rules | The 2010 Standards have no self-checkout section, and the Access Board's 2022 ANPRM is still open. Whether self-checkout lanes count under Table 227.2 is unresolved |

### 3.4 Conflicts the packs must carry

Where sources disagree, the pack names both values, and a finding shows the one the active pack
adopts (research/15 *Gaps* 5–16):
- **Treadmill rear clearance:** 2 m, 1 m, 1.25 m or 78 in.
- **Group M aisles:** sized from capacity, or the legacy 36/44 in.
- **Mercantile load factor:** 60 or 30 ft².
- **Till lighting:** 500 or 300 lx.
- **Ceiling-fan heights and wall clearances.**
- **Chandeliers over circulation:** 78 in against ADA's 80 in.
- **Accessible bed heights.**
- **ADA 2010 against A117.1-2017 clear floor spaces.**

A `recommended` pack or an `advisory` rule gives a **note**, never an error. The agents'
[judgement] defaults go to `EXPERT_REVIEW.md` (Q14), from E-102 on, before they become Riviera
defaults. Among them:
- the treadmill zone and heat gain;
- checkout lane pitch, customer aisle, queue depth and power aisle;
- the self-checkout attendant ratio and accessibility target;
- Table 227.2 rounding;
- checkstand defaults;
- the IFC classes for gym mirrors and ceiling fans.

## 4. Interiors workbench

### 4.1 Furniture library (`furniture.*`)
An *Interiors* component library generated as `bath_components` is, from the catalogue in
research/15 D1 and E2: beds by mattress size; nightstands, dressers, chests and wardrobes;
sofas, sectionals and settees; armchairs, dining and office chairs and stools; dining, coffee
and conference tables; desks, workstations and standing desks; bookcases; reception desks; rugs;
luggage racks. Each placeholder carries:
- nominal sizes and the standard property sets (`Pset_FurnitureTypeCommon` with NominalHeight,
  NominalLength, NominalDepth, IsBuiltIn; `…Chair` SeatingHeight; `…Table` NumberOfChairs;
  `…Desk` WorksurfaceArea);
- its Uniclass code (Pr_40_50_06 beds, Pr_40_50_12 seating, Pr_40_50_21 desks and tables);
- **use zones from the active pack**: a bed's side spaces (30 × 48 in under ADA, 750 or 1000 mm
  under M4(2)/M4(3)), a chair's 36 in pull-back, a drawer's 30–36 in use zone, a table's
  36 in edge clearance and 24 in per diner, attached as virtual volumes (ARC-MOD-031).

Manufacturer models come in as meshes (DM-IMP-015) and are swapped in keeping the placement,
as bathroom products are (ARC-BATH-006).

### 4.2 Room layouts (`interior.layout`)
Templates fitted to a room, as `bath.layout` does, with a shortfall report when one does not
fit:
- **Dwellings:** bedroom by bed size, living room by seat count, dining by seats, home office.
- **Hotels:** king, double-queen, accessible king, roll-in shower and junior suite, each with
  its bathroom from the bathroom toolset. Communication features (G144) are a flag on the room.
- **Workplace:** private office, meeting room by seats, lobby and reception.

Each layout is checked as it is placed: route widths, turning spaces and bed spaces (G1–G4,
G138–G139), the IRC room minimums (*Gaps* 16 notes the edition change), and
practice clearances as notes.

### 4.3 Hotel floors and the room matrix
Hotel typicals are a known pain in other tools (F1). In Riviera a guest room is a **modular
unit** (ARC-GRP-004) whose bay carries its corridor face and its service spine (bathroom risers,
HVAC). `hotel.room_matrix` assigns typicals to bays floor by floor. It checks the matrix
against:
- Table 224.2 (mobility rooms, with and without roll-in showers);
- Table 224.4 (communication rooms);
- dispersion across room classes and the 10 % overlap cap (G141–G143);
- the UK 1-in-20 rule (G147).

It reports the shortfall by class. This step needs modular units; without them a guest room
is an array along a path (ARC-MOD-033) with the counts checked from what is placed (Question 3).

### 4.4 Counters and casework
Counters share the kitchen plan's cabinet runs and countertops (`KITCHENS_PLAN.md` §4) rather
than duplicating them. This plan adds:
- **counter height zones along a run:** transaction ledge 42 in, work surface 29 in, and an
  accessible portion ≤ 36 in high and 36 in long, or 30 in for a parallel approach (§904.4);
  bar counters at 42 in with a lowered section (G22–G31);
- **knee and toe clearances** under accessible portions (§306), drawn as zones;
- **reception and service desks and cash wraps**, as counter runs with these zones;
- **closets:** shelves, rods and doors, with the accessible-rod reach check (§308).

Counters and built-in casework are `IfcFurniture` USERDEFINED ("Casework"), IsBuiltIn TRUE;
Riviera already maps that to the casework category (research/15 E1).

### 4.5 Decorative lighting (`lighting.decorative`)
Helpers that place `IfcLightFixture`s and set `MountingType` and `PlacingType` (research/15 D4,
F2 item 6). Photometric files attach through `IfcExternalReference`, and the fixtures feed the
electrical tools (MEP-ELEC-001) and lighting analysis (ANA-LIGHT-001):
- **chandeliers:** size from the room or the table below, and height above the table. Over a
  circulation path the bottom must be ≥ 80 in (§307.4). The retailer guides' 78 in fails
  there (*Gaps* 10);
- **island pendants:** count and spacing along the island;
- **wall sconces:** centre height and pair spacing, and the 4 in projection limit in
  circulation (§307.2);
- **ceiling fans:** blade height and wall clearance from the pack (the sources conflict,
  *Gaps* 9);
- **box weight:** a chandelier or fan heavier than its box's rating is a warning (NEC 314.27,
  advisory).

### 4.6 FF&E schedules
One schedule across furniture, casework, lighting and equipment, keyed by room and type mark,
with finish columns from IFC4.3's furniture material categories (Finish, Frame, Hardware,
Padding, Panel). It exports to CSV and to COBie Components and Types (DOC-OUT-015).

## 5. Retail fixtures

### 5.1 Fixture library (`retail.*`)
A *Retail* component library from research/15 B1–B9 and E2:
- gondolas, island and wall, with end caps;
- wall shelving, slatwall and pegboard;
- pallet racks for public areas;
- produce, bakery and bulk fixtures;
- apparel fixtures: rounders, 4-ways, T-stands and wall rails;
- mannequins;
- showcases, counter and tower, with their lights;
- pharmacy fixtures;
- cart corrals and EAS pedestals;
- signs.

Each carries `Riviera_RetailFixture` (§8.2):
- shelf count, pitch and depth;
- base depth;
- load per shelf;
- an anchored flag;
- LED watts per foot.

Each also carries its face and access zones. Section sizes and depths come from the research's
manufacturer rules (B1). Values the research marked [U], such as mannequins, produce tables and
bulk bins, are placeholders marked as such.

### 5.2 Fixture runs (`retail.run`)
A run is drawn as a path, sketched or typed (standing rule). It is split into sections by the
manufacturers' length and depth rules, and given end caps (width = island depth). Each run
reports its aisle width to the neighbouring runs as it is drawn:
- the accessible route, ≥ 36 in (G1);
- 180° turns at gondola ends (G2);
- passing spaces where an aisle is under 60 in (G3);
- the Group M public aisle and merchandise-pad accessway (G73–G74), with their conflict shown;
- practice aisle widths as notes.

A run is a path array of fixture components (ARC-MOD-033), or a modular unit once ARC-GRP-004
is built.

### 5.3 Store checks (`retail.check`)
- **Occupant load** from the spaces: mercantile 60 ft² (or 30, as the pack chooses), stock
  300 ft² (G71–G72). It feeds the egress tools (ANA-ROUTE).
- **Fitting rooms:** 5 % accessible per cluster, turning space, door swing, bench, and shelves
  and hooks (G66–G70).
- **Signs:** a bottom height ≥ 80 in over circulation, and projection ≤ 4 in (§307).
- **Integration:**
  - top of storage against the sprinkler deflector (18 in, or 36 in for ESFR, [S]), and high-piled
    flags (fire code ch. 32, advisory);
  - gondola anchoring from height and leveller spacing, and pallet racks needing anchoring and
    restraint (FEMA 460);
  - EAS pedestals on their own circuit.

### 5.4 Department stores
Showcases with an accessible portion where goods are served across them (§904.4). Escalators and
moving walks come from the vertical-transport tools (ARC-VT-010…012). Department-specific zones
(B13) are layout templates.

### 5.5 Signs in IFC
`IfcSign` (PICTORAL, MARKER) exists only in IFC 4.3, as a component rather than a
furnishing. IFC4 projects fall back to `IfcFurniture` USERDEFINED ("Sign") or an annotation,
the way guideway devices fall back (research/15 E1).

### 5.6 Planogram exchange
No standard planogram schema was found (research/15 F2 item 8). Riviera exports each fixture
with its shelf positions as IFC, and as JSON to an MIT-licensed schema,
`schemas/retail/riviera.fixture-plan-1.schema.json` (interop policy). A planogram tool gets a
clean macro plan, and its facings can come back as shelf labels. Question 6.

## 6. Checkouts

### 6.1 Staffed checkstands (`checkstand.place`)
One parametric type per checkstand style (research/15 C1–C5):
- belt and non-belt, single and tandem, express;
- cash wraps and service, pharmacy and customer-service counters.

Its parameters (C12):
- transport length and width;
- deck height;
- bag well or carousel;
- hand, left or right;
- scanner-scale cut-out;
- card terminal height;
- impulse merchandising;
- lane light;
- an accessible flag.

Its parts aggregate as `IfcSystemFurnitureElement` modules. The belt is geometry, with an
optional `IfcConveyorSegment` in IFC 4.3. The scanner is an `IfcCommunicationsAppliance`
USERDEFINED ("BARCODESCANNER"): SCANNER means a document scanner (E1).

An accessible checkstand is checked as it is placed:
- aisle ≥ 36 in;
- counter ≤ 38 in, with edge protection ≤ 2 in;
- check-writing surface 28–34 in where provided;
- the ISA sign;
- an accessible queue (G104–G108).

### 6.2 Self-checkout (`sco.place_pod`)
Self-checkout units and pods (C7–C8), each unit an `IfcFurniture` USERDEFINED ("SELFCHECKOUT")
aggregating its terminal, scanner and display:
- service clearances as `IfcBuildingElementProxy` PROVISIONFORSPACE;
- a pod's courtyard and the attendant's sightlines to every screen;
- the shared queue zone.

With no fixed US rule, each unit is checked against an accessibility profile built from the
Access Board's ANPRM and EN 301 549, as advisory (research/15 C9; F2 item 10).

### 6.3 Front ends (`frontend.layout`)
Lanes at a pitch along the store front, with handing patterns and tandem pairs, queue zones
(`IfcSpatialZone` USERDEFINED "QUEUE"), the power aisle, sightlines and the exit gates (an
`IfcDoor` GATE; TURNSTILE in IFC 4.3). Checks as it is laid out:
- **Accessible aisles per function** by Table 227.2, the small-store exception, dispersion, and
  the alterations mode (G100–G103).
- **Egress at the front end:** gates fail open, and the lanes leave the required exit width (C11).
- **Circuits:** each lane's POS-IG, EAS and BELT circuits feed the electrical model through floor
  boxes (C12).

Lane pitch, queue depth and power aisle are [judgement] (§3.4), so they are inputs with
practice defaults, never errors. Whether a self-checkout bank is a "function" under Table 227.2
is unresolved. The default counts each bank separately, and a pack setting changes that
(Question 5).

A **lane-count calculator** (an M/M/c queue from peak customers, basket size and items per
hour) is proposed by the research as a helper. The literature recommends simulation instead, so
it ships marked as an estimate (Question 5).

## 7. Gyms

### 7.1 Equipment library (`gym.*`)
A *Gym* component library from research/15 A1 and E2:
- **cardio:** treadmills, ellipticals and cross trainers, steppers, bikes, rowers, upper-body
  ergometers;
- **strength:** selectorised machines by function, cable and functional trainers, and multigyms
  and leverage units;
- **free weights:** power racks, half racks and Smith machines; dumbbell racks, benches and plate
  trees; lifting platforms;
- **other:** suspension trainers and punch bags with their anchors, power plates and trampolines,
  mirrors.

Each is `IfcFurniture` USERDEFINED with its ObjectType, as Uniclass maps Pr_40_70_84 (E1; no
IFC sports class exists). Each carries `Riviera_ExerciseEquipment`:
- supply voltage and rated current;
- a dedicated-circuit flag;
- heat gain;
- use-zone dimensions.

Each also carries its use zone:
- treadmill sides and rear (G37–G38, with the conflict);
- a rower's 2.74 × 1.22 m;
- a dumbbell rack's live depth (G39–G40).

### 7.2 Layouts and checks (`gym.layout`, `gym.check`)
Gym circuits and studios fitted to a room. Checks:
- **Type coverage:** one of each machine type accessible, on an accessible route, with a
  30 × 48 in space that may overlap neighbours (G32–G34). In the UK, inclusive transfer space
  and IFI counts (G35–G36).
- **Use zones** from the pack, overlapping neither each other nor walls.
- **Ceiling height** against the tallest equipment plus the user's rise (steppers, racks,
  overhead lifts), and suspended anchors and fans above.
- **Mirrors** not behind treadmills; safety glazing flagged.

### 7.3 Building integration
- **Circuits:** treadmills and powered cardio on dedicated circuits; the loads feed the
  electrical model.
- **Heat gain:** per machine in use and per exerciser, feeding spaces and the energy model
  (ANA-EM-003). The defaults are [judgement] (§3.4).
- **Outdoor air** for weight and aerobics rooms (ASHRAE 62.1 as adopted in Washington,
  advisory).
- **Floor loads:** racks, platforms and dumbbell storage as point and area loads, for the
  structural tools.
- **Anchors:** racks and suspension points connected to their structure
  (`IfcMechanicalFastener` with `IfcRelConnectsElements`).

## 8. IFC mapping and property sets

### 8.1 Classes, settled once (proposal)
Following Uniclass's published mappings and research/15 E1–E2:

| Element | IFC4 | IFC 4.3 |
|---|---|---|
| Furniture | `IfcFurniture` with its PredefinedType (BED, CHAIR, SOFA, TABLE, DESK, SHELF) or USERDEFINED | same |
| Counters, casework, reception desks | `IfcFurniture` USERDEFINED ("Casework", "SERVICECOUNTER"), IsBuiltIn TRUE | same |
| Exercise equipment | `IfcFurniture` USERDEFINED ("Treadmill", …) | same |
| Gondolas, shelving | `IfcFurniture` SHELF, parts aggregated | same |
| Showcases, checkstands, self-checkouts | `IfcFurniture` USERDEFINED, parts aggregated | same |
| Signs | `IfcFurniture` USERDEFINED ("Sign") | `IfcSign` PICTORAL or MARKER |
| Lane lights | `IfcLightFixture` USERDEFINED ("LANELIGHT") | `IfcSignal` VISUAL |
| Exit gates | `IfcDoor` GATE | `IfcDoor` TURNSTILE or GATE |
| Queue, sightline and service zones | `IfcSpatialZone` USERDEFINED; service clearance `IfcBuildingElementProxy` PROVISIONFORSPACE | adds RESERVATION |
| Decorative lighting | `IfcLightFixture` with MountingType and PlacingType | same |
| Fitting rooms, guest rooms, gyms | `IfcSpace`, the function in ObjectType or LongName, plus Uniclass SL codes | same |

The IFC 4.3 classes are written in IFC4X3 projects through the class override the guideway
devices use (`rxd:ifc4x3_class`, ARC-ALN-002), with the IFC4 class as the fallback.

The gym mirror and ceiling fan classes are open:
- **mirror:** `IfcFurniture` or `IfcCovering`. `IfcSign` MIRROR is meant for signage;
- **ceiling fan:** a fan, or a light fixture for a fan-light.

Both are Question 4.

### 8.2 Riviera property sets (MIT)
No standard set exists for retail, checkout or gym data (E1). Three sets are proposed, added to
`schemas/psets/riviera-psets-1.json` and published:
- **`Riviera_RetailFixture`:** shelf count, shelf pitch, base depth, load per shelf, anchored
  flag, and the accessible-portion data;
- **`Riviera_Checkstand`:** IsAccessible and Function;
- **`Riviera_ExerciseEquipment`:** supply voltage, rated current, dedicated circuit, heat gain,
  and the use-zone dimensions.

## 9. Schedules, sheets and exports

- **FF&E schedule** across categories, by room and type mark, with finishes (§4.6).
- **Fixture schedule** by run: sections, shelves, loads and anchoring.
- **Front-end schedule:** lanes by function, accessible lanes against Table 227.2, and circuits.
- **Gym equipment schedule:** types, accessibility coverage, circuits and heat gain.
- **Guest-room matrix** by floor and class against Tables 224.2 and 224.4.
- **Compliance reports** (`interior.check`, `retail.check`, `frontend.check`, `gym.check`): HTML
  and BCF, as `bath.check`.
- **Planogram exchange** (§5.6) and **COBie**: checkouts, self-checkouts, showcases and gym
  equipment are managed assets. Loose furniture is optional.

## 10. Build order (proposed)

1. The rules schema decision (kitchen Question 1, Question 1 here), adding the type-coverage,
   array-count and integration rule kinds. First packs `ada_2010_interiors` and
   `practice_interiors`; `interior.check`.
2. The *Interiors* furniture library with use zones from the pack, and the FF&E schedule
   (§4.1, §4.6).
3. Room layouts for dwellings and workplaces (§4.2).
4. Decorative lighting helpers (§4.5).
5. Counter height zones and closets on the kitchen plan's cabinet runs and countertops (§4.4).
   This step comes after kitchen steps 3–4, or brings them forward.
6. Hotel guest-room templates and the room matrix (§4.3), on modular units if built by then.
7. The *Retail* fixture library, fixture runs and store checks, with `us_ibc_mercantile`
   (§5.1–5.3).
8. Checkstands and the front-end layout with Table 227.2 (§6.1, §6.3).
9. Self-checkout units and pods with the EN 301 549 profile (§6.2).
10. The *Gym* library, layouts and checks, and building integration (§7).
11. Department-store specifics, IFC 4.3 signs and signals, and the planogram exchange (§5.4–5.6).

**Progress (2026-11-06):**
- Step 1 is built: shared rule packs and use zones (ARC-RULE-001/002).
- Step 2 is built:
  - the bundled Interiors, Gym and Retail Component Libraries (DM-LIB-006, ARC-FURN-001), which
    also bring steps 7 and 10's generic fixtures and equipment ahead of time;
  - the FF&E schedule (DOC-SCH-016).
- Step 3 is built: room layouts for dwellings and workplaces (ARC-INT-001).
- Step 4 is built: decorative lighting (ARC-INT-003).
- Next is step 5, counters, on the kitchen plan's cabinet runs.

Each step gets its spec section first, its samples and a help walkthrough. Every tool is a
registered command, exposed to Lua and MCP:
- `furniture.*`, `interior.*`, `hotel.*`, `lighting.*`;
- `retail.*`, `checkstand.*`, `sco.*`, `frontend.*`, `gym.*`.

Every tool that takes points, paths or outlines (fixture runs, front ends, room layouts,
lighting along an island) takes sketched or typed points (standing rules).

**With kitchens.** The two plans share a rules schema (step 1 of each) and counters (kitchen
§4, step 5 here). Building them together means one schema decision and one counter tool.
Question 7 asks which goes first.

## 11. Questions for the owner

1. **Rules schema:** as kitchen Question 1, one generalised `riviera.rules-1` for every domain
   (proposed), or separate schemas per toolset?
2. **Packs:** which ship by default? The proposal is the ADA and UK accessibility packs, the
   mercantile pack and the practice packs (§3.2). Gym guidance from Sport England and the IFI
   and the EN 301 549 self-checkout profile would follow as advisory.
3. **Modular units:** build ARC-GRP-004 before hotel floors, fixture runs and front ends, or
   start them on path arrays and move them onto units later? (Kitchen Question 5 asks the same.)
4. **IFC classes:** accept the mapping in §8.1, `IfcFurniture` USERDEFINED for equipment,
   checkstands and casework, with IFC 4.3 signs, signals and gates where available? For gym
   mirrors, `IfcFurniture` or `IfcCovering`? For ceiling fans, a fan or a light fixture?
5. **Checkout rules that are not settled:**
   - count each self-checkout bank as its own function under Table 227.2 (the default)?
   - ship the lane-count calculator as an estimate, or leave it out?
6. **Planogram exchange:** publish an MIT fixture-plan schema (§5.6) now, or wait for a planogram
   tool to test against?
7. **Order:** which comes first among this plan's four areas? And with kitchens:
   - this plan first;
   - kitchens first;
   - or the shared steps (schema, counters) first, then each in turn?

   The proposal is interiors first (§10), since the furniture library, zones and layouts are
   what the retail, checkout and gym areas build on.

## 11a. The owner's answers (2026-11-06)

- **Q1, rules schema:** one shared schema, `riviera.rules-1`, for every domain. The bath packs
  keep working.
- **Q7, order:** the shared steps first (the rules schema, then the counter tool), then
  interiors. Kitchens follow, reusing the counters.
- **Q3, modular units:** start hotel floors, fixture runs and front ends on path arrays, and move
  them onto ARC-GRP-004 when it is built.
- **Q4, IFC classes:** the mapping in §8.1 is accepted, with IFC4 fallbacks for the 4.3-only
  classes. The gym mirror and ceiling fan classes are still open.

Still open: Q2 (default packs), Q5 (checkout rules), Q6 (planogram schema), and the mirror and
fan classes. Each is asked when its build step comes up.

