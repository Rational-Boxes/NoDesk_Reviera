-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Storefront doors
-- description: A shop front with a revolving door, an automatic sliding door, a pair of glass swing doors and an automatic storefront door.
--
-- Reviera sample model (test-data/samples): the door operations (ARC-DOOR-002) side by side in a
-- shop's front wall, and their plan symbols and virtual volumes:
-- - a revolving door 2.2 m across (its drum, wings and canopy);
-- - an automatic bi-parting sliding door with glass leaves, which takes the wall its leaves slide
--   over (its slide space is a virtual volume beside the opening);
-- - a pair of glass storefront doors swinging out;
-- - a single automatic storefront door (a powered operator over the head);
-- - an ordinary single door at the back.
-- The shop has a floor and a flat roof, and the ADA 2010 rules are on: manual swinging doors
-- get maneuvering clearances; automatic, sliding and revolving doors have none of their own.

reviera.clearance.set_rule_set{ standard = "ada_2010" }
local l1 = reviera.level.list()[1].id
local function wall(a, b)
  return reviera.wall.create{
    curve = { line = { start = a, ["end"] = b } }, base_level = l1,
    top_constraint = { unconnected = 4.0 },
  }
end
local function type_named(name)
  for _, t in ipairs(reviera.type.list()) do
    if t.name == name then return t.id end
  end
end

-- The shop: 24 × 10 m on the walls' centrelines, clockwise.
local W, D = 24.0, 10.0
local c = { {0, 0}, {W, 0}, {W, D}, {0, D} }
local walls = {}
for i = 1, 4 do
  walls[i] = wall(c[i % 4 + 1], c[i])
end
-- walls[1] is the front, from (24, 0) to (0, 0): `point_along` counts from the east end.
local front, back = walls[1], walls[3]
-- The back door makes the default door type; the others are duplicates of it.
reviera.door.place{ host_wall_id = back, point_along = 3.0 }
local base = type_named("Single Door 900 x 2100")
local function door_type(name, params)
  local t = reviera.type.duplicate{ type_id = base, new_name = name }
  for _, p in ipairs(params) do
    reviera.type.set_param{ type_id = t, param = p[1], value = p[2] }
  end
  return t
end
local revolving = door_type("Revolving 2200", {
  { "width", 2.2 }, { "height", 2.4 }, { "operation", "revolving" },
})
local slider = door_type("Automatic Sliding 2000", {
  { "width", 2.0 }, { "height", 2.3 }, { "operation", "sliding" }, { "panel", "glass" },
  { "automatic", true },
})
local pair = door_type("Storefront Pair 1800", {
  { "width", 1.8 }, { "height", 2.3 }, { "operation", "double_swing" }, { "panel", "glass" },
})
local auto_single = door_type("Automatic Storefront 1000", {
  { "width", 1.0 }, { "height", 2.3 }, { "panel", "glass" }, { "automatic", true },
})

-- Along the front, east to west.
reviera.door.place{ type_id = revolving, host_wall_id = front, point_along = 4.0 }
reviera.door.place{ type_id = slider, host_wall_id = front, point_along = 10.0 }
reviera.door.place{ type_id = pair, host_wall_id = front, point_along = 16.0 }
reviera.door.place{ type_id = auto_single, host_wall_id = front, point_along = 20.5 }
-- Shop windows between them, clear of the sliding door's leaves.
reviera.window.place{ host_wall_id = back, point_along = 12.0 }
local shop = reviera.type.duplicate{ type_id = type_named("Window 1200 x 1200"), new_name = "Shop Window 1800 x 2400" }
reviera.type.set_param{ type_id = shop, param = "width", value = 1.8 }
reviera.type.set_param{ type_id = shop, param = "height", value = 2.4 }
for _, wnd in ipairs({ 6.6, 13.6, 22.6 }) do
  reviera.window.place{ type_id = shop, host_wall_id = front, point_along = wnd, sill_height = 0.4 }
end

reviera.floor.create{ loops = { {
  { line = { start = {0.1, 0.1}, ["end"] = {W - 0.1, 0.1} } },
  { line = { start = {W - 0.1, 0.1}, ["end"] = {W - 0.1, D - 0.1} } },
  { line = { start = {W - 0.1, D - 0.1}, ["end"] = {0.1, D - 0.1} } },
  { line = { start = {0.1, D - 0.1}, ["end"] = {0.1, 0.1} } },
} }, level_id = l1 }
local o = 0.1
local curves = {}
local f = { {-o, -o}, {W + o, -o}, {W + o, D + o}, {-o, D + o} }
for i = 1, 4 do
  curves[i] = { line = { start = f[i], ["end"] = f[i % 4 + 1] } }
end
local roof = reviera.roof.create_footprint{ curves = curves, level_id = l1, base_offset = 4.0 }
reviera.roof.set_shape{ roof_id = roof, shape = "flat", overhang = 0.3 }
reviera.room.place{ level_id = l1, point = {12.0, 5.0}, name = "Shop" }

reviera.view.create_plan{ level_ids = { l1 } }
reviera.view.create_3d{}
print("Storefront built")
return true
