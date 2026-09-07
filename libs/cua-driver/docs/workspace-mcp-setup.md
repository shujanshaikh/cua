# Workspaces through the installed Cua Driver MCP server

Workspace tools use the ordinary Cua Driver runtime, native platform adapters,
permission model, daemon, and MCP transport. No separate workspace MCP product
or SDK diagnostic is required.

## Install the candidate

For uncommitted source changes, the repository installer builds the separate
`cua-driver-local` product. It exercises app packaging and daemon startup without
replacing the released `cua-driver`. Do not expect the public installer to include
unreleased checkout changes.

```sh
bash libs/cua-driver/scripts/install-local.sh --require-stable-signing
cua-driver-local workspace-config --output "$HOME/cua-agent-workspaces"
```

The second command creates a new directory and prints an MCP configuration for
server `cua-driver`. Its command points to the installed executable; its arguments
are `mcp --socket <dedicated-socket>`. It does not use `--direct`. Register the
printed command, arguments, and environment in your MCP client. The same command
is `cua-driver workspace-config` when using a released build containing this feature.

The manifest enables ordinary `list_apps` discovery and `launch_app` for installed
applications through `resources.desktop.workspace_launch_apps: true`. No app
aliases or app-specific grants are required. Review `manifest.json` before enabling
the connection. The setting requires version 3, selected-window isolation, and
workspace-only access. It does not grant input to existing personal windows.
This setup also allows HTTP(S) browsing within launched windows and briefly
showing Mission Control when creating a Space.

On macOS, the MCP proxy starts the normal installed app daemon, forwarding the
configured permission mode and approved manifest. The daemon uses the installed
app's Accessibility and Screen Recording grants. OS permissions must be granted
by the user when required. A dedicated socket separates this policy from other
computer-use sessions. An existing daemon with a different manifest is rejected;
stop that daemon and reconnect after changing policy.

## Agent workflow

Create a workspace, then use ordinary app names or bundle IDs:

```json
{"tool":"create_workspace","arguments":{"session":"my-work"}}
{"tool":"launch_app","arguments":{"session":"my-work","name":"Zed","urls":["/absolute/path/to/project"]}}
{"tool":"launch_app","arguments":{"session":"my-work","name":"Helium"}}
{"tool":"launch_app","arguments":{"session":"my-work","name":"Helium"}}
```

The normal launcher resolves the app and accepts its usual open targets and
arguments. Workspace mode forces a fresh process, verifies its identity and new
window, moves that window, and verifies membership before returning success.
The response keeps the normal app metadata and adds `window_id` and
`workspace_space_id`. Only admitted windows appear in its `windows` list.
Repeat the same `session` on `launch_app` and subsequent window/browser calls.
`launch_app` exposes this optional field in MCP on every platform. Omitting it
uses the connection's unnamed session, which does not inherit a named workspace.
All observations and input use the returned PID/window ID. The shared resource
authorization layer accepts the session's live launch grant for exact-window
observation and background input. It rechecks native lifetime and workspace
membership on each authorization; no static window-ID entry or process-wide app
grant is needed for a newly launched workspace window. `list_windows` remains
workspace-filtered; `list_apps` includes installed apps so discovery works before
launching. Keep the same session across follow-ups.

Each `launch_app` call requests a new instance. Standalone Chromium browsers,
including Helium, receive independent driver-created profiles and CDP endpoints.
Agent-supplied profile/debugging flags are refused. Profiles are retained in the
system temporary directory; session cleanup does not delete browsing data.
Use browser target/tab IDs for subsequent tabs and navigation.

Existing trusted `launch_workspace_app` recipes still work and remain idempotent.
`available_apps` lists only these optional legacy aliases, not an app allowlist.
New generated configurations have no aliases. A failed launch or move may leave
a process or retained window; inspect `get_workspace_state` before retrying.

Normal computer use is not equivalent to an isolated OS login. macOS Spaces
share processes, menus, clipboard, and physical input. Apps that reuse an existing
process, expose ambiguous windows, or require activation can refuse background
workspace launch. Global desktop input, foreground menu invocation, and other
operations without an exact-window boundary remain unavailable. These refusals
protect the user's active desktop; enabling an app does not waive them.

## Lifetime

Normal sessions retain their existing idle cleanup policy. While an MCP session
owns a workspace, ordinary idle eviction is suspended. This supports pauses
between conversation turns without changing the timeout for other computer use.
`get_session_state` reports `expires_in_seconds: null` during retention.

Keep one session label across follow-ups. Releasing the workspace restores idle
eviction. Explicit session ending, MCP transport disconnect, process shutdown,
and trusted host authorization deadlines still revoke access through the normal
cleanup hooks. Browser targets and native window grants are not restored by
reusing a label. Cross-connection recovery requires a separately authenticated
ownership protocol and is not implemented by this setup.

Native Space creation remains macOS-only. Windows and Linux return the existing
explicit unsupported result. Inactive screenshot capture can still time out,
and apps that need activation to create their first window remain unsupported.
Accessibility readback can work even when a screenshot is unavailable.

## Workspace cursors

Each macOS workspace has its own click-through cursor overlay window. These
movable panels opt out of fullscreen participation and tiling. Every native move,
including cursor placement, refuses fullscreen, unknown, or ambiguous source
and destination Spaces before submitting a WindowServer operation. Panels are
hidden when their destination is no longer an ordinary desktop. The driver
verifies Space membership before displaying cursor pixels. Workspace cursors
are excluded from the global overlay, including while a workspace is being
created or released. Ending a session removes its panel; deleting an empty Space
first removes its decorative panel. Other sessions keep their own overlays.

The shared renderer rejects workspace pixels on global or mismatched surfaces.
Windows and Linux continue to refuse native workspace creation; their global
renderers also suppress workspace-only cursor pixels. Native placement uses the
same private macOS Space APIs as workspace windows. Visual placement requires
checking the installed candidate on macOS; unit tests cover pixel isolation and
existing cursor/session behavior.
