//! Workspace launch admission reuses the normal background app launcher.
use cua_driver_core::{
    selected_windows::{WindowSelectionBackend, WindowTarget},
    workspace::{LaunchedWorkspaceWindow, WorkspaceApplication},
};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(super) fn launch(recipe: &WorkspaceApplication) -> Result<LaunchedWorkspaceWindow, String> {
    launch_verified(
        json!({
            "bundle_id": recipe.bundle_id,
            "creates_new_application_instance": true,
            "additional_arguments": recipe.arguments,
            "urls": recipe.urls,
        }),
        &recipe.bundle_id,
        &recipe.urls,
    )
    .map(|(window, _)| window)
}

pub(super) fn launch_app(args: &Value) -> Result<(LaunchedWorkspaceWindow, Value), String> {
    let bundle = if let Some(bundle) = args.get("bundle_id").and_then(Value::as_str) {
        bundle.to_owned()
    } else {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .ok_or("workspace_launch_failed: provide bundle_id or name")?;
        crate::apps::locate_by_name(name)
            .and_then(|locator| locator.app_ref_and_bundle_id().1)
            .ok_or_else(|| {
                format!("workspace_launch_failed: installed app '{name}' was not found")
            })?
    };
    let mut args = args.clone();
    args["bundle_id"] = json!(bundle);
    // Existing processes may own personal windows. Never redirect their open events.
    args["creates_new_application_instance"] = json!(true);
    let mut arguments: Vec<String> = serde_json::from_value(
        args.get("additional_arguments")
            .cloned()
            .unwrap_or(json!([])),
    )
    .map_err(|_| "workspace_launch_failed: additional_arguments must be strings")?;
    if arguments
        .iter()
        .any(|a| a.starts_with("--user-data-dir") || a.contains("remote-debugging"))
    {
        return Err("workspace_launch_failed: workspace browser profiles and debugging endpoints are driver-managed".into());
    }
    if crate::browser::platform::workspace_chromium(&bundle) {
        let profile = std::env::temp_dir()
            .canonicalize()
            .map_err(|e| format!("workspace_launch_failed: {e}"))?
            .join(format!("cua-workspace-browser-{}", uuid::Uuid::new_v4()));
        arguments.extend([
            format!("--user-data-dir={}", profile.display()),
            "--remote-debugging-port=0".into(),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            "--new-window".into(),
        ]);
    }
    args["additional_arguments"] = json!(arguments);
    let urls: Vec<String> = serde_json::from_value(args.get("urls").cloned().unwrap_or(json!([])))
        .map_err(|_| "workspace_launch_failed: urls must be strings")?;
    launch_verified(args, &bundle, &urls)
}

fn launch_verified(
    args: Value,
    bundle_id: &str,
    urls: &[String],
) -> Result<(LaunchedWorkspaceWindow, Value), String> {
    let active_before = active_spaces()?;
    // Read kernel process identities, avoiding NSWorkspace's cached app list.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err("workspace_launch_failed: process inventory unavailable".into());
    }
    let mut pids = vec![0_i32; count as usize + 1024];
    let count = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr().cast(),
            (pids.len() * std::mem::size_of::<i32>()) as i32,
        )
    };
    if count <= 0 || count as usize >= pids.len() {
        return Err("workspace_launch_failed: process inventory incomplete".into());
    }
    let before: HashSet<_> = pids.into_iter().take(count as usize).collect();
    let frontmost = crate::apps::frontmost_pid();
    let result = tokio::runtime::Handle::current()
        .block_on(crate::tools::launch_app::LaunchAppTool.launch_workspace(args));
    if result.is_error == Some(true) {
        let reason = result
            .content
            .iter()
            .filter_map(|content| match content {
                cua_driver_core::protocol::Content::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "workspace_launch_failed: {reason}; no window access granted"
        ));
    }
    let data = result
        .structured_content
        .as_ref()
        .ok_or("workspace_launch_failed: missing native launch result")?;
    let pid = data
        .get("pid")
        .and_then(|v| v.as_i64())
        .and_then(|v| i32::try_from(v).ok())
        .ok_or("workspace_launch_failed: missing native process identity")?;
    if before.contains(&pid) {
        return Err(
            "workspace_launch_not_isolated: app reused an existing process; no access granted"
                .into(),
        );
    }
    if crate::apps::frontmost_pid() != frontmost || active_spaces()? != active_before {
        return Err(format!("workspace_launch_background_failed: foreground changed during launch; created pid {pid} left untouched and unauthorized"));
    }
    // NSWorkspace's runningApplications list can remain cached in a direct
    // MCP process. Resolve this exact PID rather than polling that list.
    if crate::apps::bundle_id_for_pid(pid).as_deref() != Some(bundle_id) {
        return Err(
            "workspace_launch_failed: launched bundle identity does not match trusted recipe"
                .into(),
        );
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let windows = loop {
        let windows: Vec<_> = crate::windows::all_windows()
            .into_iter()
            .filter(|w| w.pid == pid)
            .collect();
        if !windows.is_empty() || std::time::Instant::now() >= deadline {
            break windows;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    if crate::apps::frontmost_pid() != frontmost || active_spaces()? != active_before {
        return Err(format!("workspace_launch_background_failed: foreground changed while waiting for pid {pid}; no window access granted"));
    }
    if windows.is_empty() {
        return Err(format!("workspace_launch_no_window: created pid {pid} did not expose a background window; the app may require activation, which this workspace will not perform"));
    }
    // Document apps may also create an untitled window or an open panel.
    // Bind only the exact document the trusted recipe requested; those other
    // windows remain outside the selection, including keyboard delivery.
    if urls.len() == 1 && std::path::Path::new(&urls[0]).is_file() {
        let mut matches = Vec::new();
        for window in &windows {
            let target = WindowTarget {
                pid: i64::from(pid),
                window_id: u64::from(window.window_id),
            };
            if let Ok(identity) = crate::selected_windows::MacosWindowSelection.bind(target) {
                if crate::selected_windows::matches_document(
                    identity.as_ref(),
                    std::path::Path::new(&urls[0]),
                ) {
                    matches.push(LaunchedWorkspaceWindow { target, identity });
                }
            }
        }
        if matches.len() == 1 {
            return Ok((matches.remove(0), data.clone()));
        }
        return Err(format!("workspace_launch_ambiguous: created pid {pid} has no unique exact document window; no windows approved"));
    }
    if windows.len() != 1 {
        return Err(format!("workspace_launch_ambiguous: created pid {pid} has {} top-level windows; no windows approved", windows.len()));
    }
    let target = WindowTarget {
        pid: i64::from(pid),
        window_id: u64::from(windows[0].window_id),
    };
    let identity = crate::selected_windows::MacosWindowSelection.bind(target)?;
    Ok((LaunchedWorkspaceWindow { target, identity }, data.clone()))
}

fn active_spaces() -> Result<Vec<(String, u64)>, String> {
    let displays = super::managed_displays()?;
    let mut active = Vec::new();
    for display in displays
        .as_array()
        .ok_or("workspace_launch_failed: display inventory unavailable")?
    {
        let id = display
            .get("Display Identifier")
            .and_then(|v| v.as_str())
            .ok_or("workspace_launch_failed: display identity unavailable")?;
        let space = display
            .pointer("/Current Space/ManagedSpaceID")
            .and_then(|v| v.as_u64())
            .ok_or("workspace_launch_failed: active Space unavailable")?;
        active.push((id.to_owned(), space));
    }
    active.sort();
    Ok(active)
}
