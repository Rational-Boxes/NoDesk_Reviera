-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Manor with tower
-- description: A two-storey house and a round three-storey tower with spiral stairs, shaped windows, layered walls, a curtain wall, columns, a shaft, roof windows, vaulted ceilings and a deck.
--
-- Reviera sample model (test-data/samples), a demonstration of most of what Reviera models:
-- - three levels with plans and a 3D view; the model follows the US residential code (IRC);
-- - a two-storey house: walls on each level, partitions, doors (an arched front door) and
--   windows (round-top, Gothic, an oculus), floors, rooms and automatic ceilings, a hipped roof
--   with eaves and a gable dormer with a round-top window, the walls attached;
-- - a straight stair drawn short that completes itself to Level 2, with railings, and openings
--   cut in the ceiling and floor over it for the code's headroom, guarded at the top;
-- - a round tower of curved walls with Gothic windows and an oculus, two spiral stairs (Level 1
--   to 2 and 2 to 3), each opening the floor above it, and an onion dome;
-- - a deck behind the house with a guard railing and a gate;
-- - layered exterior walls (render, insulation, block and plaster), a cased opening in the hall
--   partition, a glazed curtain wall onto the garden, a service shaft, a skylight in the roof,
--   roof windows and a roof hatch, vaulted ceilings upstairs following the roof and a domed one
--   in the tower top, and two round
--   porch columns (the second made by an array) attached to the eave.

reviera.code.set{ code = "irc", occupancy = "dwelling" }

local l1 = reviera.level.list()[1].id
local l2 = reviera.level.create{ elevation = 3.0 }
local l3 = reviera.level.create{ elevation = 6.0 }
local function wall(level, a, b, height, through)
  local curve = { line = { start = a, ["end"] = b } }
  if through then curve.through = through end
  return reviera.wall.create{
    curve = curve, base_level = level, top_constraint = { unconnected = height or 3.0 },
  }
end
local function square(x0, y0, x1, y1)
  local p = { {x0, y0}, {x1, y0}, {x1, y1}, {x0, y1} }
  local out = {}
  for i = 1, 4 do
    out[i] = { line = { start = p[i], ["end"] = p[i % 4 + 1] } }
  end
  return out
end
local function type_named(name)
  for _, t in ipairs(reviera.type.list()) do
    if t.name == name then return t.id end
  end
end

-- ---- The house: 14 × 9 m on the walls' centrelines, two storeys ----
local W, D = 14.0, 9.0
local c = { {0, 0}, {W, 0}, {W, D}, {0, D} }
local ground, upper = {}, {}
for i = 1, 4 do
  -- Clockwise, so each wall's exterior faces out.
  ground[i] = wall(l1, c[i % 4 + 1], c[i])
  upper[i] = wall(l2, c[i % 4 + 1], c[i])
end
-- ground[1] is the south wall (14,0)→(0,0), [2] east (14,9)→(14,0), [3] north, [4] west.
-- The exterior walls are layered (ARC-CMPD-001), 200 mm in all, exterior first.
local exterior = reviera.type.duplicate{ type_id = type_named("Generic - 200mm"), new_name = "Exterior - Render on Block 200" }
reviera.compound.set_layers{ type_id = exterior, layers = {
  { ["function"] = "finish1", material = "Lime Render", thickness = "15 mm" },
  { ["function"] = "thermal_air", material = "Wood Fibre Insulation", thickness = "60 mm" },
  { ["function"] = "structure", material = "Concrete Block", thickness = "110 mm" },
  { ["function"] = "finish2", material = "Gypsum Plaster", thickness = "15 mm" },
} }
local outer_walls = {}
for i = 1, 4 do
  outer_walls[#outer_walls + 1] = ground[i]
  outer_walls[#outer_walls + 1] = upper[i]
end
reviera.element.set_type{ element_ids = outer_walls, type_id = exterior }
-- Partitions stop under the floor above (its slab is 300 mm deep).
local hall = wall(l1, {7, 0}, {7, D}, 2.7)
local split = wall(l2, {7, 3.2}, {7, D})
-- A wide cased opening between the living room and the hall (ARC-OPEN-003).
reviera.opening.create_wall{ wall_id = hall, u0 = 1.6, u1 = 3.4, top_constraint = { unconnected = 2.2 } }

-- Doors and windows. The first of each makes the default types; the shaped ones are duplicates.
reviera.door.place{ host_wall_id = hall, point_along = 6.0 }
reviera.window.place{ host_wall_id = ground[3], point_along = 3.5 }
local arched_door = reviera.type.duplicate{ type_id = type_named("Single Door 900 x 2100"), new_name = "Arched Door 1000 x 2400" }
reviera.type.set_param{ type_id = arched_door, param = "width", value = 1.0 }
reviera.type.set_param{ type_id = arched_door, param = "height", value = 2.4 }
reviera.type.set_param{ type_id = arched_door, param = "shape", value = "round_top" }
local round_top = reviera.type.duplicate{ type_id = type_named("Window 1200 x 1200"), new_name = "Round Top 1000 x 1800" }
for _, p in ipairs({ {"width", 1.0}, {"height", 1.8}, {"default_sill", 0.6} }) do
  reviera.type.set_param{ type_id = round_top, param = p[1], value = p[2] }
end
reviera.type.set_param{ type_id = round_top, param = "shape", value = "round_top" }
reviera.type.set_param{ type_id = round_top, param = "muntins", value = 3 }
local gothic = reviera.type.duplicate{ type_id = round_top, new_name = "Gothic 800 x 1800" }
reviera.type.set_param{ type_id = gothic, param = "width", value = 0.8 }
reviera.type.set_param{ type_id = gothic, param = "head_profile", value = "gothic" }
local oculus = reviera.type.duplicate{ type_id = type_named("Window 1200 x 1200"), new_name = "Oculus 900" }
reviera.type.set_param{ type_id = oculus, param = "shape", value = "round" }
reviera.type.set_param{ type_id = oculus, param = "width", value = 0.9 }
reviera.type.set_param{ type_id = oculus, param = "muntins", value = 4 }

-- The south front (its wall runs from x = 14 to 0, so `point_along` counts from the east).
reviera.door.place{ type_id = arched_door, host_wall_id = ground[1], point_along = 10.5 }
for _, a in ipairs({ 12.8, 8.0, 4.5, 2.2 }) do
  reviera.window.place{ type_id = round_top, host_wall_id = ground[1], point_along = a }
end
for _, a in ipairs({ 12.0, 9.0, 5.0, 2.0 }) do
  reviera.window.place{ type_id = round_top, host_wall_id = upper[1], point_along = a, sill_height = 0.8 }
end
reviera.window.place{ type_id = oculus, host_wall_id = upper[1], point_along = 7.0, sill_height = 1.2 }
-- North, east and west.
reviera.window.place{ host_wall_id = ground[3], point_along = 10.5 }
reviera.window.place{ host_wall_id = upper[3], point_along = 3.5 }
reviera.window.place{ host_wall_id = upper[3], point_along = 10.5 }
reviera.window.place{ type_id = gothic, host_wall_id = ground[2], point_along = 4.5 }
reviera.window.place{ type_id = gothic, host_wall_id = ground[4], point_along = 4.5 }
reviera.window.place{ type_id = gothic, host_wall_id = upper[2], point_along = 4.5 }
reviera.window.place{ type_id = gothic, host_wall_id = upper[4], point_along = 4.5 }
-- A back door to the deck, swinging in (outward it would sweep the deck boards).
reviera.door.place{ host_wall_id = ground[3], point_along = 7.6, flip_facing = true }
-- A glazed screen onto the garden in the north wall (ARC-CW-001), cutting its own opening:
-- the default Storefront type, its panels at most 0.8 m wide, a transom at 2.1 m.
reviera.curtain.create{
  curve = { start = {4.3, D}, ["end"] = {6.9, D} }, base_level = l1,
  top_constraint = { unconnected = 2.5 },
}
local storefront = type_named("Storefront")
reviera.type.set_param{ type_id = storefront, param = "u_spacing", value = 0.8 }
reviera.type.set_param{ type_id = storefront, param = "v_spacing", value = 2.1 }

-- Floors: the ground floor, and Level 2 inside the walls.
reviera.floor.create{ loops = { square(0.1, 0.1, W - 0.1, D - 0.1) }, level_id = l1 }
local floor2 = reviera.floor.create{ loops = { square(0.1, 0.1, W - 0.1, D - 0.1) }, level_id = l2 }
-- Rooms and ceilings on Level 1.
reviera.room.place{ level_id = l1, point = {3.5, 4.5}, name = "Living" }
reviera.room.place{ level_id = l1, point = {10.5, 4.5}, name = "Hall and Kitchen" }
reviera.ceiling.create_auto{ level_id = l1, point = {3.5, 4.5} }
reviera.ceiling.create_auto{ level_id = l1, point = {10.5, 4.5} }

-- The main stair, drawn short along the hall's south wall: it completes itself to Level 2
-- (the last run carries on), its railings with it.
local stair = reviera.stair.create{
  base_level = l1, width = 1.1,
  runs = { { start = {8.0, 1.0}, ["end"] = {9.5, 1.0} } },
}
-- Open the ceiling and the floor over it for the model's code (IRC: 6 ft 8 in), and guard the
-- open edge at the top.
local check = reviera.stair.headroom_check{ stair_id = stair }
reviera.stair.create_openings{ stair_id = stair }
for _, o in ipairs(check.obstructions) do
  if o.host == floor2 then
    local xmin, xmax, ymax = math.huge, -math.huge, -math.huge
    for _, p in ipairs(o.loops[1]) do
      xmin, xmax, ymax = math.min(xmin, p[1]), math.max(xmax, p[1]), math.max(ymax, p[2])
    end
    reviera.railing.create{ path = { {xmin, ymax + 0.05}, {xmax, ymax + 0.05} }, base_level = l2 }
  end
end
reviera.room.place{ level_id = l2, point = {3.5, 4.5}, name = "Bedroom" }
reviera.room.place{ level_id = l2, point = {10.5, 6.0}, name = "Study" }
-- A service shaft through the Level 2 floor, from Level 1 up to Level 2 (ARC-OPEN-004).
reviera.opening.create_shaft{
  loops = { { {12.6, 7.9}, {13.6, 7.9}, {13.6, 8.6}, {12.6, 8.6} } },
  base_level = l1, top_constraint = { level = l2 },
}

-- The hipped roof on the walls' exterior faces, with eaves, a gable dormer to the south, and
-- the upper walls attached.
local o = 0.1
local f = { {-o, -o}, {W + o, -o}, {W + o, D + o}, {-o, D + o} }
local curves = {}
for i = 1, 4 do
  curves[i] = { line = { start = f[i], ["end"] = f[i % 4 + 1] } }
end
local roof = reviera.roof.create_footprint{ curves = curves, level_id = l1, base_offset = 6.0 }
reviera.roof.set_eaves{ roof_id = roof, overhang = 0.6, fascia = true }
reviera.dormer.create{ roof_id = roof, point = {7.0, 0.6}, style = "gable", front_height = 0.8 }
-- A round-top window in the dormer's front wall (found by its point), as the owner added it in
-- the window: placed at the type's sill, it fits itself onto the wall's open face above the
-- main roof and under the dormer roof.
-- (The point is off the hall partition below; the window then goes to the front's middle.)
local dormer_window = reviera.window.place{ type_id = round_top, point = {7.4, 0.7} }
reviera.insert.set_position{ element_id = dormer_window, along = 0.8 }
-- A vaulted ceiling upstairs, shaped to the roof's underside (ARC-CEIL-001): the bedroom and
-- study are open to each other south of the partition, so one ceiling covers both.
reviera.ceiling.create_auto{ level_id = l2, point = {3.5, 4.5}, follow_roof = true }
-- A roof hatch over the study, cut plumb through the north slope (ARC-OPEN-002).
reviera.opening.create_vertical{ host_id = roof, loops = { { {9.6, 6.6}, {10.8, 6.6}, {10.8, 7.6}, {9.6, 7.6} } } }
-- Two roof windows in the south slope, either side of the dormer, lighting the vaulted rooms.
for _, x in ipairs({ 3.5, 10.5 }) do
  reviera.skylight.create{ roof_id = roof, point = { x, 2.2 } }
end
-- ---- Two castle towers on the front corners, built onto the house ----
-- Octagons with their facets square to the house; the facet against the house is left out, so
-- the tower's walls end on the house's side wall and a door leads straight in; where their
-- ends meet the house's walls at an angle they are cut by them (a boolean, ARC-MOD-028). Three
-- storeys and a battlement walk behind a parapet of merlons and crenels.
local TH = 9.4
local A = 2.4                                -- the octagon's apothem, to the walls' centrelines
local RC = A / math.cos(math.pi / 8)         -- to its corners
local function oct(cx, cy, r, k)              -- corner k, the facets axis-aligned
  local a = math.pi / 8 + k * math.pi / 4
  return { cx + r * math.cos(a), cy + r * math.sin(a) }
end
local function oct_loop(cx, cy, apothem)
  local r, out = apothem / math.cos(math.pi / 8), {}
  for k = 0, 7 do
    out[#out + 1] = { line = { start = oct(cx, cy, r, k), ["end"] = oct(cx, cy, r, k + 1) } }
  end
  return out
end
-- `open` is the facet left out (0 east … facet k runs from corner k to k + 1).
local function castle_tower(cx, cy, open)
  local walls = {}
  for k = 0, 7 do
    if k ~= open then
      -- Clockwise, so the exterior faces out.
      walls[k] = reviera.wall.create{
        curve = { line = { start = oct(cx, cy, RC, k + 1), ["end"] = oct(cx, cy, RC, k) } },
        base_level = l1, top_constraint = { unconnected = TH - 0.3 },
      }
    end
  end
  for _, l in ipairs({ l1, l2, l3 }) do
    reviera.floor.create{ loops = { oct_loop(cx, cy, A - 0.1) }, level_id = l }
  end
  -- The battlement walk, and the parapet on the seven outer facets.
  reviera.floor.create{ loops = { oct_loop(cx, cy, A + 0.1) }, level_id = l3, offset = TH - 6.0 }
  for k = 0, 7 do
    if k ~= open then
      local a, b = oct(cx, cy, RC, k + 1), oct(cx, cy, RC, k)
      local dx, dy = b[1] - a[1], b[2] - a[2]
      local p = function(t) return { a[1] + dx * t, a[2] + dy * t } end
      for _, piece in ipairs({ {0, 0.2, 1.2}, {0.2, 0.4, 0.55}, {0.4, 0.6, 1.2}, {0.6, 0.8, 0.55}, {0.8, 1.0, 1.2} }) do
        reviera.wall.create{
          curve = { line = { start = p(piece[1]), ["end"] = p(piece[2]) } },
          base_level = l3, base_offset = TH - 6.0, top_constraint = { unconnected = piece[3] },
        }
      end
    end
  end
  return walls
end
-- South-west: its east facet against the west wall; south-east: its west facet against the
-- east wall. Each stands forward of the front by 1.5 m.
local sw_cx, se_cx, t_cy = -0.1 - A, W + 0.1 + A, 1.0
local sw = castle_tower(sw_cx, t_cy, 7)
local se = castle_tower(se_cx, t_cy, 3)
-- Doors from the house into the towers on both floors (the west wall runs north from (0, 0),
-- the east wall south from (14, 9)).
for _, w in ipairs({ ground[4], upper[4] }) do
  reviera.door.place{ host_wall_id = w, point_along = t_cy }
end
-- The east doors swing into the house, clear of the spiral in the tower.
for _, w in ipairs({ ground[2], upper[2] }) do
  reviera.door.place{ host_wall_id = w, point_along = D - t_cy, flip_facing = true }
end
-- Gothic windows on the towers' south and outer facets, arrow-slit high on the third storey.
for _, f in ipairs({ { sw, 5 }, { sw, 4 }, { se, 5 }, { se, 6 } }) do
  local len = 2 * RC * math.sin(math.pi / 8)
  for _, sill in ipairs({ 0.9, 3.9 }) do
    reviera.window.place{ type_id = gothic, host_wall_id = f[1][f[2]], point_along = len / 2, sill_height = sill }
  end
  reviera.window.place{ host_wall_id = f[1][f[2]], point_along = len / 2, sill_height = 7.2, width = 0.25, height = 1.2 }
end
-- The south-west tower climbs by an L winder (ARC-STAIR-004); the south-east by spirals.
local winder = reviera.stair.create{
  base_level = l1, width = 0.8, tread_depth = 0.28,
  runs = { { start = { sw_cx - 1.6, t_cy - 1.2 }, corner = { sw_cx + 1.2, t_cy - 1.2 }, ["end"] = { sw_cx + 1.2, t_cy + 0.48 } } },
}
reviera.stair.create_openings{ stair_id = winder }
for _, b in ipairs({ l1, l2 }) do
  local s = reviera.stair.create{
    base_level = b, width = 0.9,
    runs = { { start = {se_cx + 1.2, t_cy}, center = {se_cx, t_cy} } },
  }
  reviera.stair.create_openings{ stair_id = s }
end

-- ---- The round tower behind the east wing, joined by a barrel-vaulted gallery ----
local cx, cy, R = 19.5, 6.9, 2.6
-- Two arcs split north and south, so the west arc faces the gallery.
local west_arc = wall(l1, {cx, cy - R}, {cx, cy + R}, TH, { {cx - R, cy} })
local east_arc = wall(l1, {cx, cy + R}, {cx, cy - R}, TH, { {cx + R, cy} })
-- In from the gallery (swinging out into it, clear of the spiral stair), and out to the garden.
reviera.door.place{ type_id = arched_door, host_wall_id = west_arc, point = {cx - R - 0.3, cy} }
reviera.door.place{ type_id = arched_door, host_wall_id = east_arc, point = {cx + R + 0.3, cy} }
for _, s in ipairs({ 1.0, 4.0 }) do
  reviera.window.place{ type_id = gothic, host_wall_id = west_arc, point = {cx - 1.8, cy + 1.9}, sill_height = s }
  reviera.window.place{ type_id = gothic, host_wall_id = east_arc, point = {cx + 1.8, cy - 1.9}, sill_height = s + 0.4 }
end
reviera.window.place{ type_id = oculus, host_wall_id = east_arc, point = {cx + R + 0.3, cy}, sill_height = 7.4 }
reviera.window.place{ type_id = gothic, host_wall_id = west_arc, point = {cx - 1.8, cy - 1.9}, sill_height = 7.0 }
local function disc(r)
  local loop, n = {}, 48
  for k = 0, n - 1 do
    local a0, a1 = 2 * math.pi * k / n, 2 * math.pi * (k + 1) / n
    loop[#loop + 1] = { line = { start = { cx + r * math.cos(a0), cy + r * math.sin(a0) },
                                 ["end"] = { cx + r * math.cos(a1), cy + r * math.sin(a1) } } }
  end
  return loop
end
for _, l in ipairs({ l1, l2, l3 }) do
  reviera.floor.create{ loops = { disc(R - 0.1) }, level_id = l }
end
for _, b in ipairs({ l1, l2 }) do
  local s = reviera.stair.create{
    base_level = b, width = 0.9,
    runs = { { start = {cx + 1.2, cy}, center = {cx, cy} } },
  }
  reviera.stair.create_openings{ stair_id = s }
end
reviera.room.place{ level_id = l1, point = {cx - 1.6, cy}, name = "Tower" }
local tower_ceiling = reviera.ceiling.create{ loops = { disc(R - 0.1) }, level_id = l3 }
local dome = reviera.roof.create_dome{
  level_id = l1, base_offset = TH, center = {cx, cy},
  plan = { circle = { radius = R + 0.3 } }, profile = "onion",
}
reviera.roof.attach_walls{ roof_id = dome }
reviera.ceiling.set_shape{ ceiling_ids = { tower_ceiling }, shape = "follow_roof", roof_id = dome }
-- The gallery: walls from the house's east face to the drum (ending where the drum's face
-- begins), a floor, and a semicircular barrel vault (ARC-ROOF-008).
local g0, g1 = cy - 0.9, cy + 0.9
local gx1 = cx - math.sqrt((R + 0.1) ^ 2 - 0.8 ^ 2)
wall(l1, {gx1, g0}, {W + 0.1, g0})
wall(l1, {W + 0.1, g1}, {gx1, g1})
reviera.door.place{ host_wall_id = ground[2], point_along = D - cy }
local gallery_floor = reviera.floor.create{ loops = { square(W + 0.1, g0 + 0.1, gx1, g1 - 0.1) }, level_id = l1 }
local vault = {}
local vf = { {W + 0.1, g0 - 0.1}, {gx1, g0 - 0.1}, {gx1, g1 + 0.1}, {W + 0.1, g1 + 0.1} }
for i = 1, 4 do
  vault[i] = { line = { start = vf[i], ["end"] = vf[i % 4 + 1] }, overhang = 0 }
end
local barrel = reviera.roof.create_arched{
  curves = vault, level_id = l1, base_offset = 3.0, profile = "semicircle",
  overhang = 0, end_overhang = 0,
}

-- ---- The portico: four Doric columns from the sample Component Library ----
-- A stone platform with two steps down to the garden, the columns (the temple's, at 45 %),
-- an entablature, and a pediment: a gable roof extruded from a reference plane on the
-- portico's front (ARC-ROOF-002, ARC-REFP-001). The house's name runs along the frieze in
-- model text on the same plane (ARC-MTXT-001).
local library = reviera.script_dir .. "/../library"
local stone = {}
for _, t in ipairs({ 600, 400, 200 }) do
  stone[t] = reviera.type.duplicate{ type_id = type_named("Generic Floor - 300mm"), new_name = "Stone " .. t }
  reviera.type.set_param{ type_id = stone[t], param = "thickness", value = t / 1000 }
end
local px0, px1, py = 0.3, 6.7, -2.95
reviera.floor.create{ loops = { square(px0, py, px1, -0.1) }, level_id = l1, type_id = stone[600] }
reviera.floor.create{ loops = { square(px0, py - 0.3, px1, py) }, level_id = l1, offset = -0.2, type_id = stone[400] }
reviera.floor.create{ loops = { square(px0, py - 0.6, px1, py - 0.3) }, level_id = l1, offset = -0.4, type_id = stone[200] }
local colonnade = reviera.array.along_path{
  name = "Portico",
  sources = { { library = library, item = "Classical/Columns/parthenon_column.obj" } },
  path = { points = { {1.1, -2.25}, {5.9, -2.25} } },
  level = l1,
  distribution = { mode = "number", value = 4 },
  orientation = "same",
}
for _, t in ipairs(reviera.mesh_object.types()) do
  reviera.mesh_object.set_type{ type_id = t.id, scale = 0.45 }
end
local column_top = 10.43 * 0.45
local entablature = reviera.type.duplicate{ type_id = type_named("Generic Floor - 300mm"), new_name = "Entablature 600" }
reviera.type.set_param{ type_id = entablature, param = "thickness", value = 0.6 }
reviera.floor.create{ loops = { square(px0, py, px1, -0.8) }, level_id = l1, offset = column_top + 0.6, type_id = entablature }
local front = reviera.refplane.create{ name = "Portico Front", start = {px0 - 0.2, py}, ["end"] = {px1 + 0.2, py} }
reviera.roof.create_extrusion{
  ref_plane = front, ref_level = l1, start = 0, ["end"] = -(-0.8 - py),
  profile = {
    { line = { start = {0.0, column_top + 0.85}, ["end"] = {3.4, column_top + 1.95} } },
    { line = { start = {3.4, column_top + 1.95}, ["end"] = {6.8, column_top + 0.85} } },
  },
}
reviera.modeltext.create{
  ref_plane = front, point = {3.4, column_top + 0.2}, text = "BELMONT", size = 0.3, depth = 0.03,
  align = "center",
}

-- ---- A deck behind the house, with a guard railing and a gate ----
local dk = { {4.0, 9.2}, {12.0, 9.2}, {12.0, 12.5}, {4.0, 12.5} }
local outline = {}
for i = 1, 4 do
  outline[i] = { line = { start = dk[i], ["end"] = dk[i % 4 + 1] } }
end
reviera.deck.create{ outline = { outline }, level_id = l1 }
local guard = reviera.railing.create{
  path = { {4.05, 9.25}, {4.05, 12.45}, {11.95, 12.45}, {11.95, 9.25} }, base_level = l1,
}
reviera.railing.add_gate{ railing_id = guard, at = 7.2 }

-- ---- Booleans where parts meet (ARC-MOD-028): the house's eaves are cut by the towers, the
-- towers' walls by the house's, and the gallery's vault and floor by the drum ----
local cut_by = { [roof] = true, [barrel] = true, [gallery_floor] = true }
for _, t in ipairs({ sw, se }) do
  for _, w in pairs(t) do cut_by[w] = true end
end
for _, o in ipairs(reviera.overlap.find{}.overlaps) do
  if cut_by[o.a] then
    reviera.overlap.resolve{ a = o.a, b = o.b, mode = "cut_a" }
  elseif cut_by[o.b] then
    reviera.overlap.resolve{ a = o.a, b = o.b, mode = "cut_b" }
  end
end

reviera.view.create_plan{ level_ids = { l1, l2, l3 } }
reviera.view.create_3d{}
print("Manor built")
return true
