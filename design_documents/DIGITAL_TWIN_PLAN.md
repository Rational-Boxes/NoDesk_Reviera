# Digital twins, operational data and smart-city features — plan

**Status:** Draft 0.1 · 2026-10-29 · proposal for the owner's review (questions in §9).
**Request (owner, 2026-10-29):** "research … digital-twin, operational data acquisition, and 'smart
city' features integrated with greater BIM workflows and propose advanced features."
**Research:** `research/12_digital_twin_smart_city.md` (sources D001–D187, D058b, D138b, logged
as Session 4 in RESEARCH_LOG.md). The source IDs below point there.
**Related:**
- spec Ch 2 (data model, RXD, published `Reviera_*` property sets);
- Ch 4 (MEP systems, spaces, zones);
- Ch 6 (BCF, COL-API);
- Ch 7 and Ch 8 (analysis, compute jobs, remote analysis servers);
- DOC-OUT-015 (COBie);
- FILEENGINE_INTEGRATION.md;
- the MIT-interfaces policy.

## 1. What a twin means for Reviera

A digital twin couples a model of an asset with data from the real asset, flowing at least from
asset to model. A mature twin also acts on what the data shows D004 D005 D010. The UK National
Digital Twin Programme stresses that the asset must run safely without its twin D011.

Reviera's part is to be where the twin is **specified, linked and understood**:
- design the sensors and controls;
- bind model elements to live points;
- bring operational data back into the model's views and analyses;
- hand the result to the city and to twin platforms in open formats.

Reviera is not a building management system (BMS) or an IoT platform. It reads from them, and
writes to them only if you decide it should (§9 Q1).

What Reviera already has to build on:
- IFC 4.3 read and write with RXD;
- MEP systems, spaces and zones (IfcZone);
- COBie 2.4;
- BCF 2.1/3.0;
- the energy model with EnergyPlus, gbXML and HBJSON;
- solar, daylight and embodied-carbon analyses;
- site location and IfcMapConversion;
- the PVGIS, Open-Meteo and Nominatim connectors;
- compute jobs and remote analysis servers;
- Lua and MCP for every command;
- the published `Reviera_*` property-set file;
- the FileEngine integration design.

## 2. The keystone: point bindings

Every twin feature needs a durable link from an element's GlobalId to an outside **point**. A
point is a BACnet object, an MQTT or Sparkplug metric, a SensorThings Datastream, a Haystack
point, or a time-series database (TSDB) series. Brick's ref-schema already publishes this shape
(an IFC GlobalId reference, a BACnet reference, an MQTT topic, an InfluxDB measurement) D039.
CityGML 3.0's SensorConnection is the city-side equivalent D105.

- **A binding** is a record on an element (a sensor, a meter, a space, an equipment item or a
  port):
  - the point's protocol and address;
  - its quantity and unit;
  - its role (measured, setpoint, command, status);
  - its sampling interval;
  - the connection it is read through (a per-user setting, like FileEngine connections; never
    stored in the model).
- **In the model:** a binding is a `Reviera_PointBinding` set beside the RXD record. On export
  it is also written in standard form, as IfcPerformanceHistory assigned to the element with
  IfcRelAssignsToControl, with a library reference to the point D021 D022.
- **Commands:**
  - `twin.bind`, `twin.unbind`, `twin.bindings`;
  - `twin.discover(connection)`, which lists a source's points, for example Sparkplug birth
    certificates D071, BACnet object lists, or SensorThings Datastreams;
  - `twin.match`, which proposes bindings by name, tag or space. Each proposal is one undo step.

## 3. Proposed features, ranked

| # | Feature | What it does | Rests on |
|---|---|---|---|
| 1 | **Point bindings** (§2) | Element ↔ point records; discovery; matching; IFC export | Brick ref-schema D039; IfcPerformanceHistory D021; SensorConnection D105 |
| 2 | **Read-only connectors** (compute jobs) | Read the latest value and history; no server inside Reviera | SensorThings 1.1 REST/MQTT D109; Haystack `hisRead`/`watchPoll` D054; MQTT 5 + Sparkplug B (ISO/IEC 20237) D071 D073; InfluxDB 3, Timescale/PostgreSQL, Prometheus D080 D081 D083; BACnet/IP and BACnet/SC D066 D094; OPC UA via open62541 (MPL-2.0) D093 |
| 3 | **Sensor and controls layout** | Place IfcSensor, IfcActuator, IfcController, IfcUnitaryControlElement, IfcFlowMeter by predefined type; draw control links; produce a **points schedule** for the controls contractor | IFC 4.3 enums D015 D018; IfcRelFlowControlElements D019; Brick/Haystack point classes D036 D056; ISO 81346 designations D104 |
| 4 | **Semantic export** | IFC → Brick TTL validated by SHACL; RealEstateCore/DTDL; Haystack; import a Brick model as sensors | Brick 1.4 (1.5 adds controllers) D036 D037 D040; REC/DTDL D060 D062; Haystack 5/Xeto D053 D056; ASHRAE 223P once published D045 D050 |
| 5 | **Live overlays** | Spaces and equipment coloured by the latest or aggregated value in plans and 3D, with a time slider, legends and comfort bands | Existing colour-fill schemes; EN 16798-1 D171; ASHRAE 55 PMV D173; CO₂ above outdoor D172 |
| 6 | **Calibration report** | EnergyPlus results against meter data, monthly and hourly; NMBE and CV(RMSE) against the Guideline 14 / FEMP limits (±5 %/15 % monthly, ±10 %/30 % hourly); actual-year weather; a parameter-tuning job | FEMP guide citing G14-2023 D103; IPMVP Option D from a public guide D102; Open-Meteo |
| 7 | **Fault detection** | Brick-keyed rule sets (G36-style checks) as Lua rules; each fault becomes a BCF topic on the element | open-fdd style D169; Guideline 36 scope D098; BCF |
| 8 | **Operational carbon** | Meter energy × grid factors, location- and market-based (Scope 2 dual reporting, hourly-ready), beside embodied carbon for a whole-life view | GHG Protocol Scope 2 D177; ANA-CARB-001 |
| 9 | **O&M register** | IfcAsset and IfcInventory; maintenance, condition, warranty and service-life property sets; IfcTask/IfcWorkSchedule plans; COBie Job/Resource/Event round-trip | IFC asset sets D025 D026 D027; COBie D033; ISO 19650-3 D003 |
| 10 | **"Twin-ready" IDS packs** | Handover checks: sensors typed and bound, spaces in zones, assets identified, site at LoGeoRef 50 | IDS 1.0 D140 D141 |
| 11 | **Georeferencing checker** | Reports the LoGeoRef level; validates IfcMapConversion against IfcProjectedCRS/EPSG; warns when only latitude/longitude is set | LoGeoRef D119 D120; IFC 4.3 D118 |
| 12 | **City export** | CityJSON 2.0 with a `+Reviera` extension; CityGML 3.0 LoD1–3 with Dynamizer/SensorConnection for bound attributes; 3D Tiles 1.1 with GlobalIds in feature metadata; a footprint and roof generaliser for the envelope | D105 D107 D115; GeoBIM benchmark D124 D125 |
| 13 | **Publishing to twin platforms** | One-way first, ids from GlobalIds: NGSI-LD (Smart Data Models Building, temporal API); Eclipse Ditto Things; DTDL twins; AAS Digital Nameplate and Time Series submodels for products; SensorThings Things/Datastreams | D126 D128 D152 D061 D156 D157 D109 |
| 14 | **City scenario hand-off** | GeoJSON for URBANopt, CityJSON for urban energy models, roof solar potential per surface, OGC API Processes clients; run on remote analysis servers | D137 D136 D149 D146 |

## 4. How it fits Reviera's architecture

- **Commands everywhere.** Every feature is a registered command (`twin.*`, `ops.*`, `city.*`),
  in Lua and MCP. Agents can read, bind and analyse; credentials and any write path to a BMS
  stay UI and Lua only.
- **Connectors are compute jobs.** They poll or subscribe, store a bounded local cache, and stop
  when Reviera closes. Long histories stay in the external TSDB, with Reviera querying ranges.
  Remote analysis servers can host the always-on connectors and scheduled calibration (§9 Q8).
- **Time in views.** The time slider is a view setting. Overlays read the cache, and a view can
  be saved with a time or a range for sheets (for example "comfort, July 2027").
- **FileEngine.** It holds the shared point registry and the twin's documents (commissioning
  reports, FDD findings as BCF, calibration reports), so a team works from one binding set
  (§9 Q2).
- **Interfaces MIT, implementation GPL.**
  - The binding schema, the IDS packs, the CityJSON extension and the client crates are MIT.
  - Protocol stacks are chosen by licence (§8).

## 5. Data model additions (published, MIT)

- **`Reviera_PointBinding`:**
  - Protocol, Address, Quantity, Unit, Role, Interval;
  - ConnectionHint, which names a connection, never its credentials;
  - Source (Brick or Haystack id).
- **`Reviera_TwinStatus`:**
  - LastValue and LastSeen;
  - DataQuality (ok, stale, out of range, missing);
  - Calibration figures on the energy model.
- **Categories:**
  - `cat.sensors`, `cat.controllers` and `cat.actuators`, where IFC classes don't already give
    them;
  - `cat.assets` for IfcAsset registers.
- **Standard first:** use the IFC control elements, IfcPerformanceHistory and the `Pset_*PHistory`
  sets D023, and the asset sets, wherever they fit. No Reviera property repeats one.

## 6. Phases

1. **Foundations:**
   - the sensor and controls layout with its points schedule (#3);
   - point bindings with discovery and matching (#1);
   - the SensorThings and MQTT/Sparkplug connectors (#2, read-only);
   - live overlays (#5);
   - the georeferencing checker (#11).
2. **Analysis:**
   - the calibration report (#6);
   - operational carbon (#8);
   - fault detection with BCF (#7);
   - the Haystack, TSDB and BACnet connectors (#2).
3. **Semantics and handover:**
   - Brick export, validation and import (#4);
   - the O&M register and COBie round-trip (#9);
   - the twin-ready IDS packs (#10).
4. **City:**
   - CityJSON, CityGML 3.0 and 3D Tiles export (#12);
   - scenario hand-off (#14).
5. **Platforms:**
   - NGSI-LD, Ditto, DTDL, AAS and SensorThings publishing (#13);
   - RealEstateCore/DTDL and Haystack export (#4).

Each phase is specced (a new spec chapter section, probably Ch 7 §7.x "Operations and digital
twins") before it is built. Provisional defaults go into EXPERT_REVIEW, as the glazed-enclosure
work did.

## 7. Safety, privacy and security defaults

- **Read-only by default.** No command, setpoint write or Sparkplug command is offered unless you
  decide otherwise (Q1). If writes come, they go through the BACnet priority array at a low
  priority, are logged, and are never available to agents D011 D066.
- **Transport.** BACnet/SC or TLS for live building links, and IEC 62443 zones and conduits as
  the vocabulary for documenting a connection D066 D162.
- **Occupancy.** Aggregated at space level, never per person. Retention is bounded, the cache can
  be purged, and occupancy is excluded from model files unless exported on purpose (GDPR guidance
  D163).
- **Credentials.** In the OS keyring, as for FileEngine and the remote analysis servers.

## 8. Licence cautions (from the research)

- **Network only, never linked or bundled:**
  - AGPL-3.0: Grafana and FIWARE Orion/Orion-LD;
  - BSL-1.1: EMQX from 5.9 and ThingsBoard from 4.4;
  - the Timescale License: TimescaleDB `tsl/`;
  - LGPL-3.0 FROST-Server and GPL-3.0 urban-model-platform: run as servers only.
- **Data, not code:** Haystack/Xeto libraries are AFL-3.0, which is GPL-incompatible. Read them as
  data or generate them; never vendor them.
- **Out of GPL binaries:** EPL-2.0 (Ditto, Tahu, Sparkplug), unless a GPL secondary licence is
  declared.
- **GPL only:** bacnet-stack (GPL-2.0+ with a linking exception) may be used in GPL code, but
  never in the MIT interface crates. Prefer permissive stacks for those: BACpypes (MIT, as a
  sidecar) and open62541 (MPL-2.0).
- **Documents:** cite IFC documentation (CC BY-ND) and the IFC5 examples; never copy them.

## 9. Questions for the owner

1. **Writing to the BMS.** Read-only only, or later supervised writes (setpoints and schedules
   through the BACnet priority array, logged, never by agents)?
2. **Where bindings live.** In the model (`Reviera_PointBinding` and RXD), or a shared registry
   on FileEngine that models reference, or both, with the registry as master?
3. **First semantic target.** Brick first (published, permissive, has an IFC alignment), Haystack 5,
   or wait for ASHRAE 223P (public review 2025, publication expected end of 2026)?
4. **Time-series storage.** Connect only to external TSDBs (InfluxDB 3 Core, Timescale/PostgreSQL,
   Prometheus, QuestDB), or also ship a small embedded store for the cache?
5. **Occupancy privacy.** Space-level aggregation as a hard rule, or a minimum group size
   (zone/floor) configurable per project?
6. **Newer IFC georeferencing.** IfcMapConversionScaled, IfcRigidOperation and IfcGeographicCRS
   are labelled IFC4X3_ADD1 in the repository. Adopt them now, or keep IFC 4.3 core entities until
   their release status is clear?
7. **First city format.** CityJSON 2.0 (lighter, CC0, easy extension) or CityGML 3.0 (Dynamizer
   for live data), with 3D Tiles for viewers either way?
8. **Always-on parts.** Should remote analysis servers host connectors, scheduled calibration and
   city scenario runs, so the twin keeps updating when no one has Reviera open?
9. **Scope of phase 1.** Commercial buildings first (as with greenhouses), with residential and
   city-scale later?

## 10. Sources

See `research/12_digital_twin_smart_city.md` §10 for D001–D187; §9 there lists the sites that
refused automated access and what was used instead.
