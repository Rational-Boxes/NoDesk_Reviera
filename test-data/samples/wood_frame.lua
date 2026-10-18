-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Wood-frame house
-- description: A two-storey light wood-frame house to US residential practice: 2x6 stud bearing walls on strip footings, 2x10 floor joists at 16" with a flush built-up beam on posts, a plywood subfloor, Howe roof trusses at 24", and its analytical model with supports and ASCE 7 combinations.
--
-- Reviera sample model (test-data/samples): platform framing (IRC; dimension lumber from the NA
-- lumber catalogue, PS 20 dressed sizes, in Spruce-Pine-Fir No.1/No.2 or Douglas Fir-Larch No.2):
-- - a 32 × 24 ft house, Level 1 on a slab on grade, Level 2 at 9'-1" and the roof plate at 18'-2";
-- - exterior walls of 2x6 studs (layers: siding, sheathing, 2x6 studs, gypsum), bearing, on
--   24" × 12" concrete strip footings;
-- - Level 2 floor joists of 2x10 at 16" o.c. as beam systems, spanning 12 ft from the exterior
--   walls to a flush beam of 4x12 down the middle, carried by 4x6 posts on spread footings;
-- - a 3/4" plywood subfloor on the joists, structural; Level 2 walls on it;
-- - Howe roof trusses of 2x6 chords and 2x4 webs at 24" o.c., bearing on the walls;
-- - the analytical model: members for the joists, beam, posts and trusses (split where they
--   frame together), panels for the bearing walls and the subfloor, pinned supports at the post
--   bases, the wall bottoms and where joists, beam and trusses bear on the walls, dead, live and
--   snow cases with area loads on the subfloor, and the ASCE 7 combinations.

reviera.code.set{ code = "irc" }
local FT, IN = 0.3048, 0.0254
local l1 = reviera.level.list()[1].id
local l2 = reviera.level.create{ elevation = 9 * FT + 1 * IN, name = "Level 2" }
local plate = reviera.level.create{ elevation = 18 * FT + 2 * IN, name = "Roof Plate" }
local W, D = 32 * FT, 24 * FT

local function type_named(name)
  for _, t in ipairs(reviera.type.list()) do
    if t.name == name then return t.id end
  end
end

-- Sections, in the region's lumber grade unless named.
local joist = reviera.beam.load_section{ catalog = "na_lumber", designation = "2x10" }
local flush = reviera.beam.load_section{
  catalog = "na_lumber", designation = "4x12", material = "Wood Douglas Fir-Larch No.2",
}
local post = reviera.column.load_section{
  catalog = "na_lumber", designation = "4x6", material = "Wood Douglas Fir-Larch No.2",
}
local chord = reviera.beam.load_section{ catalog = "na_lumber", designation = "2x6" }
local web = reviera.beam.load_section{ catalog = "na_lumber", designation = "2x4" }

-- Bearing walls on both levels, clockwise. Level 1's stop under the joists and subfloor.
local JOIST, DECK, BEAM = 9.25 * IN, 0.75 * IN, 11.25 * IN
local c = { { 0, 0 }, { W, 0 }, { W, D }, { 0, D } }
local walls1, walls2 = {}, {}
for i = 1, 4 do
  local a, b = c[i % 4 + 1], c[i]
  walls1[i] = reviera.wall.create{
    curve = { line = { start = a, ["end"] = b } }, base_level = l1,
    top_constraint = { level = l2 }, top_offset = -(JOIST + DECK), structural = true,
  }
  walls2[i] = reviera.wall.create{
    curve = { line = { start = a, ["end"] = b } }, base_level = l2,
    top_constraint = { level = plate }, structural = true,
  }
end

-- The stud wall type, from the generic type the first wall made: 2x6 studs (5 1/2") in a 7 1/4"
-- wall.
local stud_wall = reviera.type.duplicate{ type_id = type_named("Generic - 200mm"), new_name = "Exterior - 2x6 Stud" }
reviera.compound.set_layers{ type_id = stud_wall, layers = {
  { ["function"] = "finish1", material = "Fibre Cement Siding", thickness = "5/16\"" },
  { ["function"] = "substrate", material = "OSB Sheathing", thickness = "7/16\"" },
  { ["function"] = "structure", material = "Wood Spruce-Pine-Fir No.1/No.2", thickness = "5 1/2\"" },
  { ["function"] = "finish2", material = "Gypsum Board", thickness = "1/2\"" },
} }

local all_walls = {}
for i = 1, 4 do
  all_walls[#all_walls + 1] = walls1[i]
  all_walls[#all_walls + 1] = walls2[i]
end
reviera.element.set_type{ element_ids = all_walls, type_id = stud_wall }

-- Strip footings under the Level 1 walls.
reviera.wall_foundation.create{ wall_ids = walls1 }
local outline = {
  { line = { start = { 0, 0 }, ["end"] = { W, 0 } } },
  { line = { start = { W, 0 }, ["end"] = { W, D } } },
  { line = { start = { W, D }, ["end"] = { 0, D } } },
  { line = { start = { 0, D }, ["end"] = { 0, 0 } } },
}

-- The flush beam down the middle on posts at 8 ft, its top at the joists' top; the posts stop
-- under it.
local posts = {}
for k = 1, 3 do
  posts[k] = reviera.column.place{
    type_id = post, point = { k * 8 * FT, D / 2 }, base_level = l1,
    top_constraint = { level = l2 }, top_offset = -(BEAM + DECK),
  }
end
reviera.beam.create{
  type_id = flush, reference_level = l2, curve = { start = { 0, D / 2 }, ["end"] = { W, D / 2 } },
  start_offset = -DECK, end_offset = -DECK,
}
reviera.footing.place_at_columns{ column_ids = posts }

-- Joists at 16" o.c., each half spanning from the wall to the beam.
for _, half in ipairs({ { 0, D / 2 }, { D / 2, D } }) do
  reviera.beam_system.create_sketch{
    boundary = { { 0, half[1] }, { W, half[1] }, { W, half[2] }, { 0, half[2] } },
    direction_edge = 1, type_id = joist, layout_rule = "fixed_distance", value = 16 * IN,
    level = l2, offset = -DECK,
  }
end
local deck = reviera.floor.create{
  loops = { outline }, level_id = l2, structural = true, span_direction_edge = 0,
}
-- The subfloor: 3/4" plywood.
local subfloor = reviera.type.duplicate{ type_id = type_named("Generic Floor - 300mm"), new_name = "Subfloor - 3/4\" Plywood" }
reviera.compound.set_layers{ type_id = subfloor, layers = {
  { ["function"] = "structure", material = "Wood Douglas Fir-Larch No.2", thickness = "3/4\"" },
} }
reviera.element.set_type{ element_ids = { deck }, type_id = subfloor }
-- The slab on grade (after the subfloor, which made the generic floor type).
reviera.foundation_slab.create{ boundary = { outline }, level = l1 }

-- Howe trusses at 24" o.c., 6 ft high, spanning the 24 ft width, bearing on the walls.
local n = math.floor(W / (24 * IN))
for k = 0, n do
  local x = math.min(k * 24 * IN, W)
  reviera.truss.create{
    start = { x, 0 }, ["end"] = { x, D }, level = plate, layout = "howe", panels = 8,
    height = 6 * FT, chord_type_id = chord, web_type_id = web,
  }
end

-- Where framing meets by design: a column passing through a slab cuts it; braces' and truss
-- members' ends meet at their connections and bearings, and footings meet the slab on grade and
-- each other, accepted as intended.
local function resolve_framing_overlaps()
  local kinds = {}
  local function kind(label)
    return label:match("^(%a+)")
  end
  -- The finder reports a page at a time: go on while it finds something to resolve.
  local seen = {}
  repeat
    local progress = false
    for _, o in ipairs(reviera.overlap.find{}.overlaps) do
      local a, b = kind(o.a_label), kind(o.b_label)
      local key = o.a .. ":" .. o.b
      local mode
      if a == "IfcColumn" and b == "IfcSlab" then
        mode = "cut_b"
      elseif a == "IfcSlab" and b == "IfcColumn" then
        mode = "cut_a"
      elseif a == "IfcMember" or b == "IfcMember" or a == "IfcFooting" or b == "IfcFooting" then
        mode = "accept"
      end
      if mode and not seen[key] then
        seen[key] = true
        reviera.overlap.resolve{ a = o.a, b = o.b, mode = mode }
        progress = true
      end
    end
  until not progress
end
resolve_framing_overlaps()

-- The analytical model, supports, loads and combinations.
reviera.analytical_automation.physical_to_analytical{}
reviera.boundary_condition.at_column_bases{}
-- The walls' bottom corners (shared by the walls meeting there) are pinned once each.
local model = reviera.analytical_model.info{}
local pinned = {}
for _, p in ipairs(model.panels) do
  if p.role == "wall" then
    for k, corner in ipairs(p.corners) do
      local node = p.nodes[k]
      if math.abs(corner[3]) < 1e-6 and not pinned[node] then
        pinned[node] = true
        reviera.boundary_condition.create{ kind = "point", host_id = node, state = "pinned" }
      end
    end
  end
end
-- Joists, the flush beam and trusses bear on the exterior walls: pinned supports at their ends
-- over the walls, as light-frame members are designed one by one.
local function on_wall_line(p)
  local e = 1e-3
  return math.abs(p[1]) < e or math.abs(p[1] - W) < e or math.abs(p[2]) < e or math.abs(p[2] - D) < e
end
for _, m in ipairs(model.members) do
  if m.role ~= "column" then
    for k, p in ipairs({ m.start, m["end"] }) do
      local node = m.nodes[k]
      if p[3] > 0.1 and on_wall_line(p) and not pinned[node] then
        pinned[node] = true
        reviera.boundary_condition.create{ kind = "point", host_id = node, state = "pinned" }
      end
    end
  end
end
local dl = reviera.load_case.create{ name = "DL", nature = "D", self_weight = true }
local ll = reviera.load_case.create{ name = "LL", nature = "L" }
reviera.load_case.create{ name = "SNOW", nature = "S" }
-- The analytical panel made for a floor.
local function panel_of(floor)
  for _, p in ipairs(reviera.analytical_model.info{}.panels) do
    for _, f in ipairs(p.physical) do
      if f == floor then return p.id end
    end
  end
end
-- Area loads on the subfloor: 10 psf dead (0.48 kPa) and 40 psf residential live (1.92 kPa),
-- carried to the joists.
reviera.load.create_area{ host_id = panel_of(deck), case_id = dl, value = { 0, 0, -479 } }
reviera.load.create_area{ host_id = panel_of(deck), case_id = ll, value = { 0, 0, -1915 } }
reviera.load_combination.generate{ add = true }
