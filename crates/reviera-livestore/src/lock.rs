// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The single-writer lock file `<key>.lock` (Ch 2b §2): PID, host, start time and a heartbeat.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A lock whose heartbeat is older than this, on the same host, is stale.
pub const STALE_AFTER: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    pub host: String,
    pub started_at: u64,
    pub heartbeat_at: u64,
    /// The project file, so recovery can find the base IFC even if the database is unreadable.
    pub project_path: Option<PathBuf>,
}

impl LockInfo {
    pub fn read(path: &Path) -> Option<Self> {
        serde_json::from_slice(&fs::read(path).ok()?).ok()
    }

    /// Whether the owning process is gone: its PID no longer exists on this host, or its heartbeat
    /// stopped. Locks from other hosts are only stale by heartbeat.
    pub fn is_stale(&self) -> bool {
        let same_host = self.host == hostname();
        if same_host && !pid_alive(self.pid) {
            return true;
        }
        same_host && now_secs().saturating_sub(self.heartbeat_at) > STALE_AFTER.as_secs()
    }
}

pub(crate) fn write(path: &Path, info: &LockInfo) -> std::io::Result<()> {
    let tmp = path.with_extension("lock.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec(info).expect("lock info serialises"),
    )?;
    fs::rename(tmp, path)
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn hostname() -> String {
    #[cfg(target_os = "linux")]
    if let Ok(h) = fs::read_to_string("/proc/sys/kernel/hostname") {
        return h.trim().to_owned();
    }
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_default()
}

fn pid_alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        Path::new("/proc").join(pid.to_string()).exists()
    }
    // Elsewhere, rely on the heartbeat.
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}
