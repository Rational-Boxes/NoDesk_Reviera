-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Analysis tools
-- description: A small plant room with a steel canopy over its exit: the canopy's analytical model with a roof panel and its rooflight opening, fixed supports, loads and a combination ready for Run Analysis; a path of travel from the room's far corner out through the exit door; a supply air system sized by Equal Friction and its pressure drop calculated; a distribution board's panel schedule.
--
-- Riviera sample model (test-data/samples): the Analyze tab's tools in one small building:
-- - a 12 × 8 m room, walls 4 m high, an exit door in the east wall;
-- - a steel canopy over the exit (IBC; AISC shapes): four W8X31 columns on a 4 × 6 m bay to a
--   Canopy level at 3.6 m, W12X26 beams round it; its analytical model made from the physical
--   one (STR-AM), a roof panel by boundary on the beams with a 1.5 × 1.5 m rooflight opening,
--   fixed column bases, dead (with self-weight) and roof live cases with area loads on the panel,
--   wind at a column top, and one strength combination. Analyze › Structural Analysis › Run
--   Analysis, then the Results Explorer;
-- - Route Analysis (ANA-ROUTE-001/002): route settings (1.2 m/s, 0.3 m clearance) and a path of
--   travel from the room's north-west corner out through the exit door to the canopy;
-- - an air handling unit and a supply main with three diffusers at 100 L/s on branches, sized by
--   Equal Friction at 1 Pa/m (MEP-CALC-001), then its pressure drop calculated (MEP-CALC-002):
--   Analyze › Pressure Drop › Analytical Segments lists the critical path; a duct legend
--   (Color Fill, MEP-DUCT-010) colours the plan's ducts by velocity;
-- - a distribution board LP1 with two receptacle circuits, and its panel schedule (MEP-ELEC).

riviera.code.set{ code = "ibc" }
local l1 = riviera.level.list()[1].id
local canopy = riviera.level.create{ elevation = 3.6, name = "Canopy" }
riviera.view.create_plan{ level_ids = { l1, canopy } }
riviera.view.create_3d{}
local plan
for _, v in ipairs(riviera.view.list()) do
  if v.name == "Level 1" and v.kind ~= "3d" then plan = v.id end
end
local function label(x, y, text)
  riviera.text.create{ view_id = plan, position = { x, y }, content = text }
end
local function wall(a, b)
  return riviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 4.0 } }
end

-- The room: 12 × 8 m on the walls' centrelines, the exit door in the east wall at y = 4 m.
local c = { {0, 0}, {12, 0}, {12, 8}, {0, 8} }
local walls = {}
for i = 1, 4 do
  walls[i] = wall(c[i % 4 + 1], c[i])
end
riviera.door.place{ host_wall_id = walls[2], point_along = 4.0 }

-- The steel canopy over the exit: four columns on a 4 × 6 m bay, beams round it at 3.6 m.
local column = riviera.column.load_section{ catalog = "aisc_w", designation = "W8X31" }
local beam = riviera.beam.load_section{ catalog = "aisc_w", designation = "W12X26" }
local corners = { { 12.6, 1.0 }, { 16.6, 1.0 }, { 16.6, 7.0 }, { 12.6, 7.0 } }
for _, p in ipairs(corners) do
  riviera.column.place{ type_id = column, point = p, base_level = l1, top_constraint = { level = canopy } }
end
for i = 1, 4 do
  riviera.beam.create{ type_id = beam, curve = { start = corners[i], ["end"] = corners[i % 4 + 1] }, reference_level = canopy }
end

-- Its analytical model, a roof panel on the beams with a rooflight, supports, loads and a
-- combination.
riviera.analytical_automation.physical_to_analytical{}
local roof = riviera.analytical_panel.create_by_boundary{
  boundary = { { 12.6, 1.0, 3.6 }, { 16.6, 1.0, 3.6 }, { 16.6, 7.0, 3.6 }, { 12.6, 7.0, 3.6 } },
  thickness = 0.1, material = "Concrete f'c 4000 psi",
}
riviera.analytical_opening.create{
  panel_id = roof, boundary = { { 13.85, 3.25, 3.6 }, { 15.35, 3.25, 3.6 }, { 15.35, 4.75, 3.6 }, { 13.85, 4.75, 3.6 } },
}
riviera.boundary_condition.at_column_bases{ state = "fixed" }
local dead = riviera.load_case.create{ name = "D", nature = "D", self_weight = true }
local roof_live = riviera.load_case.create{ name = "Lr", nature = "Lr" }
local wind = riviera.load_case.create{ name = "W", nature = "Wind" }
riviera.load.create_area{ host_id = roof, case_id = dead, value = { 0, 0, -1000 } }
riviera.load.create_area{ host_id = roof, case_id = roof_live, value = { 0, 0, -1000 } }
-- Wind along x at the north-west column's top, and one strength combination.
for _, m in ipairs(riviera.analytical_model.info{}.members) do
  if m.role == "column" and m.start[1] < 13 and m.start[2] > 6 then
    riviera.load.create_point{ host_id = m.nodes[2], case_id = wind, forces = { 5000, 0, 0, 0, 0, 0 } }
  end
end
riviera.load_combination.create{ name = "U1", formula = "1.2*D + 1.6*Lr + 0.5*W" }

-- Route analysis: the settings, then a path of travel from the far (north-west) corner out
-- through the exit door to the canopy.
riviera.route_settings.set{ walking_speed = 1.2, clearance = 0.3 }
riviera.path_of_travel.create{ view_id = plan, start = { 0.6, 7.4 }, ["end"] = { 13.6, 4.0 } }

-- The air system: the unit in the south-west corner, its 400 mm supply main up to 3.5 m and
-- east, three diffusers at 100 L/s on 250 mm branches off it (tees where they join).
local t = riviera.mep_type.load_starters{ names = { "Air Handling Unit", "Supply Diffuser 600", "Distribution Board", "Receptacle" } }
local ahu = riviera.mech_equipment.place{ type_id = t["Air Handling Unit"], point = { 2.0, 1.2 }, level = l1 }
local diffusers = {}
for _, x in ipairs({ 6.0, 8.0, 10.0 }) do
  diffusers[#diffusers + 1] = riviera.air_terminal.place{ type_id = t["Supply Diffuser 600"], point = { x, 4.0 }, level = l1, offset = 2.7, flow = 0.1 }.id
end
local sys = riviera.system.create{ kind = "duct", element_ids = diffusers, name = "Supply Air" }
riviera.system.select_equipment{ system_id = sys, equipment_id = ahu }
riviera.duct.create{ points = { { 3.5, 1.2, 0.9 }, { 5.0, 1.2, 0.9 }, { 5.0, 1.2, 3.5 }, { 10.6, 1.2, 3.5 } }, level = l1, shape = "round", diameter = 0.4, system_type_id = "supply_air" }
for _, x in ipairs({ 6.0, 8.0, 10.0 }) do
  riviera.duct.create{ points = { { x, 1.2, 3.5 }, { x, 4.0, 3.5 }, { x, 4.0, 2.85 } }, level = l1, shape = "round", diameter = 0.25, system_type_id = "supply_air" }
end
-- The main's far end capped.
riviera.duct.cap_open_ends{}
-- Sized by Equal Friction (1 Pa/m), then the pressure drop calculated.
riviera.mep.size{ system_id = sys, method = "equal_friction" }
riviera.mep_network.calculate{ system_id = sys }
-- A duct legend in the plan: the ducts coloured by velocity, in bands at 3.5 and 4 m/s.
local scheme = riviera.colorscheme.create{ name = "Duct Velocity", category = "cat.ducts", parameter = "Velocity", mode = "range", ranges = { 0, 3.5, 4 } }
riviera.colorscheme.set_rows{ scheme_id = scheme, include_fittings = false, rows = {
  { at_least = 0, color = "#2b83ba" }, { at_least = 3.5, color = "#abdda4" }, { at_least = 4, color = "#d7191c" },
} }
riviera.mep_color_legend.place{ view_id = plan, kind = "duct", scheme_id = scheme, point = { -4.5, 7.5 } }

-- The distribution board on the north wall, two receptacle circuits, its panel schedule.
local lp1 = riviera.electrical_equipment.place{ type_id = t["Distribution Board"], point = { 6.0, 7.75 }, level = l1 }
riviera.panel.set{ panel_id = lp1, name = "LP1" }
for _, group in ipairs({ { { 1.5, 7.85 }, { 3.5, 7.85 } }, { { 8.5, 7.85 }, { 10.5, 7.85 } } }) do
  local ids = {}
  for _, p in ipairs(group) do
    ids[#ids + 1] = riviera.device.place{ type_id = t["Receptacle"], point = p, level = l1, offset = 0.3 }
  end
  local r = riviera.circuit.create{ kind = "power", element_ids = ids }.circuit
  riviera.circuit.select_panel{ circuit_id = r, panel_id = lp1 }
end
riviera.panel_schedule.create{ panel_ids = { lp1 } }

label(12.8, 7.6, "Canopy: analytical model, roof panel with a rooflight opening, fixed bases")
label(12.8, 0.2, "Cases D, Lr, W and combination U1: Run Analysis")
label(0.6, 6.6, "Path of travel to the exit at 1.2 m/s")
label(3.6, 2.2, "Supply main sized by Equal Friction, pressure drop calculated")
label(4.6, 7.0, "LP1: panel schedule, circuits LP1-1 and LP1-2")
