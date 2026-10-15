// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The `cxx` bridge to `cpp/shim.cpp`. Every call that can reach IfcOpenShell code that throws
//! returns `Result`, so C++ exceptions become Rust errors instead of aborting.

#[cxx::bridge(namespace = "reviera_ifc")]
pub(crate) mod bridge {
    struct EntityInfo {
        id: u32,
        class_name: String,
        global_id: String,
        name: String,
    }

    struct Mesh {
        id: u32,
        guid: String,
        class_name: String,
        verts: Vec<f64>,
        faces: Vec<u32>,
    }

    struct StepRow {
        id: u32,
        class_name: String,
        global_id: String,
        attributes: String,
    }

    unsafe extern "C++" {
        include!("reviera-ifc/cpp/shim.h");

        type Model;

        fn open_model(path: &str) -> Result<UniquePtr<Model>>;
        fn new_model(schema: &str) -> Result<UniquePtr<Model>>;
        fn ifcopenshell_version() -> String;

        fn schema_name(self: &Model) -> String;
        fn entity_count(self: &Model) -> usize;
        fn entities_of_type(self: &Model, class_name: &str) -> Result<Vec<EntityInfo>>;
        fn instance_by_guid(self: &Model, guid: &str) -> i64;
        fn entity_class(self: &Model, id: u32) -> Result<String>;
        fn attribute_names(self: &Model, id: u32) -> Result<Vec<String>>;
        fn attributes_json(self: &Model, id: u32) -> Result<String>;
        fn attributes_json_many(self: &Model, ids: &[u32]) -> Result<String>;

        fn set_null(self: Pin<&mut Model>, id: u32, attr: &str) -> Result<()>;
        fn set_string(self: Pin<&mut Model>, id: u32, attr: &str, value: &str) -> Result<()>;
        fn set_real(self: Pin<&mut Model>, id: u32, attr: &str, value: f64) -> Result<()>;
        fn set_int(self: Pin<&mut Model>, id: u32, attr: &str, value: i32) -> Result<()>;
        fn set_bool(self: Pin<&mut Model>, id: u32, attr: &str, value: bool) -> Result<()>;
        fn set_enum(self: Pin<&mut Model>, id: u32, attr: &str, value: &str) -> Result<()>;
        fn set_ref(self: Pin<&mut Model>, id: u32, attr: &str, target: u32) -> Result<()>;
        fn set_ref_list(self: Pin<&mut Model>, id: u32, attr: &str, targets: &[u32]) -> Result<()>;
        fn set_real_list(self: Pin<&mut Model>, id: u32, attr: &str, values: &[f64]) -> Result<()>;
        fn set_int_list(self: Pin<&mut Model>, id: u32, attr: &str, values: &[i32]) -> Result<()>;
        /// A list of lists, given flat with each inner list's length.
        fn set_real_list_list(
            self: Pin<&mut Model>,
            id: u32,
            attr: &str,
            values: &[f64],
            lengths: &[u32],
        ) -> Result<()>;
        fn set_int_list_list(
            self: Pin<&mut Model>,
            id: u32,
            attr: &str,
            values: &[i32],
            lengths: &[u32],
        ) -> Result<()>;
        fn set_typed_string(
            self: Pin<&mut Model>,
            id: u32,
            attr: &str,
            type_name: &str,
            value: &str,
        ) -> Result<()>;
        fn set_typed_real(
            self: Pin<&mut Model>,
            id: u32,
            attr: &str,
            type_name: &str,
            value: f64,
        ) -> Result<()>;
        fn set_typed_int(
            self: Pin<&mut Model>,
            id: u32,
            attr: &str,
            type_name: &str,
            value: i32,
        ) -> Result<()>;
        fn set_typed_bool(
            self: Pin<&mut Model>,
            id: u32,
            attr: &str,
            type_name: &str,
            value: bool,
        ) -> Result<()>;
        fn referenced_by(self: &Model, id: u32, class_name: &str) -> Result<Vec<u32>>;

        fn create_entity(self: Pin<&mut Model>, class_name: &str) -> Result<u32>;
        fn remove_entity(self: Pin<&mut Model>, id: u32) -> Result<()>;
        fn write(self: &Model, path: &str) -> Result<()>;
        fn header_text(self: &Model) -> Result<String>;
        fn step_rows(self: &Model) -> Result<Vec<StepRow>>;

        fn tessellate(self: &Model, threads: u32) -> Result<Vec<Mesh>>;
    }
}

// SAFETY: an IfcFile has no thread affinity; it may move between threads as long as it is not
// used from two at once, which `&mut`/ownership on the Rust side guarantees (Model is not Sync).
unsafe impl Send for bridge::Model {}
