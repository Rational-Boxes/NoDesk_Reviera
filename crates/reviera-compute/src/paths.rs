// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Where the local compute host keeps its state, how clients find it, and where the bundled
//! runners are.

use std::path::{Path, PathBuf};

use reviera_compute_proto::transport::Address;
use serde::{Deserialize, Serialize};

/// The host's state directory: `REVIERA_COMPUTE_STATE`, else the platform's per-user state
/// directory.
pub fn state_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("REVIERA_COMPUTE_STATE") {
        return d.into();
    }
    #[cfg(windows)]
    if let Some(d) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(d).join("Reviera").join("compute");
    }
    #[cfg(target_os = "macos")]
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join("Library/Application Support/Reviera/compute");
    }
    if let Some(d) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(d).join("reviera").join("compute");
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".local/state/reviera/compute");
    }
    std::env::temp_dir().join("reviera-compute")
}

/// Where the host and its runners put their sockets: beside the state when the paths are short
/// enough for a Unix socket (about 100 bytes), otherwise a short owner-only directory in the
/// runtime directory (or the temporary directory) named after the state directory.
pub fn socket_dir(state_dir: &Path) -> PathBuf {
    let beside = state_dir.join("run");
    // The longest name used: "<runner>-<n>.sock".
    if beside
        .join("reviera-analysis-builtin-9999.sock")
        .as_os_str()
        .len()
        <= 100
    {
        return beside;
    }
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|d| d.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    let hash = reviera_compute_proto::blob::sha256_hex(state_dir.as_os_str().as_encoded_bytes());
    let dir = base.join(format!("rvc-{}", &hash[..12]));
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    dir
}

/// The address the host listens on for `state_dir`.
pub fn host_address(state_dir: &Path) -> Address {
    #[cfg(unix)]
    {
        Address::Unix(socket_dir(state_dir).join("host.sock"))
    }
    #[cfg(not(unix))]
    {
        let _ = state_dir;
        Address::Tcp("127.0.0.1:0".parse().unwrap())
    }
}

/// How a client reaches a running host. Owner-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    pub address: String,
    pub token: String,
    pub pid: u32,
}

fn host_file(state_dir: &Path) -> PathBuf {
    state_dir.join("host.json")
}

pub fn write_host_info(state_dir: &Path, info: &HostInfo) -> std::io::Result<()> {
    let path = host_file(state_dir);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(info)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(tmp, path)
}

pub fn read_host_info(state_dir: &Path) -> Option<HostInfo> {
    serde_json::from_slice(&std::fs::read(host_file(state_dir)).ok()?).ok()
}

/// Removes the host file if it is still this host's.
pub fn remove_host_info(state_dir: &Path, info: &HostInfo) {
    if read_host_info(state_dir).as_ref() == Some(info) {
        let _ = std::fs::remove_file(host_file(state_dir));
    }
}

fn exe_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

/// Programs shipped beside the running executable (and, during development, in the same
/// target directory).
pub fn beside_exe(stem: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.to_path_buf(), dir.join("..")]
        .into_iter()
        .map(|d| d.join(exe_name(stem)))
        .find(|p| p.is_file())
}

/// The bundled runners: `reviera-analysis-builtin`.
pub fn bundled_runners() -> Vec<PathBuf> {
    beside_exe("reviera-analysis-builtin").into_iter().collect()
}

/// The host program: `REVIERA_COMPUTE_HOST`, else beside the running executable.
pub fn host_program() -> Option<PathBuf> {
    std::env::var_os("REVIERA_COMPUTE_HOST")
        .map(PathBuf::from)
        .or_else(|| beside_exe("reviera-compute-host"))
}

/// Physical memory in bytes, where the platform tells.
pub fn physical_memory() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb: u64 = s
            .lines()
            .find(|l| l.starts_with("MemTotal:"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()?;
        Some(kb * 1024)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}
