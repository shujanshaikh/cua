//! Trusted exact-window selection. Native identity leases are acquired before
//! admitting agent calls, never from IDs supplied by an agent.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowTarget {
    pub pid: i64,
    pub window_id: u64,
}

/// An owned native lifetime witness, not a repeated numeric ID lookup.
pub trait WindowIdentity: Send + Sync {
    fn validate(&self) -> Result<(), String>;
}

pub trait WindowSelectionBackend: Send + Sync {
    fn bind(&self, target: WindowTarget) -> Result<Arc<dyn WindowIdentity>, String>;
}

pub struct SelectedWindows {
    windows: Mutex<HashMap<WindowTarget, Option<Arc<dyn WindowIdentity>>>>,
}

impl SelectedWindows {
    pub fn bind(
        targets: &[WindowTarget],
        backend: &dyn WindowSelectionBackend,
    ) -> Result<Self, String> {
        let mut windows = HashMap::new();
        for &target in targets {
            if target.pid <= 0 || target.window_id == 0 || windows.contains_key(&target) {
                return Err("selected_window_invalid: targets must be unique exact windows".into());
            }
            windows.insert(target, Some(backend.bind(target)?));
        }
        Ok(Self {
            windows: Mutex::new(windows),
        })
    }

    /// Once a lifetime proof fails it stays revoked, even if the numeric IDs
    /// subsequently reappear. A trusted host must create a new selection.
    pub fn validate(&self, target: WindowTarget) -> Result<(), String> {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let lease = windows
            .get_mut(&target)
            .ok_or("selected_window_denied: target is outside this session's selection")?;
        let Some(identity) = lease else {
            return Err("selected_window_stale: native lifetime is no longer valid".into());
        };
        if identity.validate().is_err() {
            *lease = None;
            return Err("selected_window_stale: native lifetime could not be re-proven".into());
        }
        Ok(())
    }

    pub fn live_targets(&self) -> Vec<WindowTarget> {
        let targets: Vec<_> = self
            .windows
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .keys()
            .copied()
            .collect();
        targets
            .into_iter()
            .filter(|target| self.validate(*target).is_ok())
            .collect()
    }

    pub fn target(args: &Value) -> Result<WindowTarget, String> {
        Ok(WindowTarget {
            pid: args
                .get("pid")
                .and_then(Value::as_i64)
                .ok_or("selected_window_required: exact pid is required")?,
            window_id: args
                .get("window_id")
                .and_then(Value::as_u64)
                .ok_or("selected_window_required: exact window_id is required")?,
        })
    }

    /// Closed admission list. New tools must explicitly prove their scope
    /// before selected-window sessions may call them.
    pub fn authorize(&self, tool: &str, args: &Value) -> Result<(), String> {
        if args.get("scope").and_then(Value::as_str) == Some("desktop") {
            return Err("selected_window_background_required: desktop input is unavailable".into());
        }
        match tool {
            "start_recording" if args.get("record_video").and_then(Value::as_bool) != Some(true) => Ok(()),
            "get_recording_state" | "stop_recording" => Ok(()),
            "get_browser_state" if args.get("target_id").is_none() => self.validate(Self::target(args)?),
            "get_browser_state" | "browser_navigate" | "browser_click" | "browser_type" | "browser_dialog" => {
                if args.get("delivery_mode").and_then(Value::as_str).is_some_and(|mode| mode.eq_ignore_ascii_case("foreground")) { return Err("selected_window_background_required: browser foreground delivery is not admitted".into()); }
                // Registry must validate implementation-attested native ownership.
                Ok(())
            }
            "list_windows" | "list_apps" | "start_session" | "get_session" | "end_session"
            | "get_session_state" | "list_sessions" | "wait"
            | "create_workspace" | "get_workspace_state" | "reveal_workspace" | "release_workspace"
            | "restore_workspace_windows" | "delete_workspace" => Ok(()),
            "get_window_state" | "click" | "double_click" | "right_click" | "scroll"
            | "drag" | "type_text" | "press_key" | "hotkey"
            | "set_value" | "move_window_to_workspace" => {
                if args.get("delivery_mode").and_then(Value::as_str).is_some_and(|mode| mode.eq_ignore_ascii_case("foreground")) {
                    return Err("selected_window_background_required: foreground delivery is not admitted".into());
                }
                self.validate(Self::target(args)?)
            }
            _ => Err("selected_window_operation_unsupported: this tool has no proven exact-window selection boundary".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Identity(Arc<AtomicBool>);
    impl WindowIdentity for Identity {
        fn validate(&self) -> Result<(), String> {
            self.0
                .load(Ordering::SeqCst)
                .then_some(())
                .ok_or("closed".into())
        }
    }
    struct Backend(Arc<AtomicBool>);
    impl WindowSelectionBackend for Backend {
        fn bind(&self, _: WindowTarget) -> Result<Arc<dyn WindowIdentity>, String> {
            Ok(Arc::new(Identity(self.0.clone())))
        }
    }
    #[test]
    fn two_windows_do_not_authorize_a_same_process_sibling() {
        let live = Arc::new(AtomicBool::new(true));
        let targets = [
            WindowTarget {
                pid: 10,
                window_id: 1,
            },
            WindowTarget {
                pid: 10,
                window_id: 2,
            },
        ];
        let selection = SelectedWindows::bind(&targets, &Backend(live)).unwrap();
        for target in targets {
            selection
                .authorize("click", &serde_json::to_value(target).unwrap())
                .unwrap();
        }
        assert!(selection
            .authorize("click", &serde_json::json!({"pid":10,"window_id":3}))
            .is_err());
        assert!(selection
            .authorize("get_desktop_state", &serde_json::json!({}))
            .is_err());
        assert!(selection
            .authorize(
                "browser_prepare",
                &serde_json::json!({"pid":10,"window_id":1})
            )
            .is_err());
    }
    #[test]
    fn closed_identity_never_revives_and_other_sessions_do_not_inherit_it() {
        let live = Arc::new(AtomicBool::new(true));
        let target = WindowTarget {
            pid: 10,
            window_id: 1,
        };
        let selection = SelectedWindows::bind(&[target], &Backend(live.clone())).unwrap();
        live.store(false, Ordering::SeqCst);
        assert!(selection.validate(target).is_err());
        live.store(true, Ordering::SeqCst);
        assert!(selection.validate(target).is_err());
        let replacement = SelectedWindows::bind(&[target], &Backend(live)).unwrap();
        replacement.validate(target).unwrap();
        assert!(
            SelectedWindows::bind(&[], &Backend(Arc::new(AtomicBool::new(true))))
                .unwrap()
                .validate(target)
                .is_err()
        );
    }
}
