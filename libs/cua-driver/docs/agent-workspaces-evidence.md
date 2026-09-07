# Workspace implementation evidence

The current tested code SHA is `305bb0ed444821a8ce3359591b91b15be2bcd4d6`. This is native development evidence on the requested personal Mac, **not canonical desktop E2E certification**. The evidence-only delivery commit does not change executable code.

Environment: macOS 26.5, build 25F71, Apple Silicon, SIP enabled, two connected displays. All controlled windows belonged to repository AppKit fixture processes. The SDK examples used the isolated worktree's `target` directory and the existing signed development identity `com.trycua.workspace-probe`. Accessibility and capture permissions were already available. No daily installation, personal window, system setting or TCC database was changed. No process injection, merge, release or upstream PR was performed for this implementation.

## Current native results

| Check | Observed result |
| --- | --- |
| Real desktop creation | The full SDK session created ordinary Mission Control Space 4078 on CGDirectDisplayID 1. Its state reported `space_created=true`, `active=false`. Creation briefly showed the existing Mission Control interface through the explicitly enabled trusted option. |
| Verified automatic movement | Both approved fixture windows moved from Space 3714 to 4078. Exact membership verified; destination remained inactive. Earlier cross-display movement also verified membership without changing active Spaces. |
| Exact Space enumeration | The minimized-inclusive private query contained both fixture IDs. Attempted deletion while occupied refused. |
| Inactive AX and capture | Both approved windows returned exact Accessibility data and one screenshot each while the workspace stayed inactive. Both screenshot images changed after text edits; the independent fixture oracle confirmed the new values. |
| Unapproved sibling | The same-process third fixture window, desktop observation and forged permission IDs were denied. The third fixture's value remained unchanged. |
| Background input | AX text edits passed on both inactive windows. Frontmost PID and physical cursor positions were unchanged across each tested write. These are bounded observations, not a controlled simultaneous-human-typing certification. |
| Keyboard ambiguity | Same-process key delivery refused; the implementation also treats unresolved off-Space siblings conservatively. No foreground/global input fallback was used. |
| Explicit reveal | The session's reveal tool selected its own desktop. The diagnostic explicitly returned to the saved original desktop. Background actions did not invoke reveal. |
| Membership-based access | Workspace-only access refused before creation and after restoration moved the approved windows outside the owned desktop. Verified movement back restored access without creating new grants. |
| Minimized/closed windows | Minimization returned no cached screenshot. Closing the second fixture revoked subsequent observation. Final restoration reported the closed-window failure while restoring the surviving minimized window. |
| Explicit deletion | After restoration, the empty session-created desktop was deleted through Mission Control. A fresh state query reported `space_exists=false`, `owned=false`. Wallpaper exclusion uses Quartz's own desktop-element filter; unknown members block cleanup. |
| Multiple sessions | Interleaved sibling observations kept their caches separate; cross-session element tokens were denied. Recording control and finalized state were unavailable to the other session. |
| Recording and preview routing | Ten approved before/after PNG frames and AX artifacts covered both approved windows. No physical-cursor sample file or display video was admitted. The existing `PipBackend` test backend received approved frames and shut down once on deletion/teardown. A recorded fixture PNG was visually inspected. |
| Revocation | `end_session` and trusted handle close refused later calls. |

The main result markers from the final SDK run are:

```text
exact_space_query_includes_fixture_windows_and_occupied_deletion_refused=true
workspace_created_and_two_windows_moved_without_switch=true
two_approved_discovered=true
window_0_error=false code=None images=1
window_1_error=false code=None images=1
window_2_error=true code=Some("selected_window_denied") images=0
unapproved_sibling_desktop_and_forged_ids_denied=true
window_0_physical_cursor_unchanged=true
window_0_frontmost_unchanged=true
window_0_fresh_frame_changed_after_ax_write=true
window_1_physical_cursor_unchanged=true
window_1_frontmost_unchanged=true
window_1_fresh_frame_changed_after_ax_write=true
two_approved_ax_writes_verified_sibling_unchanged=true
cross_session_tokens_denied_and_cache_isolated=true
recorded_approved_windows=2 recorded_frames=10
recording_and_preview_session_scoped=true
explicit_reveal_verified_and_original_desktop_restored=true
workspace_restoration_revokes_observation_until_verified_return=true
independent_fixture_value_oracle_verified=true
minimized_capture_refused_and_closed_window_revoked=true
create_error=false code=None
explicit_empty_desktop_cleanup_verified=true
session_end_and_handle_close_revoke_access=true
```

## Checks at the tested SHA

- `cargo test --locked -p cua-driver-core -p cua-driver-contract -p cua-driver-sdk --lib`: 596 core, 32 contract and 52 SDK tests passed.
- `cargo test --locked -p platform-macos --lib`: 361 passed, 2 ignored. Total passing Rust library tests: 1,041.
- Native `selected_windows` SDK example with `CUA_WORKSPACE_DISPLAY_ID=1 CUA_WORKSPACE_REVEAL=1 CUA_WORKSPACE_CLEANUP=1`: passed against fresh AppKit fixtures.
- Canonical contract generation/check and UniFFI regeneration/check: passed; generated SDK bindings remained current.
- TypeScript typecheck/build and three staged native-loader tests against mock daemons: passed.
- Separate native builds repeatedly compiled the macOS implementation throughout development. Existing Swift bridge duplicate-symbol linker warnings and the SDK's existing dead-code warning did not fail builds.

Local supporting artifacts are `/private/tmp/cua-workspace-certified-evidence`, `/private/tmp/cua-workspace-certified-sdk.log`, `/private/tmp/cua-workspace-certified-common.log`, `/private/tmp/cua-workspace-certified-macos.log`, `/private/tmp/cua-workspace-certified-node.log`, and `/private/tmp/cua-workspace-certified-bindings.log`. They contain fixture content, not personal-window captures. Follow [setup and executable examples](agent-workspaces.md) to reproduce.

## Limits and outstanding gates

- Native success is verified on this SIP-enabled macOS 26.5 host and the AppKit fixture. Private SkyLight/Dock/AX behavior is version-dependent. Older macOS native movement and Intel macOS execution remain unverified.
- The first cross-display inactive capture failed in ScreenCaptureKit with an audio/video-start error. Later same-display inactive capture passed. The root cause was not isolated; cross-display capture and automatic frame restoration are not certified.
- Controlled simultaneous human typing in a separate application, native dialog/sheet/fullscreen transition coverage, hot-plug display changes, sticky windows, real installed-browser target mapping, and visible PiP rendering are outstanding. Existing fail-closed policies remain in place. Do not infer universal background keyboard or browser support from these fixtures.
- Windows/Linux native workspace adapters and selected-window lifetime binding explicitly report unsupported. Windows/Linux desktop E2E and native compilation were not run from this Mac.
- The canonical `libs/cua-driver/tests/runners/macos-lume/run-all.sh --standalone-browser` gate was not run. Lume is absent and the user selected host-only testing. The wrapper installs a test driver in its designated VM, so running that installer workflow directly on the personal host would violate the requested isolation. Fixture tests do not replace that gate.
- A separate host cleanup command for four earlier probe desktops, 4039, 4042, 4048 and 4062, was rejected by automatic approval review because it could not verify ownership from historical IDs. No bypass was attempted. Those earlier desktops remain; fixture applications keep their preset exit timers. The final session-owned desktop 4078 was successfully deleted in the SDK test.
- Delivery is to `shujanshaikh/cua:main` only. Delivery commits carry `[skip ci]` to prevent the fork's copied release automation from creating a release PR. No PR or release was created.

## Historical evidence before the SIP-enabled implementation

This is supporting development evidence, **not desktop E2E certification**. The previously tested code SHA was `da857de94c299ef4f48ecf41a211c2f95ee1bb1d`. The following commit updates only this evidence document; it does not change the tested implementation.

Environment: macOS 26.5 (25F71), Apple Silicon, SIP enabled, two connected displays. Testing used only repository AppKit fixture processes, signed development examples, temporary mock sockets, and artifacts under `/private/tmp`. Accessibility was already available to the signed development process. No installation replacement, system-setting change, TCC database modification, process injection, personal-window mutation, merge, release, or upstream PR forms part of this delivery.

Native development results:

| Check | Observed result |
| --- | --- |
| Discover Spaces/displays | Managed display and membership queries succeeded. Evidence output strips app titles/content from the native dictionaries. |
| Automatic Space creation | `SLSSpaceCreate` returned `0`. No new managed Space appeared. Returned explicit unsupported. |
| Automatic exact fixture movement | `SLSMoveWindowsToManagedSpace`, followed by add-before-remove with bounded membership polling, did not establish destination membership. Original membership and active Spaces remained unchanged. Returned explicit unsupported. |
| Exact inactive-window binding | The fixture on an inactive ordinary Space could not be resolved through exact AX binding. Trusted session creation refused; no input/capture fallback was used. |
| Two approved windows, same-process third | Approved windows returned AX and one screenshot each. Third window, desktop observation and forged private IDs were denied with no images. |
| Background AX writes | Both approved text fields changed and were read back. The independent AppKit state file confirmed both values and the unchanged third field. |
| Cross-session isolation | A second session observed the third fixture. Its token was rejected by the first session; interleaved same-process observations did not invalidate the first session's cache. |
| Keyboard ambiguity | Process-scoped key delivery was refused with `same_pid_keyboard_ambiguity`; no key was dispatched. |
| Focus | The frontmost PID was unchanged across both tested AX writes. |
| Physical cursor | Before/after coordinates changed in the shared-host run. Cause was not established. This is inconclusive, not a passing cursor-isolation result; no corrective cursor movement was issued. |
| Minimized window | The fixture oracle confirmed minimization. `get_window_state` returned no image; it did not reuse a cached frame. AX state can remain available. |
| Closed window | Closing only the second fixture caused later observation to be denied with no image. |
| Recording | The final SDK run verified actual AX and PNG artifacts for both approved windows, including seven before/after frames. Independent artifact inspection found 15 AX files containing only the two approved windows and no third-window title or value. Display video was refused, no physical-cursor sample file was created, and another session could not read/stop the recording or read its finalized state. |
| Preview routing | The SDK's existing `PipBackend` interface received approved PNG frames through a counting test backend. Session teardown shut it down. This does not test visible AppKit rendering. |
| Revocation | `end_session` and trusted handle close revoked subsequent access. SDK library tests also covered handle-close/invocation races. |

Sanitized markers from the oracle-enabled SDK run:

```text
two_approved_discovered=true
window_0_error=false code=None images=1
window_1_error=false code=None images=1
window_2_error=true code=Some("selected_window_denied") images=0
unapproved_sibling_desktop_and_forged_ids_denied=true
window_0_physical_cursor_unchanged=false
window_0_frontmost_unchanged=true
window_1_physical_cursor_unchanged=false
window_1_frontmost_unchanged=true
two_approved_ax_writes_verified_sibling_unchanged=true
cross_session_tokens_denied_and_cache_isolated=true
recorded_approved_windows=2 recorded_frames=7
recording_and_preview_session_scoped=true
independent_fixture_value_oracle_verified=true
minimized_capture_refused_and_closed_window_revoked=true
create_error=true code=Some("workspace_operation_unsupported")
session_end_and_handle_close_revoke_access=true
```

Focused checks during development:

- `cargo check --locked -p platform-macos -p cua-driver-sdk`: passed during native integration; separate debug and release builds also completed.
- `cargo test --locked -p cua-driver-core -p cua-driver-contract --lib`: 595 core and 32 contract tests passed.
- `cargo test --locked -p cua-driver-sdk --lib`: 52 passed.
- `cargo test --locked -p platform-macos background_input_regression_tests --lib`: 3 passed. These verify refusal and mutation coordination, not native desktop E2E.
- Canonical manifest generation and `generate-uniffi-bindings.mjs --check`: passed. Bindings were generated, not edited manually.
- TypeScript typecheck/build: passed. The initial `npm test` could not install its Electron binary (`fetch failed`); four native-library cases were initially skipped before local staging. After staging the final candidate's native library, `node --test test/embedded.test.mjs test/native-loader.test.mjs` passed all four tests with no skips. This does not establish a passing full Electron suite.

At `da857de94c299ef4f48ecf41a211c2f95ee1bb1d`, the combined core, contract and SDK library run passed 679 tests. The generated-binding check and native SDK fixture run above also passed at that SHA. Space probes, the three platform background regression tests and the manifest check ran at `480fa198d4bae6d6c4bc28e6b901fa50c7e14cf7`; the later code commit changes only recording scope comparison and SDK example artifact assertions. The platform implementation and contract are unchanged between those candidates. Generated Python imports and workspace constructors were checked at the earlier candidate, with unchanged generated interfaces in the final candidate.

Compilation emits duplicate Swift-symbol linker warnings from the existing `apple-cf`/`screencapturekit` combination and an existing SDK dead-code warning. The tested builds completed; these warnings were not fixed in this workstream.

Outstanding evidence:

- Successful automatic managed-Space creation/movement under supported SIP-enabled conditions.
- Input/capture of a selected target on an inactive Space, and an independent simultaneous-human-typing oracle.
- Explicit native switching; native fullscreen transitions, Space deletion/display removal, and sheet/dialog/child-window capture behavior. Shared lifecycle tests simulate later user movement, deleted Spaces and native query failures.
- Visible PiP rendering and multiple native renderer windows. The renderer is compiled, while routing/cleanup are exercised through its existing backend contract.
- Installed-browser coverage, native Windows/Linux compilation and desktop behavior.
- Canonical Windows/Linux desktop gates and `libs/cua-driver/tests/runners/macos-lume/run-all.sh --standalone-browser`. Lume is unavailable and this task explicitly permits only this personal Mac. Local diagnostics do not replace these gates.

Native logs and approved-only trajectory artifacts remain on the development Mac under `/private/tmp/cua-workspace-*`; no raw desktop capture is committed. In particular, the early raw native dictionary probe is not publication evidence. Use only the sanitized probe and SDK markers above when sharing results.
# Agent-launched apps follow-up — 2026-09-07

Implementation SHA: `c384d1fa400f64586cd0c6d5c74ef3cfaac09825`.
Final sample/export correction: `299616bfb239921b03650c37128b8febab09f4aa`.
The latter removes an unsupported `wait` entry from the sample manifest and
exports the generated Python input type; native code is unchanged.

Environment: personal SIP-enabled macOS host, main display ID 1, signed isolated
development binary (`com.trycua.workspace-dev`). Only fresh Helium profiles and
TextEdit documents under `/private/tmp/cua-workspace-app-native` were launched.
No daily-driver installation, personal window, TCC database or SIP setting changed.

Verified through the real stdio MCP connection:

- Creation allocated desktop 4307 without switching; later iterations attached
  that task-created desktop instead of allocating repeated desktops.
- Trusted aliases launched fresh Helium and TextEdit processes, bound exact
  lifetime witnesses and verified movement from desktop 4281 to 4307 while the
  user's active desktop remained 4198.
- Discovery returned only the two admitted windows. TextEdit's auxiliary window
  was not admitted. The exact requested document was identified by AXDocument;
  AXMainWindow supplied its exact identity when AXWindows omitted it.
- Helium followed the Example Domain link using AXPress. A subsequent observation
  showed `iana.org/help/example-domains` and the title “Example Domains”.
- TextEdit accepted an AXValue write, returned `effect: confirmed` with value
  readback, and a subsequent observation contained the full written note.
- TextEdit process keyboard delivery remained refused as ambiguous. After its
  window became unavailable, workspace state reported `stale_or_unavailable`.
- The corrected generated MCP configuration initialized and advertised 64 tools,
  including `launch_workspace_app`, without any fixture window IDs.

Evidence directories include `1788783289397713000` (browser navigation and an
exact-window PNG) and `1788784445945127000` (both app launches, notes write/readback,
browser navigation, stale-window state). Recent captures timed out or returned
`ScreenCaptureKit capture already in flight`; reliable capture of these newly
launched inactive windows is **not established**. No display capture or foreground
fallback was substituted. Apps/desktops remain for explicit user cleanup.

Focused validation: 599 core, 32 contract, 52 SDK and 361 macOS unit tests passed
(two native macOS tests ignored). Canonical Rust manifest and UniFFI Python/TS
bindings regenerated; TypeScript typecheck passed. The new repository testkit
scenario `workspace_apps_macos_test` compiled. Its daemon-backed native run stopped
at `workspace_permission_required: Accessibility permission is required`, before
creating a desktop: the independently responsible daemon lacks the permission
identity inherited by the working direct MCP process. This is a failed/blocked
native gate, not a pass. No system permission was changed to bypass it.

The full macOS Lume/installed-browser gate remains unrun because no Lume environment
was supplied and testing was restricted to this Mac. Windows/Linux native gates,
23-app concurrency, reliable inactive capture, and an independent human-typing
oracle remain unverified. Earlier evidence below is historical and does not
override these latest results.
