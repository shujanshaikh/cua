//! Installed-app supporting test: AX addressing, background delivery, exact
//! window scope. Oracles: native Space membership and AX value readback.
//! Requires a fresh workspace-apps manifest and authorized development driver.
//! Apps and the created desktop remain for inspection; no destructive cleanup.
#![cfg(target_os = "macos")]

use cua_driver_testkit::{Driver, McpDriver};
use serde_json::{json, Value};

#[test]
#[ignore = "creates a desktop and isolated Helium/TextEdit instances on an authorized Mac"]
fn workspace_apps_launch_and_edit_without_switching() {
    let manifest = std::env::var("CUA_WORKSPACE_APPS_MANIFEST")
        .expect("set CUA_WORKSPACE_APPS_MANIFEST to a fresh prepared manifest");
    let config: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(
        config["resources"]["desktop"]["selected_windows_only"],
        true
    );
    assert_eq!(config["resources"]["desktop"]["workspace_only"], true);
    let mut driver = McpDriver::spawn_with_env(&[
        ("CUA_DRIVER_PERMISSION_MODE", "standard"),
        ("CUA_DRIVER_CAPABILITY_MANIFEST_FILE", &manifest),
        ("CUA_DRIVER_CAPABILITY_MANIFEST_APPROVED", "1"),
    ])
    .expect("authorized development daemon must start");
    let before = platform_macos::apps::frontmost_pid();
    let created = driver.call("create_workspace", json!({}));
    assert!(!created.is_error(), "{}", created.text());
    assert_eq!(created.structured()["active"], false);
    for alias in ["helium", "notes"] {
        let launched = driver.call("launch_workspace_app", json!({"app":alias}));
        assert!(!launched.is_error(), "{}", launched.text());
        assert_eq!(launched.structured()["active"], false);
        assert!(launched.structured()["windows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|window| window["state"] == "in_workspace"));
    }
    let windows = driver.call("list_windows", json!({}));
    let windows = windows.structured()["windows"].as_array().unwrap();
    assert_eq!(windows.len(), 2);
    let notes = windows
        .iter()
        .find(|window| window["app_name"] == "TextEdit")
        .unwrap();
    let pid = notes["pid"].as_i64().unwrap();
    let wid = notes["window_id"].as_u64().unwrap();
    let target = json!({"pid":pid,"window_id":wid,"capture_mode":"ax"});
    let observation = driver.call("get_window_state", target.clone());
    assert!(!observation.is_error(), "{}", observation.text());
    let field = observation.structured()["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|element| element["role"] == "AXTextArea")
        .unwrap();
    let value = "Cua workspace app launch and background editing verified.";
    let edit = driver.call(
        "set_value",
        json!({"pid":pid,"window_id":wid,
        "element_token":field["element_token"],"value":value}),
    );
    assert!(!edit.is_error(), "{}", edit.text());
    assert_eq!(edit.action_effect(), Some("confirmed"));
    let after = driver.call("get_window_state", target);
    assert!(after.structured()["elements"]
        .as_array()
        .unwrap()
        .iter()
        .any(|element| element["value"] == value));
    // Every sibling in this newly created process remains outside the grant.
    for sibling in platform_macos::windows::all_windows()
        .iter()
        .filter(|window| i64::from(window.pid) == pid && u64::from(window.window_id) != wid)
    {
        let denied = driver.call(
            "get_window_state",
            json!({"pid":pid,"window_id":sibling.window_id}),
        );
        assert!(denied.is_error());
        assert_eq!(denied.structured()["code"], "selected_window_denied");
    }
    assert_eq!(platform_macos::apps::frontmost_pid(), before);
    assert_eq!(
        driver.call("get_workspace_state", json!({})).structured()["active"],
        false
    );
    let ended = driver.call("end_session", json!({}));
    assert!(!ended.is_error(), "{}", ended.text());
}
