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
This setup also allows HTTP(S) browsing within launched windows, briefly
showing Mission Control, and activating the agent desktop through
`workspace_allow_activation: true`. Launches select that desktop before starting
an app. Foreground input, `bring_to_front`, and `invoke_menu` select and verify
the owned desktop before dispatch. This may switch the desktop visible to the
user. Set the flag to false or omit it to retain the older background-only policy.

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
windows, moves each attested startup window, and verifies membership before
returning success. Multiple documents or startup dialogs do not by themselves
make a launch ambiguous. Unresolved WindowServer entries are reported in
`unadmitted_windows` without granting access; independently attested siblings
can still be admitted. The response keeps normal app metadata and adds
`workspace_space_id`. A single-window launch also adds `window_id`; for a
multi-window launch, choose the intended target from `windows`. Only admitted
windows appear in that list, with membership updated after placement.
Repeat the same `session` on `launch_app` and subsequent window/browser calls.
`launch_app` exposes this optional field in MCP on every platform. Omitting it
uses the connection's unnamed session, which does not inherit a named workspace.
All observations and input use the returned PID/window ID. `verify_state` and
`set_window_frame` use their ordinary implementations with live workspace
selection checks. Verification revalidates membership on every polling sample.
Typed browser pointer actions, uploads, and downloads reuse exact bound-target
authority; normal file and host download approval rules still apply. The shared resource
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
share processes, menus, clipboard, and physical input. The activation policy
enables normal foreground window operations, including native menus, while
keeping exact-window ownership checks. Apps that reuse a personal process or
lack provable window identities can still refuse isolated launch. Global desktop
operations do not acquire an isolation boundary merely because a Space exists.

Under the background-only policy, workspace launch admission checks the new process's foreground identity and the
normal launcher's focus-suppression result after launch and after waiting for its
window. A change to another foreground app or active Space alone does not reject
the launch. macOS desktop snapshots cannot attribute a Space change to the user
or an app, and these checks do not certify uninterrupted background operation.
The normal launcher's focus suppression remains in place for background-only
workspaces and ordinary non-workspace launches. Activation-enabled workspace
launches suppress neither the new app nor its initial window creation.

Successful normal launches include `workspace_launch_focus` with foreground PIDs
and active Spaces before launch, after launch, and after window discovery. Focus
refusals report the check stage, created PID, suppression result, and before/after
snapshots. A refused process remains untouched and unauthorized; its PID is
provided for inspection, not as an input grant. Do not blindly retry a launch
that already created a process.

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
and apps that need activation require `workspace_allow_activation: true`.
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

## Capability audit

The normal driver documents supported application technologies and action routes,
not an exhaustive app allowlist. See [desktop action support](action-support.md)
and the [normal agent workflow](../rust/Skills/cua-driver/SKILL.md). The following
limits distinguish an inactive macOS Space from ordinary foreground computer use.

| Area | Workspace behavior |
| --- | --- |
| App discovery and launch | Normal app resolver; a verified fresh process is required to avoid taking personal windows. All attested startup windows can be admitted. |
| Window observation and verification | Exact-window screenshots, AX state, and bounded verification; inactive capture and AX identity can still be unavailable. |
| Native input and window geometry | Normal targeted background routes and frame mutation; foreground fallback is admitted when the host enables workspace activation. |
| Typed browser actions | Native-window-bound state, navigation, click, type, dialogs, pointer, upload and download; ordinary route limitations and file permissions remain. |
| Menu commands and foreground input | Normal macOS menu invocation temporarily activates the app. This is admitted with workspace activation enabled and refused under background-only policy. |
| Global desktop, clipboard, video and process operations | A Space is not an independent login, clipboard, input seat, or screen. These cannot be treated as workspace-local by merely allowing their tool names. Non-video session recording remains available. |
| Windows created by actions | Activation-enabled workspaces discover new windows after successful actions in a driver-launched process, attest each native identity, and verify placement. Dialog-to-document handoffs retain process-lifetime proof even if the original window closes. Arbitrary arrivals and unrelated processes remain unapproved. Windows appearing asynchronously after that discovery pass remain a coverage gap. |
| Lifetime | Normal transport/session revocation and explicit cleanup remain; workspace ownership suspends ordinary idle eviction only. |
| Other platforms | Native workspace creation is explicitly unsupported on Windows and Linux. Existing normal-driver behavior remains unchanged. |

For example, current [Ghostty startup source](https://github.com/ghostty-org/ghostty/blob/main/macos/Sources/App/AppDelegate.swift)
creates the initial terminal in `applicationDidBecomeActive`. Activation-enabled workspace launch supports this lifecycle; suppressing
activation can leave a fresh process windowless. It is an example of
the foreground requirement, not a special app exclusion. A no-window timeout
alone cannot establish that cause for any arbitrary app.

The activation policy is passed into LaunchServices itself (`activates = true`),
not only a later activation request. Background-only hosts keep the default
`false` configuration.

Launch admission retries AX binding for up to three seconds after discovering
WindowServer entries, because startup placeholders can precede usable windows.

These notes are an implementation audit, not full desktop certification. Unit
and MCP diagnostics do not replace the canonical desktop matrix documented in
[test harnesses](test-harnesses-guide.md).

### Local MCP evidence (2026-09-07)

The installed local driver admitted TextEdit's Open panel, discovered and moved
its replacement document after New Document, and confirmed typed text by AX
value readback. Ghostty launched and its terminal contents were readable through
normal `get_window_state`; both live windows verified membership in one workspace.
A closed source dialog still produces a conservative stale-target result;
new-window discovery evidence now survives that result so the agent can continue
without repeating the completed action.

One desktop activation attempt returned `AXPress postcondition did not verify`
before app launch; a subsequent attempt succeeded. Mission Control switching
reliability therefore remains unresolved. The final reporting correction has
focused automated coverage; the native launch and editing evidence preceded that
reporting-only correction. These checks do not certify all applications or routes.
