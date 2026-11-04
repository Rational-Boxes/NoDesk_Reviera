// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Riviera contributors

//! The OS credential store, with entries saved under the project's old spelling
//! (`org.bimreviera.*`, before 2026-11-04) moved to the current service name on first use.

/// The service name an entry had before the name was spelt correctly.
fn legacy(service: &str) -> Option<String> {
    service.strip_prefix("org.bimriviera.").map(|rest| format!("org.bimreviera.{rest}"))
}

/// The secret stored for `id`; one found under the old service name is copied to the current
/// one and, once copied, removed from the old (it is returned even if the copy fails).
pub(crate) fn password(service: &str, id: &str) -> keyring::Result<String> {
    match keyring::Entry::new(service, id).and_then(|e| e.get_password()) {
        Err(keyring::Error::NoEntry) => {
            let Some(old) = legacy(service) else { return Err(keyring::Error::NoEntry) };
            let entry = keyring::Entry::new(&old, id)?;
            let secret = entry.get_password()?;
            if keyring::Entry::new(service, id).and_then(|e| e.set_password(&secret)).is_ok() {
                let _ = entry.delete_credential();
            }
            Ok(secret)
        }
        other => other,
    }
}

/// Removes the secret for `id`, under the current and the old service names.
pub(crate) fn forget(service: &str, id: &str) {
    let _ = keyring::Entry::new(service, id).and_then(|e| e.delete_credential());
    if let Some(old) = legacy(service) {
        let _ = keyring::Entry::new(&old, id).and_then(|e| e.delete_credential());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_old_service_name() {
        assert_eq!(super::legacy("org.bimriviera.fileengine").as_deref(), Some("org.bimreviera.fileengine"));
        assert_eq!(super::legacy("other.service"), None);
    }
}
