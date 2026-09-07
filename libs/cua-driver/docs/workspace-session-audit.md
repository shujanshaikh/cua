# Workspace session audit

Audited Codex session `01a07c0b-0ec8-7d12-a6c6-6f4a3b474d0c`, recorded September 7,
2026. This report separates the session's evidence from subsequent development
tests. Changes remain local and uncommitted. No driver installation or active
MCP connection was replaced.

## What the agent encountered

| Step | Evidence from the session | Finding |
| --- | --- | --- |
| Start one session | `start_session` returned `permission_denied`; the same label worked on subsequent calls. | Sample manifest omitted an optional tool recommended by server instructions. |
| Create desktop and launch apps | Workspace 4385 remained inactive; Helium and TextEdit had verified membership. | Creation and placement worked. “notes” means the sample's TextEdit document, not Apple Notes. |
| Read browser | `get_browser_state` was denied by the manifest. AX readback worked. | Sample configuration did not support the recommended browser route. Helium also lacked typed Chromium recognition, hidden behind this initial denial. |
| Follow Learn more | AXPress returned `effect: unverifiable`; the immediate snapshot was unchanged. A later snapshot showed IANA's Example Domains page. | Asynchronous navigation required another observation. The agent correctly waited for evidence rather than reporting dispatch as success. |
| Write notes | AXValue write and later AX readback contained all three bullets. | Editing worked. The session did not prove a disk save. |
| Capture notes | ScreenCaptureKit timed out at 3000 ms while accessibility data remained available. | Pixel grounding was unavailable in that call. The agent correctly continued through AX. |
| Open website and GitHub tabs | New Tab and setting the omnibox value worked. Return was refused with `same_pid_keyboard_ambiguity`. | A URL in the address bar is not navigation. Exact typed browser navigation removes the need for process keyboard delivery. |
| Launch Ghostty | `workspace_app_denied`; only helium and notes were configured. | The original session never tested Ghostty's native launch or terminal input. |
| Discover tools | Broad discovery emitted a truncated 130,996-token tool-description dump. | A narrow name search and reading only the few needed schemas would avoid this context cost. |

The agent respected the important boundary: it did not switch desktops, change
sessions to bypass a refusal, or fall back to another computer-use tool. The
session's final report accurately left website navigation and Ghostty incomplete.
The main failures were configuration and missing app support, not an agent that
ignored the desired workspace workflow.

## Local changes

- The sample now permits session startup and typed browser operations, with an
  explicit selected-window browser grant. Existing origin-scoped policies remain
  separate and continue rejecting native routes that would bypass origin checks.
- Helium is classified as Chromium. A trusted workspace launch can enable an
  ephemeral debugging port only with a newly allocated, canonical profile path.
  Public `launch_app` cannot enable this path through JSON arguments.
- Browser endpoint authority distinguishes a live workspace-launched window from
  a preselected personal browser. Selection, native lifetime, endpoint ownership,
  exact tab binding, and workspace membership remain enforced.
- Launch responses identify the requested app directly. Error responses retain
  readable workspace state and an admitted partial-launch target. Window order is
  stable, and launch/restore success requires membership readback.
- A bounded window-startup wait and `workspace_launch_no_window` distinguish a
  running process from an app that can actually be used in the background.
- Workspace tool guidance explains sessions, app aliases, exact IDs, route
  availability, screenshots, and recovery. The global instruction budget remains
  at most 200 words.
- The optional Ghostty recipe avoids loading default config and restoring saved
  windows. A separate SDK diagnostic collects native evidence without replacing
  the connected development driver.

## Native evidence

The signed SDK diagnostic created Space 4415 and kept it inactive. It used fresh
app instances and files beneath `/private/tmp/cua-workspace-audit-native` and
`/private/tmp/cua-workspace-audit-browser`. The browser retry attached to that same
audit-created desktop rather than allocating another one.

- TextEdit: exact window screenshot succeeded, AX editing succeeded, and the
  subsequent AX readback contained the test bullets.
- Helium: exact native/CDP binding succeeded, semantic page inspection and CDP
  screenshot succeeded, and two new tabs navigated to `https://shujan.xyz/` and
  `https://github.com/shujanshaikh`. Readback showed each destination and page refs.
- Workspace state remained `active: false` at creation, successful launches, and
  final readback. No reveal or foreground-input operation was issued.
- Ghostty: the new process stayed alive but exposed no WindowServer window, even
  on a later read-only check. No terminal command was sent. This matches the
  [upstream report that initial window creation depends on activation](https://github.com/ghostty-org/ghostty/discussions/13287).

The diagnostic leaves its desktop, browser, note document, and windowless Ghostty
process for inspection. Closing the diagnostic revokes its session; a new MCP
connection does not inherit those window grants.

The later successful screenshot does not explain or fix the original capture
timeout. ScreenCaptureKit timeout handling remains bounded and explicit, without
foreground or whole-display fallback. General background keyboard support and
Ghostty startup remain unproven or unsupported where exact delivery is unavailable.

These are supporting macOS development checks, not full desktop certification.
Windows, X11, and Wayland still report native workspace operations unsupported.
The canonical macOS Lume gate and cross-platform desktop matrix were not run.
The tested native binary predates final test, diagnostic-argument, and additional
profile-argument validation edits; those edits require their focused checks and
do not constitute a full final native certification.

## Automated checks

- Common core library: 602 tests passed, including workspace launch recovery,
  real membership verification, trusted browser grant configuration, preselected
  personal-window refusal, sibling refusal, and browser binding revocation.
- Contract library: 32 tests passed.
- macOS launch helpers: 12 tests passed, including fresh-profile allocation,
  existing-profile refusal, duplicate-profile arguments, and public-launch
  debugging flag refusal.
- macOS browser platform helpers: 19 tests passed, including Chromium recognition.
- The final SDK native diagnostic built successfully. Python preparation syntax and
  generated manifest fields were checked; the generated manifest was also loaded
  by the successful native diagnostic.

Tests used debug symbols disabled to fit the host's limited free disk space.
Browser mock tests required loopback sockets outside the filesystem sandbox.

Changed Rust files pass rustfmt, and `git diff --check` passes. Whole-workspace
`cargo fmt --check` reports pre-existing differences in untouched native input
files such as `click.rs`, `drag.rs`, and `scroll.rs`; those files were left alone.
