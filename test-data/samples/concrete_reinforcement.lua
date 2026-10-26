-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Reinforced and precast concrete
-- description: A small cast-in-place frame (two columns on pad footings, a beam, a slab and a wall) reinforced with the Concrete tab's tools, beside a precast wall split into reinforced pieces with lifting anchors and a precast slab with custom fabric, with shop drawings, tags, a bending detail and the bar bending schedule.
--
-- Reviera sample model (test-data/samples): reinforcement to UK practice (Approved Document K
-- sets the UK region, so metric bars and BS 8666 shape codes):
-- - a cast-in-place frame: two 400 × 400 columns 4 m apart on pad footings, a 300 × 500 beam
--   over them, a 250 mm slab on the beam at Level 2 (3 m) and a 200 mm wall under its far edge;
-- - settings: area and path reinforcement as real bars, lengths rounded to 10 mm (STR-RBR-002);
--   cover types of 40 mm for the columns and 30 mm for the wall (STR-RBR-003);
-- - column C1: Ø10 links (shape 51) by Expand to Host (STR-RBR-004) and four Ø20 bars morphed
--   through them (STR-RBR-014); column C2: free-form links aligned to its axis (STR-RBR-013);
-- - the beam: links and four corner bars the same way, and a middle top bar sketched along its
--   long section, held under the top cover and at the end covers (STR-RBR-007);
-- - the slab: area reinforcement in the bottom (STR-RBR-010), path reinforcement over the beam
--   (STR-RBR-011), A252 fabric in the west half of the top (STR-RBR-012) and a Ø10 top layer in
--   the east half by Expand to Host within a boundary;
-- - the wall: area reinforcement at its exterior face turned into rebar sets (Remove Area
--   System), a single B503 fabric sheet at its interior face;
-- - precast: pieces at most 2.5 m long (Precast Settings, STR-PRE-001); a 4.5 m wall with an
--   opening split into two pieces (STR-PRE-002), reinforced with area bars and U-bars round the
--   opening (STR-PRE-003), lifting anchors (STR-PRE-005); a 4.8 × 2.4 m slab split into two
--   pieces with custom fabric (STR-PRE-004); a shop drawing sheet for each piece (STR-PRE-006);
-- - documentation: a rebar tag and a bending detail in the Level 1 plan, a multi-rebar
--   annotation in the Level 2 plan, the bar bending schedule (STR-RBR-018); the frame's sets in
--   their own numbering partition (STR-RBR-016).

reviera.code.set{ code = "uk_adk" }
local l1 = reviera.level.list()[1].id
local l2 = reviera.level.create{ elevation = 3.0, name = "Level 2" }
reviera.view.create_plan{ level_ids = { l1, l2 } }
reviera.view.create_3d{}
local plan, plan2
for _, v in ipairs(reviera.view.list()) do
  if v.kind ~= "3d" then
    if v.name == "Level 1" then plan = v.id elseif v.name == "Level 2" then plan2 = v.id end
  end
end
local function label(x, y, text)
  reviera.text.create{ view_id = plan, position = { x, y }, content = text }
end
local function type_named(name, class)
  for _, t in ipairs(reviera.type.list()) do
    if (name == nil or t.name == name) and (class == nil or t.class == class) then return t.id end
  end
end
local function loop(x0, y0, x1, y1)
  local c = { { x0, y0 }, { x1, y0 }, { x1, y1 }, { x0, y1 } }
  local out = {}
  for i = 1, 4 do
    out[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
  end
  return out
end

-- Settings, before the first reinforcement: area and path reinforcement as real bars (so a
-- system can be turned into sets), lengths rounded to 10 mm.
reviera.reinforcement_settings.set{ key = "area_path_rebar", value = true }
reviera.reinforcement_settings.set{ key = "rounding.on", value = true }
reviera.reinforcement_settings.set{ key = "rounding.bar_length", value = 0.01 }

-- The cast-in-place frame. Slab 250 thick at Level 2; the beam's top under it, the columns'
-- tops under the beam.
local SLAB, BEAM = 0.25, 0.5
local c1 = reviera.column.place{ point = { 0, 0 }, base_level = l1, top_constraint = { level = l2 }, top_offset = -(SLAB + BEAM) }
local c2 = reviera.column.place{ point = { 4, 0 }, base_level = l1, top_constraint = { level = l2 }, top_offset = -(SLAB + BEAM) }
reviera.footing.place_at_columns{ column_ids = { c1, c2 } }

-- A concrete 300 × 500 beam type, from the default beam's.
local probe = reviera.beam.create{ curve = { start = { 0, -20 }, ["end"] = { 1, -20 } }, reference_level = l1 }
local beam_type = reviera.type.duplicate{ type_id = type_named(nil, "IfcBeamType"), new_name = "Concrete Beam 300 x 500" }
reviera.element.delete{ element_ids = { probe } }
for k, v in pairs({ section = "rectangular", width = "300 mm", depth = "500 mm", material_kind = "concrete" }) do
  reviera.type.set_param{ type_id = beam_type, param = k, value = v }
end
local beam = reviera.beam.create{
  type_id = beam_type, curve = { start = { 0, 0 }, ["end"] = { 4, 0 } }, reference_level = l2,
  start_offset = -SLAB, end_offset = -SLAB,
}

local slab = reviera.floor.create{ loops = { loop(-0.2, -0.15, 4.2, 3.1) }, level_id = l2, structural = true }
local slab_type = reviera.type.duplicate{ type_id = type_named("Generic Floor - 300mm"), new_name = "Concrete Slab 250" }
reviera.compound.set_layers{ type_id = slab_type, layers = {
  { ["function"] = "structure", material = "Concrete", thickness = "250 mm" },
} }
reviera.element.set_type{ element_ids = { slab }, type_id = slab_type }

local wall = reviera.wall.create{
  curve = { line = { start = { 0, 3 }, ["end"] = { 4, 3 } } }, base_level = l1,
  top_constraint = { unconnected = 3.0 - SLAB }, structural = true,
}
local wall_type = reviera.type.duplicate{ type_id = type_named(nil, "IfcWallType"), new_name = "Concrete Wall 200" }
reviera.compound.set_layers{ type_id = wall_type, layers = {
  { ["function"] = "structure", material = "Concrete", thickness = "200 mm" },
} }
reviera.element.set_type{ element_ids = { wall }, type_id = wall_type }

-- Covers.
local c40 = reviera.rebar_cover_type.create{ name = "Columns 40", offset = 0.04 }
reviera.rebar_cover.assign{ host_ids = { c1, c2 }, cover_type_id = c40 }
local c30 = reviera.rebar_cover_type.create{ name = "Walls 30", offset = 0.03 }
reviera.rebar_cover.assign{ host_ids = { wall }, cover_type_id = c30 }

local frame = {}
local function keep(s) frame[#frame + 1] = s; return s end

-- Column C1: Ø10 links at 300 by Expand to Host, and four Ø20 bars morphed through them, each
-- touching the links from inside.
local c1_links = keep(reviera.rebar.place{ host_id = c1, shape = "51", bar_type = "d10", placement = { kind = "section" }, layout = { rule = "maximum_spacing", spacing = 0.3 } })
keep(reviera.rebar.distribute_morphed{ driver_stirrup_set_id = c1_links, bar_type = "d20", count = 4 })
-- Column C2: free-form links square to its axis.
keep(reviera.rebar.place_free_form{ host_id = c2, kind = "aligned", path_ref = "axis", bar_type = "d10", layout = { rule = "maximum_spacing", spacing = 0.3 } })

-- The beam: Ø10 links at 300, four Ø16 corner bars morphed through them, and a middle Ø16 top
-- bar sketched along its long section, held inside the links under the top cover and at the
-- end covers.
local beam_links = keep(reviera.rebar.place{ host_id = beam, shape = "51", bar_type = "d10", placement = { kind = "section" }, layout = { rule = "maximum_spacing", spacing = 0.3 } })
local corners = keep(reviera.rebar.distribute_morphed{ driver_stirrup_set_id = beam_links, bar_type = "d16", count = 4 })
local top = keep(reviera.rebar.sketch{ host_id = beam, curves = { { line = { start = { 0.5, 0.15 }, ["end"] = { 3.5, 0.15 } } } }, bar_type = "d16" })
reviera.rebar.constrain{ rebar_id = top, handle_id = "segment:0", target_kind = "cover", target_ref = "top", offset = 0.012 }
reviera.rebar.constrain{ rebar_id = top, handle_id = "start", target_kind = "cover", target_ref = "start" }
reviera.rebar.constrain{ rebar_id = top, handle_id = "end", target_kind = "cover", target_ref = "end" }

-- The slab: Ø10 at 200 both ways in the bottom; Ø12 hogging bars at 200 over the beam, 1.2 m
-- into the slab; A252 fabric in the top beyond them, over the west half.
reviera.area_reinforcement.create{
  host_id = slab, boundary = "whole_host", major_edge = 0, layers = {
    { face = "bottom", direction = "major", bar_type = "d10", spacing = 0.2 },
    { face = "bottom", direction = "minor", bar_type = "d10", spacing = 0.2 },
  },
}
reviera.path_reinforcement.create{
  host_id = slab, path = { { 0.3, -0.15 }, { 3.7, -0.15 } }, face = "top", spacing = 0.2,
  primary = { bar_type = "d12", length = 1.2 },
}
reviera.fabric_area.create{
  host_id = slab, boundary = { { -0.2, 1.4 }, { 1.9, 1.4 }, { 1.9, 3.1 }, { -0.2, 3.1 } },
  sheet_type_id = "A252", major_lap = 0.3, minor_lap = 0.3, face = "top",
}
-- In the east half, a top layer of Ø10 at 200 placed by Expand to Host within a boundary.
local slab_top = keep(reviera.rebar.place{
  host_id = slab, shape = "00", bar_type = "d10",
  placement = { kind = "layer", face = "top", angle = 0, boundary = { { 2.1, 1.4 }, { 4.2, 1.4 }, { 4.2, 3.1 }, { 2.1, 3.1 } } },
  layout = { rule = "maximum_spacing", spacing = 0.2 },
})

-- The wall: Ø12 at 200 both ways at its exterior face, as area reinforcement turned into rebar
-- sets; one B503 sheet at its interior face.
local wall_area = reviera.area_reinforcement.create{
  host_id = wall, boundary = "whole_host", layers = {
    { face = "exterior", direction = "major", bar_type = "d12", spacing = 0.2 },
    { face = "exterior", direction = "minor", bar_type = "d12", spacing = 0.2 },
  },
}
local wall_sets = reviera.area_reinforcement.remove_system{ system_id = wall_area }
for _, s in ipairs(wall_sets) do keep(s) end
reviera.fabric_sheet.place{ host_id = wall, sheet_type_id = "B503", point = { 0.6, 0.3 }, cut_by_cover = true, face = "interior" }

-- The frame's sets in their own numbering partition.
reviera.rebar.set_partition{ ids = frame, name = "Frame" }

-- Precast: wall pieces at most 2.5 m long (Precast Settings: 8 m by default); a 4.5 m wall,
-- 2.4 m high, with a 0.8 m opening, split into two, reinforced, with lifting anchors.
reviera.precast_config.set{ path = "wall.solid.segmentation.max_length", value = 2.5 }
local pw = reviera.wall.create{
  type_id = wall_type, curve = { line = { start = { 0, 7 }, ["end"] = { 4.5, 7 } } }, base_level = l1,
  top_constraint = { unconnected = 2.4 }, structural = true,
}
reviera.opening.create_wall{ wall_id = pw, u0 = 0.6, u1 = 1.4 }
local pieces = reviera.precast.split{ element_ids = { pw } }
reviera.precast.reinforce{ assembly_ids = pieces, area_type_id = "A10-150", edge_type_id = "U10-200" }
reviera.precast.place_mounting_parts{ assembly_ids = pieces }
-- A 4.8 × 2.4 m precast slab at Level 1, split into two pieces, with custom fabric at each face.
local pf = reviera.floor.create{ loops = { loop(0, 9, 4.8, 11.4) }, level_id = l1, structural = true }
reviera.element.set_type{ element_ids = { pf }, type_id = slab_type }
local slab_pieces = reviera.precast.split{ element_ids = { pf } }
reviera.precast.generate_custom_fabric{ assembly_ids = slab_pieces }
local all = {}
for _, a in ipairs(pieces) do all[#all + 1] = a end
for _, a in ipairs(slab_pieces) do all[#all + 1] = a end
reviera.precast.shop_drawings{ assembly_ids = all, group_identical = false }

-- Documentation: a tag on C1's links and their bending detail in the Level 1 plan; a
-- multi-rebar annotation across the slab's top layer in the Level 2 plan; the schedule.
reviera.rebar_tag.place{ rebar_id = c1_links, view_id = plan, point = { -1.2, 0.8 } }
reviera.bending_detail.place{ rebar_id = c1_links, view_id = plan, point = { -3.0, -1.0 } }
reviera.multi_rebar_annotation.place{ rebar_ids = { slab_top }, view_id = plan2, kind = "aligned", line = { 3.8, 2.2 } }
reviera.rebar_schedule.create{}

label(-0.5, -1.4, "C1 (west): links by Expand to Host, Ø20 bars morphed through them, 40 mm cover; tag, bending detail")
label(-0.5, -1.9, "C2 (east): free-form links along its axis")
label(0.5, 0.6, "Beam (Level 2): links, morphed corner bars, a sketched top bar under the top cover")
label(0.5, 2.0, "Slab (Level 2): area bars bottom; path bars over the beam; A252 fabric area | Ø10 top layer")
label(0.5, 3.6, "Wall: area bars made rebar sets (Remove Area System); B503 sheet; 30 mm cover")
label(0.0, 7.6, "Precast wall: split into W1-1, W1-2; area bars, U-bars round the opening, lifting anchors")
label(0.0, 11.9, "Precast slab: split into S1-1, S1-2, custom fabric; shop drawings for the four pieces")
