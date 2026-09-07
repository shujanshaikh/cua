# Agent-launched macOS workspace

This example uses the existing Cua MCP server and trusted v3 capability manifest.
The agent creates a Mission Control desktop, launches a clean Helium instance and
a separate TextEdit document, and receives access only to their exact windows.
No fixture process, copied window IDs, or fifteen-minute fixture timer is involved.

## Prepare a session

Build the development driver from your checkout. Keep its existing stable signing
identity and grant Accessibility and Screen Recording to the development process
using the repository's permission setup. Do not replace a release installation.

```sh
cd /Users/shujanshaikh/Projects/cua
python3 libs/cua-driver/examples/workspace-apps/prepare-macos.py \
  --driver "$PWD/libs/cua-driver/rust/target/debug/cua-driver" \
  --output "$PWD/libs/cua-driver/rust/target/workspace-live"
```

The output directory must be new. It reserves a fresh browser profile path, a local
notes document, the trusted manifest, and an MCP configuration fragment. Merge
the `cua-workspace-dev` entry into `~/.cursor/mcp.json`, preserving other servers.
Enable it in your client's MCP settings. The same stdio entry can be configured in Codex. This uses the ordinary stdio MCP protocol with
an isolated development runtime (`mcp --direct`), not the installed release daemon.

Prompt Cursor:

> Use cua-workspace-dev. Create a workspace, open helium and notes in it, read the
> browser page, and write a short summary in notes. Keep my current desktop active.
> Use one session throughout. Report unavailable actions without foreground fallback.

The normal tool sequence is:

```text
get_workspace_state {}
create_workspace {}
launch_workspace_app {"app":"helium"}
launch_workspace_app {"app":"notes"}
get_browser_state {"pid":<helium launched_app.pid>,"window_id":<helium launched_app.window_id>}
get_window_state {"pid":<returned pid>,"window_id":<returned window_id>}
set_value {"pid":<notes pid>,"window_id":<notes window_id>,
           "element_token":<returned text-area token>,"value":"My notes"}
```

The launch result identifies the requested app in `launched_app` with `app`,
`pid`, and `window_id`. Use those IDs directly; do not infer the app from the
order of the workspace's windows. `start_session` is optional and supported by
this manifest. If you choose a session label, repeat it throughout.

For browser work, bind with `get_browser_state`, then use its `target_id` and
`tab_id` with typed browser tools. To open a new tab, AXPress the `New Tab`
button in the approved native window, refresh the binding, and navigate the new
tab with `browser_navigate`. This avoids process-wide Return on the address bar.
Read back the resulting URL and content before reporting success.

This manifest explicitly enables `resources.browser.selected_windows_only`.
It grants browsing inside approved exact native windows alongside native app
work. It requires v3 desktop selection and cannot be mixed with origin grants.
HTTP/HTTPS navigation is allowed; file navigation remains refused. Browser
binding reuses workspace launch authority only for a live session-launched
window. Preselecting a personal browser does not authorize its DevTools endpoint.
Helium uses a new profile directory and an OS-assigned loopback debugging port.
The ordinary agent-callable `launch_app` still refuses debugging flags.

Add `--ghostty` to prepare an optional Ghostty recipe. It disables default config
loading and saved-window restoration for the new instance and sets its working
directory to the workspace output directory. These options are documented in
[Ghostty's configuration reference](https://ghostty.org/docs/config/reference).
This is a launch recipe, not certification that Ghostty supports inactive use.
The installed version tested in this audit starts windowless when inactive,
consistent with [Ghostty's activation-dependent startup report](https://github.com/ghostty-org/ghostty/discussions/13287).
The driver reports `workspace_launch_no_window` instead of activating it. Neither
`ls` nor `pwd` is claimed to have run in that case.

Ask “Reveal the workspace” when you want Cursor to call `reveal_workspace {}`.
Creation may briefly display Mission Control because this manifest explicitly
opts into the existing Dock accessibility path. Background operations do not
switch desktops. App launch uses `activates=false`, then verifies and moves the
new window; macOS may briefly show a new window during setup.

## Approval and lifecycle

`available_apps` lists trusted launch aliases. Add another app by configuring a
new recipe under `resources.desktop.workspace_applications` and a matching
launch-enabled `resources.apps` entry, then start a fresh MCP session. The agent
cannot supply bundle IDs, launch arguments, URLs, or new window grants to this
tool. The broad app resource is always intersected with the exact live selection.

Each alias launches once per owned workspace. Repeated calls return its current
state; a failed launch is retained rather than spawning more processes on retry.
Failures include `workspace_state` when it can be read. A partially moved,
admitted window also includes `launched_app`. Recover that exact window with
`move_window_to_workspace`; do not start another process. Launch and restoration
verify actual destination membership before reporting success.
The native launcher must return a new process and an exact live window witness.
Document recipes bind the exact local `AXDocument`, never a title match. Sibling
windows, subsequent windows, sheets and dialogs are not automatically approved.
Moving a personal window into the desktop does not grant access to it.

Closing a window or ending the session revokes its access. Reconnecting does not
inherit prior window grants or workspace ownership. Existing apps and desktops
remain. Use a fresh output directory for a new browser session; reusing a running
profile is refused if the browser forwards launch to its old process. Restoring
windows, revealing the desktop, and deleting a session-created empty desktop are
separate explicit tools. Release never closes apps or deletes desktops.

## Native diagnostic

The SDK example exercises launch, exact browser binding and screenshots, new-tab
navigation, notes editing, and terminal input when its background route is
available. It writes each raw result into a new evidence directory. Inspect those
results: the diagnostic continues past individual app failures to collect evidence.
It leaves its apps and desktop for inspection and never calls `reveal_workspace`.
Build and sign only the separate example with the existing development identity;
no installed or currently connected driver needs replacement.

```sh
cd libs/cua-driver/rust
cargo build --locked -p cua-driver-sdk --example workspace_apps
# Sign target/debug/examples/workspace_apps using the authorized development identity.
target/debug/examples/workspace_apps /absolute/path/to/fresh/manifest.json \
  /absolute/path/to/new/evidence '["https://example.com","https://www.iana.org/help/example-domains"]'
```

See the [session audit](../../docs/workspace-session-audit.md) for observed results
and the remaining Ghostty and capture limitations.

## Tests and limits

Focused common tests:

```sh
cd libs/cua-driver/rust
cargo test --locked -p cua-driver-core workspace
cargo test --locked -p cua-driver-core workspace_launch_recipes
cargo test --locked -p cua-driver-contract --lib
```

The ignored `workspace_apps_macos_test` uses the repository's `McpDriver` testkit
and a fresh manifest generated above. It creates real resources and leaves apps
and the desktop for inspection. It is a supporting installed-app test, not a
replacement for the canonical macOS Lume gate:

```sh
CUA_WORKSPACE_APPS_MANIFEST=/absolute/path/to/fresh/manifest.json \
cargo test -p cua-driver --test workspace_apps_macos_test -- --ignored --nocapture
```

The canonical gate remains `tests/runners/macos-lume/run-all.sh
--standalone-browser` from `libs/cua-driver`, in the designated authorized Lume
desktop. See the [test harness guide](../../docs/test-harnesses-guide.md).

This is exact-window computer use, not a separate macOS login or a display
capture of an inactive Space. Space APIs are private and version-sensitive.
Apps that reuse a process, activate themselves, lack exact AX references, or
produce ambiguous windows can be refused. TextEdit on the tested Mac also creates
an auxiliary window: AX text editing works, but process keyboard input is refused
as ambiguous. ScreenCaptureKit can time out on inactive windows; missing frames
are reported explicitly, with no display-capture or foreground fallback.
Minimized windows remain unsupported for exact capture. There is no fixed alias
count limit, but this example does not certify 23 arbitrary apps. Native workspace
launch is explicitly unsupported on other platforms.
