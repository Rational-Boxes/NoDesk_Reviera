// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

// Solid booleans on closed triangle meshes through OpenCASCADE: each closed part becomes a
// solid (its triangles as planar faces, sewn into a shell), the boolean runs on the compounds,
// and the result is triangulated back.

#include "reviera-geom/cpp/shim.h"
#include "reviera-geom/src/ffi.rs.h"

#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakePolygon.hxx>
#include <BRepBuilderAPI_MakeSolid.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <BRepClass3d_SolidClassifier.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRep_Builder.hxx>
#include <BRep_Tool.hxx>
#include <Poly_Triangulation.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Version.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Shell.hxx>
#include <TopoDS_Solid.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>

#include <array>
#include <cmath>
#include <map>
#include <numeric>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <vector>

namespace reviera_geom {

namespace {

gp_Pnt point(const Meshes &m, uint32_t i) {
  return gp_Pnt(m.verts[3 * i], m.verts[3 * i + 1], m.verts[3 * i + 2]);
}

// Welded vertices: positions equal to within 1e-7 m share an index.
struct Weld {
  std::map<std::array<long long, 3>, uint32_t> ids;
  std::vector<gp_Pnt> points;
  uint32_t id(const gp_Pnt &p) {
    std::array<long long, 3> k = {std::llround(p.X() * 1e7), std::llround(p.Y() * 1e7),
                                  std::llround(p.Z() * 1e7)};
    auto [it, fresh] = ids.emplace(k, static_cast<uint32_t>(points.size()));
    if (fresh) {
      points.push_back(p);
    }
    return it->second;
  }
};

uint64_t edge_key(uint32_t a, uint32_t b) { return (uint64_t(a) << 32) | b; }

uint32_t find(std::vector<uint32_t> &up, uint32_t i) {
  while (up[i] != i) {
    up[i] = up[up[i]];
    i = up[i];
  }
  return i;
}

TopoDS_Wire polygon_wire(const Weld &w, const std::vector<uint32_t> &loop) {
  BRepBuilderAPI_MakePolygon poly;
  for (uint32_t v : loop) {
    poly.Add(w.points[v]);
  }
  poly.Close();
  return poly.IsDone() ? poly.Wire() : TopoDS_Wire();
}

// The faces bounding triangles [first, last) of `m`: each connected run of coplanar triangles
// becomes one planar face (its boundary loops, holes included), so a body cut many times does
// not keep its pieces as separate faces, and OCCT never sees the slivers between them. The
// merge is linear in the triangles, unlike ShapeUpgrade_UnifySameDomain on the result, which
// can loop without bound. A run whose boundary does not form simple loops (it touches itself
// at a vertex) stays as its triangles.
std::vector<TopoDS_Face> merged_faces(const Meshes &m, size_t first, size_t last, double tol) {
  Weld w;
  std::vector<std::array<uint32_t, 3>> tris;
  std::vector<gp_Dir> normal;
  std::vector<double> offset;
  for (size_t t = first; t < last; ++t) {
    std::array<uint32_t, 3> v = {w.id(point(m, m.faces[3 * t])), w.id(point(m, m.faces[3 * t + 1])),
                                 w.id(point(m, m.faces[3 * t + 2]))};
    if (v[0] == v[1] || v[1] == v[2] || v[2] == v[0]) {
      continue; // degenerate
    }
    const gp_Pnt &p0 = w.points[v[0]];
    gp_Vec n = gp_Vec(p0, w.points[v[1]]).Crossed(gp_Vec(p0, w.points[v[2]]));
    if (n.Magnitude() < tol * tol) {
      continue; // degenerate
    }
    gp_Dir d(n);
    tris.push_back(v);
    normal.push_back(d);
    offset.push_back(d.X() * p0.X() + d.Y() * p0.Y() + d.Z() * p0.Z());
  }
  // Join neighbours across shared edges when they lie in one plane.
  std::unordered_map<uint64_t, uint32_t> by_edge;
  for (uint32_t t = 0; t < tris.size(); ++t) {
    for (int k = 0; k < 3; ++k) {
      by_edge.emplace(edge_key(tris[t][k], tris[t][(k + 1) % 3]), t);
    }
  }
  std::vector<uint32_t> up(tris.size());
  std::iota(up.begin(), up.end(), 0u);
  for (uint32_t t = 0; t < tris.size(); ++t) {
    for (int k = 0; k < 3; ++k) {
      auto it = by_edge.find(edge_key(tris[t][(k + 1) % 3], tris[t][k]));
      if (it == by_edge.end()) {
        continue;
      }
      uint32_t u = it->second;
      if (normal[t].Dot(normal[u]) > 1.0 - 1e-9 && std::abs(offset[t] - offset[u]) < tol) {
        up[find(up, t)] = find(up, u);
      }
    }
  }
  std::unordered_map<uint32_t, std::vector<uint32_t>> regions;
  for (uint32_t t = 0; t < tris.size(); ++t) {
    regions[find(up, t)].push_back(t);
  }
  std::vector<TopoDS_Face> faces;
  auto one_by_one = [&](const std::vector<uint32_t> &ts) {
    for (uint32_t t : ts) {
      TopoDS_Wire wire = polygon_wire(w, {tris[t][0], tris[t][1], tris[t][2]});
      if (wire.IsNull()) {
        continue;
      }
      BRepBuilderAPI_MakeFace face(wire, Standard_True);
      if (face.IsDone()) {
        faces.push_back(face.Face());
      }
    }
  };
  for (auto &[root, ts] : regions) {
    if (ts.size() == 1) {
      one_by_one(ts);
      continue;
    }
    // Boundary edges: those whose reverse is not in the run, chained into loops.
    std::unordered_map<uint64_t, bool> inside;
    for (uint32_t t : ts) {
      for (int k = 0; k < 3; ++k) {
        inside[edge_key(tris[t][k], tris[t][(k + 1) % 3])] = true;
      }
    }
    std::unordered_map<uint32_t, uint32_t> next;
    bool simple = true;
    for (uint32_t t : ts) {
      for (int k = 0; k < 3; ++k) {
        uint32_t a = tris[t][k], b = tris[t][(k + 1) % 3];
        if (!inside.count(edge_key(b, a)) && !next.emplace(a, b).second) {
          simple = false; // the boundary touches itself here
        }
      }
    }
    std::vector<std::vector<uint32_t>> loops;
    while (simple && !next.empty()) {
      uint32_t start = next.begin()->first, v = start;
      std::vector<uint32_t> loop;
      while (true) {
        auto it = next.find(v);
        if (it == next.end()) {
          simple = false;
          break;
        }
        loop.push_back(v);
        v = it->second;
        next.erase(it);
        if (v == start) {
          break;
        }
      }
      if (loop.size() < 3) {
        simple = false;
      }
      loops.push_back(std::move(loop));
    }
    // One loop runs anticlockwise about the normal (the outside); the rest are holes.
    const gp_Dir &n = normal[ts[0]];
    int outer = -1;
    for (size_t i = 0; simple && i < loops.size(); ++i) {
      gp_Vec area(0, 0, 0);
      for (size_t j = 0; j < loops[i].size(); ++j) {
        area += gp_Vec(w.points[loops[i][j]].XYZ())
                    .Crossed(gp_Vec(w.points[loops[i][(j + 1) % loops[i].size()]].XYZ()));
      }
      if (area.Dot(gp_Vec(n)) > 0) {
        simple = simple && outer < 0;
        outer = static_cast<int>(i);
      }
    }
    if (!simple || outer < 0) {
      one_by_one(ts);
      continue;
    }
    TopoDS_Wire outside = polygon_wire(w, loops[outer]);
    if (outside.IsNull()) {
      one_by_one(ts);
      continue;
    }
    BRepBuilderAPI_MakeFace face(gp_Pln(w.points[loops[outer][0]], n), outside, Standard_True);
    for (size_t i = 0; face.IsDone() && i < loops.size(); ++i) {
      if (static_cast<int>(i) != outer) {
        TopoDS_Wire hole = polygon_wire(w, loops[i]);
        if (hole.IsNull()) {
          simple = false;
          break;
        }
        face.Add(hole);
      }
    }
    if (simple && face.IsDone()) {
      faces.push_back(face.Face());
    } else {
      one_by_one(ts);
    }
  }
  return faces;
}

// The solid bounded by triangles [first, last) of `m`, facing out.
TopoDS_Shape solid(const Meshes &m, size_t first, size_t last, double tol) {
  BRepBuilderAPI_Sewing sew(tol);
  size_t faces = 0;
  for (const TopoDS_Face &f : merged_faces(m, first, last, tol)) {
    sew.Add(f);
    ++faces;
  }
  if (faces < 4) {
    throw std::runtime_error("a part has too few faces to be a solid");
  }
  sew.Perform();
  BRepBuilderAPI_MakeSolid make;
  size_t shells = 0;
  for (TopExp_Explorer e(sew.SewedShape(), TopAbs_SHELL); e.More(); e.Next()) {
    make.Add(TopoDS::Shell(e.Current()));
    ++shells;
  }
  if (shells == 0) {
    throw std::runtime_error("a part did not close into a shell");
  }
  TopoDS_Solid s = make.Solid();
  // Facing out: a point far away must be outside.
  BRepClass3d_SolidClassifier cls(s);
  cls.PerformInfinitePoint(tol);
  if (cls.State() == TopAbs_IN) {
    s.Reverse();
  }
  return s;
}

TopoDS_Shape compound(const Meshes &m, double tol) {
  BRep_Builder b;
  TopoDS_Compound c;
  b.MakeCompound(c);
  size_t first = 0;
  for (uint32_t n : m.parts) {
    b.Add(c, solid(m, first, first + n, tol));
    first += n;
  }
  return c;
}

Meshes triangulate(const TopoDS_Shape &shape) {
  BRepMesh_IncrementalMesh mesh(shape, 0.005, Standard_False, 0.3);
  Meshes out;
  for (TopExp_Explorer s(shape, TopAbs_SOLID); s.More(); s.Next()) {
    uint32_t count = 0;
    for (TopExp_Explorer e(s.Current(), TopAbs_FACE); e.More(); e.Next()) {
      const TopoDS_Face &face = TopoDS::Face(e.Current());
      TopLoc_Location loc;
      Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(face, loc);
      if (tri.IsNull()) {
        continue;
      }
      uint32_t base = static_cast<uint32_t>(out.verts.size() / 3);
      gp_Trsf tr = loc.Transformation();
      for (int i = 1; i <= tri->NbNodes(); ++i) {
        gp_Pnt p = tri->Node(i).Transformed(tr);
        out.verts.push_back(p.X());
        out.verts.push_back(p.Y());
        out.verts.push_back(p.Z());
      }
      bool reversed = face.Orientation() == TopAbs_REVERSED;
      for (int i = 1; i <= tri->NbTriangles(); ++i) {
        int a, b, c;
        tri->Triangle(i).Get(a, b, c);
        if (reversed) {
          std::swap(b, c);
        }
        out.faces.push_back(base + a - 1);
        out.faces.push_back(base + b - 1);
        out.faces.push_back(base + c - 1);
        ++count;
      }
    }
    if (count > 0) {
      out.parts.push_back(count);
    }
  }
  return out;
}

} // namespace

Meshes mesh_boolean(uint8_t op, const Meshes &a, const Meshes &b, double tolerance) {
  try {
    TopoDS_Shape sa = compound(a, tolerance), sb = compound(b, tolerance);
    TopoDS_Shape result;
    switch (op) {
    case 0: {
      BRepAlgoAPI_Cut cut(sa, sb);
      if (cut.HasErrors()) throw std::runtime_error("the subtraction failed");
      result = cut.Shape();
      break;
    }
    case 1: {
      BRepAlgoAPI_Common common(sa, sb);
      if (common.HasErrors()) throw std::runtime_error("the intersection failed");
      result = common.Shape();
      break;
    }
    case 2: {
      BRepAlgoAPI_Fuse fuse(sa, sb);
      if (fuse.HasErrors()) throw std::runtime_error("the union failed");
      result = fuse.Shape();
      break;
    }
    default:
      throw std::runtime_error("unknown boolean operation");
    }
    // Coplanar faces are not merged: ShapeUpgrade_UnifySameDomain can loop without bound (it
    // allocated past 8 GB on a roof cut four times over, in the manor sample) and cannot be
    // interrupted. The result's faces are triangulated as they are.
    return triangulate(result);
  } catch (const Standard_Failure &e) {
    throw std::runtime_error(std::string("OpenCASCADE: ") + e.GetMessageString());
  }
}

rust::String occt_version() { return rust::String(OCC_VERSION_COMPLETE); }

} // namespace reviera_geom
