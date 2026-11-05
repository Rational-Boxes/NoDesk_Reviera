-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: A gondola up a hillside
-- description: A monocable gondola 850 m up a hillside toposolid with a ridge and a dip, its alignment a straight line from the valley station to the top: with the default towers (18.3 m high, at most 152 m apart) the loaded cabins come too close to the rising ground in two spans (RM2); rebuilt with 22 m towers at most 120 m apart every span clears, the cabins at most 27 m above the ground. Ropes 6.4 m apart sag 4 % of each span; the line carries 2,400 persons an hour each way.
--
-- Riviera sample model (test-data/samples): aerial ropeways (ARC-VT-027) on the alignment
-- foundation, over the site's toposolids. One loaded sag per span (E-100).

riviera.code.set{ code = "ibc" }
local level = riviera.level.list()[1].id

-- The hillside: flat in the valley, a ridge at x = 400, a dip, then up to the top.
local profile = { { -50, 0 }, { 100, 0 }, { 400, 150 }, { 500, 135 }, { 900, 310 } }
local pts = {}
for _, p in ipairs(profile) do
  for _, y in ipairs({ -80, 80 }) do
    pts[#pts + 1] = { p[1], y, p[2] }
  end
end
riviera.toposolid.create{ level_id = level, boundary = { { -50, -80 }, { 900, -80 }, { 900, 80 }, { -50, 80 } }, points = pts }

-- The line: straight from the valley station at (0, 0) to the top station at (850, 0).
local line = riviera.alignment.create{ level = level, name = "Gondola line", points = { { 0, 0 }, { 850, 0 } } }
-- With the default towers, 18.3 m high at most 152 m apart (six spans of 141.7 m), the loaded
-- cabins come within 2.5 m of the rising ground in the last two spans (RM2: 2.5 m on spans under
-- 200 m), and below it in the fifth.
local first = riviera.ropeway.line{ alignment_id = line.alignment }
local short = {}
for _, f in ipairs(riviera.ropeway.check{}) do
  if f.rule == "RM2 clearance" then short[#short + 1] = f.message end
end
assert(#short == 2, "two spans too low")

-- Rebuilt with towers 22 m high at most 120 m apart (eight spans of 106 m): every span clears, the
-- cabins at most 27 m above the ground. 2,400 persons an hour each way at 6 m/s, cabins of ten
-- every 15 s.
riviera.element.delete{ element_ids = { first.ropeway } }
local r = riviera.ropeway.line{ alignment_id = line.alignment, tower_height = 22, tower_spacing = 120 }
local towers = 0
for _, s in ipairs(r.supports) do
  if s.kind == "tower" then towers = towers + 1 end
end
for _, f in ipairs(riviera.ropeway.check{ demand = 2000 }) do
  assert(f.rule == "overflight", "only the overflight note: " .. f.message)
end
print(string.format("ropeway: %.0f m, %d towers, %.0f an hour each way, %.0f s a trip", r.length, towers, r.capacity, r.trip))
assert(towers == 7 and r.capacity == 2400, "seven towers, 2,400 an hour")
