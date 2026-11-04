-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Structural model with problems
-- description: A small steel frame with deliberate faults for Analyze › Checks to find: a floating ring of beams, a beam resting on nothing, a member stopping 40 mm short of a column, members framing mid-span with no node, a girder moved off its analytical line, a beam with no analytical member, a member pinned at both ends, and a slab well below its beams.
--
-- Riviera sample model (test-data/samples): one sound 8 × 6 m bay (W12X53 columns, W16X26
-- girders under a structural slab, pinned column bases), then faults made on purpose, each labelled
-- in the Level 2 plan with what Check Supports and Check Consistency report for it:
--  1. A floating ring of four beams with nothing under it (S2 circular support, C2 isolated part:
--     an error, so Run Analysis holds).
--  2. A cantilever off the frame whose far end rests on nothing (S1 unsupported member, C1 free
--     end).
--  3. An analytical member drawn by hand that stops 40 mm short of a column (C1 disconnected
--     nodes, measured 40 mm against the 5 mm merge distance).
--  4. An analytical member drawn by hand across the bay from girder to girder, landing mid-span
--     where the girders have no node (C1 end on span, twice; connected to nothing, it is an
--     isolated part, C2), and a physical beam at the same place that has no analytical member
--     (C5 no counterpart, both ways).
--  5. The south girder moved 250 mm off its analytical member after automation (C3
--     physical/analytical mismatch, 250 mm against 150 mm; C4 outside the physical body).
--  6. The east girder's analytical member pinned at both ends (C7 instability hint).
--  7. A second structural slab 700 mm below the beams it should rest on (C6 overlap not
--     reflected; alone, its panel is also unsupported, S1, and an isolated part, C2).
-- Turn on C8 (missing properties) in Structural Settings to see the hand-drawn members without a
-- section. Fix them with Merge Nodes, Physical to Analytical, element moves and supports, then
-- Re-check.

riviera.code.set{ code = "ibc" }
local l1 = riviera.level.list()[1].id
local l2 = riviera.level.create{ elevation = 4.0, name = "Level 2" }
local plan
for _, v in ipairs(riviera.view.list()) do
  if v.name == "Level 2" and v.kind ~= "3d" then plan = v.id end
end
local function label(x, y, text)
  riviera.text.create{ view_id = plan, position = { x, y }, content = text }
end

local column = riviera.column.load_section{ catalog = "aisc_w", designation = "W12X53" }
local girder = riviera.beam.load_section{ catalog = "aisc_w", designation = "W16X26" }
local function col(x, y)
  return riviera.column.place{
    type_id = column, point = { x, y }, base_level = l1, top_constraint = { level = l2 },
  }
end
-- Beams 300 mm below the level, under the slab.
local SLAB = 0.3
local function beam(a, b)
  return riviera.beam.create{
    type_id = girder, reference_level = l2, curve = { start = a, ["end"] = b },
    start_offset = -SLAB, end_offset = -SLAB,
  }
end

-- The sound bay.
local cols = { col(0, 0), col(8, 0), col(8, 6), col(0, 6) }
-- A column for the member of fault 3 to reach, its top at the beams' level.
riviera.column.place{
  type_id = column, point = { 14, 6 }, base_level = l1, top_constraint = { level = l2 }, top_offset = -0.3,
}
local south = beam({ 0, 0 }, { 8, 0 })
local east = beam({ 8, 0 }, { 8, 6 })
beam({ 8, 6 }, { 0, 6 })
beam({ 0, 6 }, { 0, 0 })
local function square(x0, y0, x1, y1)
  return {
    { line = { start = { x0, y0 }, ["end"] = { x1, y0 } } },
    { line = { start = { x1, y0 }, ["end"] = { x1, y1 } } },
    { line = { start = { x1, y1 }, ["end"] = { x0, y1 } } },
    { line = { start = { x0, y1 }, ["end"] = { x0, y0 } } },
  }
end
riviera.floor.create{ loops = { square(0, 0, 8, 6) }, level_id = l2, structural = true, span_direction_edge = 1 }

-- 1. A floating ring of beams.
beam({ 22, 0 }, { 26, 0 })
beam({ 26, 0 }, { 26, 4 })
beam({ 26, 4 }, { 22, 4 })
beam({ 22, 4 }, { 22, 0 })
label(22, -1.5, "1. Floating ring: S2, C2")

-- 2. A cantilever resting on nothing.
beam({ 8, 0 }, { 11, -2 })
label(9, -3, "2. Rests on nothing: S1, C1 free end")

-- 7. A slab 700 mm below the beams it should rest on (the ring).
riviera.floor.create{
  loops = { square(22, 0, 26, 4) }, level_id = l2, offset = -1.0, structural = true,
  span_direction_edge = 1,
}
label(22, 5, "7. Slab 700 mm below its beams: C6; alone: S1, C2")

-- The analytical model and supports, as automation makes them.
riviera.analytical_automation.physical_to_analytical{}
riviera.boundary_condition.at_column_bases{}
local d = riviera.load_case.create{ name = "DL", nature = "D", self_weight = true }
riviera.load_case.create{ name = "LL", nature = "L" }
riviera.load_combination.generate{ add = true }

-- 3. A hand-drawn member 40 mm short of the column at (8, 6).
riviera.analytical_member.create{ start = { 8.04, 6, 4 - SLAB }, ["end"] = { 14, 6, 4 - SLAB } }
label(9, 6.5, "3. 40 mm short of the column: C1 gap")

-- 4. A member across the bay landing mid-span on both girders, and a beam with no member.
riviera.analytical_member.create{ start = { 4, 0, 4 - SLAB }, ["end"] = { 4, 6, 4 - SLAB } }
beam({ 4, 0 }, { 4, 6 })
label(4.3, 3, "4. Mid-span, no nodes: C1, C2; no counterparts: C5")

-- 5. The south girder moved 250 mm off its analytical member.
riviera.element.move{ element_ids = { south }, vector = { 0, 0.25, 0 } }
label(1, -1.5, "5. Girder moved 250 mm: C3, C4")

-- 6. The east girder's member pinned at both ends.
for _, m in ipairs(riviera.analytical_model.info{}.members) do
  for _, p in ipairs(m.physical) do
    if p == east then
      riviera.analytical_member.set_releases{ member_ids = { m.id }, ["end"] = "both", preset = "pinned" }
    end
  end
end
label(8.3, 2, "6. Pinned both ends: C7")

-- Overlaps left on purpose: the moved girder meets the columns at its ends.
for _, o in ipairs(riviera.overlap.find{}.overlaps) do
  local a, b = o.a_label:match("^(%a+)"), o.b_label:match("^(%a+)")
  if a == "IfcColumn" and b == "IfcSlab" then
    riviera.overlap.resolve{ a = o.a, b = o.b, mode = "cut_b" }
  elseif a == "IfcSlab" and b == "IfcColumn" then
    riviera.overlap.resolve{ a = o.a, b = o.b, mode = "cut_a" }
  end
end
local _ = d
