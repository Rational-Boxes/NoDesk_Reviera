-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: A galley kitchen
-- description: A 3.8 × 2.5 m galley kitchen with two runs of base cabinets in standard US widths and fillers, wall cabinets over one run, a tall pantry run, and countertops on both base runs, 1.23 m clear between the counters.
--
-- Riviera sample model (test-data/samples): cabinet runs and countertops (ARC-KIT-001/002).
-- cabinet.run fills a back line along a wall with units of the region's standard widths (US:
-- 36 in units, with one or two of 9–48 in to leave the least filler) and a filler for what is
-- left, facing away from the wall; countertop.from_runs tops each run of base units, 1 in past their fronts.

local l1 = riviera.level.list()[1].id
riviera.code.set{ code = "irc" }

-- Walls on centre lines round 0–4.0 × 0–2.7, 200 mm thick: inside 0.1–3.9 × 0.1–2.6.
local function wall(a, b)
  return riviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 2.7 } }
end
wall({ 0, 0 }, { 4.0, 0 })
wall({ 4.0, 0 }, { 4.0, 2.7 })
wall({ 4.0, 2.7 }, { 0, 2.7 })
wall({ 0, 2.7 }, { 0, 0 })

local function report(name, r)
  local w = {}
  for _, u in ipairs(r.units) do
    if not u.filler then w[#w + 1] = string.format("%.0f", u.width / 0.0254) end
  end
  print(string.format("%s: %.2f m, units %s in, filler %.0f mm", name, r.length, table.concat(w, " + "), r.filler * 1000))
end

-- The south run along the wall face at y = 0.1: base units the whole 3.8 m (36 + 36 + 36 + 39 in
-- and a 66 mm filler), wall units over them.
report("South base", riviera.cabinet.run{ start = { 0.1, 0.1 }, ["end"] = { 3.9, 0.1 }, level = l1, kind = "base" })
report("South wall", riviera.cabinet.run{ start = { 0.1, 0.1 }, ["end"] = { 3.9, 0.1 }, level = l1, kind = "wall" })
-- The north run at y = 2.6: base units for 3.0 m and a tall pantry run in the last 0.8 m (one
-- 30 in unit and a 38 mm filler).
report("North base", riviera.cabinet.run{ start = { 0.1, 2.6 }, ["end"] = { 3.1, 2.6 }, level = l1, kind = "base" })
report("North tall", riviera.cabinet.run{ start = { 3.1, 2.6 }, ["end"] = { 3.9, 2.6 }, level = l1, kind = "tall" })

-- Countertops on both base runs, 25 in deep: 2.5 − 2 × 25 in leaves 1.23 m between them.
for _, t in ipairs(riviera.countertop.from_runs{}) do
  print(string.format("Countertop %.2f × %.3f m, top at %.0f mm", t.length, t.depth, t.height * 1000))
end
assert(#riviera.overlap.find{}.overlaps == 0, "no unit, filler or top overlaps another or a wall")

local room = riviera.room.place{ level_id = l1, point = { 2.0, 1.3 } }
riviera.room.set{ room_ids = { room }, name = "Kitchen" }
