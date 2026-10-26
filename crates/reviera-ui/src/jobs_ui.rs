// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! Analysis jobs the window waits for (ANA-GEN-001, ANA-ENV-001 …): a command that starts a
//! compute job returns at once; the job is watched while the window is idle (at most every half
//! second), and when it succeeds its collect command keeps the results with the model and the
//! matching view opens (a failure is reported). Several jobs may run at once.

use std::time::{Duration, Instant};

use serde_json::json;

use super::{Gui, error};

/// A job being watched: its id, the command that collects it, and what it is, for messages.
#[derive(Debug, Clone)]
pub struct WatchedJob {
    pub id: String,
    pub collect: &'static str,
    pub what: &'static str,
}

impl Gui {
    /// Starts watching a job.
    pub(crate) fn watch_job(&mut self, id: String, collect: &'static str, what: &'static str) {
        self.frame.set_status_text(&format!("{what} running out of process…"), 0);
        self.jobs.push(WatchedJob { id, collect, what });
    }

    /// Polls the watched jobs (from the idle handler).
    pub(crate) fn jobs_idle(&mut self) {
        if self.jobs.is_empty() || self.jobs_polled.is_some_and(|t| t.elapsed() < Duration::from_millis(500)) {
            return;
        }
        self.jobs_polled = Some(Instant::now());
        let frame = self.frame;
        let mut k = 0;
        while k < self.jobs.len() {
            let j = self.jobs[k].clone();
            let Ok(s) = self.run("compute.status", json!({ "job_id": j.id })) else {
                k += 1;
                continue;
            };
            let state = s["state"].as_str().unwrap_or_default().to_owned();
            if !matches!(state.as_str(), "succeeded" | "failed" | "cancelled") {
                k += 1;
                continue;
            }
            self.jobs.remove(k);
            if state != "succeeded" {
                let why = s["failure"]["message"].as_str().unwrap_or(&state).to_owned();
                error(&frame, &format!("{} {state}:\n{why}", j.what));
                continue;
            }
            match self.run(j.collect, json!({ "job_id": j.id })) {
                Ok(_) => {
                    frame.set_status_text(&format!("{} finished", j.what), 0);
                    self.job_collected(j.collect);
                }
                Err(e) => error(&frame, &format!("Collecting {} failed:\n{e}", j.what.to_lowercase())),
            }
        }
    }

    /// Opens what a collected job shows.
    fn job_collected(&mut self, collect: &str) {
        match collect {
            "embodied_carbon.collect" => self.carbon_results(),
            "solar.collect" => self.solar_results(),
            "hc_loads.collect" => self.hc_loads_report(),
            "energy.collect" => self.annual_energy_results(),
            _ => {}
        }
    }
}
