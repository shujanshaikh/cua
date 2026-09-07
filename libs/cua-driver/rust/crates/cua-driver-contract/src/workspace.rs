//! Session workspace contracts. Access approval is trusted configuration and
//! is deliberately absent from these agent-callable inputs.
use crate::{
    CursorAction, CursorSemantics, Platform, SchemaMode, ToolAnnotations, ToolContract, ToolInput,
    ToolOutput,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! session_input {
    ($name:ident, $tool:literal) => {
        #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, uniffi::Record)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub session: Option<String>,
        }
        impl ToolInput for $name {
            const TOOL_NAME: &'static str = $tool;
        }
    };
}
session_input!(CreateWorkspaceInput, "create_workspace");
session_input!(GetWorkspaceStateInput, "get_workspace_state");
session_input!(RevealWorkspaceInput, "reveal_workspace");
session_input!(ReleaseWorkspaceInput, "release_workspace");
session_input!(RestoreWorkspaceWindowsInput, "restore_workspace_windows");
session_input!(DeleteWorkspaceInput, "delete_workspace");

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, uniffi::Record)]
#[serde(deny_unknown_fields)]
pub struct MoveWindowToWorkspaceInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub pid: i64,
    pub window_id: u64,
}
impl ToolInput for MoveWindowToWorkspaceInput {
    const TOOL_NAME: &'static str = "move_window_to_workspace";
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, uniffi::Record)]
pub struct WorkspaceWindowState {
    pub pid: i64,
    pub window_id: u64,
    pub original_space_ids: Vec<u64>,
    pub current_space_ids: Vec<u64>,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, uniffi::Record)]
pub struct WorkspaceStateOutput {
    pub owned: bool,
    pub space_id: Option<u64>,
    pub space_created: bool,
    pub space_exists: bool,
    pub active: bool,
    pub private_api: bool,
    pub windows: Vec<WorkspaceWindowState>,
}
impl ToolOutput for WorkspaceStateOutput {}

fn contract<I: ToolInput>(description: &str, read_only: bool, destructive: bool) -> ToolContract {
    ToolContract {
        name: I::TOOL_NAME.into(),
        description: description.into(),
        platforms: vec![Platform::Macos, Platform::Windows, Platform::Linux],
        aliases: vec![],
        capabilities: vec!["session.workspace".into()],
        annotations: ToolAnnotations {
            read_only,
            destructive,
            idempotent: I::TOOL_NAME != "create_workspace",
            open_world: false,
        },
        schema_mode: SchemaMode::CanonicalRuntime,
        cursor_semantics: Some(CursorSemantics::new(CursorAction::System)),
        input_schema: I::input_schema(),
        success_output_schema: Some(WorkspaceStateOutput::output_schema()),
        output_validator: crate::validate_typed_output::<WorkspaceStateOutput>,
    }
}

pub fn contracts() -> Vec<ToolContract> {
    vec![
        contract::<CreateWorkspaceInput>("Create a session-owned agent Space without selecting it. Trusted configuration may explicitly permit visible Mission Control setup on macOS. Requires a trusted selected-window session. When trusted configuration supplies an existing Space, attach to it and report space_created=false. Private native operations must verify their postconditions; unsupported platforms refuse.", false, false),
        contract::<GetWorkspaceStateInput>("Read the current session's workspace ownership and approved window membership. Reports user movement, stale windows and deleted Spaces without capturing a display.", true, false),
        contract::<MoveWindowToWorkspaceInput>("Move an already-approved exact window into this session's workspace and verify membership. Moving never grants access and never switches Spaces.", false, false),
        contract::<RevealWorkspaceInput>("Explicitly switch to this session's workspace. This is the only workspace operation that may switch the user's active Space.", false, false),
        contract::<ReleaseWorkspaceInput>("Release this session's workspace ownership. Preserve applications, windows and Spaces. Releasing ownership preserves window approval; workspace-only sessions lose access until a workspace is owned again.", false, false),
        contract::<RestoreWorkspaceWindowsInput>("Explicitly restore windows moved by this workspace to their recorded original Spaces. Refuse to override subsequent user movement; report partial failures. Never switch Spaces.", false, false),
        contract::<DeleteWorkspaceInput>("Explicitly delete an empty inactive Space created by this session. macOS deletion briefly shows Mission Control. Never delete a pre-existing Space or close applications. Unsupported native deletion refuses.", false, true),
    ]
}
