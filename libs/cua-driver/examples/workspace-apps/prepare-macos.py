#!/usr/bin/env python3
"""Prepare trusted Helium/TextEdit/Ghostty launch recipes; launch no applications."""
import argparse
import copy
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True,
                        help="New directory for this workspace's profile, document, and manifest")
    parser.add_argument("--driver", type=Path, required=True,
                        help="Absolute path to the development cua-driver executable")
    parser.add_argument("--ghostty", action="store_true",
                        help="Allow a fresh Ghostty terminal in this workspace")
    args = parser.parse_args()
    driver = args.driver.resolve(strict=True)
    for app in ["/Applications/Helium.app", "/System/Applications/TextEdit.app"]:
        if not Path(app).is_dir():
            raise SystemExit(f"Required application is missing: {app}")
    if args.ghostty and not Path("/Applications/Ghostty.app").is_dir():
        raise SystemExit("Required application is missing: /Applications/Ghostty.app")
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    notes = root / "Agent notes.txt"
    notes.write_text("Agent workspace notes\n")
    manifest = {
        "version": 3,
        "resources": {
            "apps": [
                {"bundle_id": "net.imput.helium", "launch": True},
                {"bundle_id": "com.apple.TextEdit", "launch": True},
            ],
            "browser": {"selected_windows_only": True},
            "desktop": {
                "selected_windows_only": True,
                "workspace_only": True,
                "workspace_allow_mission_control": True,
                "windows": [],
                "workspace_applications": {
                    "helium": {
                        "bundle_id": "net.imput.helium",
                        "arguments": ["--user-data-dir=" + str(root / "helium-profile"),
                                      "--no-first-run", "--no-default-browser-check",
                                      "--remote-debugging-port=0",
                                      "--new-window", "https://example.com"],
                    },
                    "notes": {
                        "bundle_id": "com.apple.TextEdit",
                        "arguments": ["-ApplePersistenceIgnoreState", "YES"],
                        "urls": [str(notes)],
                    },
                },
            },
            "files": {"read": [str(notes)],
                      "write": [{"dir": str(root), "recursive": True}]},
        },
        "allow": {"tools": [
            "start_session", "get_session", "get_session_state",
            "get_browser_state", "browser_navigate", "browser_click", "browser_type",
            "browser_dialog",
            "create_workspace", "get_workspace_state", "launch_workspace_app",
            "move_window_to_workspace", "reveal_workspace", "restore_workspace_windows",
            "release_workspace", "delete_workspace", "list_windows", "list_apps",
            "get_window_state", "click", "double_click", "right_click", "scroll", "drag",
            "set_value", "type_text", "press_key", "hotkey",
            "start_recording", "get_recording_state", "stop_recording", "end_session",
        ]},
    }
    second = copy.deepcopy(manifest["resources"]["desktop"]["workspace_applications"]["helium"])
    second["arguments"][0] = "--user-data-dir=" + str(root / "helium-two-profile")
    manifest["resources"]["desktop"]["workspace_applications"]["helium-two"] = second
    if args.ghostty:
        manifest["resources"]["apps"].append(
            {"bundle_id": "com.mitchellh.ghostty", "launch": True})
        manifest["resources"]["desktop"]["workspace_applications"]["ghostty"] = {
            "bundle_id": "com.mitchellh.ghostty",
            "arguments": ["--config-default-files=false", "--window-save-state=never",
                          "--working-directory=" + str(root)],
        }
    manifest_path = root / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    config = {"mcpServers": {"cua-workspace-dev": {
        "type": "stdio", "command": str(driver), "args": ["mcp", "--direct"],
        "env": {
            "CUA_DRIVER_PERMISSION_MODE": "standard",
            "CUA_DRIVER_RS_SESSION_IDLE_TTL_SECS": "3600",
            "CUA_DRIVER_CAPABILITY_MANIFEST_FILE": str(manifest_path),
            "CUA_DRIVER_CAPABILITY_MANIFEST_APPROVED": "1",
            "CUA_DRIVER_RS_TELEMETRY_ENABLED": "0", "CUA_DRIVER_RS_UPDATE_CHECK": "0",
            "CUA_DRIVER_TELEMETRY_HOME": str(root / "telemetry"),
            "CUA_DRIVER_RS_HOME": str(root / "driver-state"),
        },
    }}}
    (root / "mcp.json").write_text(json.dumps(config, indent=2) + "\n")
    print(f"Trusted manifest: {manifest_path}")
    print(f"MCP connection to merge into your client: {root / 'mcp.json'}")
    print("Prompt: Create a workspace, open helium and notes in it, read the browser page, "
          "and write a short summary in notes. Keep my current desktop active.")


if __name__ == "__main__":
    main()
