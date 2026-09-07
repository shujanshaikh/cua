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

The output directory must be new. It contains a clean browser profile, a local
notes document, the trusted manifest, and an MCP configuration fragment. Merge
the `cua-workspace-dev` entry into `~/.cursor/mcp.json`, preserving other servers.
Enable it in Cursor's MCP settings. This uses the ordinary stdio MCP protocol with
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
list_windows {}
get_window_state {"pid":<returned pid>,"window_id":<returned window_id>}
set_value {"pid":<notes pid>,"window_id":<notes window_id>,
           "element_token":<returned text-area token>,"value":"My notes"}
```

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
