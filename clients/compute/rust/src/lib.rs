// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Reviera contributors

//! The remote computation contract (BIM Reviera spec Ch 2 §2.17): the generated gRPC types of
//! `reviera.compute.v1` and the job kinds' packages, technique negotiation, blob codecs, the
//! local transport, and a toolkit for writing runners. MIT, so a runner or service under any
//! licence can implement the contract without Reviera's code.

#![allow(clippy::doc_lazy_continuation, clippy::large_enum_variant)]

pub mod blob;
pub mod negotiate;
pub mod runner;
#[cfg(feature = "subscription")]
pub mod subscription;
pub mod transport;

/// `reviera.compute.v1`: the contract.
pub mod compute {
    pub mod v1 {
        tonic::include_proto!("reviera.compute.v1");
    }
}

/// `reviera.analysis.v1`: analysis job kinds' options, distillation formats and results.
pub mod analysis {
    pub mod v1 {
        tonic::include_proto!("reviera.analysis.v1");
    }
}

/// `reviera.testing.v1`: the `test.fault` kind.
pub mod testing {
    pub mod v1 {
        tonic::include_proto!("reviera.testing.v1");
    }
}

pub use compute::v1 as pb;

/// The contract version this crate implements.
pub const CONTRACT_VERSION: &str = "reviera.compute.v1";

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Packs a message as `google.protobuf.Any` with its full type name.
pub fn to_any<M: prost::Name>(m: &M) -> prost_types::Any {
    prost_types::Any {
        type_url: M::type_url(),
        value: prost::Message::encode_to_vec(m),
    }
}

/// Unpacks `google.protobuf.Any` into `M`, checking the type name.
pub fn from_any<M: prost::Name + Default>(any: &prost_types::Any) -> Result<M, String> {
    if any.type_url != M::type_url() {
        return Err(format!("expected {}, got {}", M::type_url(), any.type_url));
    }
    M::decode(any.value.as_slice()).map_err(|e| e.to_string())
}
