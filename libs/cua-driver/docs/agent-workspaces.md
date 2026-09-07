# Agent workspaces and selected windows

This branch adds driver tools and trusted selected-window sessions. It uses the existing Rust SDK, v3 capability manifest, native background input, ScreenCaptureKit, trajectory recorder, and experimental PiP renderer. There is no new application or permission picker.

**Automatic macOS Space creation and movement did not work in the tested environment.** The implementation attempts private SkyLight operations and verifies membership; it returns `workspace_operation_unsupported` when they do not work. Attaching to a host-specified existing Space is a fallback, not evidence of automatic management. See [test evidence](agent-workspaces-evidence.md) for the tested conditions and outstanding gates.

The changes live in the requesting user's fork. No upstream PR is part of delivery. Native movement code was adapted from [trycua/cua#2429](https://github.com/trycua/cua/pull/2429), with credit to Francesco Bonacci and injaneity preserved in the implementation commit.

## Trusted selection

A trusted host writes a capability manifest and supplies its path through `TrustedSessionOptions.capability_manifest_path`. Window IDs come from the host's existing trusted discovery/approval mechanism. The agent cannot set or extend the selection through tool arguments.

```yaml
version: 3
resources:
  desktop:
    selected_windows_only: true
    windows:
      - {pid: 12345, window_id: 67890}
      - {pid: 12345, window_id: 67891}
    # Optional trusted fallback, obtained from native Space discovery:
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

Use real, currently live IDs. `selected_windows_only` and `workspace_space_id` require v3. An empty selection permits no windows. A Space ID alone does not grant window access. Existing tool and protected-resource authorization still apply; selection further narrows them.

On macOS, binding requires Accessibility permission, an exact top-level AX window, its retained remote AX object, WindowServer ownership, and the process start time. No title/geometry match authorizes a window. Failure to re-prove this witness permanently invalidates that selection entry. Process or window ID reuse does not rebind an invalidated entry. Applications with unreliable AX lifetime identity are refused; this is not a sandbox against a malicious application falsifying its own accessibility data.

The selection is immutable. For replacement, close the old trusted session and create a new one with a fresh manifest and native lifetime witnesses. Editing the loaded file or restarting an agent-callable session does not widen its authority. Handle close revokes the connection immediately; existing lifecycle hooks clear observations and ownership after any admitted work drains. Expired authority refuses dispatch/capture immediately, and the existing runtime maintenance sweep reclaims expired session resources (up to 30 seconds).

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

Browser bind operations require an approved exact native window. Subsequent operations require implementation-attested native ownership of the session's browser target and tab. Existing browser permission checks remain in place. Profile preparation, legacy `page`, process-wide browser attachment, and unbound browser scripting fallbacks are refused. Installed-browser coverage has not been run for this branch.

Desktop capture/input, foreground delivery, app launching/termination, clipboard access and tools without a proven selection boundary are unavailable. Existing ambiguous keyboard and unresolved AX refusals remain refusals. No workspace operation silently activates a window, switches a Space, posts global input or moves the physical cursor as a fallback. The shared-host cursor measurement was inconclusive; background support is not universally certified.

## Workspace lifecycle

| Operation | Behavior |
| --- | --- |
| `create_workspace` | Attempt automatic creation, or attach the Space supplied by trusted configuration. Report whether the session created it. Refuse duplicate ownership within the runtime. |
| `get_workspace_state` | Re-query native Space existence, activation and tracked window memberships. Report stale windows, deleted Spaces and later user movement. |
| `move_window_to_workspace` | Require an existing window grant, preserve its first origin, attempt movement and verify the exact destination membership. Keep origin tracking after partial failure. |
| `reveal_workspace` | Explicit native switch with a fresh activation check; unsupported/no-op calls fail. |
| `restore_workspace_windows` | Explicitly restore independent tracked windows. Do not override later user placement. Aggregate failures while completing independent restorations. Query state after a partial failure. |
| `release_workspace` | Drop ownership, leaving windows and Spaces in place. Access approval remains separate. Ownership is released even if the final native state read fails. |
| `delete_workspace` | Refuse deletion of pre-existing Spaces. Safe deletion of created Spaces is currently unsupported by the macOS adapter. |

Session end/disconnect releases workspace ownership without moving windows, closing apps, or deleting Spaces. Release does not schedule later deletion. Display topology and membership are read afresh. Fullscreen, sticky/multiple-Space membership and unknown topology are refused for movement. Native state can change concurrently with a user action; postconditions report the observed result rather than attempting a corrective switch.

macOS Space operations use private SkyLight APIs through the existing loader. Accessibility and ScreenCaptureKit are public Apple APIs; exact AX/WindowServer integration also reuses existing private helpers. Public AppKit [window collection behavior](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/canjoinallspaces) does not provide arbitrary third-party Space management. Private signatures were checked against [CGSInternal](https://github.com/NUIKit/CGSInternal/blob/master/CGSSpace.h); symbol presence is not a capability check. No SIP change, TCC database editing, process injection or Dock scripting addition is used.

Windows and Linux expose the shared workspace contract but explicitly refuse native workspace operations and selected-window lifetime binding. Their native implementations are not provided by this branch.

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

These are exact-window captures, never a capture of an inactive Space. Inactive-window support depends on live exact AX resolution and ScreenCaptureKit availability. Visible PiP, child/dialog rendering and background input on an inactive Space remain unverified here.

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

Run `target/debug/examples/selected_windows /private/tmp/cua-workspace-fixture.json /private/tmp/cua-workspace-evidence`. Its oracle-enabled run minimizes the first fixture window and closes the second; use fresh fixtures for each run. `workspace_probe --create --fixture-report <report> --move-to <ordinary-space-id>` tests automatic management only on the verified repository fixture. It prints sanitized Space metadata without application titles.

Regenerate interfaces with `cargo run --locked -p cua-driver-contract --bin cua-contract-gen -- all` and `node ../scripts/generate-uniffi-bindings.mjs`. Do not edit generated bindings manually. The canonical macOS gate remains `libs/cua-driver/tests/runners/macos-lume/run-all.sh --standalone-browser`; local fixture tests do not replace it. This task's host-only environment has no Lume installation, so that gate is outstanding.
