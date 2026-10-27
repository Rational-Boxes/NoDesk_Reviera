-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: House with dormers
-- description: A two-room house under a hipped roof with eaves and dormers, stairs, a gated railing and a deck.
--
-- Reviera sample model (test-data/samples): walls, doors and windows; a hip roof with eaves
-- bearing on the walls (fascia, soffits) and the walls attached; a gable and a shed dormer
-- joined flush; a U-shaped and an L-shaped stair with railings; a free railing with a single
-- gate, a double gate and an opening; a deck with a railing on its boards; a spot slope on the
-- roof.

local l1 = reviera.level.list()[1].id
local l2 = reviera.level.create{ elevation = 3.0 }
local function wall(a, b, height)
  return reviera.wall.create{
    curve = { line = { start = a, ["end"] = b } },
    base_level = l1, top_constraint = { unconnected = height or 3.0 },
  }
end

-- The house: 10 × 7 m on the walls' centrelines, clockwise (exteriors outside), a partition.
local c = { {0, 0}, {10, 0}, {10, 7}, {0, 7} }
local outer = {}
for i = 1, 4 do
  outer[i] = wall(c[i % 4 + 1], c[i])
end
local inner = wall({5, 0}, {5, 7})
reviera.door.place{ host_wall_id = outer[1], point_along = 7.5 }
reviera.door.place{ host_wall_id = inner, point_along = 3.5 }
reviera.window.place{ host_wall_id = outer[1], point_along = 2.5 }
reviera.window.place{ host_wall_id = outer[3], point_along = 2.5 }
reviera.window.place{ host_wall_id = outer[3], point_along = 7.5 }
reviera.window.place{ host_wall_id = outer[2], point_along = 3.5 }

-- A hipped roof on the walls' exterior faces, with eaves bearing on them.
local o = 0.1
local f = { {-o, -o}, {10 + o, -o}, {10 + o, 7 + o}, {-o, 7 + o} }
local curves = {}
for i = 1, 4 do
  curves[i] = { line = { start = f[i], ["end"] = f[i % 4 + 1] } }
end
local roof = reviera.roof.create_footprint{ curves = curves, level_id = l1, base_offset = 3.0 }
reviera.roof.set_eaves{ roof_id = roof, overhang = 0.6, fascia = true }
-- Dormers, front (gable) and back (shed), joined flush.
reviera.dormer.create{ roof_id = roof, point = {5.0, 0.5}, style = "gable", front_height = 0.7 }
reviera.dormer.create{ roof_id = roof, point = {5.0, 6.5}, style = "shed", front_height = 0.7 }

-- Stairs to the level above: a half turn and a quarter turn, with railings.
reviera.stair.create{ base_level = l1, runs = {
  { start = {13, 0}, ["end"] = {15.24, 0} }, { start = {15.24, 1.4}, ["end"] = {13, 1.4} } } }
reviera.stair.create{ base_level = l1, runs = {
  { start = {13, 4}, ["end"] = {15.24, 4} }, { start = {15.84, 4.6}, ["end"] = {15.84, 6.84} } } }

-- A front railing with a single gate, a double gate and an opening.
local rail = reviera.railing.create{ path = { {-1, -2.5}, {11, -2.5}, {11, 9} }, base_level = l1 }
reviera.railing.add_gate{ railing_id = rail, at = 3.0 }
reviera.railing.add_gate{ railing_id = rail, at = 8.0, leaves = 2 }
reviera.railing.add_opening{ railing_id = rail, at = 17.0 }

-- A deck, with a railing sketched over it (it stands on the boards).
local d = { {18, 0}, {22, 0}, {22, 3}, {18, 3} }
local outline = {}
for i = 1, 4 do
  outline[i] = { line = { start = d[i], ["end"] = d[i % 4 + 1] } }
end
reviera.deck.create{ outline = { outline }, level_id = l1 }
reviera.railing.create{ path = { {18.05, 0.05}, {21.95, 0.05}, {21.95, 2.95} }, base_level = l1 }

reviera.view.create_plan{ level_ids = { l1, l2 } }

-- A spot slope on the roof's east hip in the Level 2 plan (DOC-ANN-008): the roof's 30° pitch
-- as a percentage, with an arrow pointing downhill (east).
local plan2
for _, v in ipairs(reviera.view.list()) do
  if v.name == "Level 2" and v.kind ~= "3d" then plan2 = v.id end
end
reviera.spot.create_slope{ view_id = plan2, ref = roof, point = { 9.4, 3.5, 4.0 } }
print("House built")
return true
