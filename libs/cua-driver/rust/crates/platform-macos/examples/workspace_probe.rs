//! Content-free native investigation. Never activate personal windows.
fn main() {
    let args: Vec<_> = std::env::args().collect();
    println!("accessibility={}", unsafe {
        platform_macos::ax::bindings::AXIsProcessTrusted()
    });
    println!(
        "display_ids={:?}",
        core_graphics::display::CGDisplay::active_displays()
    );
    println!("before={:?}", platform_macos::spaces::managed_displays());
    if args.iter().any(|arg| arg == "--mission-control-create") {
        println!(
            "mission_control_create={:?}",
            platform_macos::spaces::create_with_mission_control(
                args.iter()
                    .position(|a| a == "--display")
                    .map(|i| args[i + 1].parse().unwrap())
            )
        );
    }
    if args.iter().any(|arg| arg == "--create") {
        println!("create={:?}", platform_macos::spaces::create());
    }
    if let Some(index) = args.iter().position(|arg| arg == "--fixture-report") {
        let data = std::fs::read(&args[index + 1]).unwrap();
        let report: serde_json::Value = serde_json::from_slice(&data).unwrap();
        let pid = report["windows"][0]["pid"].as_i64().unwrap() as i32;
        // Only touch the exact repo-built AppKit fixture process.
        let app = platform_macos::apps::list_running_apps()
            .into_iter()
            .find(|app| app.pid == pid)
            .unwrap();
        assert_eq!(app.bundle_id.as_deref(), Some("com.trycua.harness.appkit"));
        for row in report["windows"].as_array().unwrap() {
            let wid = row["window_id"].as_u64().unwrap() as u32;
            assert_eq!(
                platform_macos::windows::resolve_window_owner(pid, wid),
                platform_macos::windows::WindowOwner::SamePid
            );
            println!(
                "fixture_geometry={:?}",
                platform_macos::windows::window_bounds_by_id(wid)
            );
            println!(
                "fixture_membership={{wid:{wid},spaces:{:?}}}",
                platform_macos::spaces::membership(wid)
            );
        }
        if let Some(index) = args.iter().position(|arg| arg == "--move-to") {
            let space: u64 = args[index + 1].parse().unwrap();
            let wid = report["windows"][0]["window_id"].as_u64().unwrap() as u32;
            use cua_driver_core::workspace::WorkspaceBackend;
            println!(
                "move={:?}",
                platform_macos::spaces::MacosWorkspaces.move_window(
                    cua_driver_core::selected_windows::WindowTarget {
                        pid: i64::from(pid),
                        window_id: u64::from(wid)
                    },
                    space,
                )
            );
            println!(
                "membership_after={:?}",
                platform_macos::spaces::membership(wid)
            );
        }
    }
    println!("after={:?}", platform_macos::spaces::managed_displays());
}
