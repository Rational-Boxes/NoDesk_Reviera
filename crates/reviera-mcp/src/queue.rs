// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The command queue (TECH_STACK §3.3): one thread owns the [`App`], and requests from agents run
//! on it in order, so only one transaction is ever open.

use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

use reviera_app::App;

/// A request to run on the thread that owns the [`App`].
pub type Job = Box<dyn FnOnce(&mut App) + Send>;

/// A handle to the application thread. When the last handle is dropped, the thread finishes the
/// queued jobs, flushes unsaved work and stops before the drop returns, so the process never exits
/// in the middle of a write.
#[derive(Debug, Clone)]
pub struct AppHandle {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    tx: Option<mpsc::Sender<Job>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Closing the queue ends the thread's loop.
        self.tx.take();
        let thread = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(t) = thread
            && t.thread().id() != std::thread::current().id()
        {
            let _ = t.join();
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("the application thread has stopped")]
pub struct Stopped;

impl AppHandle {
    /// Starts the application thread. `make` builds the app on that thread.
    pub fn spawn(make: impl FnOnce() -> App + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let thread = std::thread::Builder::new()
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
        Self {
            inner: Arc::new(Inner {
                tx: Some(tx),
                thread: Mutex::new(Some(thread)),
            }),
        }
    }

    /// A handle whose jobs the caller runs itself, on the thread that already owns an [`App`] (the
    /// window's UI thread, which drains the receiver with [`run_job`]).
    pub fn external() -> (Self, mpsc::Receiver<Job>) {
        let (tx, rx) = mpsc::channel::<Job>();
        (
            Self {
                inner: Arc::new(Inner {
                    tx: Some(tx),
                    thread: Mutex::new(None),
                }),
            },
            rx,
        )
    }

    /// Runs `f` on the application thread and waits for its result.
    pub async fn call<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut App) -> R + Send + 'static,
    ) -> Result<R, Stopped> {
        let (reply, rx) = tokio::sync::oneshot::channel();
        self.inner
            .tx
            .as_ref()
            .ok_or(Stopped)?
            .send(Box::new(move |app| {
                let _ = reply.send(f(app));
            }))
            .map_err(|_| Stopped)?;
        rx.await.map_err(|_| Stopped)
    }
}

/// Runs one queued job on `app` (for [`AppHandle::external`]).
pub fn run_job(job: Job, app: &mut App) {
    job(app);
}
