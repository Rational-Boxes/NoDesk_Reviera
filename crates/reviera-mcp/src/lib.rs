// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! MCP server: one tool per registered command, model resources, permission levels (TECH_STACK
//! §3.6, spec Ch 2 §2.16).
//!
//! Every call goes through the command queue ([`AppHandle`]) to [`reviera_app::App::execute`], so
//! agents get the same transactions, undo, warnings and permission checks as the user. Calls are
//! attributed as `{source: mcp, actor: <client name>, session_id}`.
//!
//! Built so far: stdio transport (DM-MCP-002), the generated tool catalogue with toolsets and
//! discovery tools (DM-MCP-003), the session tools `undo`/`redo`/`history`, `query_elements`,
//! `get_element`, `session.begin_group`/`end_group`, `preview`/`confirm` (DM-MCP-004), confirm
//! mode (DM-MCP-005), the core resources (DM-MCP-006) and `get_view_image` (a PNG of a view:
//! captured by the window, or rendered offscreen headless). Not yet: streamable HTTP and tokens,
//! rate and size limits, path allow-lists, subscriptions, prompts, `execute_lua`.

pub mod bridge;
mod queue;
mod server;
pub mod toolsets;

pub use queue::{AppHandle, Job, Stopped, run_job};
pub use server::{Approval, Approver, McpConfig, McpServer};

/// Serves MCP on stdin/stdout until the client disconnects (`reviera --mcp-stdio`).
pub async fn run_stdio(
    app: AppHandle,
    config: McpConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use rmcp::ServiceExt;
    let running = McpServer::new(app, config)
        .serve(rmcp::transport::stdio())
        .await?;
    running.waiting().await?;
    Ok(())
}
