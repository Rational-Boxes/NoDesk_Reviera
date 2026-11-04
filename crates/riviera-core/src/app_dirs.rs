// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! Riviera's folders in the user's settings and data locations. One made before the project's
//! name was spelt correctly (`Reviera`, `reviera`; before 2026-11-04) is moved to the current
//! name the first time it is needed.

use std::path::{Path, PathBuf};

/// `base/name` (`name` being "Riviera" or "riviera"); where only the old spelling's folder exists
/// it is renamed first, or used as it is if it cannot be.
pub fn app_dir(base: &Path, name: &str) -> PathBuf {
    let new = base.join(name);
    let old = base.join(name.replace("Riviera", "Reviera").replace("riviera", "reviera"));
    if old != new && !new.exists() && old.is_dir() && std::fs::rename(&old, &new).is_err() {
        return old;
    }
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_folder_is_moved_to_the_new_name() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("reviera/plugins")).unwrap();
        std::fs::write(d.path().join("reviera/plugins/a.lua"), "x").unwrap();
        let got = app_dir(d.path(), "riviera");
        assert_eq!(got, d.path().join("riviera"));
        assert!(got.join("plugins/a.lua").exists() && !d.path().join("reviera").exists());
        // Nothing to move: the new name.
        assert_eq!(app_dir(d.path(), "Riviera"), d.path().join("Riviera"));
    }
}
