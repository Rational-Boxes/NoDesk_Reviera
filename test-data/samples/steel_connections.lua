-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Steel connection detailing
-- description: One braced steel bay detailed for fabrication: connections placed by automation and from the library, a custom end-plate-and-doubler connection used twice, a shear plate checked and presized, a coped infill beam, hand plates with bolts and a weld, miter, saw, cut-through and cut-by cuts, a shortened end, corner and contour cuts, and steel numbering ready for DSTV NC output.
--
-- Reviera sample model (test-data/samples): the Steel tab's tools (STR-STL-001 to STR-STL-010)
-- on a small frame to US practice (IBC; AISC shapes):
-- - a 6 × 6 m bay: four W12X53 columns to Level 2 at 4 m, W18X35 girders east–west, W16X26
--   beams north–south, a W12X26 infill beam at mid-span and an HSS6X6X3/8 brace on the south side;
-- - a custom connection: an extended end plate and a doubler plate on the far side of the
--   column's web, made into a type at the south-west girder end and placed again at the
--   north-west one, the doubler's thickness exposed and set to 12 mm;
-- - the infill beam extended to the girders' webs (Shorten) and coped at both ends;
-- - connection automation for every other join: base plates, shear plates (beam to column and
--   beam to beam) and the brace's gussets;
-- - a shear plate given a design reaction, checked with the sample connection design provider
--   (AISC 360) and presized;
-- - a detailing bench at Level 2 east of the bay: a bolted splice of two plates with a welded
--   stiffener, a chamfered and a rounded corner and a contour-cut hole; a miter, a saw cut, a
--   diaphragm cut through by a beam with a gap, a plate cut by another and a skewed shortened end;
-- - steel numbering: part and assembly marks in each element's Tag. Steel › Fabrication › NC
--   Export writes a DSTV file for each part mark.

reviera.code.set{ code = "ibc" }
local l1 = reviera.level.list()[1].id
local l2 = reviera.level.create{ elevation = 4.0, name = "Level 2" }
reviera.view.create_plan{ level_ids = { l2 } }
reviera.view.create_3d{}
local plan
for _, v in ipairs(reviera.view.list()) do
  if v.name == "Level 2" and v.kind ~= "3d" then plan = v.id end
end
local function label(x, y, text)
  reviera.text.create{ view_id = plan, position = { x, y }, content = text }
end

local IN = 0.0254
local column = reviera.column.load_section{ catalog = "aisc_w", designation = "W12X53" }
local girder = reviera.beam.load_section{ catalog = "aisc_w", designation = "W18X35" }
local beam = reviera.beam.load_section{ catalog = "aisc_w", designation = "W16X26" }
local infill = reviera.beam.load_section{ catalog = "aisc_w", designation = "W12X26" }
local hss = reviera.brace.load_section{ catalog = "aisc_hss_rect", designation = "HSS6X6X3/8" }
local function member(ty, a, b)
  return reviera.beam.create{ type_id = ty, curve = { start = a, ["end"] = b }, reference_level = l2 }
end

-- The bay.
local cols = {}
for k, p in ipairs({ { 0, 0 }, { 6, 0 }, { 6, 6 }, { 0, 6 } }) do
  cols[k] = reviera.column.place{ type_id = column, point = p, base_level = l1, top_constraint = { level = l2 } }
end
local south = member(girder, { 0, 0 }, { 6, 0 })
local north = member(girder, { 0, 6 }, { 6, 6 })
member(beam, { 0, 0 }, { 0, 6 })
member(beam, { 6, 0 }, { 6, 6 })
local mid = member(infill, { 3, 0 }, { 3, 6 })
reviera.brace.create{ type_id = hss, start_point = { 0, 0 }, start_level = l1, end_point = { 6, 0 }, end_level = l2 }
label(0.5, 3.0, "Bay: W12X53 columns, W18X35 girders, W16X26 beams")
label(0.5, 2.5, "Brace HSS6X6X3/8 on the south side")

-- A custom connection (Steel › Custom › Customize): an extended end plate from the library at
-- the south-west girder end and a 10 mm doubler by hand (Steel › Detailing › Plate) on the far
-- side of the column's web, made into a type and placed again at the north-west girder end,
-- where it makes both in the same place relative to the join. Its doubler's thickness is exposed
-- and set to 12 mm (Set Parameter), which every instance follows.
local ep = reviera.structural_connection.create{ member_ids = { cols[1], south }, type_id = "end_plate" }
local far, lowest
for _, f in ipairs(reviera.face.list{ element_ids = { cols[1] } }) do
  if f.face:sub(1, 3) == "web" and (lowest == nil or f.normal[1] < lowest) then
    far, lowest = f.face, f.normal[1]
  end
end
local doubler = reviera.plate.create{
  face = { element = cols[1], face = far }, contour = { { 3.5, -0.1 }, { 3.9, -0.1 }, { 3.9, 0.1 }, { 3.5, 0.1 } },
  thickness = 0.01,
}
local custom = reviera.structural_connection.customize{
  connection_ids = { ep }, element_ids = { doubler }, name = "End plate and doubler",
}
reviera.structural_connection.create{ member_ids = { cols[4], north }, type_id = custom.type_id }
reviera.custom_connection.expose_parameter{ type_id = custom.type_id, path = "Plate 1.thickness" }
reviera.custom_connection.set_parameter{ type_id = custom.type_id, path = "Plate 1.thickness", value = 0.012 }
label(-1.5, -1.0, "Custom connection: end plate + doubler")
label(-1.5, 7.0, "Custom connection placed again")

-- The infill beam stops clear of the girders' flanges: extend each end to 13 mm off the girder's
-- web (W18X35, a 0.300" web), then cope it at both girders (Steel › Cuts › Cope).
local reach = 0.300 * IN / 2 + 0.013
local length = reviera.beam.info{ beam_ids = { mid } }[1].length
local extend = ((6 - 2 * reach) - length) / 2
reviera.steel_modifier.shorten{ member_id = mid, ["end"] = "start", length = -extend }
reviera.steel_modifier.shorten{ member_id = mid, ["end"] = "end", length = -extend }
reviera.steel_cut.cope{ member_ids = { south, mid } }
reviera.steel_cut.cope{ member_ids = { north, mid } }
label(3.2, 0.5, "Infill W12X26: extended and coped")

-- Every other join by Connection Automation (Steel › Connections › Automate), the Default rules.
local auto = reviera.structural_connection.automate{}
label(6.5, -1.0, "Automation: base plates, shear plates, gussets")

-- A shear plate's design reaction (246 kN), checked and presized with the sample provider.
local sp
for _, j in ipairs(auto.joins) do
  if j.status == "placed" and j.kind == "beam_column" and j.members[2] == north then sp = j.connection end
end
reviera.structural_connection.set_design_forces{ connection_ids = { sp }, forces = { { group = "ASCE7 2.3.2", v = 246000 } } }
reviera.structural_connection.check{ connection_ids = { sp }, code = "aisc360" }
reviera.structural_connection.presize{ connection_ids = { sp }, code = "aisc360" }
label(6.5, 6.5, "Shear plate: checked and presized (AISC 360)")

-- The detailing bench, east of the bay at Level 2.
local Z = 4.0
local function plate(origin, x_dir, normal, contour, t, just)
  return reviera.plate.create{
    plane = { origin = origin, x_dir = x_dir, normal = normal }, contour = contour, thickness = t,
    justification = just,
  }
end
local function rect(w, h)
  return { { -w / 2, -h / 2 }, { w / 2, -h / 2 }, { w / 2, h / 2 }, { -w / 2, h / 2 } }
end

-- A bolted splice (Steel › Detailing): a 15 mm plate, a 10 mm cover plate on it and 2 × 3 3/4"
-- bolts at 75 mm through both, the US default; a 10 mm stiffener standing on the 15 mm plate with
-- a 6 mm fillet weld.
local p15 = plate({ 10, 1, Z }, { 1, 0, 0 }, { 0, 0, 1 }, rect(0.4, 0.3), 0.015)
local p10 = reviera.plate.create{ face = { element = p15, face = "top" }, contour = rect(0.3, 0.2), thickness = 0.010 }
reviera.bolt_pattern.create{
  element_ids = { p15, p10 }, face = { element = p10, face = "top" },
  layout = { shape = "rect", rows = 2, columns = 3, spacing_x = 0.075, spacing_y = 0.075 },
}
local stiff = plate({ 10.18, 1, Z + 0.015 }, { 0, 1, 0 }, { 1, 0, 0 }, { { -0.1, 0 }, { 0.1, 0 }, { 0.1, 0.1 }, { -0.1, 0.1 } }, 0.010, "centre")
local edge
for _, e in ipairs(reviera.weld.edges{ element_ids = { p15, stiff } }) do
  if math.abs(e[1][1] - 10.185) < 1e-6 and math.abs(e[2][1] - 10.185) < 1e-6 then edge = e end
end
reviera.weld.create{ element_ids = { p15, stiff }, edge = edge, weld_type = "fillet", a = 0.006 }
label(9.6, 1.5, "Plates, 2 x 3 bolts, 6 mm fillet weld")

-- Corner cuts (a 50 mm chamfer and a 50 mm round) and a 50 mm square contour-cut hole on a
-- plate (Steel › Modifiers).
local pc = plate({ 12, 1, Z }, { 1, 0, 0 }, { 0, 0, 1 }, { { 0, 0 }, { 0.3, 0 }, { 0.3, 0.2 }, { 0, 0.2 } }, 0.01)
reviera.steel_modifier.corner_cut{ plate_id = pc, corner = 0, kind = "straight", size = 0.05 }
reviera.steel_modifier.corner_cut{ plate_id = pc, corner = 2, kind = "convex", size = 0.05 }
reviera.steel_modifier.contour_cut{
  element_id = pc, face = { element = pc, face = "top" },
  contour = { { 0.125, 0.075 }, { 0.175, 0.075 }, { 0.175, 0.125 }, { 0.125, 0.125 } },
}
label(11.8, 1.5, "Corner cuts, contour cut")

-- Cut By (Steel › Cuts): a crossing plate takes its shape out of another.
local pb = plate({ 14, 1, Z }, { 1, 0, 0 }, { 0, 0, 1 }, { { 0, 0 }, { 0.3, 0 }, { 0.3, 0.2 }, { 0, 0.2 } }, 0.01)
local crossing = plate({ 14.25, 1, Z - 0.05 }, { 0, 1, 0 }, { 1, 0, 0 }, { { 0.02, 0 }, { 0.06, 0 }, { 0.06, 0.1 }, { 0.02, 0.1 } }, 0.01, "centre")
reviera.steel_cut.cut_by{ cut_id = pb, cutter_id = crossing }
label(13.8, 1.5, "Cut by")

-- A miter at a corner of two W12X26s.
local ma = member(infill, { 10, 3 }, { 12, 3 })
local mb = member(infill, { 12, 3 }, { 12, 5 })
reviera.steel_cut.miter{ a = ma, b = mb }
label(10.0, 3.3, "Miter")

-- A saw cut: a W12X26 cut flush with a W16X26's flange.
local sa = member(infill, { 14, 3 }, { 14, 5 })
local sb = member(beam, { 13, 5 }, { 15, 5 })
reviera.steel_cut.saw{ a = sa, b = sb, part = "flange" }
label(14.2, 3.3, "Saw cut")

-- Cut through: a 20 mm diaphragm plate with a W12X26 passing through it, a 10 mm gap round it.
local bt = member(infill, { 10, 7 }, { 13, 7 })
local diaphragm = plate({ 11.5, 7, Z - 0.2 }, { 0, 1, 0 }, { 1, 0, 0 }, rect(0.4, 0.5), 0.02, "centre")
reviera.steel_cut.cut_through{ cut_id = diaphragm, cutter_id = bt, gap = 0.01 }
label(10.0, 7.3, "Cut through (10 mm gap)")

-- Shorten: a free W12X26's end back 200 mm, the cut tilted 30° across its height.
local ss = member(infill, { 14, 7 }, { 17, 7 })
reviera.steel_modifier.shorten{ member_id = ss, ["end"] = "end", length = 0.2, angle_h = 30 }
label(14.2, 7.3, "Shortened, 30 deg end")

-- Part and assembly marks (Steel › Fabrication › Numbering).
reviera.steel_numbering.run{}
label(10.0, -1.0, "Numbered: marks in each Tag")

-- The brace runs node to node: its ends meet the columns, gussets and bolts at its connections,
-- accepted as intended.
for _, o in ipairs(reviera.overlap.find{}.overlaps) do
  if o.a_label:match("^IfcMember") or o.b_label:match("^IfcMember") then
    reviera.overlap.resolve{ a = o.a, b = o.b, mode = "accept" }
  end
end
