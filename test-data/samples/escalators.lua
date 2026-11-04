-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Escalators and moving walks in a department store
-- description: Three floors of a store under the IBC: an up-and-down pair of escalators and a single escalator above them, an inclined moving walk for trolleys and a horizontal walk along the top floor, each sized under the vertical-transport rule packs with the floor openings its pits and headroom need; Check Escalators and Check Clearances find nothing.
--
-- Riviera sample model (test-data/samples): escalators and moving walks (ARC-VT-010…012). Under
-- the IBC the rules are A17.1's (through Oregon's checklists, advisory): at most 30° and 0.5 m/s,
-- 2130 mm headroom, a safety zone at each end twice the handrail centreline distance deep and
-- 100 mm clear beside the handrails; `practice_us` gives the makers' generic sizes.

riviera.code.set{ code = "ibc" }
local l1 = riviera.level.list()[1].id
local l2 = riviera.level.create{ elevation = 5.0 }
local l3 = riviera.level.create{ elevation = 10.0 }

-- A 40 × 24 m floor on every level.
local c = { {0, 0}, {40, 0}, {40, 24}, {0, 24} }
local loop = {}
for i = 1, 4 do
  loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
end
for _, l in ipairs({ l1, l2, l3 }) do
  riviera.floor.create{ loops = { loop }, level_id = l }
end

-- 1. An up-and-down pair from Level 1 to Level 2 (5 m rise): 30°, 1000 mm steps, 2 flat
--    steps, each 1.732 × 5 + 4.70 = 13.36 m long and good for about 6000 persons an hour. Their
--    handrails' outer edges are 200 mm apart, clear of each one's 100 mm side clearance. Each
--    cuts a pit in Level 1 and an opening in Level 2 over its upper pit and as far down the
--    incline as its 2130 mm headroom reaches the 300 mm slab.
local up = riviera.escalator.create{ point = { 4.0, 4.0 }, base_level = l1, top_level = l2 }
riviera.escalator.create{ point = { 4.0, 4.0 + up.handrail_outer + 0.2 }, base_level = l1, top_level = l2 }

-- 2. A single escalator from Level 2 to Level 3, beside the pair.
riviera.escalator.create{ point = { 4.0, 10.0 }, base_level = l2, top_level = l3 }

-- 3. An inclined moving walk for shopping trolleys from Level 1 to Level 2: 12°, 1000 mm
--    pallets, 23.5 m of incline.
riviera.escalator.create{ kind = "moving_walk", point = { 4.0, 16.0 }, base_level = l1, top_level = l2 }

-- 4. A horizontal walk along Level 3, 30 m between its working points, 1400 mm pallets, on the
--    floor.
riviera.escalator.create{ kind = "moving_walk", point = { 5.0, 20.0 }, base_level = l3, length = 30.0, step_width = 1.4 }

assert(#riviera.escalator.check{} == 0, "escalator.check found problems")
