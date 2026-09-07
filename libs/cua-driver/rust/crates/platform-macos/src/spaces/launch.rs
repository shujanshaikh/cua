//! Workspace launch admission reuses the normal background app launcher.
use cua_driver_core::{
    selected_windows::{WindowSelectionBackend, WindowTarget},
    tool::Tool,
    workspace::{LaunchedWorkspaceWindow, WorkspaceApplication},
};
use serde_json::json;
use std::collections::HashSet;

pub(super) fn launch(recipe: &WorkspaceApplication) -> Result<LaunchedWorkspaceWindow, String> {
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
    let result = tokio::runtime::Handle::current().block_on(
        crate::tools::launch_app::LaunchAppTool.invoke(json!({
            "bundle_id": recipe.bundle_id,
            "creates_new_application_instance": true,
            "additional_arguments": recipe.arguments,
            "urls": recipe.urls,
        })),
    );
    if result.is_error == Some(true) {
        return Err("workspace_launch_failed: background launcher refused the configured app; no window access granted".into());
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
    if crate::apps::bundle_id_for_pid(pid).as_deref() != Some(recipe.bundle_id.as_str()) {
        return Err(
            "workspace_launch_failed: launched bundle identity does not match trusted recipe"
                .into(),
        );
    }
    let windows: Vec<_> = crate::windows::all_windows()
        .into_iter()
        .filter(|w| w.pid == pid)
        .collect();
    // Document apps may also create an untitled window or an open panel.
    // Bind only the exact document the trusted recipe requested; those other
    // windows remain outside the selection, including keyboard delivery.
    if recipe.urls.len() == 1 && std::path::Path::new(&recipe.urls[0]).is_file() {
        let mut matches = Vec::new();
        for window in &windows {
            let target = WindowTarget {
                pid: i64::from(pid),
                window_id: u64::from(window.window_id),
            };
            if let Ok(identity) = crate::selected_windows::MacosWindowSelection.bind(target) {
                if crate::selected_windows::matches_document(
                    identity.as_ref(),
                    std::path::Path::new(&recipe.urls[0]),
                ) {
                    matches.push(LaunchedWorkspaceWindow { target, identity });
                }
            }
        }
        if matches.len() == 1 {
            return Ok(matches.remove(0));
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
    Ok(LaunchedWorkspaceWindow { target, identity })
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
