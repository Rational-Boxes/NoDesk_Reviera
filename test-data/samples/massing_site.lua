-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Massing and site
-- description: A podium block and a tower as conceptual masses with mass floors, modelled by face (walls, floors, roofs, a curtain system), on a sloping toposolid graded to a building pad, with a path, a lawn, site components and a property line.
--
-- Reviera sample model (test-data/samples): two in-place masses (ARC-MASS-001), a 24 × 14 m
-- podium 8 m high and a 10 × 10 m tower first 12 m high, with mass floors on every level
-- (ARC-MASS-002); walls by face round the podium, floors by mass floor in both, a roof by face
-- on each, and a curtain system by face round the tower (ARC-MASS-003, ARC-ROOF-003,
-- ARC-CW-007). The tower is then stretched to 16 m and its roof and curtain system updated to
-- the face. The site: a toposolid falling 1.5 m to the south (ARC-TOPO-001), contours every
-- 250 mm (ARC-TOPO-004), a graded region cut level to a building pad, its cut and fill measured
-- (ARC-TOPO-003), a raised path and lawn sub-divided from it (ARC-TOPO-002), shrubs and benches
-- standing on it and a property line from a table of bearings (ARC-SITE-001).

local l1 = reviera.level.list()[1].id
local levels = { l1 }
for k = 1, 3 do
  levels[#levels + 1] = reviera.level.create{ elevation = 4.0 * k }
end
reviera.view.create_plan{ level_ids = levels }
reviera.view.create_3d{}
local plan
for _, v in ipairs(reviera.view.list()) do
  if v.name == "Level 1" and v.kind ~= "3d" then plan = v.id end
end
local function label(x, y, text)
  reviera.text.create{ view_id = plan, position = { x, y }, content = text }
end
local function rect(x0, y0, x1, y1)
  return { { x0, y0 }, { x1, y0 }, { x1, y1 }, { x0, y1 } }
end

-- ---- The masses ----
-- Faces are named by form and side: side0 runs from the profile's first point to its second
-- (here the south face), then round counter-clockwise; `top` is the upper face.
local podium = reviera.mass.create{ name = "Podium", forms = { {
  kind = "extrusion", start = 0.0, ["end"] = 8.0, level = l1, profile = rect(0, 0, 24, 14),
} } }
local tower = reviera.mass.create{ name = "Tower", forms = { {
  kind = "extrusion", start = 0.0, ["end"] = 12.0, level = l1, profile = rect(28, 2, 38, 12),
} } }
reviera.mass.set_floors{ mass_id = podium, all = true }
reviera.mass.set_floors{ mass_id = tower, all = true }

-- ---- Model by face ----
-- The podium's walls stand outside its faces (their interior finish face on the face), so the
-- floors by mass floor fill the mass inside them; its roof sits on the top face.
reviera.wall.create_by_face{ mass_id = podium, location_line = "finish_face_interior",
  faces = { "form0.side0", "form0.side1", "form0.side2", "form0.side3" } }
local podium_floors = reviera.floor.create_by_mass_floor{ mass_id = podium }
reviera.roof.create_by_face{ mass_id = podium, faces = { "form0.top" }, thickness = 0.3 }
-- The tower: a curtain system round its four sides and a roof, then stretched to 16 m and both
-- updated to the faces; its floors are made from the four mass floors after.
local cs = reviera.curtainsystem.create_by_face{ mass_id = tower,
  faces = { "form0.side0", "form0.side1", "form0.side2", "form0.side3" } }
local troof = reviera.roof.create_by_face{ mass_id = tower, faces = { "form0.top" }, thickness = 0.3 }
reviera.mass.set_form{ mass_id = tower, index = 0, ["end"] = 16.0 }
reviera.element.update_to_face{ element_ids = { cs, troof[1] } }
local tower_floors = reviera.floor.create_by_mass_floor{ mass_id = tower }

-- ---- The site ----
-- Contours every 250 mm through the level.
reviera.site.set_settings{ contour_interval = 0.25, passing_elevation = 0.0 }
-- The existing ground: 60 × 42 m, rising 1.5 m from south to north (heights above Level 1),
-- below the buildings' slabs everywhere under them.
local function ground(y) return -1.26 + 1.5 * (y + 12) / 42 end
local pts = {}
for _, x in ipairs({ -10, 50 }) do
  for _, y in ipairs({ -12, 30 }) do
    pts[#pts + 1] = { x, y, ground(y) }
  end
end
local existing = reviera.toposolid.create{ level_id = l1, boundary = rect(-10, -12, 50, 30), points = pts }
-- The proposed ground: a graded copy with a level building pad 46 × 24 m at the slabs'
-- underside, 300 mm below the level: filled to the south, cut to the north. A ring of points
-- 2 m out keeps the ground beyond as it was.
local graded = reviera.toposolid.create_graded_region{ toposolid_id = existing }
local pad = {}
local function edge(x, y, out)
  pad[#pad + 1] = out and { x, y, ground(y) } or { x, y, -0.3 }
end
for _, r in ipairs({ { -4, -6, 42, 18, false }, { -6, -8, 44, 20, true } }) do
  local x0, y0, x1, y1, out = r[1], r[2], r[3], r[4], r[5]
  for x = x0, x1, 2 do
    edge(x, y0, out)
    edge(x, y1, out)
  end
  for y = y0 + 2, y1 - 2, 2 do
    edge(x0, y, out)
    edge(x1, y, out)
  end
end
reviera.toposolid.set_points{ toposolid_id = graded, points = pad }
-- The existing ground is kept for cut and fill, hidden under its graded copy.
-- A gravel path from the podium to the south boundary and a lawn north of the buildings, both
-- raised 50 mm from the graded ground.
reviera.toposolid.subdivide{ toposolid_id = graded, region = rect(10, -12, 13, -0.3), offset = 0.05 }
reviera.toposolid.subdivide{ toposolid_id = graded, region = rect(0, 22, 38, 28), offset = 0.05 }

-- Site components on the pad: shrubs and benches, as box types until there is a planting library.
local shrub = reviera.mep_type.create{ name = "Shrub 1200", category = "planting",
  size = { 1.2, 1.2, 1.5 }, color = { 0.30, 0.55, 0.25, 1.0 } }
local bench = reviera.mep_type.create{ name = "Bench 1800", category = "furniture",
  size = { 1.8, 0.6, 0.45 }, color = { 0.55, 0.40, 0.25, 1.0 } }
for _, x in ipairs({ 3, 7, 17, 21 }) do
  reviera.sitecomponent.place{ type_id = shrub, point = { x, -3.5 }, host_id = graded, level = l1 }
end
reviera.sitecomponent.place{ type_id = bench, point = { 26, -2.5 }, host_id = graded, level = l1 }
reviera.sitecomponent.place{ type_id = bench, point = { 33, -2.5 }, host_id = graded, level = l1 }

-- The property line, from a table of distances and bearings.
reviera.propertyline.create{ origin = { -8, -10 }, table = {
  { distance = 38.0, bearing = "N 0°E" },
  { distance = 56.0, bearing = "N 90°E" },
  { distance = 38.0, bearing = "S 0°E" },
  { distance = 56.0, bearing = "S 90°W" },
} }

label(0, 15.0, "Podium mass: walls by face, floors by mass floor, roof by face")
label(28, 13.0, "Tower mass: curtain system and roof by face, stretched to 16 m and updated")
label(-3, -7.5, "Graded region: building pad 400 mm down; cut and fill")
label(13.5, -11, "Sub-division: path")
label(1, 26.5, "Sub-division: lawn")
label(2, -5.0, "Site components: shrubs and benches")
label(-7.5, 27.5, "Property line 56 × 38 m")
