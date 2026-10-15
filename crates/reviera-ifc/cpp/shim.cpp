// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

#include "reviera-ifc/cpp/shim.h"

#include <cmath>
#include <algorithm>
#include <cstdio>
#include <fstream>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>

#include <ifcparse/IfcFile.h>
#include <ifcparse/IfcParse.h>
#include <ifcparse/IfcSchema.h>
#include <ifcgeom/Iterator.h>
#include <ifcgeom/hybrid_kernel.h>

#include "reviera-ifc/src/ffi.rs.h"

namespace reviera_ifc {

namespace {

// ---- JSON encoding of attribute values (decoded by `AttrValue` on the Rust side) ----

void json_string(std::ostringstream& out, const std::string& s) {
    out << '"';
    for (unsigned char c : s) {
        switch (c) {
            case '"': out << "\\\""; break;
            case '\\': out << "\\\\"; break;
            case '\n': out << "\\n"; break;
            case '\r': out << "\\r"; break;
            case '\t': out << "\\t"; break;
            default:
                if (c < 0x20) {
                    char buf[8];
                    std::snprintf(buf, sizeof buf, "\\u%04x", c);
                    out << buf;
                } else {
                    out << c;
                }
        }
    }
    out << '"';
}

void json_real(std::ostringstream& out, double d) {
    if (!std::isfinite(d)) {
        out << "null";
        return;
    }
    char buf[32];
    std::snprintf(buf, sizeof buf, "%.17g", d);
    out << buf;
}

void tagged(std::ostringstream& out, const char* tag) { out << "{\"t\":\"" << tag << "\""; }

void encode_instance(std::ostringstream& out, IfcUtil::IfcBaseClass* inst);

struct Encoder {
    std::ostringstream& out;

    void operator()(const Derived&) { tagged(out, "derived"); out << '}'; }
    void operator()(const Blank&) { tagged(out, "null"); out << '}'; }
    void operator()(const empty_aggregate_t&) { tagged(out, "list"); out << ",\"v\":[]}"; }
    void operator()(const empty_aggregate_of_aggregate_t&) { tagged(out, "list"); out << ",\"v\":[]}"; }
    void operator()(int v) { tagged(out, "int"); out << ",\"v\":" << v << '}'; }
    void operator()(bool v) { tagged(out, "bool"); out << ",\"v\":" << (v ? "true" : "false") << '}'; }
    void operator()(const boost::logic::tribool& v) {
        tagged(out, "logical");
        out << ",\"v\":" << (boost::logic::indeterminate(v) ? "null" : (v ? "true" : "false")) << '}';
    }
    void operator()(double v) { tagged(out, "real"); out << ",\"v\":"; json_real(out, v); out << '}'; }
    void operator()(const std::string& v) { tagged(out, "string"); out << ",\"v\":"; json_string(out, v); out << '}'; }
    void operator()(const boost::dynamic_bitset<>& v) {
        std::string bits;
        boost::to_string(v, bits);
        tagged(out, "binary");
        out << ",\"v\":";
        json_string(out, bits);
        out << '}';
    }
    void operator()(const EnumerationReference& v) {
        tagged(out, "enum");
        out << ",\"v\":";
        json_string(out, v.value());
        out << '}';
    }
    void operator()(IfcUtil::IfcBaseClass* inst) { encode_instance(out, inst); }

    template <typename T>
    void list(const std::vector<T>& items) {
        tagged(out, "list");
        out << ",\"v\":[";
        for (std::size_t i = 0; i < items.size(); ++i) {
            if (i) out << ',';
            (*this)(items[i]);
        }
        out << "]}";
    }
    void operator()(const std::vector<int>& v) { list(v); }
    void operator()(const std::vector<double>& v) { list(v); }
    void operator()(const std::vector<std::string>& v) { list(v); }
    void operator()(const std::vector<boost::dynamic_bitset<>>& v) { list(v); }
    void operator()(const std::vector<std::vector<int>>& v) { list(v); }
    void operator()(const std::vector<std::vector<double>>& v) { list(v); }
    void operator()(const aggregate_of_instance::ptr& v) {
        tagged(out, "list");
        out << ",\"v\":[";
        bool first = true;
        for (auto* inst : *v) {
            if (!first) out << ',';
            first = false;
            encode_instance(out, inst);
        }
        out << "]}";
    }
    void operator()(const aggregate_of_aggregate_of_instance::ptr& v) {
        tagged(out, "list");
        out << ",\"v\":[";
        bool first_outer = true;
        for (auto it = v->begin(); it != v->end(); ++it) {
            if (!first_outer) out << ',';
            first_outer = false;
            tagged(out, "list");
            out << ",\"v\":[";
            bool first = true;
            for (auto* inst : *it) {
                if (!first) out << ',';
                first = false;
                encode_instance(out, inst);
            }
            out << "]}";
        }
        out << "]}";
    }
};

// Entity instances are references by STEP id; simple-type instances inside selects (for example
// IfcLabel('x') in a property value) are written inline with their type name.
void encode_instance(std::ostringstream& out, IfcUtil::IfcBaseClass* inst) {
    if (inst == nullptr) {
        tagged(out, "null");
        out << '}';
    } else if (inst->declaration().as_entity()) {
        tagged(out, "ref");
        out << ",\"v\":" << inst->id() << '}';
    } else {
        tagged(out, "typed");
        out << ",\"v\":{\"type_name\":";
        json_string(out, inst->declaration().name());
        out << ",\"value\":";
        inst->get_attribute_value(0).apply_visitor(Encoder{out});
        out << "}}";
    }
}

// ---- Helpers ----

std::string str(rust::Str s) { return std::string(s); }

const IfcParse::entity& entity_decl(IfcUtil::IfcBaseClass* inst) {
    const auto* e = inst->declaration().as_entity();
    if (!e) throw std::runtime_error("#" + std::to_string(inst->id()) + " is not an entity instance");
    return *e;
}

IfcUtil::IfcBaseClass* instance(IfcParse::IfcFile& file, std::uint32_t id) {
    IfcUtil::IfcBaseClass* inst = nullptr;
    try {
        inst = file.instance_by_id(static_cast<int>(id));
    } catch (const IfcParse::IfcException&) {
    }
    if (!inst) throw std::runtime_error("no entity #" + std::to_string(id));
    return inst;
}

std::size_t attr_index(IfcUtil::IfcBaseClass* inst, rust::Str name) {
    const auto& e = entity_decl(inst);
    auto i = e.attribute_index(str(name));
    if (i < 0) throw std::runtime_error(e.name() + " has no attribute " + str(name));
    return static_cast<std::size_t>(i);
}

const IfcParse::enumeration_type* enum_type_of(const IfcParse::entity& e, std::size_t index) {
    const auto* named = e.all_attributes()[index]->type_of_attribute()->as_named_type();
    const auto* en = named ? named->declared_type()->as_enumeration_type() : nullptr;
    if (!en) throw std::runtime_error(e.all_attributes()[index]->name() + " is not an enumeration");
    return en;
}

const IfcParse::declaration& declaration(const IfcParse::IfcFile& file, rust::Str name) {
    const IfcParse::declaration* decl = nullptr;
    try {
        decl = file.schema()->declaration_by_name(str(name));
    } catch (const IfcParse::IfcException&) {
    }
    if (!decl) throw std::runtime_error(str(name) + " is not in schema " + file.schema()->name());
    return *decl;
}

// A defined-type value such as IfcText('x'), added to the file the way IfcOpenShell's own
// create_entity() does.
template <typename T>
IfcUtil::IfcBaseClass* typed_value(IfcParse::IfcFile& file, rust::Str type_name, const T& value) {
    const auto& decl = declaration(file, type_name);
    if (decl.as_entity()) throw std::runtime_error(str(type_name) + " is an entity, not a defined type");
    IfcEntityInstanceData data(in_memory_attribute_storage(1));
    auto* inst = file.schema()->instantiate(&decl, std::move(data));
    inst->set_attribute_value(0, value);
    return file.addEntity(inst);
}

// IfcOpenShell builds each schema's tables on first use, which is not thread-safe; build them all
// once before any file is opened or created. A function-local static (initialised once, thread
// safely, by the compiler's guard) rather than std::call_once, whose MinGW implementation hands
// the function over through thread-local variables that a static winpthreads and the libstdc++
// DLL each have their own copy of.
void init_schemas() {
    static const bool built = [] {
        for (const char* name : {"IFC2X3", "IFC4", "IFC4X3_ADD2"}) IfcParse::schema_by_name(name);
        return true;
    }();
    (void)built;
}

}  // namespace

// ---- Model ----

Model::Model(std::unique_ptr<IfcParse::IfcFile> file) : file_(std::move(file)) {}
Model::~Model() = default;

std::unique_ptr<Model> open_model(rust::Str path) {
    init_schemas();
    // IfcOpenShell 0.8.5's FileReader does not check fopen() and crashes on unreadable paths.
    if (!std::ifstream(str(path)).good()) throw std::runtime_error("cannot read " + str(path));
    auto file = std::make_unique<IfcParse::IfcFile>(str(path));
    using S = IfcParse::file_open_status;
    switch (file->good().value()) {
        case S::SUCCESS: break;
        case S::READ_ERROR: throw std::runtime_error("cannot read " + str(path));
        case S::NO_HEADER: throw std::runtime_error(str(path) + " has no IFC header");
        case S::UNSUPPORTED_SCHEMA: throw std::runtime_error(str(path) + " uses an unsupported schema");
        case S::INVALID_SYNTAX: throw std::runtime_error(str(path) + " is not valid IFC-SPF");
        default: throw std::runtime_error("cannot open " + str(path));
    }
    return std::make_unique<Model>(std::move(file));
}

std::unique_ptr<Model> new_model(rust::Str schema) {
    init_schemas();
    const auto* def = IfcParse::schema_by_name(str(schema));
    return std::make_unique<Model>(std::make_unique<IfcParse::IfcFile>(def));
}

rust::String ifcopenshell_version() { return IFCOPENSHELL_VERSION; }

rust::String Model::schema_name() const { return file_->schema()->name(); }

std::size_t Model::entity_count() const {
    std::size_t n = 0;
    for (auto it = file_->begin(); it != file_->end(); ++it) ++n;
    return n;
}

rust::Vec<EntityInfo> Model::entities_of_type(rust::Str class_name) const {
    rust::Vec<EntityInfo> out;
    auto list = file_->instances_by_type(str(class_name));
    if (!list) return out;
    for (auto* inst : *list) {
        auto* e = inst->as<IfcUtil::IfcBaseEntity>();
        EntityInfo info;
        info.id = inst->id();
        info.class_name = inst->declaration().name();
        const auto& decl = entity_decl(inst);
        if (decl.attribute_index("GlobalId") >= 0) {
            auto g = e->get("GlobalId");
            if (!g.isNull()) info.global_id = static_cast<std::string>(g);
        }
        if (decl.attribute_index("Name") >= 0) {
            auto n = e->get("Name");
            if (!n.isNull()) info.name = static_cast<std::string>(n);
        }
        out.push_back(std::move(info));
    }
    return out;
}

std::int64_t Model::instance_by_guid(rust::Str guid) const {
    try {
        auto* inst = file_->instance_by_guid(str(guid));
        return inst ? static_cast<std::int64_t>(inst->id()) : -1;
    } catch (const IfcParse::IfcException&) {
        return -1;
    }
}

rust::String Model::entity_class(std::uint32_t id) const { return instance(*file_, id)->declaration().name(); }

rust::Vec<rust::String> Model::attribute_names(std::uint32_t id) const {
    rust::Vec<rust::String> out;
    for (const auto* a : entity_decl(instance(*file_, id)).all_attributes()) out.push_back(a->name());
    return out;
}

rust::String Model::attributes_json(std::uint32_t id) const {
    auto* inst = instance(*file_, id);
    const auto attrs = entity_decl(inst).all_attributes();
    std::ostringstream out;
    out << '[';
    for (std::size_t i = 0; i < attrs.size(); ++i) {
        if (i) out << ',';
        inst->get_attribute_value(i).apply_visitor(Encoder{out});
    }
    out << ']';
    return out.str();
}

rust::String Model::attributes_json_many(rust::Slice<const std::uint32_t> ids) const {
    std::ostringstream out;
    out << '[';
    bool first = true;
    for (auto id : ids) {
        if (!first) out << ',';
        first = false;
        auto* inst = instance(*file_, id);
        const auto attrs = entity_decl(inst).all_attributes();
        out << "{\"class\":";
        json_string(out, inst->declaration().name());
        out << ",\"attributes\":{";
        for (std::size_t i = 0; i < attrs.size(); ++i) {
            if (i) out << ',';
            json_string(out, attrs[i]->name());
            out << ':';
            inst->get_attribute_value(i).apply_visitor(Encoder{out});
        }
        out << "}}";
    }
    out << ']';
    return out.str();
}

void Model::set_null(std::uint32_t id, rust::Str attr) {
    auto* inst = instance(*file_, id);
    inst->unset_attribute_value(attr_index(inst, attr));
}

void Model::set_string(std::uint32_t id, rust::Str attr, rust::Str value) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), str(value));
}

void Model::set_real(std::uint32_t id, rust::Str attr, double value) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), value);
}

void Model::set_int(std::uint32_t id, rust::Str attr, std::int32_t value) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), static_cast<int>(value));
}

void Model::set_bool(std::uint32_t id, rust::Str attr, bool value) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), value);
}

void Model::set_enum(std::uint32_t id, rust::Str attr, rust::Str value) {
    auto* inst = instance(*file_, id);
    auto i = attr_index(inst, attr);
    const auto* en = enum_type_of(entity_decl(inst), i);
    inst->set_attribute_value(i, EnumerationReference(en, en->lookup_enum_offset(str(value))));
}

void Model::set_ref(std::uint32_t id, rust::Str attr, std::uint32_t target) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), instance(*file_, target));
}

void Model::set_ref_list(std::uint32_t id, rust::Str attr, rust::Slice<const std::uint32_t> targets) {
    auto* inst = instance(*file_, id);
    auto list = aggregate_of_instance::ptr(new aggregate_of_instance);
    for (auto t : targets) list->push(instance(*file_, t));
    inst->set_attribute_value(attr_index(inst, attr), list);
}

void Model::set_real_list(std::uint32_t id, rust::Str attr, rust::Slice<const double> values) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), std::vector<double>(values.begin(), values.end()));
}

void Model::set_int_list(std::uint32_t id, rust::Str attr, rust::Slice<const std::int32_t> values) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), std::vector<int>(values.begin(), values.end()));
}

// Splits `values` into inner lists of `lengths`.
template <typename T, typename S>
static std::vector<std::vector<T>> nested(rust::Slice<const S> values, rust::Slice<const std::uint32_t> lengths) {
    std::vector<std::vector<T>> out;
    out.reserve(lengths.size());
    std::size_t at = 0;
    for (auto n : lengths) {
        if (at + n > values.size()) throw std::runtime_error("list of lists: lengths exceed the values");
        out.emplace_back(values.begin() + at, values.begin() + at + n);
        at += n;
    }
    return out;
}

void Model::set_real_list_list(std::uint32_t id, rust::Str attr, rust::Slice<const double> values,
                               rust::Slice<const std::uint32_t> lengths) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), nested<double>(values, lengths));
}

void Model::set_int_list_list(std::uint32_t id, rust::Str attr, rust::Slice<const std::int32_t> values,
                              rust::Slice<const std::uint32_t> lengths) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), nested<int>(values, lengths));
}

void Model::set_typed_string(std::uint32_t id, rust::Str attr, rust::Str type_name, rust::Str value) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), typed_value(*file_, type_name, str(value)));
}

void Model::set_typed_real(std::uint32_t id, rust::Str attr, rust::Str type_name, double value) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), typed_value(*file_, type_name, value));
}

void Model::set_typed_int(std::uint32_t id, rust::Str attr, rust::Str type_name, std::int32_t value) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), typed_value(*file_, type_name, static_cast<int>(value)));
}

void Model::set_typed_bool(std::uint32_t id, rust::Str attr, rust::Str type_name, bool value) {
    auto* inst = instance(*file_, id);
    inst->set_attribute_value(attr_index(inst, attr), typed_value(*file_, type_name, value));
}

rust::Vec<std::uint32_t> Model::referenced_by(std::uint32_t id, rust::Str class_name) const {
    instance(*file_, id);
    rust::Vec<std::uint32_t> out;
    auto list = file_->getInverse(static_cast<int>(id), &declaration(*file_, class_name), -1);
    if (list) {
        for (auto* inst : *list) out.push_back(inst->id());
    }
    return out;
}

std::uint32_t Model::create_entity(rust::Str class_name) {
    const auto* schema = file_->schema();
    const auto* decl = &declaration(*file_, class_name);
    if (!decl->as_entity()) throw std::runtime_error(str(class_name) + " is not an entity type");
    IfcEntityInstanceData data(in_memory_attribute_storage(decl->as_entity()->attribute_count()));
    auto* inst = schema->instantiate(decl, std::move(data));
    if (auto* e = inst->as<IfcUtil::IfcBaseEntity>()) e->populate_derived();
    return file_->addEntity(inst)->id();
}

void Model::remove_entity(std::uint32_t id) { file_->removeEntity(instance(*file_, id)); }

void Model::write(rust::Str path) const {
    std::ofstream out(str(path));
    if (!out) throw std::runtime_error("cannot write " + str(path));
    out << *file_;
    if (!out) throw std::runtime_error("failed writing " + str(path));
}

rust::String Model::header_text() const {
    std::ostringstream out;
    file_->header().write(out);
    return out.str();
}

rust::Vec<StepRow> Model::step_rows() const {
    std::vector<IfcUtil::IfcBaseClass*> sorted;
    for (auto it = file_->begin(); it != file_->end(); ++it) sorted.push_back(it->second);
    std::sort(sorted.begin(), sorted.end(), [](const auto* a, const auto* b) { return a->id() < b->id(); });
    rust::Vec<StepRow> out;
    for (auto* inst : sorted) {
        const auto* ent = inst->declaration().as_entity();
        if (!ent) continue;
        std::ostringstream text;
        inst->toString(text, true);
        const std::string line = text.str();
        const std::string& type = inst->declaration().name_uc();
        // "#<id>=<TYPE>(...)": keep only the attribute list.
        auto at = line.find('=');
        StepRow row;
        row.id = inst->id();
        row.class_name = type;
        row.attributes = line.substr(at + 1 + type.size());
        if (ent->attribute_index("GlobalId") >= 0) {
            auto g = inst->as<IfcUtil::IfcBaseEntity>()->get("GlobalId");
            if (!g.isNull()) row.global_id = static_cast<std::string>(g);
        }
        out.push_back(std::move(row));
    }
    return out;
}

rust::Vec<Mesh> Model::tessellate(std::uint32_t threads) const {
    namespace s = ifcopenshell::geometry::settings;
    ifcopenshell::geometry::Settings settings;
    settings.get<s::UseWorldCoords>().value = true;
    settings.get<s::WeldVertices>().value = true;

    rust::Vec<Mesh> out;
    IfcGeom::Iterator it(ifcopenshell::geometry::kernels::construct(file_.get(), "opencascade", settings), settings,
                         file_.get(), static_cast<int>(threads ? threads : 1));
    if (!it.initialize()) return out;
    do {
        auto* elem = static_cast<const IfcGeom::TriangulationElement*>(it.get());
        const auto& geom = elem->geometry();
        Mesh m;
        m.id = static_cast<std::uint32_t>(elem->id());
        m.guid = elem->guid();
        m.class_name = elem->type();
        for (double v : geom.verts()) m.verts.push_back(v);
        for (int f : geom.faces()) m.faces.push_back(static_cast<std::uint32_t>(f));
        out.push_back(std::move(m));
    } while (it.next());
    return out;
}

}  // namespace reviera_ifc
