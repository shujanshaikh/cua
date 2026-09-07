//! Visible setup through the Dock's AX hierarchy, following Hammerspoon's
//! hs.spaces implementation. AXPress is public; Dock identifiers and the
//! CoreDock notification are private and version-dependent.
use super::*;
use crate::ax::bindings::*;
use core_foundation::base::CFRelease;
use std::time::{Duration, Instant};

static SETUP: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn display_uuid(display: u32) -> Option<String> {
    use core_foundation::uuid::{CFUUIDCreateString, CFUUIDRef, CFUUID};
    extern "C" {
        fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> CFUUIDRef;
    }
    unsafe {
        let raw = CGDisplayCreateUUIDFromDisplayID(display);
        if raw.is_null() {
            return None;
        }
        let uuid = CFUUID::wrap_under_create_rule(raw);
        let string = CFUUIDCreateString(std::ptr::null(), uuid.as_concrete_TypeRef());
        if string.is_null() {
            return None;
        }
        Some(CFString::wrap_under_create_rule(string).to_string())
    }
}

fn display_row(displays: &Value, display: u32) -> Option<&Value> {
    let uuid = display_uuid(display)?;
    displays
        .as_array()?
        .iter()
        .find(|d| d["Display Identifier"] == uuid)
}

struct Element(AXUIElementRef);
impl Drop for Element {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0.cast());
        }
    }
}
impl Element {
    fn children(&self) -> Vec<Self> {
        unsafe { copy_children(self.0).into_iter().map(Self).collect() }
    }
    fn child(&self, id: &str) -> Option<Self> {
        self.children()
            .into_iter()
            .find(|e| unsafe { copy_string_attr(e.0, "AXIdentifier").as_deref() == Some(id) })
    }
}

type Notify = unsafe extern "C" fn(*const c_void, i32);
struct MissionControl {
    dock: Element,
    notify: Notify,
}
impl MissionControl {
    fn open() -> Result<Self, String> {
        if !unsafe { AXIsProcessTrusted() } {
            return Err(
                "workspace_permission_required: Accessibility permission is required".into(),
            );
        }
        let apps = unsafe {
            objc2_app_kit::NSRunningApplication::runningApplicationsWithBundleIdentifier(
                &objc2_foundation::NSString::from_str("com.apple.dock"),
            )
        };
        let app = unsafe { apps.firstObject() }
            .ok_or("workspace_operation_unsupported: Dock not found")?;
        let pid = unsafe { app.processIdentifier() };
        let dock = Element(unsafe { AXUIElementCreateApplication(pid) });
        if dock.0.is_null() {
            std::mem::forget(dock);
            return Err("workspace_operation_unsupported: Dock AX unavailable".into());
        }
        unsafe {
            AXUIElementSetMessagingTimeout(dock.0, 0.25);
        }
        // Do not take over Mission Control if the user is already interacting.
        if dock.child("mc").is_some() {
            return Err("workspace_setup_busy: Mission Control is already open".into());
        }
        let pointer = crate::input::skylight::find_sym(b"CoreDockSendNotification\0")
            .ok_or("workspace_operation_unsupported: CoreDock notification unavailable")?;
        let result = Self {
            dock,
            notify: unsafe { std::mem::transmute::<*mut c_void, Notify>(pointer) },
        };
        result.toggle();
        Ok(result)
    }
    fn toggle(&self) {
        unsafe {
            (self.notify)(
                CFString::new("com.apple.expose.awake")
                    .as_concrete_TypeRef()
                    .cast(),
                0,
            );
        }
    }
    fn spaces(&self, display: u32) -> Result<Element, String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(mc) = self.dock.child("mc") {
                if let Some(group) = mc
                    .children()
                    .into_iter()
                    .find(|e| unsafe {
                        copy_string_attr(e.0, "AXIdentifier").as_deref() == Some("mc.display")
                            && copy_number_attr(e.0, "AXDisplayID") == Some(display as f64)
                    })
                    .and_then(|e| e.child("mc.spaces"))
                {
                    return Ok(group);
                }
            }
            if Instant::now() >= deadline {
                return Err("workspace_operation_unsupported: Mission Control display AX hierarchy unavailable".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
impl Drop for MissionControl {
    fn drop(&mut self) {
        // Only dismiss the setup we opened; never use global keyboard events.
        if self.dock.child("mc").is_some() {
            self.toggle();
        }
    }
}

pub(super) fn create(display: Option<u32>) -> Result<u64, String> {
    let _lock = SETUP
        .try_lock()
        .map_err(|_| "workspace_setup_busy: another setup is running")?;
    let display = display.unwrap_or_else(|| core_graphics::display::CGDisplay::main().id);
    let before = managed_displays()?;
    display_row(&before, display).ok_or("workspace_operation_unsupported: display mapping unavailable; separate display Spaces are required")?;
    let mc = MissionControl::open()?;
    let button = mc
        .spaces(display)?
        .child("mc.spaces.add")
        .ok_or("workspace_operation_unsupported: Add Desktop button unavailable")?;
    let status = unsafe {
        AXUIElementPerformAction(button.0, CFString::new("AXPress").as_concrete_TypeRef())
    };
    if status != 0 {
        return Err(format!(
            "workspace_operation_unsupported: Add Desktop AXPress returned {status}"
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let after = managed_displays()?;
        let added: Vec<_> = after
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|d| {
                d.get("Spaces")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .filter(|s| s["type"] == 0)
            .filter_map(|s| s["ManagedSpaceID"].as_u64())
            .filter(|id| !contains_space(&before, *id))
            .collect();
        if added.len() == 1 {
            if !display_row(&after, display)
                .is_some_and(|d| contains_space(&Value::Array(vec![d.clone()]), added[0]))
            {
                return Err(format!("workspace_create_ambiguous: desktop appeared on another display; no ownership assumed; space_id={}", added[0]));
            }
            return Ok(added[0]);
        }
        if added.len() > 1 {
            return Err(format!("workspace_create_ambiguous: multiple desktops appeared; no ownership assumed; space_ids={added:?}"));
        }
        if Instant::now() >= deadline {
            return Err("workspace_create_incomplete: Add Desktop did not produce a managed desktop before deadline; inspect workspace state before retry".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Explicit switching only. Never called by movement, capture, or input.
pub(super) fn reveal(space: u64) -> Result<(), String> {
    operate(space, false)
}

pub(super) fn delete(space: u64) -> Result<(), String> {
    operate(space, true)
}

fn ensure_empty(space: u64) -> Result<(), String> {
    for window in space_window_ids(space)? {
        if !crate::windows::is_desktop_background(window)? {
            return Err(
                "workspace_not_empty: restore or move windows before explicit deletion".into(),
            );
        }
    }
    Ok(())
}

fn operate(space: u64, delete: bool) -> Result<(), String> {
    let _lock = SETUP
        .try_lock()
        .map_err(|_| "workspace_setup_busy: another setup is running")?;
    let before = managed_displays()?;
    let (display, row) = core_graphics::display::CGDisplay::active_displays()
        .map_err(|_| "workspace_display_unavailable")?
        .into_iter()
        .find_map(|id| {
            let row = display_row(&before, id)?;
            contains_space(&Value::Array(vec![row.clone()]), space).then_some((id, row))
        })
        .ok_or("workspace_space_deleted: Space or display unavailable")?;
    if row
        .pointer("/Current Space/ManagedSpaceID")
        .and_then(Value::as_u64)
        == Some(space)
    {
        return if delete {
            Err("workspace_active: explicit deletion requires an inactive desktop".into())
        } else {
            Ok(())
        };
    }
    let spaces = row["Spaces"]
        .as_array()
        .ok_or("workspace_state_unavailable")?;
    let index = spaces
        .iter()
        .position(|s| s["ManagedSpaceID"] == space)
        .ok_or("workspace_space_deleted")?;
    if delete {
        if spaces[index]["type"] != 0 || spaces.iter().filter(|s| s["type"] == 0).count() < 2 {
            return Err(
                "workspace_operation_unsupported: cannot delete fullscreen or the last desktop"
                    .into(),
            );
        }
        ensure_empty(space)?;
    }
    let mc = MissionControl::open()?;
    std::thread::sleep(Duration::from_millis(300));
    let list = mc
        .spaces(display)?
        .child("mc.spaces.list")
        .ok_or("workspace_operation_unsupported: desktop list unavailable")?;
    let children = list.children();
    if children.len() != spaces.len()
        || display_row(&managed_displays()?, display).map(|d| &d["Spaces"]) != Some(&row["Spaces"])
    {
        return Err("workspace_state_changed: desktop order changed during explicit reveal".into());
    }
    if delete {
        ensure_empty(space)?;
    }
    let action = if delete { "AXRemoveDesktop" } else { "AXPress" };
    let status = unsafe {
        AXUIElementPerformAction(
            children[index].0,
            CFString::new(action).as_concrete_TypeRef(),
        )
    };
    if status != 0 {
        return Err(format!(
            "workspace_operation_unsupported: {action} returned {status}"
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let after = managed_displays()?;
        if delete && !contains_space(&after, space) {
            return Ok(());
        }
        if !delete
            && display_row(&after, display)
                .and_then(|d| d.pointer("/Current Space/ManagedSpaceID"))
                .and_then(Value::as_u64)
                == Some(space)
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "workspace_operation_incomplete: {action} postcondition did not verify"
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
