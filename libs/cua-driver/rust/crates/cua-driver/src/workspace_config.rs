//! Operator setup for the ordinary CLI -> MCP proxy -> app daemon path.
use serde_json::{json, Value};
use std::path::Path;

pub fn prepare(output: &Path) -> anyhow::Result<Value> {
    anyhow::ensure!(
        cfg!(target_os = "macos"),
        "native workspaces are currently supported only on macOS"
    );
    // Never overwrite profiles, documents, or a previously approved policy.
    std::fs::create_dir(output)?;
    let root = output.canonicalize()?;
    let notes = root.join("Agent notes.txt");
    std::fs::write(&notes, "Agent workspace notes\n")?;
    let manifest = manifest(&root);
    let path = root.join("manifest.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest)?)?;
    let socket = std::env::temp_dir().join(format!("cua-{}.sock", uuid::Uuid::new_v4().simple()));
    let config = json!({"mcpServers":{"cua-driver":{
        "command": std::env::current_exe()?,
        "args":["mcp","--socket",socket],
        "env":{
            "CUA_DRIVER_PERMISSION_MODE":"standard",
            "CUA_DRIVER_CAPABILITY_MANIFEST_FILE":path,
            "CUA_DRIVER_CAPABILITY_MANIFEST_APPROVED":"1"
        }
    }}});
    std::fs::write(root.join("mcp.json"), serde_json::to_vec_pretty(&config)?)?;
    Ok(config)
}

fn manifest(root: &Path) -> Value {
    json!({"version":3,"resources":{
        "browser":{"selected_windows_only":true},
        "desktop":{"selected_windows_only":true,"workspace_only":true,
            "workspace_allow_mission_control":true,"windows":[],
            "workspace_launch_apps":true},
        "files":{"read":[{"dir":root,"recursive":true}],"write":[{"dir":root,"recursive":true}]}
    },"allow":{"tools":[
        "start_session","get_session","get_session_state","list_sessions","end_session",
        "create_workspace","get_workspace_state","launch_workspace_app","move_window_to_workspace",
        "reveal_workspace","restore_workspace_windows","release_workspace","delete_workspace",
        "get_browser_state","browser_navigate","browser_click","browser_type","browser_dialog",
        "launch_app","list_apps","list_windows","get_window_state","verify_state","click","double_click",
        "right_click","scroll","drag","set_value","type_text","press_key","hotkey",
        "invoke_menu","start_recording","get_recording_state","stop_recording"
    ]}})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normal_app_launch_is_enabled_without_aliases() {
        let root = tempfile::tempdir().unwrap();
        let manifest = manifest(root.path());
        let path = root.path().join("manifest.json");
        std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let loaded = cua_driver_core::session_manifest::load_manifest(&path).unwrap();
        assert!(loaded.workspace_launch_apps());
        assert!(loaded.workspace_only());
        assert!(loaded.workspace_application_aliases().is_empty());
        assert!(manifest["allow"]["tools"]
            .as_array()
            .unwrap()
            .contains(&json!("launch_app")));
    }
}
