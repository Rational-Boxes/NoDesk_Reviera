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
#include <gp_Pnt.hxx>

#include <stdexcept>
#include <string>

namespace reviera_geom {

namespace {

gp_Pnt point(const Meshes &m, uint32_t i) {
  return gp_Pnt(m.verts[3 * i], m.verts[3 * i + 1], m.verts[3 * i + 2]);
}

// The solid bounded by triangles [first, last) of `m`, facing out.
TopoDS_Shape solid(const Meshes &m, size_t first, size_t last, double tol) {
  BRepBuilderAPI_Sewing sew(tol);
  size_t faces = 0;
  for (size_t t = first; t < last; ++t) {
    gp_Pnt p0 = point(m, m.faces[3 * t]), p1 = point(m, m.faces[3 * t + 1]),
           p2 = point(m, m.faces[3 * t + 2]);
    gp_Vec n = gp_Vec(p0, p1).Crossed(gp_Vec(p0, p2));
    if (n.Magnitude() < tol * tol) {
      continue; // degenerate
    }
    BRepBuilderAPI_MakePolygon poly(p0, p1, p2, Standard_True);
    if (!poly.IsDone()) {
      continue;
    }
    BRepBuilderAPI_MakeFace face(poly.Wire(), Standard_True);
    if (face.IsDone()) {
      sew.Add(face.Face());
      ++faces;
    }
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
