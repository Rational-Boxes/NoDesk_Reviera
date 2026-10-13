// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Shared REST and authentication client: OAuth 2.0 (PKCE, device code), proxies, TLS, retries and
//! OS-keyring credential storage (spec Ch 6 COL-API-001). Credentials never leave this crate.
