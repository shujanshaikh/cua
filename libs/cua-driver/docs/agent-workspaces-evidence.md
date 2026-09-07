# Workspace implementation evidence

This is supporting development evidence, **not desktop E2E certification**. The final tested code SHA is `da857de94c299ef4f48ecf41a211c2f95ee1bb1d`. The following commit updates only this evidence document; it does not change the tested implementation.

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
