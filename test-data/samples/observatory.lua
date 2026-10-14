-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Observatory
-- description: A domed observatory drum of curved walls, a vaulted annex, a spline garden wall.
--
-- Reviera sample model (test-data/samples): curved walls (arcs and a spline), a hemispherical
-- dome with an oculus, a barrel-vaulted annex, doors and windows on curved walls, a round
-- floor and room. The turret beside it comes from the Castle Turret plugin (samples.json).
local level = reviera.level.list()[1].id
local function wall(a, b, through, height, base)
  -- An empty table would be read as an object: leave `through` out for a straight wall.
  local curve = { line = { start = a, ["end"] = b } }
  if through then curve.through = through end
  return reviera.wall.create{
    curve = curve,
    base_level = level, base_offset = base or 0,
    top_constraint = { unconnected = height },
  }
end

-- The drum: two semicircles of radius 5 (centreline), clockwise so the exterior is outside.
local R, H = 5.0, 5.0
local north = wall({-R, 0}, {R, 0}, {{0, R}}, H)
local south = wall({R, 0}, {-R, 0}, {{0, -R}}, H)
-- A door facing south, windows round the drum.
reviera.door.place{ host_wall_id = south, point = {0, -R - 0.3} }
for _, p in ipairs({ {-3.6, 3.6}, {3.6, 3.6}, {0, R + 0.3} }) do
  reviera.window.place{ host_wall_id = north, point = p, sill_height = 1.6 }
end
reviera.window.place{ host_wall_id = south, point = {-3.6, -3.6}, sill_height = 1.6 }
-- A round floor and the observation room inside.
local floor = {}
local n = 48
for k = 0, n - 1 do
  local a0, a1 = 2 * math.pi * k / n, 2 * math.pi * (k + 1) / n
  floor[#floor + 1] = { line = { start = { (R - 0.1) * math.cos(a0), (R - 0.1) * math.sin(a0) },
                                 ["end"] = { (R - 0.1) * math.cos(a1), (R - 0.1) * math.sin(a1) } } }
end
reviera.floor.create{ loops = { floor }, level_id = level }
reviera.room.place{ level_id = level, point = {0, 0}, name = "Observation" }
-- The dome on the drum, overhanging it a little and open at the crown (the oculus as the
-- viewing slot); the drum's walls rise to its underside.
local dome = reviera.roof.create_dome{
  level_id = level, base_offset = H, center = {0, 0},
  plan = { circle = { radius = R + 0.35 } }, profile = "hemisphere", oculus = 1.2,
}
reviera.roof.attach_walls{ roof_id = dome }

-- The control annex east of the drum, under a barrel vault.
local x0, x1, y0, y1 = 8.0, 16.0, -3.0, 3.0
wall({x0, y0}, {x0, y1}, nil, 3.2)
wall({x0, y1}, {x1, y1}, nil, 3.2)
wall({x1, y1}, {x1, y0}, nil, 3.2)
local front = wall({x1, y0}, {x0, y0}, nil, 3.2)
reviera.door.place{ host_wall_id = front, point = {12.0, y0 - 0.3} }
reviera.window.place{ host_wall_id = front, point = {9.8, y0 - 0.3} }
reviera.window.place{ host_wall_id = front, point = {14.2, y0 - 0.3} }
local o = 0.1
local vault = reviera.roof.create_arched{
  curves = {
    { line = { start = {x0 - o, y0 - o}, ["end"] = {x1 + o, y0 - o} } },
    { line = { start = {x1 + o, y0 - o}, ["end"] = {x1 + o, y1 + o} } },
    { line = { start = {x1 + o, y1 + o}, ["end"] = {x0 - o, y1 + o} } },
    { line = { start = {x0 - o, y1 + o}, ["end"] = {x0 - o, y0 - o} } },
  },
  level_id = level, base_offset = 3.2, rise = 1.6, profile = "segment",
  overhang = 0.3, end_overhang = 0.4,
}
-- The end walls fill up to the arch (gable infill).
reviera.roof.attach_walls{ roof_id = vault }
-- A link between drum and annex: two short walls.
wall({R + 0.05, -1.2}, {x0, -1.2}, nil, 3.0)
wall({x0, 1.2}, {R + 0.05, 1.2}, nil, 3.0)

-- A garden wall sweeping round the south side, a smooth spline.
wall({-7.0, -4.0}, {17.0, -7.0}, {{-4.0, -8.5}, {2.0, -9.5}, {9.0, -7.5}, {13.0, -8.5}}, 1.2)

reviera.view.create_plan{ level_ids = { level } }

print("Observatory built")
return true
