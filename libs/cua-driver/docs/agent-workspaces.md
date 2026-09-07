# Agent workspaces and selected windows

This branch adds driver tools and trusted selected-window sessions. It uses the existing Rust SDK, v3 capability manifest, native background input, ScreenCaptureKit, trajectory recorder, and experimental PiP renderer. There is no new application or permission picker.

**Automatic creation and movement work on the tested SIP-enabled macOS 26.5 host.** Creation briefly opens Mission Control and presses its existing Add Desktop button. Movement uses the macOS 26 SkyLight bridged operation and verifies actual membership without switching desktops. The trusted host must opt into visible setup. Older or incompatible macOS versions return explicit unsupported results. See [test evidence](agent-workspaces-evidence.md) for scope and outstanding gates.

The changes live in the requesting user's fork. No upstream PR is part of delivery. Native movement code was adapted from [trycua/cua#2429](https://github.com/trycua/cua/pull/2429), with credit to Francesco Bonacci and injaneity preserved in the implementation commit.

For the agent-driven “create a desktop, open Helium and notes, use them” flow,
see the [workspace apps MCP example](../examples/workspace-apps/README.md).
`launch_workspace_app` accepts a trusted alias and admits only the new native
window created by that recipe. The following static-window setup is still
available for hosts that already have approved windows.

For the September 7 session audit and subsequent Helium/TextEdit checks, see
[workspace session audit](workspace-session-audit.md). The workspace-apps example
now supports exact-window typed browser navigation and an optional Ghostty recipe;
Ghostty's inactive startup remains limited on the tested installed version.

## Trusted selection

A trusted host writes a capability manifest and supplies its path through `TrustedSessionOptions.capability_manifest_path`. Window IDs come from the host's existing trusted discovery/approval mechanism. The agent cannot set or extend the selection through tool arguments.

```yaml
version: 3
resources:
  desktop:
    selected_windows_only: true
    workspace_only: true
    workspace_allow_mission_control: true
    # Optional numeric CGDirectDisplayID; defaults to the main display:
    # workspace_display_id: 1
    windows:
      - {pid: 12345, window_id: 67890}
      - {pid: 12345, window_id: 67891}
    # To attach an existing desktop instead, omit the two creation options above:
    # workspace_space_id: 100
  files:
    write:
      - {dir: /absolute/path/to/session-recording, recursive: true}
allow:
  tools:
    - list_windows
    - list_apps
    - get_window_state
    - click
    - set_value
    - type_text
    - press_key
    - create_workspace
    - get_workspace_state
    - move_window_to_workspace
    - reveal_workspace
    - restore_workspace_windows
    - release_workspace
    - delete_workspace
    - start_recording
    - get_recording_state
    - stop_recording
    - end_session
```

Use real, currently live IDs. `selected_windows_only` and `workspace_space_id` require v3. An empty selection permits no windows. A Space ID alone does not grant window access. With `workspace_only: true`, observation and input require both an approved lifetime identity and current membership in the owned desktop. Before creation, after release, after Space deletion, or while the user moves a window elsewhere, access is denied. Exact approved windows can still be moved or restored through workspace tools. New arrivals receive no automatic approval. Existing tool and protected-resource authorization still apply; selection further narrows them.

On macOS, binding requires Accessibility permission, an exact top-level AX window, its retained remote AX object, WindowServer ownership, and the process start time. No title/geometry match authorizes a window. The driver retains the exact AX object across movement and carries session authority into native workers. Binding an already inactive window can use serialized, bounded private AX token recovery; unresolved targets remain refused. Failure to re-prove this witness permanently invalidates that selection entry. Process or window ID reuse does not rebind an invalidated entry. Applications with unreliable AX lifetime identity are refused; this is not a sandbox against a malicious application falsifying its own accessibility data.

The trusted selection policy is immutable. Explicitly configured workspace launch recipes can admit their newly created exact windows; arbitrary arrivals cannot. For replacement, close the old trusted session and create a new one with a fresh manifest and native lifetime witnesses. Editing the loaded file or restarting an agent-callable session does not widen its authority. Handle close revokes the connection immediately; existing lifecycle hooks clear observations and ownership after any admitted work drains. Expired authority refuses dispatch/capture immediately, and the existing runtime maintenance sweep reclaims expired session resources (up to 30 seconds).

## SDK and tools

[`examples/selected_windows.rs`](../rust/crates/cua-driver-sdk/examples/selected_windows.rs) is an executable example using actual Cua SDK interfaces. It creates two trusted sessions, exercises exact-window reads and input, and tests recording/preview isolation against the repository AppKit fixture.

```rust,ignore
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
    public_session: "agent-work".into(),
    mode: SessionPermissionMode::Standard,
    ttl_seconds: 600,
    idle_ttl_seconds: 600,
    capability_manifest_path: Some("/absolute/path/selection.yaml".into()),
    bounded_manifest_path: None,
})?;
let created = session.create_workspace(CreateWorkspaceInput { session: None }).await?;
// Inspect created.is_error and created.error_code before moving anything.
let state = session.get_workspace_state(GetWorkspaceStateInput { session: None }).await?;
let moved = session.move_window_to_workspace(MoveWindowToWorkspaceInput {
    session: None, pid: 12345, window_id: 67890,
}).await?;
// Reveal only in response to an explicit request to switch Spaces:
let revealed = session.reveal_workspace(RevealWorkspaceInput { session: None }).await?;
let released = session.release_workspace(ReleaseWorkspaceInput { session: None }).await?;
session.close();
```

Import SDK options from `cua_driver_sdk` and workspace inputs from `cua_driver_contract`. Workspace methods are also generated for the existing Python and TypeScript bindings. The ordinary tool surface uses the same names, for example:

```json
{"name":"move_window_to_workspace","arguments":{"pid":12345,"window_id":67890}}
```

Exact input calls include both `pid` and `window_id`, even when using an `element_token` returned by `get_window_state`. Process-only targeting is unavailable in selected sessions. Discovery output, AX caches, element tokens, image coordinate transforms, recording observations and preview routing are session-scoped. An unapproved sibling in the same application remains inaccessible.

New windows receive no inherited grant. Sheets and application menu bars are excluded from selected AX trees. Child windows are excluded from ScreenCaptureKit window capture. Separately addressable top-level dialogs require a new trusted selection; unresolved sheets and child surfaces are unsupported. Parent-rendered pixels remain part of the approved parent surface. Minimized or hidden windows have no selected screenshot fallback.

Browser bind operations require an approved exact native window. Subsequent operations require implementation-attested native ownership of the session's browser target and tab. Existing browser permission checks remain in place. Profile preparation, legacy `page`, process-wide browser attachment, and unbound browser scripting fallbacks are refused. Helium binding, screenshots, and new-tab navigation have supporting native evidence in the [session audit](workspace-session-audit.md); the canonical installed-browser gate remains outstanding.

Desktop capture/input, foreground delivery, app launching/termination, clipboard access and tools without a proven selection boundary are unavailable. Existing ambiguous keyboard and unresolved AX refusals remain refusals. No workspace operation silently activates a window, switches a Space, posts global input or moves the physical cursor as a fallback. The shared-host cursor measurement was inconclusive; background support is not universally certified.

## Workspace lifecycle

`create_workspace` creates and owns one ordinary Mission Control desktop, or attaches the exact trusted `workspace_space_id`. Repeated creation returns the owned state. The runtime prevents its sessions from owning the same desktop. Ownership is local to a driver runtime, not a macOS-wide reservation against other driver processes.

`workspace_allow_mission_control: true` is trusted v3 configuration. It permits visible setup, not foreground input. The agent cannot enable it through tool arguments. The driver uses public Accessibility AXPress against private Dock identifiers and a private CoreDock notification. It refuses setup if Mission Control is already open, display mapping is unavailable, or the native result is ambiguous. Creation is not silent: pause ordinary user interaction during this brief setup.

Window movement uses private `SLSBridgedMoveWindowsToManagedSpaceOperation` on compatible macOS 26 systems. The submit function is a local C++ SkyLight symbol, resolved read-only in the driver's own loaded image. No Dock injection, scripting addition, SIP change, TCC database modification, or system-setting change is involved. Legacy private movement is attempted only when the newer route is absent, never after an asynchronous submission. Every route requires fresh exact membership verification. A two-second timeout reports an incomplete operation; query state before retrying.

State reports created versus attached ownership, existence, active status, original placement and current membership. Movement records the original Space before native mutation so partial failures remain inspectable. Fullscreen and sticky/multiple-membership windows refuse automatic movement. Removed displays and deleted Spaces invalidate state or access; the driver does not recreate them silently.

Only `reveal_workspace` selects the workspace, through a verified Mission Control AX action. Background capture, input, movement and restoration never invoke it. `restore_workspace_windows` restores recorded membership explicitly and refuses to overwrite a later user placement. It restores Space membership, not an earlier cross-display frame geometry. Cross-display capture is not certified; keep the workspace on the target windows' display.

`release_workspace` and session teardown release ownership without deleting a desktop or closing an app. Window approval remains independent; workspace-only sessions lose observation/input access when ownership ends. `delete_workspace` explicitly removes only an empty, inactive ordinary desktop created by this session. It refuses attached desktops, the last desktop on a display, and any desktop containing windows. Deletion briefly shows Mission Control and verifies absence afterward. Concurrent external window movement or Dock changes cannot be made transactional through these private APIs; ambiguous outcomes are reported rather than destructively rolled back.

Native source attribution and MIT notices for yabai, Hammerspoon and Paneru are in [THIRD_PARTY_NOTICES.md](../rust/crates/platform-macos/THIRD_PARTY_NOTICES.md), in addition to the preserved Cua #2429 contributor credit.

Windows and Linux retain the common contract and explicitly refuse native workspace operations and selected-window lifetime binding. Native adapters for them are not implemented here.

## Existing recording and preview

`start_recording` with `record_video: false` records approved exact-window frames and AX state. Selected sessions cannot request the existing display-video path; the physical cursor sampler is also disabled for selected recording. Recorder ownership is checked at control and write boundaries, including finalized-state access. There is still one active trajectory recorder per runtime; a second selected session must wait for its owner to stop it.

The trusted **in-process Rust** method `session.attach_experimental_preview(backend)` takes ownership of a backend returned by the existing `pip_preview::start_pip`. The host uses the existing platform factory and AppKit event loop. The renderer now owns its native handles per instance; queued frames are discarded after shutdown. This is a host operation, not an agent tool or a new UI. Replacement/end closes the old backend. Remote/private-worker preview attachment is explicitly unavailable. The global CLI PiP callback is not used for selected-session frames.

```rust,ignore
pip_preview::set_pip_backend_factory(Box::new(
    platform_macos::pip::MacosPipBackendFactory,
));
let backend = pip_preview::start_pip(&existing_experimental_pip_config)?;
session.attach_experimental_preview(backend)?;
```

These are exact-window captures, never a capture of an inactive Space. Inactive-window support depends on live exact AX resolution and ScreenCaptureKit availability. Inactive-desktop exact-window screenshots and AX text edits passed the AppKit fixture. Screenshots changed after text writes, and recording contained only approved windows. Visible PiP rendering and child/dialog behavior still require their native matrix; the test backend verifies frame routing and shutdown. Minimized capture is refused. This is not universal background keyboard support: same-process keyboard ambiguity remains refused.

## Development setup

Build in an isolated worktree and keep its `rust/target` and `rust/test-apps` separate from the daily installation. Follow the repository [harness guide](test-harnesses-guide.md) and existing signing/TCC setup. Use your own stable development signing identity. No installer or permission-setting command is needed by these tests when the development process already has the required access.

From `libs/cua-driver/rust`:

```sh
cargo check --locked -p platform-macos -p cua-driver-sdk
cargo test --locked -p cua-driver-core -p cua-driver-contract --lib
cargo test --locked -p cua-driver-sdk --lib
../tests/fixtures/build/macos.sh --only appkit
cargo build --locked -p platform-macos --example workspace_probe
cargo build --locked -p cua-driver-sdk --example selected_windows
```

Sign the isolated fixture bundle and example executables using the existing development identity before native execution. Launch the fixture directly with `CUA_HARNESS_WORKSPACE_REPORT=/private/tmp/cua-workspace-fixture.json`, `CUA_HARNESS_WORKSPACE_SECONDS=900`, and optionally `CUA_HARNESS_WORKSPACE_SCREEN=last`. This fixture mode creates three windows without activation and terminates only its own process on timeout.

Run `target/debug/examples/selected_windows /private/tmp/cua-workspace-fixture.json /private/tmp/cua-workspace-evidence`. Its oracle-enabled run minimizes the first fixture window and closes the second; use fresh fixtures for each run. `workspace_probe --mission-control-create --display <CGDirectDisplayID>` tests visible creation. `workspace_probe --fixture-report <report> --move-to <ordinary-space-id>` moves only the verified repository fixture. `--create` retains the older non-visible private-call diagnostic. It prints sanitized Space metadata without application titles.

For the full SDK workspace diagnostic, use a fresh fixture and an empty evidence directory:

```sh
CUA_WORKSPACE_DISPLAY_ID=1 CUA_WORKSPACE_REVEAL=1 CUA_WORKSPACE_CLEANUP=1 \
  target/debug/examples/selected_windows /private/tmp/cua-workspace-fixture.json /private/tmp/cua-workspace-evidence
```

Replace display 1 with the fixture's actual display. This opt-in run briefly shows Mission Control, explicitly reveals the created desktop and returns to the saved original desktop, restores surviving windows and explicitly deletes its empty created desktop. Omit `CUA_WORKSPACE_REVEAL` for background-only tests after setup; omit `CUA_WORKSPACE_CLEANUP` to retain the desktop. No daily installation is replaced.

Regenerate interfaces with `cargo run --locked -p cua-driver-contract --bin cua-contract-gen -- all` and `node ../scripts/generate-uniffi-bindings.mjs`. Do not edit generated bindings manually. The canonical macOS gate remains `libs/cua-driver/tests/runners/macos-lume/run-all.sh --standalone-browser`; local fixture tests do not replace it. This task's host-only environment has no Lume installation, so that gate is outstanding.
