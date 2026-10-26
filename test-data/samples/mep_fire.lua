-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Fire protection
-- description: An office floor with a corridor and a store room behind EI 60 walls and FD30S doors: a wet system of pendent heads under the ceilings and sidewall heads in the corridor, a dry system of upright heads in the unceiled store, valves on both risers, a fire damper where a duct crosses the corridor wall, and a fire alarm circuit of smoke detectors and call points.
--
-- Reviera sample model (test-data/samples): fire-rated wall and door types (Fire Rating, written
-- as Pset_WallCommon / Pset_DoorCommon FireRating); sprinkler heads by mounting (MEP-PIPE-007):
-- pendent heads hung from the office ceiling, sidewall heads on the corridor wall's face, upright
-- heads 3 m up in the store, which has no ceiling; each head type's K-factor, rating, response
-- and coverage set with sprinkler_type.set (the bulb colours follow the ratings: red 68 °C,
-- orange 57 °C, green 93 °C); a wet riser and main with branch lines teed in as they are drawn,
-- and a dry riser with its own branches, each head joined by Connect Into; an isolating and a
-- check valve on each riser; the Fire Protection Wet and Dry systems. A duct through the
-- corridor wall with a fire damper in its plane, the wall cut by it (MEP-DUCT-004); openings are made where runs pass
-- through walls and ceilings (MEP-PEN-001). Smoke detectors and manual call points on a fire
-- alarm circuit from a panel named FACP (MEP-ELEC-002).

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
local function type_named(name, class)
  for _, t in ipairs(reviera.type.list()) do
    if (name == nil or t.name == name) and (class == nil or t.class == class) then return t.id end
  end
end
local function wall(a, b, ty)
  return reviera.wall.create{ type_id = ty, curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 3.6 } }
end

-- The floor: 16 × 8 m on the walls' centrelines. Office (x 0–10, y 0–6), corridor north of it
-- (y 6–8), store room east of both (x 10–16).
local c = { {0, 0}, {16, 0}, {16, 8}, {0, 8} }
local walls = {}
for i = 1, 4 do
  walls[i] = wall(c[i % 4 + 1], c[i])
end
reviera.door.place{ host_wall_id = walls[4], point_along = 7.0 }

-- Fire-rated walls round the corridor and store, and fire doors in them.
local fire_wall = reviera.type.duplicate{ type_id = type_named("Generic - 200mm", "IfcWallType"), new_name = "Fire Wall 200 EI 60" }
local store_wall = wall({ 10, 0 }, { 10, 8 }, fire_wall)
local corridor_wall = wall({ 0, 6 }, { 10, 6 }, fire_wall)
reviera.param.set{ element_ids = { store_wall }, param_ref = "type.fire_rating", value = "EI 60" }
local fire_door = reviera.type.duplicate{ type_id = type_named("Single Door 900 x 2100"), new_name = "Fire Door 900 x 2100 FD30S" }
local d1 = reviera.door.place{ host_wall_id = corridor_wall, point_along = 8.5, type_id = fire_door }
reviera.door.place{ host_wall_id = store_wall, point_along = 7.0, type_id = fire_door }
reviera.param.set{ element_ids = { d1 }, param_ref = "type.fire_rating", value = "FD30S" }

-- Ceilings in the office (2.7 m) and corridor (2.6 m); the store is open to the deck.
reviera.ceiling.create_auto{ level_id = l1, point = { 5, 3 }, offset = 2.7 }
reviera.ceiling.create_auto{ level_id = l1, point = { 5, 7 }, offset = 2.6 }

-- Head types: office pendents K 80, 68 °C quick response, 12 m²; corridor sidewalls K 80, 57 °C,
-- 9 m²; store uprights K 115, 93 °C standard response, 9 m², on the dry system.
local t = reviera.mep_type.load_starters{ names = { "Pendent Sprinkler", "Upright Sprinkler", "Sidewall Sprinkler", "Smoke Detector", "Manual Call Point", "Distribution Board" } }
reviera.sprinkler_type.set{ type_id = t["Pendent Sprinkler"], k_factor = 80.0, activation_temperature = 68.0, response = "quick", activation = "bulb", coverage_area = 12.0 }
reviera.sprinkler_type.set{ type_id = t["Sidewall Sprinkler"], k_factor = 80.0, activation_temperature = 57.0, response = "quick", activation = "bulb", coverage_area = 9.0 }
reviera.sprinkler_type.set{ type_id = t["Upright Sprinkler"], k_factor = 115.0, activation_temperature = 93.0, response = "standard", activation = "bulb", coverage_area = 9.0 }
reviera.connector.set{ element_id = t["Upright Sprinkler"], connector_index = 0, classification = "fire_protection_dry" }

local pieces = {}
local function keep(run)
  for _, s in ipairs(run.segments) do pieces[#pieces + 1] = s end
  return run
end
local function pipe(points, d, system)
  return keep(reviera.pipe.create{ points = points, level = l1, diameter = d, system_type_id = system })
end
-- Connect Into takes the nearest level line the head's connector can reach (each tee splits a
-- line, so its pieces are tried, nearest first).
local function join(head)
  local port = reviera.connector.list{ element_id = head }[1]
  local p, d = port.position, port.direction
  local r = { p[1], p[2] }
  if math.abs(d[3]) < 0.5 then r = { p[1] + 0.3 * d[1], p[2] + 0.3 * d[2] } end
  local near = {}
  for _, s in ipairs(pieces) do
    local ends = reviera.connector.list{ element_id = s }
    local a, b = ends[1].position, ends[2].position
    local ux, uy = b[1] - a[1], b[2] - a[2]
    local l = math.sqrt(ux * ux + uy * uy)
    local reach = math.abs(d[3]) < 0.5 or d[3] * (a[3] - p[3]) > 0
    if l > 1e-6 and math.abs(a[3] - b[3]) < 1e-6 and reach then
      local t = ((r[1] - a[1]) * ux + (r[2] - a[2]) * uy) / l
      if t > 0 and t < l then
        near[#near + 1] = { s, math.abs((r[1] - a[1]) * uy - (r[2] - a[2]) * ux) / l }
      end
    end
  end
  table.sort(near, function(x, y) return x[2] < y[2] end)
  local why = {}
  for _, n in ipairs(near) do
    local ok, rep = pcall(reviera.mep.connect_into, { element_id = head, connector_index = 0, target_id = n[1] })
    if ok then
      keep(rep)
      return
    end
    why[#why + 1] = tostring(rep)
  end
  error("could not connect " .. tostring(head) .. ": " .. table.concat(why, "; "))
end

-- The wet riser in the store, its main west along the corridor at 3.0 m, a cross main into the
-- office and two branch lines there; each run starting or ending on another's side is teed in.
local WET = "fire_protection_wet"
pipe({ { 0.5, 1.5, 3.0 }, { 9.5, 1.5, 3.0 } }, 0.04, WET)
local riser = pipe({ { 10.6, 7.3, 0.3 }, { 10.6, 7.3, 3.0 }, { 0.6, 7.3, 3.0 } }, 0.065, WET)
pipe({ { 9.2, 7.3, 3.0 }, { 9.2, 1.5, 3.0 } }, 0.05, WET)
pipe({ { 0.5, 4.5, 3.0 }, { 9.2, 4.5, 3.0 } }, 0.04, WET)
local wet_valve = reviera.pipe_accessory.place{ kind = "butterfly_valve", point = { 10.6, 7.3, 0.9 } }.accessory
reviera.pipe_accessory.place{ kind = "check_valve", point = { 10.6, 7.3, 1.6 } }

local wet = {}
for _, y in ipairs({ 1.5, 4.5 }) do
  for _, x in ipairs({ 1.25, 3.75, 6.25, 8.75 }) do
    local h = reviera.sprinkler.place{ type_id = t["Pendent Sprinkler"], point = { x, y }, level = l1 }
    join(h)
    wet[#wet + 1] = h
  end
end
-- Sidewall heads on the corridor face of its wall, fed through it from the office's line.
for _, x in ipairs({ 2.5, 7.0 }) do
  local h = reviera.sprinkler.place{ type_id = t["Sidewall Sprinkler"], point = { x, 6.15 }, level = l1 }
  join(h)
  wet[#wet + 1] = h
end
reviera.system.create{ kind = "piping", element_ids = wet, system_type_id = WET, name = "Wet System" }

-- The dry riser in the store and two branch lines at 2.7 m under the upright heads.
local DRY = "fire_protection_dry"
pipe({ { 11.6, 0.8, 2.7 }, { 11.6, 7.2, 2.7 } }, 0.04, DRY)
pipe({ { 14.4, 0.8, 2.7 }, { 14.4, 7.2, 2.7 } }, 0.04, DRY)
pipe({ { 11.6, 3.5, 2.7 }, { 14.4, 3.5, 2.7 } }, 0.05, DRY)
pipe({ { 15.4, 4.0, 0.3 }, { 15.4, 4.0, 2.7 }, { 14.4, 4.0, 2.7 } }, 0.065, DRY)
reviera.pipe_accessory.place{ kind = "gate_valve", point = { 15.4, 4.0, 0.9 } }
reviera.pipe_accessory.place{ kind = "check_valve", point = { 15.4, 4.0, 1.6 } }
local dry = {}
for _, x in ipairs({ 11.6, 14.4 }) do
  for _, y in ipairs({ 2.0, 6.0 }) do
    local h = reviera.sprinkler.place{ type_id = t["Upright Sprinkler"], point = { x, y }, level = l1 }
    join(h)
    dry[#dry + 1] = h
  end
end
reviera.system.create{ kind = "piping", element_ids = dry, system_type_id = DRY, name = "Dry System" }

-- A 400 × 250 duct at 3.35 m from the office into the corridor, a fire damper in the plane of
-- the corridor wall. The duct's opening is made for it, the damper's is not: the wall is cut
-- by the damper (View › Check Overlaps, Cut).
local duct = reviera.duct.create{ points = { { 5.0, 2.5, 3.35 }, { 5.0, 7.5, 3.35 } }, level = l1, shape = "rectangular", width = 0.4, height = 0.25 }
local damper = reviera.duct_accessory.place{ kind = "fire_damper", duct_id = duct.segments[1], point = { 5.0, 6.0 } }.accessory
for _, o in ipairs(reviera.overlap.find{ element_ids = { damper } }.overlaps) do
  reviera.overlap.resolve{ a = o.a, b = o.b, mode = (o.a == damper) and "cut_b" or "cut_a" }
end

-- Fire alarm: the detectors and call points (24 V fire alarm connectors), a panel FACP in the
-- corridor, one circuit.
local facp = reviera.electrical_equipment.place{ type_id = t["Distribution Board"], point = { 1.6, 7.75 }, level = l1 }
reviera.panel.set{ panel_id = facp, name = "FACP" }
local alarm = {}
for _, p in ipairs({ { 2.5, 3.0, 2.65 }, { 7.5, 3.0, 2.65 }, { 3.5, 7.0, 2.55 }, { 8.0, 7.0, 2.55 }, { 13.0, 5.0, 3.3 } }) do
  alarm[#alarm + 1] = reviera.device.place{ type_id = t["Smoke Detector"], point = { p[1], p[2] }, level = l1, offset = p[3], work_plane = true }
end
for _, p in ipairs({ { 0.15, 6.4 }, { 10.15, 6.2 } }) do
  alarm[#alarm + 1] = reviera.device.place{ type_id = t["Manual Call Point"], point = p, level = l1, offset = 1.4 }
end
local circuit = reviera.circuit.create{ kind = "fire_alarm", element_ids = alarm }.circuit
reviera.circuit.select_panel{ circuit_id = circuit, panel_id = facp }

label(0.3, -1.0, "Office: 8 pendent heads (K 80, 68 °C red bulbs) under the 2.7 m ceiling, wet")
label(0.3, -1.6, "Smoke detectors and call points on fire alarm circuit FACP-1")
label(10.3, -1.0, "Store: no ceiling, 4 upright heads (K 115, 93 °C green), dry system")
label(10.3, -1.6, "Risers: isolating and check valves")
label(0.3, 8.6, "Corridor: 2 sidewall heads (57 °C orange); EI 60 walls, FD30S doors")
label(5.3, 9.2, "Fire damper where the duct crosses the corridor wall")
