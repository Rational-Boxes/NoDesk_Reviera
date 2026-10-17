-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Steel-framed office
-- description: A two-storey steel frame on a 30 × 25 ft grid: W-shape columns, girders and beams, infill beam systems, HSS X-bracing, a concrete slab on the framing, footings, and its analytical model with supports, loads and ASCE 7 combinations ready to analyse.
--
-- Reviera sample model (test-data/samples): structural steel framing to US practice (IBC; AISC
-- shapes in ASTM A992, HSS in A500 Gr C):
-- - grids A–D (30 ft bays) and 1–3 (25 ft bays); Level 2 at 14 ft and the Roof at 28 ft;
-- - W12X53 columns from the footings to the roof, one piece through Level 2;
-- - W18X35 girders along the grid letters' bays, W16X26 beams along the numbers;
-- - W12X26 infill beams in every bay as beam systems, at most 10 ft apart, framing into the
--   girders; all framing 6" below the level so the slab sits on top;
-- - X-bracing of HSS6X6X3/8 in bay A–B on grid 1, both storeys;
-- - a 6" concrete slab on Level 2 and a roof slab, both structural, spanning across the beams;
-- - a spread footing under each column;
-- - the analytical model made from the physical one (members split where beams frame into
--   girders, panels for the slabs), pinned supports at the column bases, dead, live, roof live
--   and wind load cases with line loads on the Level 2 girders, and the ASCE 7 strength
--   combinations. Run Analyze › Structural Analysis › Run Analysis to analyse it.

reviera.code.set{ code = "ibc" }
local l1 = reviera.level.list()[1].id
local l2 = reviera.level.create{ elevation = 4.2672, name = "Level 2" }
local roof = reviera.level.create{ elevation = 8.5344, name = "Roof" }

local FT = 0.3048
local xs = { 0, 30 * FT, 60 * FT, 90 * FT }
local ys = { 0, 25 * FT, 50 * FT }
local W, D = xs[4], ys[3]

-- Grids.
local grids = {}
for k, x in ipairs(xs) do
  grids[#grids + 1] = reviera.grid.create{
    curve = { line = { start = { x, -2.0 }, ["end"] = { x, D + 2.0 } } },
    name = ({ "A", "B", "C", "D" })[k],
  }
end
for k, y in ipairs(ys) do
  grids[#grids + 1] = reviera.grid.create{
    curve = { line = { start = { -2.0, y }, ["end"] = { W + 2.0, y } } },
    name = tostring(k),
  }
end

-- Sections.
local column = reviera.column.load_section{ catalog = "aisc_w", designation = "W12X53" }
local girder = reviera.beam.load_section{ catalog = "aisc_w", designation = "W18X35" }
local beam = reviera.beam.load_section{ catalog = "aisc_w", designation = "W16X26" }
local infill = reviera.beam.load_section{ catalog = "aisc_w", designation = "W12X26" }
local hss = reviera.brace.load_section{ catalog = "aisc_hss_rect", designation = "HSS6X6X3/8" }

-- Columns at every grid crossing, base to roof.
local cols = reviera.column.place_at_grids{
  type_id = column, grid_ids = grids, base_level = l1, top_constraint = { level = roof },
}

-- Framing at each floor, 6" below the level for the slab.
local SLAB = 0.1524
for _, level in ipairs({ l2, roof }) do
  for _, y in ipairs(ys) do
    for i = 1, 3 do
      reviera.beam.create{
        type_id = girder, reference_level = level,
        curve = { start = { xs[i], y }, ["end"] = { xs[i + 1], y } },
        start_offset = -SLAB, end_offset = -SLAB,
      }
    end
  end
  for _, x in ipairs(xs) do
    for j = 1, 2 do
      reviera.beam.create{
        type_id = beam, reference_level = level,
        curve = { start = { x, ys[j] }, ["end"] = { x, ys[j + 1] } },
        start_offset = -SLAB, end_offset = -SLAB,
      }
    end
  end
  -- Infill beams spanning between the girders, at most 10 ft apart.
  for i = 1, 3 do
    for j = 1, 2 do
      reviera.beam_system.create_sketch{
        boundary = {
          { xs[i], ys[j] }, { xs[i + 1], ys[j] }, { xs[i + 1], ys[j + 1] }, { xs[i], ys[j + 1] },
        },
        direction_edge = 1, type_id = infill, layout_rule = "maximum_spacing", value = 10 * FT,
        level = level, offset = -SLAB,
      }
    end
  end
end

-- X-bracing in bay A–B on grid 1, both storeys.
for _, s in ipairs({ { l1, l2, 0.0, -SLAB }, { l2, roof, -SLAB, -SLAB } }) do
  reviera.brace.create{
    type_id = hss, start_point = { xs[1], 0 }, start_level = s[1], start_offset = s[3],
    end_point = { xs[2], 0 }, end_level = s[2], end_offset = s[4],
  }
  reviera.brace.create{
    type_id = hss, start_point = { xs[2], 0 }, start_level = s[1], start_offset = s[3],
    end_point = { xs[1], 0 }, end_level = s[2], end_offset = s[4],
  }
end

-- Slabs on the framing.
local outline = {
  { line = { start = { 0, 0 }, ["end"] = { W, 0 } } },
  { line = { start = { W, 0 }, ["end"] = { W, D } } },
  { line = { start = { W, D }, ["end"] = { 0, D } } },
  { line = { start = { 0, D }, ["end"] = { 0, 0 } } },
}
local slabs = {}
for _, level in ipairs({ l2, roof }) do
  slabs[#slabs + 1] = reviera.floor.create{
    loops = { outline }, level_id = level, structural = true, span_direction_edge = 1,
  }
end
-- A 6" normal-weight concrete slab type for both.
local slab_type
for _, t in ipairs(reviera.type.list()) do
  if t.name == "Generic Floor - 300mm" then slab_type = t.id end
end
slab_type = reviera.type.duplicate{ type_id = slab_type, new_name = "Concrete Slab 6\"" }
reviera.compound.set_layers{ type_id = slab_type, layers = {
  { ["function"] = "structure", material = "Concrete f'c 4000 psi", thickness = "6\"" },
} }
reviera.element.set_type{ element_ids = slabs, type_id = slab_type }

-- Footings under the columns.
reviera.footing.place_at_columns{ column_ids = cols }

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
local dead = reviera.load_case.create{ name = "DL", nature = "D", self_weight = true }
local live = reviera.load_case.create{ name = "LL", nature = "L" }
reviera.load_case.create{ name = "LR", nature = "Lr" }
local wind = reviera.load_case.create{ name = "WIND", nature = "Wind" }
-- Slab and finishes, and office live load, on the Level 2 girders as line loads: 50 psf dead
-- and 50 psf live over half a bay each side (12.5 ft), about 9.1 kN/m each.
local model = reviera.analytical_model.info{}
local q = 9100.0
for _, m in ipairs(model.members) do
  local z = m.start[3]
  if m.role == "beam" and math.abs(m.start[2] - m["end"][2]) < 1e-6
      and math.abs(z - (4.2672 - SLAB)) < 0.2 and math.abs(m.start[2] - ys[2]) < 1e-6 then
    reviera.load.create_line{ host_id = m.id, case_id = dead, start = { 0, 0, -q } }
    reviera.load.create_line{ host_id = m.id, case_id = live, start = { 0, 0, -q } }
  end
end
-- Wind on the roof at grid A, along x.
for _, n in ipairs(model.members) do
  if n.role == "column" and n["end"][3] > 8.0 and math.abs(n["end"][1]) < 1e-6 then
    reviera.load.create_point{ host_id = n.nodes[2], case_id = wind, forces = { 15000, 0, 0, 0, 0, 0 } }
  end
end
reviera.load_combination.generate{ add = true }
