// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The writer thread (Ch 2b §4): it owns the connection, batches messages that arrive within the
//! group-commit window into one SQLite transaction, and keeps the lock heartbeat fresh.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reviera_ifc::StepRow;
use rusqlite::{Connection, Transaction, params};

use crate::lock::{self, LockInfo};
use crate::schema::set_meta;

/// An element row: the element's current state.
#[derive(Debug, Clone)]
pub(crate) struct ElementRow {
    pub id: i64,
    pub guid: String,
    pub class: String,
    pub json: String,
}

/// Rows written or removed by one event.
#[derive(Debug, Clone, Default)]
pub(crate) struct StateDelta {
    pub upserts: Vec<ElementRow>,
    pub deletes: Vec<i64>,
}

#[derive(Debug, Clone)]
pub(crate) struct TxnRow {
    pub seq: i64,
    pub at: String,
    pub origin: String,
    pub name: String,
    pub forward: Vec<u8>,
    pub inverse: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum Msg {
    Commit {
        txn: TxnRow,
        delta: StateDelta,
        head: i64,
        next_id: i64,
    },
    Undo {
        steps: usize,
        delta: StateDelta,
        head: i64,
    },
    Redo {
        steps: usize,
        delta: StateDelta,
        head: i64,
    },
    Assimilate {
        count: usize,
        merged: Option<TxnRow>,
    },
    Rollback {
        count: usize,
        delta: StateDelta,
        head: i64,
    },
    Saved {
        saved_seq: i64,
        meta: Vec<(String, String)>,
        header: String,
        rows: Vec<StepRow>,
    },
    Flush(SyncSender<i64>),
    Close,
    /// The store was dropped without a clean close: write what is queued, leave the store
    /// unclean (recoverable) and stop.
    Detach,
}

/// What the writer has made durable, for `live.status`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Durable {
    pub head: i64,
    pub saved: i64,
    pub last_commit_at: Option<String>,
}

impl Msg {
    /// Whether the message carries model data (counted as pending until written).
    pub fn is_data(&self) -> bool {
        !matches!(self, Msg::Flush(_) | Msg::Close | Msg::Detach)
    }
}

pub(crate) struct Writer {
    pub durable: Arc<Mutex<Durable>>,
    pub pending: Arc<AtomicUsize>,
    pub conn: Connection,
    pub rx: Receiver<Msg>,
    pub window: Duration,
    pub lock_path: PathBuf,
    pub lock: LockInfo,
    pub head: i64,
}

const HEARTBEAT: Duration = Duration::from_secs(10);

impl Writer {
    pub fn run(mut self) -> rusqlite::Result<()> {
        let mut last_beat = Instant::now();
        loop {
            let first = match self.rx.recv_timeout(HEARTBEAT) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => {
                    self.beat(&mut last_beat);
                    continue;
                }
                // The project was dropped without a clean close: leave the store marked unclean.
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            };
            let mut batch = vec![first];
            let deadline = Instant::now() + self.window;
            while !matches!(
                batch.last(),
                Some(Msg::Flush(_) | Msg::Close | Msg::Detach | Msg::Saved { .. })
            ) {
                match self
                    .rx
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                {
                    Ok(m) => batch.push(m),
                    Err(_) => break,
                }
            }

            let data = batch.iter().filter(|m| m.is_data()).count();
            let mut saved_seq = None;
            let tx = self.conn.transaction()?;
            let mut flushes = Vec::new();
            let mut close = false;
            let mut detach = false;
            let mut saved = false;
            for msg in batch {
                match msg {
                    Msg::Flush(reply) => flushes.push(reply),
                    Msg::Close => close = true,
                    Msg::Detach => detach = true,
                    msg => {
                        if let Msg::Saved { saved_seq: s, .. } = &msg {
                            saved = true;
                            saved_seq = Some(*s);
                        }
                        apply(&tx, msg, &mut self.head)?;
                    }
                }
            }
            if close {
                set_meta(&tx, "clean_shutdown", "1")?;
            }
            tx.commit()?;
            self.pending.fetch_sub(data, Ordering::SeqCst);
            if data > 0 {
                let mut d = self.durable.lock().unwrap();
                d.head = self.head;
                d.saved = saved_seq.unwrap_or(d.saved);
                d.last_commit_at = Some(crate::time::iso8601(std::time::SystemTime::now()));
            }
            if saved || close {
                self.conn
                    .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            }
            for reply in flushes {
                let _ = reply.send(self.head);
            }
            if close {
                let _ = std::fs::remove_file(&self.lock_path);
                return Ok(());
            }
            if detach {
                return Ok(());
            }
            if last_beat.elapsed() >= HEARTBEAT {
                self.beat(&mut last_beat);
            }
        }
    }

    fn beat(&mut self, last: &mut Instant) {
        self.lock.heartbeat_at = lock::now_secs();
        let _ = lock::write(&self.lock_path, &self.lock);
        *last = Instant::now();
    }
}

fn apply(tx: &Transaction<'_>, msg: Msg, head: &mut i64) -> rusqlite::Result<()> {
    match msg {
        Msg::Commit {
            txn,
            delta,
            head: h,
            next_id,
        } => {
            // A new transaction clears the redo stack.
            tx.execute("DELETE FROM txn WHERE undone = 1", [])?;
            insert_txn(tx, &txn)?;
            apply_delta(tx, &delta, txn.seq)?;
            set_meta(tx, "next_id", &next_id.to_string())?;
            set_head(tx, head, h)?;
        }
        Msg::Undo {
            steps,
            delta,
            head: h,
        } => {
            tx.execute(
                "UPDATE txn SET undone = 1 WHERE seq IN (SELECT seq FROM txn WHERE undone = 0 ORDER BY seq DESC LIMIT ?1)",
                [steps as i64],
            )?;
            apply_delta(tx, &delta, h)?;
            set_head(tx, head, h)?;
        }
        Msg::Redo {
            steps,
            delta,
            head: h,
        } => {
            tx.execute(
                "UPDATE txn SET undone = 0 WHERE seq IN (SELECT seq FROM txn WHERE undone = 1 ORDER BY seq ASC LIMIT ?1)",
                [steps as i64],
            )?;
            apply_delta(tx, &delta, h)?;
            set_head(tx, head, h)?;
        }
        Msg::Assimilate { count, merged } => {
            tx.execute(
                "DELETE FROM txn WHERE seq IN (SELECT seq FROM txn WHERE undone = 0 ORDER BY seq DESC LIMIT ?1)",
                [count as i64],
            )?;
            if let Some(row) = merged {
                insert_txn(tx, &row)?;
            }
        }
        Msg::Rollback {
            count,
            delta,
            head: h,
        } => {
            tx.execute(
                "DELETE FROM txn WHERE seq IN (SELECT seq FROM txn WHERE undone = 0 ORDER BY seq DESC LIMIT ?1)",
                [count as i64],
            )?;
            apply_delta(tx, &delta, h)?;
            set_head(tx, head, h)?;
        }
        Msg::Saved {
            saved_seq,
            meta,
            header,
            rows,
        } => {
            write_ifc(tx, &header, &rows, saved_seq)?;
            for (k, v) in &meta {
                set_meta(tx, k, v)?;
            }
            set_meta(tx, "saved_seq", &saved_seq.to_string())?;
            let last: i64 =
                tx.query_row("SELECT coalesce(max(seq), 0) FROM txn", [], |r| r.get(0))?;
            set_meta(tx, "saved_txn_seq", &last.to_string())?;
        }
        Msg::Flush(_) | Msg::Close | Msg::Detach => {}
    }
    Ok(())
}

fn set_head(tx: &Transaction<'_>, head: &mut i64, h: i64) -> rusqlite::Result<()> {
    *head = h;
    set_meta(tx, "head_seq", &h.to_string())
}

pub(crate) fn insert_txn(conn: &Connection, row: &TxnRow) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO txn (seq, at, origin, name, forward, inverse, undone) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
        params![row.seq, row.at, row.origin, row.name, row.forward, row.inverse],
    )?;
    Ok(())
}

pub(crate) fn apply_delta(conn: &Connection, delta: &StateDelta, seq: i64) -> rusqlite::Result<()> {
    let mut del = conn.prepare_cached("DELETE FROM element WHERE id = ?1")?;
    for id in &delta.deletes {
        del.execute([id])?;
    }
    let mut up = conn.prepare_cached(
        "INSERT INTO element (id, global_id, class, json, seq) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET global_id = excluded.global_id, class = excluded.class,
                                      json = excluded.json, seq = excluded.seq",
    )?;
    for e in &delta.upserts {
        up.execute(params![e.id, e.guid, e.class, e.json, seq])?;
    }
    Ok(())
}

/// Replaces the materialised IFC graph.
pub(crate) fn write_ifc(
    conn: &Connection,
    header: &str,
    rows: &[StepRow],
    seq: i64,
) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM ifc_entity", [])?;
    let mut ins = conn.prepare_cached(
        "INSERT INTO ifc_entity (sid, ifc_type, global_id, step, seq) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for r in rows {
        ins.execute(params![r.id.0, r.class, r.global_id, r.attributes, seq])?;
    }
    set_meta(conn, "ifc_header", header)
}
