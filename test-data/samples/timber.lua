-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Mass timber frame
-- description: A three-storey glulam post-and-beam frame to European practice: GL24h columns and GL28h beams on a 6 × 7.5 m grid, CLT floors spanning between the beams, glulam diagonal bracing, pad footings, and its analytical model with EN 1990 combinations.
--
-- Reviera sample model (test-data/samples): a mass timber office frame (Germany: DIN 18065 sets
-- the European region, so Eurocode materials and EN 1990 load combinations apply):
-- - grids 1–4 (6 m) and A–C (7.5 m); Levels 1–3 at 3.5 m and the roof at 10.5 m;
-- - glulam columns 300 × 300 in GL24h (EN 14080), one per storey;
-- - glulam beams 150 × 300 in GL28h along the grid letters, spanning 6 m, their tops 200 mm
--   below each level for the CLT;
-- - 200 mm five-layer CLT floors of C24 boards (EN 338), structural, spanning the 7.5 m between
--   the beams, on Levels 2 and 3 and the roof;
-- - glulam diagonal braces 150 × 150 in the end bays on grid A, every storey;
-- - pad footings under the ground floor columns;
-- - the analytical model: members for columns, beams and braces, panels for the CLT floors,
--   pinned supports at the column bases, permanent, imposed and snow cases with area loads on
--   the CLT, and the EN 1990 6.10 combinations.

reviera.code.set{ code = "din_18065" }
local l1 = reviera.level.list()[1].id
local levels = { l1 }
for k = 2, 4 do
  levels[k] = reviera.level.create{
    elevation = (k - 1) * 3.5, name = k == 4 and "Roof" or ("Level " .. k),
  }
end

local xs = { 0, 6, 12, 18 }
local ys = { 0, 7.5, 15 }
local W, D = xs[4], ys[3]
local grids = {}
for k, x in ipairs(xs) do
  grids[#grids + 1] = reviera.grid.create{
    curve = { line = { start = { x, -2 }, ["end"] = { x, D + 2 } } }, name = tostring(k),
  }
end
for k, y in ipairs(ys) do
  grids[#grids + 1] = reviera.grid.create{
    curve = { line = { start = { -2, y }, ["end"] = { W + 2, y } } }, name = ({ "A", "B", "C" })[k],
  }
end

local column = reviera.column.load_section{
  catalog = "eu_timber", designation = "300x300", material = "Glulam GL24h (EN 14080)",
}
local beam = reviera.beam.load_section{
  catalog = "eu_timber", designation = "150x300", material = "Glulam GL28h (EN 14080)",
}
local brace = reviera.brace.load_section{
  catalog = "eu_timber", designation = "150x150", material = "Glulam GL24h (EN 14080)",
}

local function type_named(name)
  for _, t in ipairs(reviera.type.list()) do
    if t.name == name then return t.id end
  end
end

-- Columns storey by storey.
local ground = {}
for k = 1, 3 do
  -- Each storey's columns stand on the beams below (their tops 200 mm under the level, through
  -- the CLT); the top storey's reach the roof.
  local cols = reviera.column.place_at_grids{
    type_id = column, grid_ids = grids, base_level = levels[k], base_offset = k == 1 and 0 or -0.2,
    top_constraint = { level = levels[k + 1] }, top_offset = k == 3 and 0 or -0.2,
  }
  if k == 1 then ground = cols end
end

-- Beams along the grid letters at each floor, under the CLT.
local CLT = 0.2
local outline = {
  { line = { start = { 0, 0 }, ["end"] = { W, 0 } } },
  { line = { start = { W, 0 }, ["end"] = { W, D } } },
  { line = { start = { W, D }, ["end"] = { 0, D } } },
  { line = { start = { 0, D }, ["end"] = { 0, 0 } } },
}
local floors = {}
for k = 2, 4 do
  for _, y in ipairs(ys) do
    for i = 1, 3 do
      reviera.beam.create{
        type_id = beam, reference_level = levels[k],
        curve = { start = { xs[i], y }, ["end"] = { xs[i + 1], y } },
        start_offset = -CLT, end_offset = -CLT,
      }
    end
  end
  floors[#floors + 1] = reviera.floor.create{
    loops = { outline }, level_id = levels[k], structural = true, span_direction_edge = 1,
  }
end
-- The CLT type, from the generic floor type the first floor made.
local clt = reviera.type.duplicate{ type_id = type_named("Generic Floor - 300mm"), new_name = "CLT 200 (5 layers)" }
reviera.compound.set_layers{ type_id = clt, layers = {
  { ["function"] = "structure", material = "Timber C24 (EN 338)", thickness = "200 mm" },
} }
reviera.element.set_type{ element_ids = floors, type_id = clt }

-- Diagonal braces in the end bays on grid A, every storey.
for k = 1, 3 do
  local base = k == 1 and 0 or -CLT
  reviera.brace.create{
    type_id = brace, start_point = { xs[1], 0 }, start_level = levels[k], start_offset = base,
    end_point = { xs[2], 0 }, end_level = levels[k + 1], end_offset = -CLT,
  }
  reviera.brace.create{
    type_id = brace, start_point = { xs[4], 0 }, start_level = levels[k], start_offset = base,
    end_point = { xs[3], 0 }, end_level = levels[k + 1], end_offset = -CLT,
  }
end

reviera.footing.place_at_columns{ column_ids = ground }

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
local g = reviera.load_case.create{ name = "G", nature = "G", self_weight = true }
local q = reviera.load_case.create{ name = "Q", nature = "Q" }
local snow = reviera.load_case.create{ name = "S", nature = "S" }
-- The analytical panel made for a floor.
local function panel_of(floor)
  for _, p in ipairs(reviera.analytical_model.info{}.panels) do
    for _, f in ipairs(p.physical) do
      if f == floor then return p.id end
    end
  end
end
-- Area loads on the CLT: 1.5 kPa finishes and services on every floor, 3.0 kPa office imposed
-- load on Levels 2 and 3, 1.0 kPa snow on the roof. Analysis carries them to the beams.
for k, f in ipairs(floors) do
  local panel = panel_of(f)
  reviera.load.create_area{ host_id = panel, case_id = g, value = { 0, 0, -1500 } }
  if k < #floors then
    reviera.load.create_area{ host_id = panel, case_id = q, value = { 0, 0, -3000 } }
  else
    reviera.load.create_area{ host_id = panel, case_id = snow, value = { 0, 0, -1000 } }
  end
end
reviera.load_combination.generate{ add = true }
