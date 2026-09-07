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

The manifest permits Helium, a second independent Helium window, and TextEdit.
Review `manifest.json` before enabling the connection. This setup allows HTTP(S)
browsing within the exact launched windows and briefly showing Mission Control
when creating a Space. Add other trusted application recipes there before the
daemon starts; applications still need to support inactive launch and input.

On macOS, the MCP proxy starts the normal installed app daemon, forwarding the
configured permission mode and approved manifest. The daemon uses the installed
app's Accessibility and Screen Recording grants. OS permissions must be granted
by the user when required. A dedicated socket separates this policy from other
computer-use sessions. An existing daemon with a different manifest is rejected;
stop that daemon and reconnect after changing policy.

## Agent workflow

Ask your agent to use `cua-driver`, create a workspace, launch `helium`,
`helium-two`, and `notes`, and keep your current desktop active. All observations
and input must use the exact PID/window ID returned for each app. Open tabs using
the observed New Tab control, refresh `get_browser_state`, and navigate using
the returned target and tab IDs. Both Helium processes have independent profiles.

Each alias launches once within an owned workspace. A new workspace gets a new
browser profile through the trusted `{workspace}` argument placeholder, replaced
by a driver-generated UUID. Retrying the same launch stays idempotent. Profile
folders are retained after ending a session; cleanup never deletes user data.
The example notes recipe opens its configured document, so its content persists.

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
