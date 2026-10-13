// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Typed errors with stable string codes (DM-CMD-001: `{code, message, details}`).

use std::borrow::Cow;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct Error {
    /// Stable code such as `element.not_found` or `permission.denied`.
    pub code: Cow<'static, str>,
    pub message: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

impl Error {
    pub fn new(code: impl Into<Cow<'static, str>>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: Value::Null,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
