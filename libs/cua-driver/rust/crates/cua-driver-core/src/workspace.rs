//! Session-owned placement, independent from selected-window authorization.
//! Space metadata is not a security boundary and is never a capture source.

use crate::{
    protocol::ToolResult,
    selected_windows::{SelectedWindows, WindowIdentity, WindowTarget},
    tool::{Tool, ToolDef, ToolRegistry},
};
use async_trait::async_trait;
use cua_driver_contract::{WorkspaceStateOutput, WorkspaceWindowState};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkspaceCreationOptions {
    /// Trusted host opt-in to briefly showing Mission Control during setup.
    pub allow_mission_control: bool,
    pub display_id: Option<u32>,
}

/// Launch recipes are loaded by the trusted host, never accepted as tool arguments.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceApplication {
    pub bundle_id: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub urls: Vec<String>,
}

impl WorkspaceApplication {
    /// Only the trusted recipe can request substitution; the scope is driver-minted.
    fn for_workspace(&self, scope: &str) -> Self {
        Self {
            bundle_id: self.bundle_id.clone(),
            arguments: self
                .arguments
                .iter()
                .map(|arg| arg.replace("{workspace}", scope))
                .collect(),
            urls: self.urls.clone(),
        }
    }
}

pub struct LaunchedWorkspaceWindow {
    pub target: WindowTarget,
    pub identity: Arc<dyn WindowIdentity>,
}

pub trait WorkspaceBackend: Send + Sync {
    fn launch(&self, _recipe: &WorkspaceApplication) -> Result<LaunchedWorkspaceWindow, String> {
        Err("workspace_operation_unsupported: isolated workspace app launch is unavailable".into())
    }
    fn create(&self) -> Result<u64, String>;
    fn create_with_options(&self, _options: WorkspaceCreationOptions) -> Result<u64, String> {
        self.create()
    }
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
    profile_scope: String,
    // Preserve the first origin even after retries and partial native failures.
    moved: HashMap<WindowTarget, Vec<u64>>,
    launched: HashMap<String, WindowTarget>,
    launch_failures: HashMap<String, String>,
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
        let available_apps = crate::tool::current_dispatch_authorization_context()
            .and_then(|context| {
                context
                    .capability_manifest()
                    .map(|m| m.workspace_application_aliases())
            })
            .unwrap_or_default();
        let Some(workspace) = workspace else {
            return Ok(WorkspaceStateOutput {
                available_apps,
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
                    .native_identity(target)
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
            .collect::<Vec<_>>();
        let mut windows = windows;
        windows.sort_by_key(|window| (window.pid, window.window_id));
        Ok(WorkspaceStateOutput {
            available_apps,
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
                        None => self.backend.create_with_options(
                            context
                                .capability_manifest()
                                .map(|m| m.workspace_creation_options())
                                .unwrap_or_default(),
                        )?,
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
                    selection.set_workspace(Some(space));
                    crate::cursor_events::set_workspace(session, Some(space));
                    crate::session::retain_workspace(session, true);
                    owned.insert(
                        session.into(),
                        Workspace {
                            space,
                            created: attached.is_none(),
                            profile_scope: uuid::Uuid::new_v4().to_string(),
                            moved: HashMap::new(),
                            launched: HashMap::new(),
                            launch_failures: HashMap::new(),
                        },
                    );
                }
            }
            "get_workspace_state" => {}
            "release_workspace" => {
                crate::session::retain_workspace(session, false);
                selection.set_workspace(None);
                crate::cursor_events::set_workspace(session, None);
                crate::pip_hook::clear_session(session);
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
                    "launch_workspace_app" => {
                        let alias = args
                            .get("app")
                            .and_then(Value::as_str)
                            .ok_or("workspace_app_required")?;
                        let recipe = context
                            .capability_manifest()
                            .ok_or("workspace_requires_manifest")?
                            .workspace_application(alias)?;
                        if !self.backend.state(workspace.space)?.0 {
                            return Err(
                                "workspace_space_deleted: cannot launch into a missing workspace"
                                    .into(),
                            );
                        }
                        if let Some(error) = workspace.launch_failures.get(alias) {
                            return Err(error.clone());
                        }
                        if let Some(target) = workspace.launched.get(alias) {
                            selection.validate(*target)?;
                            // Do not override a later user placement on an idempotent call.
                        } else {
                            let launched = match self
                                .backend
                                .launch(&recipe.for_workspace(&workspace.profile_scope))
                            {
                                Ok(launched) => launched,
                                Err(error) => {
                                    // A native error may still have created a process. Retain
                                    // the outcome rather than repeatedly spawning on retries.
                                    workspace
                                        .launch_failures
                                        .insert(alias.into(), error.clone());
                                    return Err(error);
                                }
                            };
                            if context.is_revoked() || context.is_expired() {
                                return Err("authorization_revoked: launch completed after session ended; no window access granted".into());
                            }
                            let target = launched.target;
                            if let Err(error) =
                                selection.admit_launched_window(target, launched.identity)
                            {
                                workspace
                                    .launch_failures
                                    .insert(alias.into(), error.clone());
                                return Err(error);
                            }
                            workspace.launched.insert(alias.into(), target);
                            workspace.moved.insert(target, vec![]);
                            let origin = self.backend.membership(target)?;
                            if origin.len() != 1 {
                                return Err("workspace_launch_incomplete: ambiguous initial membership; window retained for explicit recovery".into());
                            }
                            workspace.moved.insert(target, origin);
                            self.backend.move_window(target, workspace.space)?;
                            if self.backend.membership(target)? != [workspace.space] {
                                return Err("workspace_move_incomplete: launched window destination did not verify; query get_workspace_state and recover with move_window_to_workspace".into());
                            }
                            selection.validate(target)?;
                        }
                    }
                    "move_window_to_workspace" => {
                        let target = SelectedWindows::target(args)?;
                        selection.native_identity(target)?;
                        let origin = self.backend.membership(target)?;
                        if origin.len() != 1 {
                            return Err(
                                "workspace_operation_unsupported: window membership is ambiguous"
                                    .into(),
                            );
                        }
                        workspace.moved.entry(target).or_insert(origin);
                        selection.native_identity(target)?;
                        self.backend.move_window(target, workspace.space)?;
                        selection.native_identity(target)?;
                        if self.backend.membership(target)? != [workspace.space] {
                            return Err(
                                "workspace_move_incomplete: destination membership did not verify"
                                    .into(),
                            );
                        }
                    }
                    "reveal_workspace" => self.backend.reveal(workspace.space)?,
                    "restore_workspace_windows" => {
                        let mut failures: Vec<String> = Vec::new();
                        for (&target, origin) in &workspace.moved {
                            let outcome = (|| {
                                selection.native_identity(target)?;
                                if origin.len() != 1 {
                                    return Err("workspace_restore_incomplete: original membership is unavailable".into());
                                }
                                let current = self.backend.membership(target)?;
                                if &current == origin {
                                    return Ok(());
                                }
                                if current != [workspace.space] {
                                    return Err("workspace_user_moved: refusing to override a later placement".into());
                                }
                                self.backend.move_window(target, origin[0])?;
                                selection.native_identity(target)?;
                                if self.backend.membership(target)? != *origin {
                                    return Err("workspace_restore_incomplete: original membership did not verify".into());
                                }
                                Ok(())
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
                        crate::session::retain_workspace(session, false);
                        selection.set_workspace(None);
                        crate::cursor_events::set_workspace(session, None);
                        crate::pip_hook::clear_session(session);
                        owned.remove(session);
                        return Ok(snapshot);
                    }
                    _ => return Err(unsupported()),
                }
            }
        }
        crate::session::retain_workspace(session, owned.contains_key(session));
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
            "launch_workspace_app" => check!(LaunchWorkspaceAppInput),
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
        let workspaces = self.workspaces.clone();
        let name = self.def.name.clone();
        let outcome = crate::tool::spawn_blocking_with_authorization(move || {
            let result = workspaces.execute(&name, &args);
            let recovery = if result.is_err() {
                workspaces.execute("get_workspace_state", &args).ok()
            } else {
                None
            };
            let launch_target = args.get("app").and_then(Value::as_str).and_then(|alias| {
                let session = args
                    .get("session")
                    .or_else(|| args.get("_session_id"))?
                    .as_str()?;
                let owned = workspaces
                    .owned
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let target = owned.get(session)?.launched.get(alias)?;
                Some(json!({"app":alias,"pid":target.pid,"window_id":target.window_id}))
            });
            (result, recovery, launch_target)
        })
        .await;
        let (outcome, recovery, launched_app) = outcome
            .unwrap_or_else(|error| (Err(format!("workspace_worker_failed: {error}")), None, None));
        match outcome {
            Ok(state) => {
                let mut structured =
                    serde_json::to_value(state).expect("workspace state serializes");
                if let Some(app) = launched_app {
                    structured["launched_app"] = app;
                }
                ToolResult::text("Workspace state verified.").with_structured(structured)
            }
            Err(message) => {
                let mut structured = json!({"code":message.split(':').next().unwrap_or("workspace_failed"), "message":message});
                if let Some(state) = recovery {
                    structured["workspace_state"] =
                        serde_json::to_value(state).expect("workspace state serializes");
                }
                if let Some(app) = launched_app {
                    structured["launched_app"] = app;
                }
                ToolResult::error(message).with_structured(structured)
            }
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
    #[test]
    fn profiles_are_scoped_to_the_owned_workspace() {
        let recipe = WorkspaceApplication {
            bundle_id: "browser".into(),
            arguments: vec!["--user-data-dir=/profiles/helium-{workspace}".into()],
            urls: vec![],
        };
        assert_eq!(
            recipe.for_workspace("a").arguments,
            recipe.for_workspace("a").arguments
        );
        assert_ne!(
            recipe.for_workspace("a").arguments,
            recipe.for_workspace("b").arguments
        );
        assert_eq!(
            recipe.for_workspace("a").arguments[0],
            "--user-data-dir=/profiles/helium-a"
        );
    }

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
        launch_count: std::sync::atomic::AtomicUsize,
        launch_fails: std::sync::atomic::AtomicBool,
        move_noop: std::sync::atomic::AtomicBool,
    }
    impl WindowSelectionBackend for Native {
        fn bind(&self, _: WindowTarget) -> Result<Arc<dyn WindowIdentity>, String> {
            Ok(Arc::new(Live))
        }
    }
    impl WorkspaceBackend for Native {
        fn launch(&self, _: &WorkspaceApplication) -> Result<LaunchedWorkspaceWindow, String> {
            self.launch_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.launch_fails.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(
                    "workspace_launch_ambiguous: native process created without a unique window"
                        .into(),
                );
            }
            Ok(LaunchedWorkspaceWindow {
                target: WindowTarget {
                    pid: 50,
                    window_id: 51,
                },
                identity: Arc::new(Live),
            })
        }
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
            if self.move_noop.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok(());
            }
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
            "version":3,"resources":{"apps":[{"bundle_id":"com.test.app","launch":true}],"desktop":{"workspace_applications":{"notes":{"bundle_id":"com.test.app"}},"selected_windows_only":true,"workspace_only":true,"windows":windows,"workspace_space_id":100}},
            "allow":{"tools":["launch_workspace_app","create_workspace","get_workspace_state","move_window_to_workspace","reveal_workspace","restore_workspace_windows","release_workspace","delete_workspace","end_session","list_windows"]}
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
    async fn workspace_launch_uses_trusted_alias_and_admits_only_attested_window() {
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
        let selected = context(&auth, "launch", &[]);
        let other = context(&auth, "other", &[]);
        let mut tools = ToolRegistry::new();
        tools.set_window_selection_backend(native.clone());
        tools.set_workspace_backend(native.clone());
        tools.register_session_tools();
        tools.bind_selected_windows(&selected).unwrap();
        tools.bind_selected_windows(&other).unwrap();
        let call = |name: &'static str, args: Value| {
            tools.invoke_with_context(name, args, selected.clone())
        };
        assert_eq!(
            call(
                "launch_workspace_app",
                json!({"session":"launch","app":"notes"})
            )
            .await
            .is_error,
            Some(true)
        );
        assert_ne!(
            call("create_workspace", json!({"session":"launch"}))
                .await
                .is_error,
            Some(true)
        );
        assert_eq!(
            call(
                "launch_workspace_app",
                json!({"session":"launch","app":"unapproved"})
            )
            .await
            .is_error,
            Some(true)
        );
        assert_eq!(
            call(
                "launch_workspace_app",
                json!({"session":"launch","app":"notes","pid":999})
            )
            .await
            .is_error,
            Some(true)
        );
        assert_eq!(
            native
                .launch_count
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        for _ in 0..2 {
            let result = call(
                "launch_workspace_app",
                json!({"session":"launch","app":"notes"}),
            )
            .await;
            assert_ne!(result.is_error, Some(true), "{result:?}");
            assert_eq!(
                result.structured_content.as_ref().unwrap()["launched_app"],
                json!({"app":"notes","pid":50,"window_id":51})
            );
        }
        assert_eq!(
            native
                .launch_count
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        let target = WindowTarget {
            pid: 50,
            window_id: 51,
        };
        let selection = selected.selected_windows().unwrap().unwrap();
        assert!(selection.validate(target).is_ok());
        assert!(selection.is_live_launched_window(target));
        assert!(!other
            .selected_windows()
            .unwrap()
            .unwrap()
            .is_live_launched_window(target));
        assert!(selection
            .validate(WindowTarget {
                pid: 50,
                window_id: 52
            })
            .is_err());
        assert!(other
            .selected_windows()
            .unwrap()
            .unwrap()
            .validate(target)
            .is_err());
        native.membership.lock().unwrap().insert(target, vec![1]);
        assert_eq!(
            call(
                "launch_workspace_app",
                json!({"session":"launch","app":"notes"})
            )
            .await
            .is_error,
            Some(true)
        );
        assert_eq!(native.membership(target).unwrap(), vec![1]);
        assert!(!selection.is_live_launched_window(target));
        assert_eq!(
            native
                .launch_count
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn incomplete_launch_exposes_exact_recovery_without_spawning_again() {
        let native = Arc::new(Native::default());
        native
            .move_noop
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let auth = SessionAuthorizationRegistry::with_ceiling(
            SessionModeCeiling::for_trusted_sessions(
                [PermissionMode::Standard],
                false,
                Duration::from_secs(60),
                Duration::from_secs(60),
            )
            .unwrap(),
        );
        let selected = context(&auth, "recover", &[]);
        let mut tools = ToolRegistry::new();
        tools.set_window_selection_backend(native.clone());
        tools.set_workspace_backend(native.clone());
        tools.register_session_tools();
        tools.bind_selected_windows(&selected).unwrap();
        let call = |name: &'static str, args: Value| {
            tools.invoke_with_context(name, args, selected.clone())
        };
        assert_ne!(
            call("create_workspace", json!({"session":"recover"}))
                .await
                .is_error,
            Some(true)
        );
        let failure = call(
            "launch_workspace_app",
            json!({"session":"recover","app":"notes"}),
        )
        .await;
        assert_eq!(failure.is_error, Some(true));
        let state = failure.structured_content.unwrap();
        assert_eq!(state["code"], "workspace_move_incomplete");
        assert_eq!(
            state["launched_app"],
            json!({"app":"notes","pid":50,"window_id":51})
        );
        assert_eq!(
            state["workspace_state"]["windows"][0]["original_space_ids"],
            json!([1])
        );
        native
            .move_noop
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert_ne!(
            call(
                "move_window_to_workspace",
                json!({"session":"recover","pid":50,"window_id":51})
            )
            .await
            .is_error,
            Some(true)
        );
        assert_ne!(
            call(
                "launch_workspace_app",
                json!({"session":"recover","app":"notes"})
            )
            .await
            .is_error,
            Some(true)
        );
        assert_eq!(
            native
                .launch_count
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        native
            .move_noop
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            call("restore_workspace_windows", json!({"session":"recover"}))
                .await
                .is_error,
            Some(true)
        );
    }

    #[tokio::test]
    async fn partial_launch_failure_does_not_spawn_again_on_retry() {
        let native = Arc::new(Native::default());
        native
            .launch_fails
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let auth = SessionAuthorizationRegistry::with_ceiling(
            SessionModeCeiling::for_trusted_sessions(
                [PermissionMode::Standard],
                false,
                Duration::from_secs(60),
                Duration::from_secs(60),
            )
            .unwrap(),
        );
        let selected = context(&auth, "failed-launch", &[]);
        let mut tools = ToolRegistry::new();
        tools.set_window_selection_backend(native.clone());
        tools.set_workspace_backend(native.clone());
        tools.register_session_tools();
        tools.bind_selected_windows(&selected).unwrap();
        let call = |name: &'static str, args: Value| {
            tools.invoke_with_context(name, args, selected.clone())
        };
        assert_ne!(
            call("create_workspace", json!({"session":"failed-launch"}))
                .await
                .is_error,
            Some(true)
        );
        for _ in 0..2 {
            assert_eq!(
                call(
                    "launch_workspace_app",
                    json!({"session":"failed-launch","app":"notes"})
                )
                .await
                .is_error,
                Some(true)
            );
        }
        assert_eq!(
            native
                .launch_count
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert!(selected
            .selected_windows()
            .unwrap()
            .unwrap()
            .live_targets()
            .is_empty());
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
        assert!(selected
            .selected_windows()
            .unwrap()
            .unwrap()
            .validate(a)
            .is_err());
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
        assert!(selected
            .selected_windows()
            .unwrap()
            .unwrap()
            .validate(a)
            .is_err());
        selected
            .selected_windows()
            .unwrap()
            .unwrap()
            .native_identity(a)
            .unwrap();
        selected
            .selected_windows()
            .unwrap()
            .unwrap()
            .validate(b)
            .unwrap();
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
        assert!(selected
            .selected_windows()
            .unwrap()
            .unwrap()
            .validate(b)
            .is_err());
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
