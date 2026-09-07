//! Session-owned placement, independent from selected-window authorization.
//! Space metadata is not a security boundary and is never a capture source.

use crate::{
    protocol::ToolResult,
    selected_windows::{SelectedWindows, WindowTarget},
    tool::{Tool, ToolDef, ToolRegistry},
};
use async_trait::async_trait;
use cua_driver_contract::{WorkspaceStateOutput, WorkspaceWindowState};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub trait WorkspaceBackend: Send + Sync {
    fn create(&self) -> Result<u64, String>;
    fn state(&self, space: u64) -> Result<(bool, bool), String>;
    fn membership(&self, target: WindowTarget) -> Result<Vec<u64>, String>;
    fn move_window(&self, target: WindowTarget, space: u64) -> Result<(), String>;
    fn reveal(&self, space: u64) -> Result<(), String>;
    fn delete(&self, _space: u64) -> Result<(), String> {
        Err("workspace_operation_unsupported: safe native Space deletion is unavailable".into())
    }
}

struct Unsupported;
impl WorkspaceBackend for Unsupported {
    fn create(&self) -> Result<u64, String> {
        Err(unsupported())
    }
    fn state(&self, _: u64) -> Result<(bool, bool), String> {
        Err(unsupported())
    }
    fn membership(&self, _: WindowTarget) -> Result<Vec<u64>, String> {
        Err(unsupported())
    }
    fn move_window(&self, _: WindowTarget, _: u64) -> Result<(), String> {
        Err(unsupported())
    }
    fn reveal(&self, _: u64) -> Result<(), String> {
        Err(unsupported())
    }
}
fn unsupported() -> String {
    "workspace_operation_unsupported: native session workspaces are unavailable on this platform"
        .into()
}

struct Workspace {
    space: u64,
    created: bool,
    // Preserve the first origin even after retries and partial native failures.
    moved: HashMap<WindowTarget, Vec<u64>>,
}

struct Workspaces {
    backend: Arc<dyn WorkspaceBackend>,
    owned: Mutex<HashMap<String, Workspace>>,
}

impl Workspaces {
    fn snapshot(
        &self,
        workspace: Option<&Workspace>,
        selection: &SelectedWindows,
    ) -> Result<WorkspaceStateOutput, String> {
        let Some(workspace) = workspace else {
            return Ok(WorkspaceStateOutput {
                owned: false,
                space_id: None,
                space_created: false,
                space_exists: false,
                active: false,
                private_api: cfg!(target_os = "macos"),
                windows: vec![],
            });
        };
        let (exists, active) = self.backend.state(workspace.space)?;
        let windows = workspace
            .moved
            .iter()
            .map(|(&target, original)| {
                let current = selection
                    .validate(target)
                    .and_then(|_| self.backend.membership(target));
                let (current_space_ids, state) = match current {
                    Err(_) => (vec![], "stale_or_unavailable"),
                    Ok(ids) if !exists => (ids, "space_deleted"),
                    Ok(ids) if ids == [workspace.space] => (ids, "in_workspace"),
                    Ok(ids) if &ids == original => (ids, "at_origin"),
                    Ok(ids) => (ids, "moved_or_transitioning"),
                };
                WorkspaceWindowState {
                    pid: target.pid,
                    window_id: target.window_id,
                    original_space_ids: original.clone(),
                    current_space_ids,
                    state: state.into(),
                }
            })
            .collect();
        Ok(WorkspaceStateOutput {
            owned: true,
            space_id: Some(workspace.space),
            space_created: workspace.created,
            space_exists: exists,
            active,
            private_api: cfg!(target_os = "macos"),
            windows,
        })
    }

    fn execute(&self, name: &str, args: &Value) -> Result<WorkspaceStateOutput, String> {
        let context = crate::tool::current_dispatch_authorization_context()
            .ok_or("workspace_requires_trusted_session")?;
        let selection = context.selected_windows()?.ok_or("workspace_requires_selected_windows: configure approved windows separately at trusted startup")?;
        let session = args
            .get("session")
            .or_else(|| args.get("_session_id"))
            .and_then(Value::as_str)
            .ok_or("workspace_session_required")?;
        let mut owned = self.owned.lock().unwrap_or_else(|error| error.into_inner());
        match name {
            "create_workspace" => {
                if !owned.contains_key(session) {
                    let attached = context
                        .capability_manifest()
                        .and_then(|manifest| manifest.workspace_space_id());
                    let space = match attached {
                        Some(space) => space,
                        None => self.backend.create()?,
                    };
                    if owned.values().any(|workspace| workspace.space == space) {
                        return Err(
                            "workspace_already_owned: another session owns this Space".into()
                        );
                    }
                    if attached.is_some() && !self.backend.state(space)?.0 {
                        return Err(
                            "workspace_space_deleted: configured Space does not exist".into()
                        );
                    }
                    // Retain a successful native allocation even if the following
                    // state query fails, so explicit release remains possible.
                    owned.insert(
                        session.into(),
                        Workspace {
                            space,
                            created: attached.is_none(),
                            moved: HashMap::new(),
                        },
                    );
                }
            }
            "get_workspace_state" => {}
            "release_workspace" => {
                let released = owned.remove(session);
                let mut snapshot =
                    self.snapshot(released.as_ref(), selection)
                        .map_err(|error| {
                            format!(
                                "workspace_released_state_unavailable: ownership released; {error}"
                            )
                        })?;
                snapshot.owned = false;
                return Ok(snapshot);
            }
            _ => {
                let workspace = owned
                    .get_mut(session)
                    .ok_or("workspace_not_found: this session owns no workspace")?;
                match name {
                    "move_window_to_workspace" => {
                        let target = SelectedWindows::target(args)?;
                        selection.validate(target)?;
                        let origin = self.backend.membership(target)?;
                        if origin.len() != 1 {
                            return Err(
                                "workspace_operation_unsupported: window membership is ambiguous"
                                    .into(),
                            );
                        }
                        workspace.moved.entry(target).or_insert(origin);
                        selection.validate(target)?;
                        self.backend.move_window(target, workspace.space)?;
                        selection.validate(target)?;
                        if self.backend.membership(target)? != [workspace.space] {
                            return Err(
                                "workspace_move_incomplete: destination membership did not verify"
                                    .into(),
                            );
                        }
                    }
                    "reveal_workspace" => self.backend.reveal(workspace.space)?,
                    "restore_workspace_windows" => {
                        let mut failures = Vec::new();
                        for (&target, origin) in &workspace.moved {
                            let outcome = (|| {
                                selection.validate(target)?;
                                let current = self.backend.membership(target)?;
                                if &current == origin {
                                    return Ok(());
                                }
                                if current != [workspace.space] {
                                    return Err("workspace_user_moved: refusing to override a later placement".into());
                                }
                                self.backend.move_window(target, origin[0])
                            })();
                            if let Err(error) = outcome {
                                failures.push(error);
                            }
                        }
                        if !failures.is_empty() {
                            return Err(format!(
                                "workspace_restore_incomplete: {}",
                                failures.join("; ")
                            ));
                        }
                    }
                    "delete_workspace" => {
                        if !workspace.created {
                            return Err(
                                "workspace_not_created: cannot delete a pre-existing Space".into(),
                            );
                        }
                        self.backend.delete(workspace.space)?;
                        let mut snapshot = self.snapshot(Some(workspace), selection)?;
                        if snapshot.space_exists {
                            return Err("workspace_delete_incomplete: Space still exists".into());
                        }
                        snapshot.owned = false;
                        owned.remove(session);
                        return Ok(snapshot);
                    }
                    _ => return Err(unsupported()),
                }
            }
        }
        self.snapshot(owned.get(session), selection)
    }
}

struct WorkspaceTool {
    def: ToolDef,
    workspaces: Arc<Workspaces>,
}
#[async_trait]
impl Tool for WorkspaceTool {
    fn def(&self) -> &ToolDef {
        &self.def
    }
    async fn invoke(&self, args: Value) -> ToolResult {
        use cua_driver_contract::*;
        macro_rules! check {
            ($input:ty) => {
                crate::tool_args::parse_typed_input::<$input>(&self.def.name, args.clone())
                    .map(|_| ())
            };
        }
        let checked = match self.def.name.as_str() {
            "create_workspace" => check!(CreateWorkspaceInput),
            "get_workspace_state" => check!(GetWorkspaceStateInput),
            "move_window_to_workspace" => check!(MoveWindowToWorkspaceInput),
            "reveal_workspace" => check!(RevealWorkspaceInput),
            "restore_workspace_windows" => check!(RestoreWorkspaceWindowsInput),
            "release_workspace" => check!(ReleaseWorkspaceInput),
            "delete_workspace" => check!(DeleteWorkspaceInput),
            _ => return ToolResult::error(unsupported()),
        };
        if let Err(error) = checked {
            return error;
        }
        match self.workspaces.execute(&self.def.name, &args) {
            Ok(state) => ToolResult::text("Workspace state verified.").with_structured(serde_json::to_value(state).expect("workspace state serializes")),
            Err(message) => ToolResult::error(message.clone()).with_structured(json!({"code":message.split(':').next().unwrap_or("workspace_failed"), "message":message})),
        }
    }
}

pub fn register(registry: &mut ToolRegistry, backend: Option<Arc<dyn WorkspaceBackend>>) {
    let workspaces = Arc::new(Workspaces {
        backend: backend.unwrap_or_else(|| Arc::new(Unsupported)),
        owned: Mutex::default(),
    });
    let weak = Arc::downgrade(&workspaces);
    registry.retain_session_end_hook(crate::session::register_scoped_session_end_hook(
        move |session| {
            if let Some(workspaces) = weak.upgrade() {
                workspaces
                    .owned
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .remove(session);
            }
        },
    ));
    for contract in cua_driver_contract::manifest()
        .tools
        .into_iter()
        .filter(|contract| {
            contract
                .capabilities
                .iter()
                .any(|capability| capability == "session.workspace")
        })
    {
        registry.register(Box::new(WorkspaceTool {
            def: ToolDef {
                name: contract.name,
                description: contract.description,
                input_schema: contract.input_schema,
                read_only: contract.annotations.read_only,
                destructive: contract.annotations.destructive,
                idempotent: contract.annotations.idempotent,
                open_world: contract.annotations.open_world,
            },
            workspaces: workspaces.clone(),
        }));
    }
}

#[cfg(all(test, feature = "yaml"))]
mod tests {
    use super::*;
    use crate::authorization::PermissionMode;
    use crate::selected_windows::{WindowIdentity, WindowSelectionBackend};
    use crate::session_authorization::{
        DelegatedSessionRequest, EffectiveAuthorizationContext, SessionAuthorizationRegistry,
        SessionModeCeiling,
    };
    use std::time::Duration;

    struct Live;
    impl WindowIdentity for Live {
        fn validate(&self) -> Result<(), String> {
            Ok(())
        }
    }
    #[derive(Default)]
    struct Native {
        membership: Mutex<HashMap<WindowTarget, Vec<u64>>>,
        state_fails: std::sync::atomic::AtomicBool,
        deleted: std::sync::atomic::AtomicBool,
    }
    impl WindowSelectionBackend for Native {
        fn bind(&self, _: WindowTarget) -> Result<Arc<dyn WindowIdentity>, String> {
            Ok(Arc::new(Live))
        }
    }
    impl WorkspaceBackend for Native {
        fn create(&self) -> Result<u64, String> {
            Ok(100)
        }
        fn state(&self, space: u64) -> Result<(bool, bool), String> {
            if self.state_fails.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("workspace_state_unavailable: display query failed".into());
            }
            Ok((
                space == 100 && !self.deleted.load(std::sync::atomic::Ordering::SeqCst),
                false,
            ))
        }
        fn membership(&self, target: WindowTarget) -> Result<Vec<u64>, String> {
            Ok(self
                .membership
                .lock()
                .unwrap()
                .get(&target)
                .cloned()
                .unwrap_or(vec![1]))
        }
        fn move_window(&self, target: WindowTarget, space: u64) -> Result<(), String> {
            self.membership.lock().unwrap().insert(target, vec![space]);
            Ok(())
        }
        fn reveal(&self, _: u64) -> Result<(), String> {
            Err("workspace_operation_unsupported: native reveal refused".into())
        }
    }

    fn context(
        registry: &SessionAuthorizationRegistry,
        session: &str,
        windows: &[WindowTarget],
    ) -> Arc<EffectiveAuthorizationContext> {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), serde_json::to_vec(&json!({
            "version":3,"resources":{"desktop":{"selected_windows_only":true,"windows":windows,"workspace_space_id":100}},
            "allow":{"tools":["create_workspace","get_workspace_state","move_window_to_workspace","reveal_workspace","restore_workspace_windows","release_workspace","delete_workspace","end_session","list_windows"]}
        })).unwrap()).unwrap();
        let manifest = Arc::new(crate::session_manifest::load_manifest(file.path()).unwrap());
        let (host, connection) = registry.trusted_in_process_binding();
        registry
            .bind_delegated_session(
                &host,
                &connection,
                DelegatedSessionRequest {
                    public_session: session.into(),
                    transport_session: format!("transport-{session}"),
                    mode: PermissionMode::Standard,
                    ttl: Duration::from_secs(60),
                    idle_ttl: Duration::from_secs(60),
                    capability_manifest: Some(manifest),
                },
            )
            .unwrap();
        registry
            .resolve_delegated(&connection, session, &format!("transport-{session}"))
            .unwrap()
    }

    #[tokio::test]
    async fn registry_enforces_selection_and_preserves_user_placement_on_release() {
        let a = WindowTarget {
            pid: 10,
            window_id: 1,
        };
        let b = WindowTarget {
            pid: 10,
            window_id: 2,
        };
        let third = WindowTarget {
            pid: 10,
            window_id: 3,
        };
        let native = Arc::new(Native::default());
        let auth = SessionAuthorizationRegistry::with_ceiling(
            SessionModeCeiling::for_trusted_sessions(
                [PermissionMode::Standard],
                false,
                Duration::from_secs(60),
                Duration::from_secs(60),
            )
            .unwrap(),
        );
        let selected = context(&auth, "selected", &[a, b]);
        let other = context(&auth, "other", &[third]);
        let mut tools = ToolRegistry::new();
        tools.set_window_selection_backend(native.clone());
        tools.set_workspace_backend(native.clone());
        tools.register_session_tools();
        tools.bind_selected_windows(&selected).unwrap();
        tools.bind_selected_windows(&other).unwrap();
        let call = |name: &'static str, args: Value| {
            tools.invoke_with_context(name, args, selected.clone())
        };
        assert_ne!(
            call("create_workspace", json!({"session":"selected"}))
                .await
                .is_error,
            Some(true)
        );
        for target in [a, b] {
            assert_ne!(
                call(
                    "move_window_to_workspace",
                    json!({"session":"selected","pid":target.pid,"window_id":target.window_id})
                )
                .await
                .is_error,
                Some(true)
            );
        }
        assert_eq!(call("move_window_to_workspace",json!({"session":"selected","pid":third.pid,"window_id":third.window_id,"_selected_windows":[third]})).await.is_error,Some(true));
        assert_eq!(
            tools
                .invoke_with_context(
                    "create_workspace",
                    json!({"session":"other"}),
                    other.clone()
                )
                .await
                .is_error,
            Some(true)
        );
        let state = tools
            .invoke_with_context("get_workspace_state", json!({"session":"other"}), other)
            .await;
        assert_eq!(state.structured_content.unwrap()["owned"], false);
        assert_eq!(
            call("delete_workspace", json!({"session":"selected"}))
                .await
                .is_error,
            Some(true)
        );
        assert_eq!(
            call("reveal_workspace", json!({"session":"selected"}))
                .await
                .is_error,
            Some(true)
        );
        native.membership.lock().unwrap().insert(a, vec![9]); // Simulated user move.
        assert_eq!(
            call("restore_workspace_windows", json!({"session":"selected"}))
                .await
                .is_error,
            Some(true)
        );
        assert_eq!(native.membership(a).unwrap(), vec![9]);
        assert_eq!(native.membership(b).unwrap(), vec![1]); // Independent restoration completed.
        native
            .deleted
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let deleted = call("get_workspace_state", json!({"session":"selected"})).await;
        assert_eq!(deleted.structured_content.unwrap()["space_exists"], false);
        native
            .deleted
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let released = call("release_workspace", json!({"session":"selected"})).await;
        assert_eq!(released.structured_content.unwrap()["owned"], false);
        assert_eq!(native.membership(a).unwrap(), vec![9]);
        assert_ne!(
            call("create_workspace", json!({"session":"selected"}))
                .await
                .is_error,
            Some(true)
        );
        native
            .state_fails
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            call("release_workspace", json!({"session":"selected"}))
                .await
                .is_error,
            Some(true)
        );
        native
            .state_fails
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            call("get_workspace_state", json!({"session":"selected"}))
                .await
                .structured_content
                .unwrap()["owned"],
            false
        );
        assert_ne!(
            call("end_session", json!({"session":"selected"}))
                .await
                .is_error,
            Some(true)
        );
        assert_eq!(
            call(
                "move_window_to_workspace",
                json!({"session":"selected","pid":a.pid,"window_id":a.window_id})
            )
            .await
            .is_error,
            Some(true)
        );
    }
}
