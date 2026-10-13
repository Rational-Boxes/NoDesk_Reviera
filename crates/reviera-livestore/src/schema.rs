// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Database schema version 1 (Ch 2b §3) and connection settings (§5).
//!
//! Implementation note: the core keeps edited element state apart from the IFC population until
//! save, so the materialised state is two parts: `ifc_entity` holds the IFC graph as of the last
//! open or save, and `element` holds the current elements. `txn.origin` holds the origin as JSON,
//! so session-scoped undo still works after recovery.

use rusqlite::Connection;

pub const SCHEMA_VERSION: &str = "1";

/// Database page size (§5). For encrypted stores it is set as `cipher_page_size` when unlocking.
pub const PAGE_SIZE: u32 = 8192;

pub const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS ifc_entity (
  sid        INTEGER PRIMARY KEY,
  ifc_type   TEXT    NOT NULL,
  global_id  TEXT,
  step       TEXT    NOT NULL,
  seq        INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS ifc_entity_gid ON ifc_entity(global_id) WHERE global_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ifc_entity_type ON ifc_entity(ifc_type);

CREATE TABLE IF NOT EXISTS element (
  id        INTEGER PRIMARY KEY,
  global_id TEXT    NOT NULL UNIQUE,
  class     TEXT    NOT NULL,
  json      TEXT    NOT NULL,
  seq       INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS rxd_document (
  path  TEXT PRIMARY KEY,
  kind  TEXT NOT NULL,
  json  TEXT NOT NULL,
  seq   INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS rxd_element (
  global_id TEXT PRIMARY KEY,
  scope     TEXT NOT NULL,
  json      TEXT NOT NULL,
  geom_hash TEXT,
  seq       INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS txn (
  seq        INTEGER PRIMARY KEY,
  at         TEXT    NOT NULL,
  origin     TEXT    NOT NULL,
  name       TEXT    NOT NULL,
  command    TEXT,
  params     TEXT,
  group_id   INTEGER,
  forward    BLOB    NOT NULL,
  inverse    BLOB    NOT NULL,
  undone     INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE IF NOT EXISTS cache_tessellation (
  global_id TEXT, lod TEXT, geom_hash TEXT, mesh BLOB,
  PRIMARY KEY (global_id, lod)
) STRICT;

CREATE TABLE IF NOT EXISTS session (
  id         INTEGER PRIMARY KEY,
  kind       TEXT NOT NULL,
  owner      TEXT,
  tool       TEXT NOT NULL,
  context    TEXT NOT NULL,
  started_at TEXT NOT NULL,
  state      TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS session_op (
  session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  n          INTEGER NOT NULL,
  at         TEXT NOT NULL,
  op         TEXT NOT NULL,
  PRIMARY KEY (session_id, n)
) STRICT;

CREATE TABLE IF NOT EXISTS snapshot (
  seq        INTEGER PRIMARY KEY,
  at         TEXT NOT NULL,
  reason     TEXT NOT NULL,
  label      TEXT,
  data       BLOB NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS base_entity (
  sid INTEGER PRIMARY KEY, ifc_type TEXT NOT NULL, global_id TEXT, step TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS base_rxd (
  key TEXT PRIMARY KEY, json TEXT NOT NULL
) STRICT;
"#;

/// Durability settings (§5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Synchronous {
    Normal,
    Full,
}

pub fn configure(
    conn: &Connection,
    synchronous: Synchronous,
    encrypted: bool,
) -> rusqlite::Result<()> {
    if !encrypted {
        conn.execute_batch(&format!("PRAGMA page_size = {PAGE_SIZE};"))?;
    }
    let sync = match synchronous {
        Synchronous::Normal => "NORMAL",
        Synchronous::Full => "FULL",
    };
    conn.execute_batch(&format!(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = {sync};
         PRAGMA foreign_keys = ON;
         PRAGMA cache_size = -65536;
         PRAGMA wal_autocheckpoint = 512;"
    ))
}

pub fn get_meta(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    use rusqlite::OptionalExtension;
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()
}

pub fn set_meta(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}
