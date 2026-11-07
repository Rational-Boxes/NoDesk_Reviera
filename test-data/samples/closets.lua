-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Closets
-- description: Three reach-in closets along a hallway, 1.2 m wide and 24 in deep: a single rod at 66 in, double hang at 40 and 80 in, and an accessible closet with its rod at 42 in and shelf at 46 in, within the 2010 ADA Standards' side reach (§811, §308.3.2).
--
-- Riviera sample model (test-data/samples): closets (ARC-INT-005). closet.create fits rods and
-- a 12 in shelf along a closet's back wall face, the rods centred in its depth. The accessible
-- closet's parts are typed for the interiors rule packs, which check their heights.

local l1 = riviera.level.list()[1].id
riviera.code.set{ code = "ibc" }
riviera.clearance.set_rule_set{ standard = "ada_2010" }
riviera.rules.set_packs{ domain = "interiors", packs = { "ada_2010_interiors", "practice_interiors" } }

local function wall(a, b)
  return riviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 2.7 } }
end
-- The back wall along y = 0, its face at y = 0.1, and partitions every 1.4 m, 24 in deep: closets
-- 1.2 m wide between their faces.
wall({ 0, 0 }, { 4.2, 0 })
for _, x in ipairs({ 0, 1.4, 2.8, 4.2 }) do
  wall({ x, 0.1 }, { x, 0.1 + 0.6096 })
end

local layouts = { "single", "double", "accessible" }
for i, layout in ipairs(layouts) do
  local x0 = 1.4 * (i - 1) + 0.1
  local c = riviera.closet.create{ start = { x0, 0.1 }, ["end"] = { x0 + 1.2, 0.1 }, level = l1, layout = layout }
  print(string.format("%s: %d rod(s) and a shelf over %.2f m", layout, #c.rods, c.length))
end

assert(#riviera.rules.check{} == 0, "the accessible rod and shelf are within reach")
assert(#riviera.overlap.find{}.overlaps == 0, "no rod or shelf overlaps a wall")
