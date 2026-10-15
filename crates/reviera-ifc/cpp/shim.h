// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

// Thin C++ layer over IfcOpenShell for reviera-ifc. Entities are addressed by STEP instance
// number inside one open file; Reviera's durable identity is the GlobalId (spec DM-ID-001).

#pragma once

#include <cstdint>
#include <memory>

#include "rust/cxx.h"

namespace IfcParse {
class IfcFile;
}

namespace reviera_ifc {

struct EntityInfo;
struct Mesh;
struct StepRow;

class Model {
public:
    explicit Model(std::unique_ptr<IfcParse::IfcFile> file);
    ~Model();

    rust::String schema_name() const;
    std::size_t entity_count() const;
    rust::Vec<EntityInfo> entities_of_type(rust::Str class_name) const;
    std::int64_t instance_by_guid(rust::Str guid) const;
    rust::String entity_class(std::uint32_t id) const;
    rust::Vec<rust::String> attribute_names(std::uint32_t id) const;
    rust::String attributes_json(std::uint32_t id) const;
    rust::String attributes_json_many(rust::Slice<const std::uint32_t> ids) const;

    void set_null(std::uint32_t id, rust::Str attr);
    void set_string(std::uint32_t id, rust::Str attr, rust::Str value);
    void set_real(std::uint32_t id, rust::Str attr, double value);
    void set_int(std::uint32_t id, rust::Str attr, std::int32_t value);
    void set_bool(std::uint32_t id, rust::Str attr, bool value);
    void set_enum(std::uint32_t id, rust::Str attr, rust::Str value);
    void set_ref(std::uint32_t id, rust::Str attr, std::uint32_t target);
    void set_ref_list(std::uint32_t id, rust::Str attr, rust::Slice<const std::uint32_t> targets);
    void set_real_list(std::uint32_t id, rust::Str attr, rust::Slice<const double> values);
    void set_int_list(std::uint32_t id, rust::Str attr, rust::Slice<const std::int32_t> values);
    void set_real_list_list(std::uint32_t id, rust::Str attr, rust::Slice<const double> values,
                            rust::Slice<const std::uint32_t> lengths);
    void set_int_list_list(std::uint32_t id, rust::Str attr, rust::Slice<const std::int32_t> values,
                           rust::Slice<const std::uint32_t> lengths);
    void set_typed_string(std::uint32_t id, rust::Str attr, rust::Str type_name, rust::Str value);
    void set_typed_real(std::uint32_t id, rust::Str attr, rust::Str type_name, double value);
    void set_typed_int(std::uint32_t id, rust::Str attr, rust::Str type_name, std::int32_t value);
    void set_typed_bool(std::uint32_t id, rust::Str attr, rust::Str type_name, bool value);
    rust::Vec<std::uint32_t> referenced_by(std::uint32_t id, rust::Str class_name) const;

    std::uint32_t create_entity(rust::Str class_name);
    void remove_entity(std::uint32_t id);
    void write(rust::Str path) const;
    rust::String header_text() const;
    rust::Vec<StepRow> step_rows() const;

    rust::Vec<Mesh> tessellate(std::uint32_t threads, rust::Slice<const std::uint32_t> skip, bool skip_derived) const;

private:
    std::unique_ptr<IfcParse::IfcFile> file_;
};

std::unique_ptr<Model> open_model(rust::Str path);
std::unique_ptr<Model> new_model(rust::Str schema);
rust::String ifcopenshell_version();

}  // namespace reviera_ifc
