//! Supporting macOS workspace diagnostic using a fresh workspace-apps manifest.
//! Creates apps and a desktop, leaves them for inspection, never reveals it.
use cua_driver_sdk::{
    ConfiguredDriverOptions, CuaDriver, CuaDriverSession, RuntimeAuthorizationOptions,
    SessionPermissionMode, TrustedSessionOptions,
};
use serde_json::{json, Value};
use std::path::Path;

async fn call(
    session: &CuaDriverSession,
    evidence: &Path,
    name: &str,
    args: Value,
    label: &str,
) -> Value {
    #[cfg(target_os = "macos")]
    let before = active_spaces();
    let result = session
        .call_tool(name.into(), args.to_string())
        .await
        .expect("SDK dispatch");
    std::fs::write(evidence.join(format!("{label}.json")), &result.raw_json).unwrap();
    println!(
        "{label}: error={} {}",
        result.is_error,
        result.text.chars().take(300).collect::<String>()
    );
    let mut data: Value =
        serde_json::from_str(result.structured_json.as_deref().unwrap_or("{}")).unwrap();
    data["is_error"] = json!(result.is_error || data["status"] == "refused");
    #[cfg(target_os = "macos")]
    {
        let after = active_spaces();
        std::fs::write(evidence.join(format!("{label}-desktop.json")),
            serde_json::to_vec_pretty(&json!({"before":before,"after":after})).unwrap()).unwrap();
        assert_eq!(before, after, "{label} changed the human desktop");
    }
    data
}

#[cfg(target_os = "macos")]
fn active_spaces() -> Value {
    let displays = platform_macos::spaces::managed_displays().expect("native Space inventory");
    json!(displays.as_array().expect("displays").iter().map(|display|
        json!({"display":display["Display Identifier"],"space":display["Current Space"]["ManagedSpaceID"]})
    ).collect::<Vec<_>>())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = std::env::args()
        .nth(1)
        .ok_or("fresh manifest path required")?;
    let evidence = std::env::args()
        .nth(2)
        .ok_or("new evidence directory required")?;
    let urls: Vec<String> = serde_json::from_str(&std::env::args().nth(3).unwrap_or_else(|| {
        r#"["https://example.com","https://www.iana.org/help/example-domains"]"#.into()
    }))?;
    let evidence = Path::new(&evidence);
    std::fs::create_dir(evidence)?;
    let config: Value = serde_json::from_slice(&std::fs::read(&manifest)?)?;
    assert_eq!(config["resources"]["desktop"]["workspace_only"], true);
    assert_eq!(
        config["resources"]["desktop"]["selected_windows_only"],
        true
    );
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
        public_session: "workspace-apps-audit".into(),
        mode: SessionPermissionMode::Standard,
        ttl_seconds: 600,
        idle_ttl_seconds: 600,
        capability_manifest_path: Some(manifest),
        bounded_manifest_path: None,
    })?;
    let created = call(&session, evidence, "create_workspace", json!({}), "create").await;
    assert_eq!(created["is_error"], false);
    assert_eq!(created["active"], false);
    let mut browsers = Vec::new();
    for app in ["helium", "helium-two", "notes", "ghostty"] {
        if config["resources"]["desktop"]["workspace_applications"]
            .get(app)
            .is_none()
        {
            continue;
        }
        let launched = call(
            &session,
            evidence,
            "launch_workspace_app",
            json!({"app":app}),
            &format!("launch-{app}"),
        )
        .await;
        if launched["is_error"] == true {
            panic!("{app} launch failed: {launched}");
        }
        assert_eq!(launched["active"], false);
        let target = json!({"pid":launched["launched_app"]["pid"],"window_id":launched["launched_app"]["window_id"]});
        let state = call(
            &session,
            evidence,
            "get_window_state",
            target.clone(),
            &format!("observe-{app}"),
        )
        .await;
        if app == "notes" {
            if let Some(field) = state["elements"]
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["role"] == "AXTextArea"))
            {
                let mut args = target.clone();
                args["element_token"] = field["element_token"].clone();
                args["value"] = json!(
                    "• Workspace note edit.\n• Exact window readback.\n• Desktop remains inactive."
                );
                call(&session, evidence, "set_value", args, "edit-notes").await;
                let mut args = target.clone();
                args["include_screenshot"] = json!(false);
                let verified = call(&session, evidence, "get_window_state", args, "verify-notes").await;
                assert!(verified["elements"].as_array().unwrap().iter().any(|row|
                    row["label"] == "• Workspace note edit.\n• Exact window readback.\n• Desktop remains inactive."));
            }
        } else if app.starts_with("helium") {
            let bound = call(
                &session,
                evidence,
                "get_browser_state",
                target.clone(),
                &format!("{app}-bind-browser"),
            )
            .await;
            assert_eq!(bound["is_error"], false, "browser binding failed");
            if bound["is_error"] == false {
                let args = json!({"target_id":bound["target_id"],"tab_id":bound["tabs"][0]["tab_id"],"include_screenshot":true,"snapshot_format":"semantic_v2"});
                call(
                    &session,
                    evidence,
                    "get_browser_state",
                    args,
                    &format!("{app}-browser-page"),
                )
                .await;
            }
            // New tabs stay inside the approved native window. Navigate via
            // CDP, avoiding process-wide Return delivery on Chromium's omnibox.
            for (index, url) in urls.iter().enumerate() {
                let mut args = target.clone();
                args["include_screenshot"] = json!(false);
                let state = call(
                    &session,
                    evidence,
                    "get_window_state",
                    args,
                    &format!("{app}-tab-{index}-ax"),
                )
                .await;
                let button = state["elements"].as_array().and_then(|rows| {
                    rows.iter()
                        .find(|row| row["role"] == "AXButton" && row["label"] == "New Tab")
                });
                let Some(button) = button else {
                    panic!("{app} has no New Tab button");
                };
                let mut args = target.clone();
                args["element_token"] = button["element_token"].clone();
                call(
                    &session,
                    evidence,
                    "click",
                    args,
                    &format!("{app}-tab-{index}-create"),
                )
                .await;
                let bound = call(
                    &session,
                    evidence,
                    "get_browser_state",
                    target.clone(),
                    &format!("{app}-tab-{index}-bind"),
                )
                .await;
                let tab = bound["tabs"].as_array().and_then(|tabs| {
                    tabs.iter().find(|tab| {
                        tab["url"].as_str().is_some_and(|url| {
                            url.starts_with("chrome://newtab") || url == "about:blank"
                        })
                    })
                });
                let tab = tab.expect("new tab was observed");
                {
                    let url = format!("{url}#workspace-{app}-{index}");
                    let args =
                        json!({"target_id":bound["target_id"],"tab_id":tab["tab_id"],"url":url});
                    call(
                        &session,
                        evidence,
                        "browser_navigate",
                        args.clone(),
                        &format!("{app}-tab-{index}-navigate"),
                    )
                    .await;
                    let mut observe = args;
                    observe.as_object_mut().unwrap().remove("url");
                    let verified = call(
                        &session,
                        evidence,
                        "get_browser_state",
                        observe,
                        &format!("{app}-tab-{index}-verify"),
                    )
                    .await;
                    assert_eq!(verified["url"], url, "navigation readback");
                }
            }
            let final_tabs = call(&session,evidence,"get_browser_state",target.clone(),&format!("{app}-final-tabs")).await;
            browsers.push((app, target, final_tabs));
        } else if app == "ghostty" {
            let routes = state["background_input"]["routes"].as_array();
            if routes.is_some_and(|rows| {
                rows.iter()
                    .any(|row| row["route"] == "pid_keyboard" && row["status"] == "available")
            }) {
                let mut args = target.clone();
                args["text"] = json!("ls\npwd\n");
                call(&session, evidence, "type_text", args, "terminal-commands").await;
                let mut args = target.clone();
                args["include_screenshot"] = json!(false);
                call(
                    &session,
                    evidence,
                    "get_window_state",
                    args,
                    "terminal-readback",
                )
                .await;
            }
        }
    }
    for (app, target, expected) in &browsers {
        let actual = call(&session,evidence,"get_browser_state",target.clone(),&format!("{app}-isolation-readback")).await;
        assert_eq!(actual["tabs"], expected["tabs"], "other app changed {app}'s tabs");
    }
    if browsers.len() == 2 {
        assert_ne!(browsers[0].1["window_id"], browsers[1].1["window_id"]);
        let denied = call(&session,evidence,"get_browser_state",json!({
            "target_id":browsers[0].2["target_id"],"tab_id":browsers[1].2["tabs"][0]["tab_id"]
        }),"cross-window-tab-denied").await;
        assert_eq!(denied["is_error"], true, "foreign tab must be refused");
    }
    let state = call(
        &session,
        evidence,
        "get_workspace_state",
        json!({}),
        "final-workspace",
    )
    .await;
    assert_eq!(state["active"], false);
    std::fs::write(evidence.join("passed.json"),serde_json::to_vec_pretty(&json!({"passed":true,"browser_windows":browsers.len(),"workspace":state})).unwrap())?;
    session.close();
    driver.shutdown().await?;
    Ok(())
}
