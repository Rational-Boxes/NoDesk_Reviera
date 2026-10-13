// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The command queue (TECH_STACK §3.3): one thread owns the [`App`], and requests from agents run
//! on it in order, so only one transaction is ever open.

use std::sync::mpsc;

use reviera_app::App;

type Job = Box<dyn FnOnce(&mut App) + Send>;

#[derive(Debug, Clone)]
pub struct AppHandle {
    tx: mpsc::Sender<Job>,
}

#[derive(Debug, thiserror::Error)]
#[error("the application thread has stopped")]
pub struct Stopped;

impl AppHandle {
    /// Starts the application thread. `make` builds the app on that thread.
    pub fn spawn(make: impl FnOnce() -> App + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("reviera-app".into())
            .spawn(move || {
                let mut app = make();
                while let Ok(job) = rx.recv() {
                    job(&mut app);
                }
                // The queue closed: keep unsaved work recoverable.
                app.flush_all();
            })
            .expect("the application thread starts");
        Self { tx }
    }

    /// Runs `f` on the application thread and waits for its result.
    pub async fn call<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut App) -> R + Send + 'static,
    ) -> Result<R, Stopped> {
        let (reply, rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(Box::new(move |app| {
                let _ = reply.send(f(app));
            }))
            .map_err(|_| Stopped)?;
        rx.await.map_err(|_| Stopped)
    }
}
