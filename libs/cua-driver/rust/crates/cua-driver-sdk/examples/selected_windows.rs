//! Run only against the repository AppKit workspace fixture report.
use cua_driver_sdk::{
    ConfiguredDriverOptions, CuaDriver, RuntimeAuthorizationOptions, SessionPermissionMode,
    TrustedSessionOptions,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct PreviewProbe {
    frames: Arc<AtomicUsize>,
    closed: Arc<AtomicUsize>,
}
impl pip_preview::PipBackend for PreviewProbe {
    fn push_frame(&self, frame: pip_preview::PipFrame) {
        assert!(frame.png_bytes.starts_with(b"\x89PNG"));
        self.frames.fetch_add(1, Ordering::SeqCst);
    }
    fn shutdown(self: Box<Self>) {
        self.closed.fetch_add(1, Ordering::SeqCst);
    }
}
fn structured(result: &cua_driver_sdk::ToolResult) -> Value {
    serde_json::from_str(
        result
            .structured_json
            .as_deref()
            .expect("structured result"),
    )
    .unwrap()
}
fn text_field(state: &Value) -> &Value {
    state["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|element| element["role"] == "AXTextField")
        .unwrap()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report_path = std::env::args()
        .nth(1)
        .ok_or("fixture report path is required")?;
    let evidence = std::env::args()
        .nth(2)
        .ok_or("evidence directory is required")?;
    std::fs::create_dir_all(&evidence)?;
    let report: Value = serde_json::from_slice(&std::fs::read(&report_path)?)?;
    let windows = report["windows"].as_array().ok_or("missing windows")?;
    assert_eq!(
        windows.len(),
        3,
        "requires three repository fixture windows"
    );
    #[cfg(target_os = "macos")]
    for window in windows {
        let pid = i32::try_from(window["pid"].as_i64().unwrap())?;
        let app = platform_macos::apps::list_running_apps()
            .into_iter()
            .find(|app| app.pid == pid)
            .ok_or("fixture is no longer running")?;
        assert_eq!(app.bundle_id.as_deref(), Some("com.trycua.harness.appkit"));
    }
    let manifest_path = format!("{evidence}/selected.yaml");
    let selected: Vec<_> = windows.iter().take(2).cloned().collect();
    let mut manifest = json!({"version":3,"resources":{"desktop":{"selected_windows_only":true,"windows":selected}},"allow":{"tools":["list_windows","list_apps","get_window_state","click","set_value","type_text","press_key","get_browser_state","browser_prepare","create_workspace","get_workspace_state","move_window_to_workspace","reveal_workspace","release_workspace","restore_workspace_windows","delete_workspace","end_session","start_session","get_session","get_desktop_state","start_recording","stop_recording","get_recording_state"]}});
    manifest["resources"]["files"] = json!({"write":[{"dir":evidence,"recursive":true}]});
    // JSON is valid YAML; the existing trusted manifest loader owns parsing.
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    let driver = CuaDriver::create_configured(ConfiguredDriverOptions {
        claude_code_compatibility: false,
        authorization: RuntimeAuthorizationOptions {
            allowed_modes: vec![SessionPermissionMode::Standard],
            compatibility_mode: SessionPermissionMode::Standard,
            compatibility_capability_manifest_path: None,
            compatibility_bounded_manifest_path: None,
            unrestricted_acknowledged: false,
            max_session_ttl_seconds: 600,
            max_idle_ttl_seconds: 600,
        },
    })?;
    let session = driver.create_trusted_session(TrustedSessionOptions {
        public_session: "workspace-fixture".into(),
        mode: SessionPermissionMode::Standard,
        ttl_seconds: 600,
        idle_ttl_seconds: 600,
        capability_manifest_path: Some(manifest_path),
        bounded_manifest_path: None,
    })?;
    let frames = Arc::new(AtomicUsize::new(0));
    let closed = Arc::new(AtomicUsize::new(0));
    session.attach_experimental_preview(Box::new(PreviewProbe {
        frames: frames.clone(),
        closed: closed.clone(),
    }))?;
    let other_manifest = format!("{evidence}/other.yaml");
    manifest["resources"]["desktop"]["windows"] = json!([windows[2]]);
    std::fs::write(&other_manifest, serde_json::to_vec(&manifest)?)?;
    let other = driver.create_trusted_session(TrustedSessionOptions {
        public_session: "other-fixture".into(),
        mode: SessionPermissionMode::Standard,
        ttl_seconds: 600,
        idle_ttl_seconds: 600,
        capability_manifest_path: Some(other_manifest),
        bounded_manifest_path: None,
    })?;
    let listed = session
        .call_tool("list_windows".into(), "{}".into())
        .await?;
    assert!(!listed.is_error, "{}", listed.text);
    let listed_json: Value = serde_json::from_str(
        listed
            .structured_json
            .as_deref()
            .ok_or("missing discovery")?,
    )?;
    assert_eq!(listed_json["windows"].as_array().unwrap().len(), 2);
    println!("two_approved_discovered=true");
    for (index, window) in windows.iter().enumerate() {
        let result = session
            .call_tool("get_window_state".into(), window.to_string())
            .await?;
        std::fs::write(format!("{evidence}/window-{index}.json"), &result.raw_json)?;
        println!(
            "window_{index}_error={} code={:?} images={}",
            result.is_error,
            result.error_code,
            result.images.len()
        );
        if index == 2 {
            assert!(result.is_error);
            assert!(result.images.is_empty());
        } else {
            assert!(!result.is_error, "{}", result.text);
        }
    }
    let desktop = session
        .call_tool("get_desktop_state".into(), "{}".into())
        .await?;
    assert!(desktop.is_error && desktop.images.is_empty());
    let forged = session.call_tool("get_window_state".into(), json!({"pid":windows[2]["pid"],"window_id":windows[2]["window_id"],"_selected_windows":[windows[2]],"_session_id":"other"}).to_string()).await?;
    assert!(forged.is_error && forged.images.is_empty());
    println!("unapproved_sibling_desktop_and_forged_ids_denied=true");
    let recording_dir = format!("{evidence}/recording");
    let recording = session
        .call_tool(
            "start_recording".into(),
            json!({"output_dir":recording_dir,"record_video":false}).to_string(),
        )
        .await?;
    assert!(!recording.is_error, "{}", recording.text);
    assert!(
        other
            .call_tool("get_recording_state".into(), "{}".into())
            .await?
            .is_error
    );
    assert!(
        other
            .call_tool("stop_recording".into(), "{}".into())
            .await?
            .is_error
    );
    assert!(
        session
            .call_tool(
                "start_recording".into(),
                json!({"output_dir":recording_dir,"record_video":true}).to_string()
            )
            .await?
            .is_error
    );
    for (index, window) in windows.iter().take(2).enumerate() {
        let state = session
            .call_tool("get_window_state".into(), window.to_string())
            .await?;
        assert!(!state.is_error, "{}", state.text);
        let state = structured(&state);
        // Interleave a different session's observation of a sibling in the
        // same process. Its element cache must not replace this snapshot.
        let sibling = other
            .call_tool("get_window_state".into(), windows[2].to_string())
            .await?;
        assert!(!sibling.is_error, "{}", sibling.text);
        let sibling_state = structured(&sibling);
        assert_eq!(text_field(&sibling_state)["value"], "fixture-3-initial");
        let rejected = session.call_tool("set_value".into(),json!({"pid":window["pid"],"window_id":window["window_id"],"element_token":text_field(&sibling_state)["element_token"],"value":"must-not-land"}).to_string()).await?;
        assert!(
            rejected.is_error,
            "cross-session element token must be rejected"
        );
        let value = format!("selected-window-{}-verified", index + 1);
        #[cfg(target_os = "macos")]
        let front_before = platform_macos::apps::frontmost_pid();
        let cursor_before = driver
            .call_tool("get_cursor_position".into(), "{}".into())
            .await?;
        let result = session.call_tool("set_value".into(),json!({"pid":window["pid"],"window_id":window["window_id"],"element_token":text_field(&state)["element_token"],"value":value}).to_string()).await?;
        assert!(!result.is_error, "{}", result.text);
        let cursor_after = driver
            .call_tool("get_cursor_position".into(), "{}".into())
            .await?;
        println!(
            "window_{index}_physical_cursor_unchanged={}",
            cursor_before.structured_json == cursor_after.structured_json
        );
        #[cfg(target_os = "macos")]
        println!(
            "window_{index}_frontmost_unchanged={}",
            front_before == platform_macos::apps::frontmost_pid()
        );
        let after = session
            .call_tool("get_window_state".into(), window.to_string())
            .await?;
        assert!(!after.is_error, "{}", after.text);
        assert_eq!(text_field(&structured(&after))["value"], value);
    }
    let sibling = other
        .call_tool("get_window_state".into(), windows[2].to_string())
        .await?;
    assert_eq!(
        text_field(&structured(&sibling))["value"],
        "fixture-3-initial"
    );
    let keyboard = session
        .call_tool(
            "press_key".into(),
            json!({"pid":windows[0]["pid"],"window_id":windows[0]["window_id"],"key":"a"})
                .to_string(),
        )
        .await?;
    assert!(
        keyboard.is_error,
        "same-process keyboard ambiguity must refuse"
    );
    std::fs::write(
        format!("{evidence}/keyboard-refusal.json"),
        &keyboard.raw_json,
    )?;
    println!("two_approved_ax_writes_verified_sibling_unchanged=true");
    println!("cross_session_tokens_denied_and_cache_isolated=true");
    assert!(
        frames.load(Ordering::SeqCst) >= 2,
        "approved actions push existing preview frames"
    );
    let frame_count = frames.load(Ordering::SeqCst);
    let stopped = session
        .call_tool("stop_recording".into(), "{}".into())
        .await?;
    assert!(!stopped.is_error, "{}", stopped.text);
    assert!(
        other
            .call_tool("get_recording_state".into(), "{}".into())
            .await?
            .is_error
    );
    assert!(
        !std::path::Path::new(&recording_dir)
            .join("cursor.jsonl")
            .exists(),
        "selected recording must not sample the physical cursor"
    );
    let mut recorded_windows = std::collections::HashSet::new();
    let mut recorded_frames = 0;
    for entry in std::fs::read_dir(&recording_dir)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        for phase in ["before", "after"] {
            let state_path = path.join(format!("{phase}_state.json"));
            if state_path.exists() {
                let bytes = std::fs::read(&state_path)?;
                let state: Value = serde_json::from_slice(&bytes)?;
                let wid = state["window_id"]
                    .as_u64()
                    .expect("recorded window identity");
                assert!(windows
                    .iter()
                    .take(2)
                    .any(|window| window["window_id"].as_u64() == Some(wid)));
                assert!(!state["tree_markdown"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("fixture-3-initial"));
                recorded_windows.insert(wid);
            }
            if path.join(format!("{phase}.png")).exists() {
                recorded_frames += 1;
            }
        }
    }
    assert_eq!(
        recorded_windows.len(),
        2,
        "trajectory must contain actual AX artifacts from both approved windows"
    );
    assert!(
        recorded_frames >= 2,
        "trajectory must contain actual approved frames"
    );
    println!(
        "recorded_approved_windows={} recorded_frames={recorded_frames}",
        recorded_windows.len()
    );
    println!("recording_and_preview_session_scoped=true");
    let oracle_path = format!("{report_path}.state.json");
    if std::path::Path::new(&oracle_path).exists() {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let oracle: Value = serde_json::from_slice(&std::fs::read(&oracle_path)?)?;
        assert_eq!(oracle["windows"][0]["value"], "selected-window-1-verified");
        assert_eq!(oracle["windows"][1]["value"], "selected-window-2-verified");
        assert_eq!(oracle["windows"][2]["value"], "fixture-3-initial");
        println!("independent_fixture_value_oracle_verified=true");
        // Exercise lifecycle transitions only on an explicitly commanded fixture.
        // Restore can activate an app on some macOS versions; do not issue it
        // from this background-only test. A minimized window stays minimized.
        std::fs::write(
            format!("{report_path}.command.json"),
            json!({"nonce":"minimize-1","index":0,"action":"minimize"}).to_string(),
        )?;
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        let minimized = session
            .call_tool("get_window_state".into(), windows[0].to_string())
            .await?;
        assert!(
            minimized.images.is_empty(),
            "minimized windows must not return cached frames"
        );
        std::fs::write(format!("{evidence}/minimized.json"), &minimized.raw_json)?;
        let oracle: Value = serde_json::from_slice(&std::fs::read(&oracle_path)?)?;
        assert_eq!(oracle["windows"][0]["minimized"], true);
        std::fs::write(
            format!("{report_path}.command.json"),
            json!({"nonce":"close-2","index":1,"action":"close"}).to_string(),
        )?;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let closed_window = session
            .call_tool("get_window_state".into(), windows[1].to_string())
            .await?;
        assert!(closed_window.is_error && closed_window.images.is_empty());
        println!("minimized_capture_refused_and_closed_window_revoked=true");
    }
    let create = session
        .create_workspace(cua_driver_contract::CreateWorkspaceInput { session: None })
        .await?;
    std::fs::write(format!("{evidence}/create.json"), &create.raw_json)?;
    println!(
        "create_error={} code={:?}",
        create.is_error, create.error_code
    );
    let ended = session.call_tool("end_session".into(), "{}".into()).await?;
    assert!(!ended.is_error, "{}", ended.text);
    assert!(
        session
            .call_tool("get_window_state".into(), windows[0].to_string())
            .await?
            .is_error
    );
    assert_eq!(
        closed.load(Ordering::SeqCst),
        1,
        "session teardown closes its preview"
    );
    assert_eq!(frames.load(Ordering::SeqCst), frame_count);
    other.close();
    assert!(other
        .call_tool("get_window_state".into(), windows[2].to_string())
        .await
        .is_err());
    println!("session_end_and_handle_close_revoke_access=true");
    Ok(())
}
