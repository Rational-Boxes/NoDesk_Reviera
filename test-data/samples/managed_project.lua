-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Managed project
-- description: A small office and store set up with the Manage tab's project-wide tools: units and code, global parameters driving a dimension and parameters, a shared parameter, Uniclass codes, keynotes, ADA clearances, saved selections, the project location and MEP settings.
--
-- Reviera sample model (test-data/samples): the Manage tab's settings on a one-room office with
-- a store. Model Properties (Metric (mm) with a space between digit groups, the IBC, ADA 2010
-- door clearances); global parameters (DM-GPAR-001): StoreWidth labels a dimension and moves
-- the partition to 3600 mm from the east wall, OfficeWidth reports the other side (8400 mm),
-- OfficeArea is a formula over it, SillHeight drives the windows' sills and PartitionAcoustic the
-- partition's shared Acoustic Rating; a shared parameter with its library's identity embedded
-- (DM-SPAR-001); Uniclass 2015 element codes on the walls, doors and windows (DM-CLS-001);
-- a keynote table with element and user keynotes on the plan; two saved selections written as IfcGroups (ARC-SEL-002); the location,
-- true north, survey point and CRS (DM-LOC-001); an Office space type and a copper pipe segment
-- (MEP-SET-001, MEP-SET-004).

local l1 = reviera.level.list()[1].id
reviera.view.create_plan{ level_ids = { l1 } }
reviera.view.create_3d{}
local plan
for _, v in ipairs(reviera.view.list()) do
  if v.name == "Level 1" and v.kind ~= "3d" then plan = v.id end
end
local function label(x, y, text)
  reviera.text.create{ view_id = plan, position = { x, y }, content = text }
end
local function wall(a, b)
  return reviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 3.0 } }
end

-- Model Properties: units, number format, building code and accessibility rules.
reviera.units.set_preset{ preset = "metric_mm" }
reviera.units.set_number_format{ decimal_symbol = ".", digit_grouping = " " }
reviera.code.set{ code = "ibc", occupancy = "other" }
reviera.clearance.set_rule_set{ standard = "ada_2010" }

-- The building: 12 × 8 m on the walls' centrelines, clockwise; a partition at 8 m for now (the
-- StoreWidth global moves it).
local c = { {0, 0}, {12, 0}, {12, 8}, {0, 8} }
local outer = {}
for i = 1, 4 do
  outer[i] = wall(c[i % 4 + 1], c[i])
end
local partition = wall({ 8, 0 }, { 8, 8 })
local doors = {
  reviera.door.place{ host_wall_id = outer[1], point_along = 6.5 },
  reviera.door.place{ host_wall_id = partition, point_along = 4.0 },
}
local windows = {
  reviera.window.place{ host_wall_id = outer[1], point_along = 9.5 },
  reviera.window.place{ host_wall_id = outer[3], point_along = 2.0 },
  reviera.window.place{ host_wall_id = outer[3], point_along = 5.5 },
  reviera.window.place{ host_wall_id = outer[3], point_along = 10.2 },
}

-- A shared parameter from the practice's parameter library: its GUID and the library's id, name
-- and version are kept in the project, so it needs no library file.
local acoustic = "7c3e9a14-2b6d-4f80-9a1e-5d2c8b4f6a03"
reviera.param.project.add({
  def = { name = "Acoustic Rating", data_type = "text", tooltip = "Sound insulation, as the specification gives it",
    guid = acoustic, library = { id = "4a8f2c6e-1d3b-4e7a-8c5f-9b0e2d4a6c18", name = "Practice Parameters", version = "1.0.0" } },
  categories = { "cat.walls" }, is_instance = true,
})
reviera.param.set{ element_ids = outer, param_ref = acoustic, value = "Rw 50 dB" }

-- Global parameters. StoreWidth labels the dimension from the east wall to the partition: the
-- partition moves to 3600 mm from it. OfficeWidth reports the dimension on the other side.
local axis = function(w, x, y) return { element = w, sub = "axis", point = { x, y } } end
local store_dim = reviera.dim.create_aligned{ view_id = plan, refs = { axis(outer[2], 12, 6), axis(partition, 8, 6) }, line_point = { 10, 6 } }
local office_dim = reviera.dim.create_aligned{ view_id = plan, refs = { axis(outer[4], 0, 6), axis(partition, 8, 6) }, line_point = { 4, 6 } }
reviera.global.create{ name = "StoreWidth", data_type = "length", value = 3.6, group = "Layout", tooltip = "The store, east wall to partition" }
reviera.global.label_dimension{ global_id = "StoreWidth", dimension_id = store_dim }
reviera.global.create{ name = "OfficeWidth", data_type = "length", reporting = true, group = "Layout", tooltip = "Measured: west wall to partition" }
reviera.global.label_dimension{ global_id = "OfficeWidth", dimension_id = office_dim }
reviera.global.create{ name = "OfficeArea", data_type = "area", formula = "OfficeWidth * 8 m", group = "Layout", tooltip = "On the walls' centrelines" }
-- SillHeight drives every window's sill; PartitionAcoustic the partition's Acoustic Rating.
reviera.global.create{ name = "SillHeight", data_type = "length", value = 1.0, group = "Openings" }
reviera.global.associate{ global_id = "SillHeight", element_ids = windows, param_ref = "insert.sill" }
reviera.global.create{ name = "PartitionAcoustic", data_type = "text", value = "Rw 45 dB", group = "Acoustics" }
reviera.global.associate{ global_id = "PartitionAcoustic", element_ids = { partition }, param_ref = acoustic }

-- Classification: the Uniclass 2015 element codes used here, walls, doors and windows classified.
reviera.classification.create{ name = "Uniclass 2015", source = "NBS", location = "https://uniclass.thenbs.com", items = {
  { code = "EF_25", title = "Wall and barrier elements" },
  { code = "EF_25_10", parent = "EF_25", title = "Walls" },
  { code = "EF_25_30", parent = "EF_25", title = "Doors and windows" },
} }
local walls = { outer[1], outer[2], outer[3], outer[4], partition }
reviera.classification.assign{ element_ids = walls, system_id = "Uniclass 2015", code = "EF_25_10" }
local openings = { doors[1], doors[2], windows[1], windows[2], windows[3], windows[4] }
reviera.classification.assign{ element_ids = openings, system_id = "Uniclass 2015", code = "EF_25_30" }

-- Keynotes: the table Load Keynotes reads from a keynote file beside the project.
reviera.keynotes.set{ path = "keynotes.txt", entries = {
  { key = "08", text = "Openings" },
  { key = "08 11 13", text = "Hollow metal doors and frames", parent = "08" },
  { key = "08 51 13", text = "Aluminium windows", parent = "08" },
  { key = "09", text = "Finishes" },
  { key = "09 21 16", text = "Gypsum board assemblies", parent = "09" },
} }
-- Keynote tags (DOC-ANN-014): the door and window types carry their keys (Type Properties ›
-- Keynote), shown by element keynotes on the front door and the south window; the partition
-- has a user keynote picked from the table, the key alone.
reviera.param.set{ element_ids = { doors[1] }, param_ref = "type.keynote", value = "08 11 13" }
reviera.param.set{ element_ids = { windows[1] }, param_ref = "type.keynote", value = "08 51 13" }
reviera.keynote.create{ view_id = plan, host_ref = doors[1], position = { 7.0, -0.6 } }
reviera.keynote.create{ view_id = plan, host_ref = windows[1], position = { 3.6, -0.6 } }
reviera.keynote.create{ view_id = plan, mode = "user", key = "09 21 16", host_ref = partition, position = { 9.6, 2.4 }, style = "number" }

-- Saved selections, written on save as IfcGroups.
reviera.selectionset.save{ name = "Exterior walls", element_ids = outer }
reviera.selectionset.save{ name = "Doors and windows", element_ids = openings }
reviera.selectionset.set_export{ as_ifc_group = true }

-- The project location: place, latitude and longitude, true north 12° from project north, the
-- survey point at the origin with its UTM zone 10N coordinates.
reviera.location.set_all{ location = {
  place = "Portland, Oregon", latitude = 45.52, longitude = -122.68, elevation = 15.0,
  time_zone = -8.0, daylight_saving = true, true_north = 12.0,
  project_base_point = { 0, 0, 0 },
  survey_point = { point = { 0, 0, 0 }, shared = { 525000.0, 5040800.0, 15.0 } },
  crs = "EPSG:32610",
} }

-- MEP settings: an Office space type for the loads, and a copper pipe segment (ASTM B88 Type L).
reviera.space_type.create{ name = "Office", values = {
  lighting_load_density = 8.0, power_load_density = 10.0, area_per_person = 10.0,
  sensible_heat_per_person = 75.0, latent_heat_per_person = 55.0, outside_air_per_person = 10.0,
  heating_set_point = 20.0, cooling_set_point = 24.0,
} }
reviera.pipe_segment.create{ material_id = "Copper", schedule = "Type L", roughness = 0.0000015, sizes = {
  { nominal = 0.0127, inner = 0.013843, outer = 0.015875 },
  { nominal = 0.01905, inner = 0.019939, outer = 0.022225 },
  { nominal = 0.0254, inner = 0.026035, outer = 0.028575 },
} }

label(0.5, -1.4, "StoreWidth (3600) labels the store's dimension; OfficeWidth reports the office's")
label(0.5, 8.8, "SillHeight 1000 drives the windows; PartitionAcoustic the partition's Acoustic Rating")
