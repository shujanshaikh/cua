//! Private SkyLight Space operations. Symbol availability is not capability
//! evidence: every mutation checks a fresh WindowServer postcondition.

use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
use serde_json::Value;
use std::ffi::c_void;

unsafe fn symbol<T: Copy>(name: &[u8]) -> Result<T, String> {
    crate::input::skylight::find_sym(name)
        .map(|pointer| std::mem::transmute_copy(&pointer))
        .ok_or_else(|| {
            "workspace_operation_unsupported: required private SkyLight symbol is unavailable"
                .into()
        })
}

fn connection() -> Result<u32, String> {
    crate::input::skylight::main_connection_id()
        .filter(|id| *id != 0)
        .ok_or_else(|| "workspace_operation_unsupported: no WindowServer connection".into())
}

/// Read managed displays without enumerating application titles or content.
pub fn managed_displays() -> Result<Value, String> {
    type CopyDisplays = unsafe extern "C" fn(u32) -> *const c_void;
    let copy: CopyDisplays = unsafe { symbol(b"SLSCopyManagedDisplaySpaces\0")? };
    let raw = unsafe { copy(connection()?) };
    if raw.is_null() {
        return Err("workspace_state_unavailable: display query returned null".into());
    }
    let value = unsafe { CFType::wrap_under_create_rule(raw) };
    unsafe fn json(value: &CFType) -> Value {
        if let Some(value) = value.downcast::<CFString>() {
            return Value::String(value.to_string());
        }
        if let Some(value) = value.downcast::<CFNumber>() {
            return value.to_i64().map(Value::from).unwrap_or(Value::Null);
        }
        if let Some(value) = value.downcast::<CFArray>() {
            return Value::Array(
                value
                    .iter()
                    .map(|value| json(&CFType::wrap_under_get_rule(*value)))
                    .collect(),
            );
        }
        if let Some(value) = value.downcast::<CFDictionary>() {
            let (keys, values) = value.get_keys_and_values();
            return Value::Object(
                keys.into_iter()
                    .zip(values)
                    .filter_map(|(key, value)| {
                        let key = CFType::wrap_under_get_rule(key).downcast::<CFString>()?;
                        Some((key.to_string(), json(&CFType::wrap_under_get_rule(value))))
                    })
                    .collect(),
            );
        }
        Value::Null
    }
    let raw = unsafe { json(&value) };
    Ok(Value::Array(raw.as_array().ok_or("workspace_state_unavailable: displays are not an array")?.iter().map(|display| {
        serde_json::json!({
            "Display Identifier": display.get("Display Identifier"),
            "Current Space": {"ManagedSpaceID": display.pointer("/Current Space/ManagedSpaceID")},
            "Spaces": display.get("Spaces").and_then(Value::as_array).map(|spaces| spaces.iter().map(|space| serde_json::json!({"ManagedSpaceID":space.get("ManagedSpaceID"), "type":space.get("type")})).collect::<Vec<_>>()).unwrap_or_default()
        })
    }).collect()))
}

pub fn membership(window_id: u32) -> Result<Vec<u64>, String> {
    crate::input::skylight::SpaceQuery::new()
        .and_then(|query| query.window_space_ids(window_id))
        .ok_or_else(|| "workspace_state_unavailable: window membership is unknown".into())
}

/// Attempt creation without activating the Space. A raw, unmanaged server
/// Space is not a Mission Control desktop and must never be reported as one.
pub fn create() -> Result<u64, String> {
    type Create = unsafe extern "C" fn(u32, *const c_void, *const c_void) -> u64;
    type Destroy = unsafe extern "C" fn(u32, u64);
    let create: Create = unsafe { symbol(b"SLSSpaceCreate\0")? };
    // Resolve cleanup before allocating anything.
    let destroy: Destroy = unsafe { symbol(b"SLSSpaceDestroy\0")? };
    let before = managed_displays()?;
    let options = CFDictionary::from_CFType_pairs(&[
        (CFString::new("type"), CFNumber::from(0_i32).as_CFType()),
        (
            CFString::new("uuid"),
            CFString::new(&uuid::Uuid::new_v4().to_string()).as_CFType(),
        ),
    ]);
    let cid = connection()?;
    let id = unsafe { create(cid, std::ptr::null(), options.as_concrete_TypeRef().cast()) };
    if id == 0 {
        return Err("workspace_operation_unsupported: SLSSpaceCreate returned zero".into());
    }
    let after = managed_displays();
    if after.as_ref().is_ok_and(|after| contains_space(after, id)) && !contains_space(&before, id) {
        return Ok(id);
    }
    // Only the resource returned by our own allocation is eligible for rollback.
    if !contains_space(&before, id) {
        unsafe {
            destroy(cid, id);
        }
    }
    Err(format!("workspace_operation_unsupported: SLSSpaceCreate did not create a managed Mission Control Space; raw_server_space_id={id}; rollback requested; cleanup is unverified"))
}

pub fn contains_space(displays: &Value, id: u64) -> bool {
    displays.as_array().is_some_and(|displays| {
        displays.iter().any(|display| {
            display
                .get("Spaces")
                .and_then(Value::as_array)
                .is_some_and(|spaces| {
                    spaces.iter().any(|space| {
                        space.get("ManagedSpaceID").and_then(Value::as_u64) == Some(id)
                    })
                })
        })
    })
}

fn wait_for_membership(
    window_id: u32,
    predicate: impl Fn(&[u64]) -> bool,
) -> Result<Vec<u64>, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        let observed = membership(window_id)?;
        if predicate(&observed) || std::time::Instant::now() >= deadline {
            return Ok(observed);
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// Adapted from #2429 by Francesco Bonacci and injaneity. Add before removing
/// the old membership, because these private calls can silently do nothing.
pub fn move_window(window_id: u32, target: u64) -> Result<Vec<u64>, String> {
    type Change = unsafe extern "C" fn(u32, *const c_void, *const c_void);
    let displays = managed_displays()?;
    if !contains_space(&displays, target) {
        return Err("workspace_space_deleted: destination no longer exists".into());
    }
    let old = membership(window_id)?;
    if old == [target] {
        return Ok(old);
    }
    if old.len() != 1 {
        return Err(
            "workspace_operation_unsupported: ambiguous or sticky window membership".into(),
        );
    }
    type ManagedMove = unsafe extern "C" fn(u32, *const c_void, u64);
    let windows = CFArray::from_CFTypes(&[CFNumber::from(i64::from(window_id))]);
    if let Ok(managed_move) = unsafe { symbol::<ManagedMove>(b"SLSMoveWindowsToManagedSpace\0") } {
        unsafe {
            managed_move(connection()?, windows.as_concrete_TypeRef().cast(), target);
        }
        if wait_for_membership(window_id, |ids| ids == [target])? == [target] {
            return Ok(old);
        }
    }
    let add: Change = unsafe { symbol(b"SLSAddWindowsToSpaces\0")? };
    let remove: Change = unsafe { symbol(b"SLSRemoveWindowsFromSpaces\0")? };
    let windows = CFArray::from_CFTypes(&[CFNumber::from(i64::from(window_id))]);
    let targets = CFArray::from_CFTypes(&[CFNumber::from(
        i64::try_from(target).map_err(|_| "invalid Space id")?,
    )]);
    let cid = connection()?;
    unsafe {
        add(
            cid,
            windows.as_concrete_TypeRef().cast(),
            targets.as_concrete_TypeRef().cast(),
        );
    }
    if !wait_for_membership(window_id, |ids| ids.contains(&target))?.contains(&target) {
        return Err("workspace_operation_unsupported: SkyLight managed move and add did not establish destination membership".into());
    }
    let originals = CFArray::from_CFTypes(&[CFNumber::from(old[0] as i64)]);
    unsafe {
        remove(
            cid,
            windows.as_concrete_TypeRef().cast(),
            originals.as_concrete_TypeRef().cast(),
        );
    }
    let observed = wait_for_membership(window_id, |ids| ids == [target])?;
    if observed != [target] {
        // Keep partial state visible; restoration is an explicit caller operation.
        return Err(format!(
            "workspace_move_incomplete: observed membership {observed:?}"
        ));
    }
    Ok(old)
}

/// The shared manager owns session state; this adapter owns only native calls.
pub struct MacosWorkspaces;
impl cua_driver_core::workspace::WorkspaceBackend for MacosWorkspaces {
    fn create(&self) -> Result<u64, String> {
        create()
    }
    fn state(&self, space: u64) -> Result<(bool, bool), String> {
        let displays = managed_displays()?;
        let exists = contains_space(&displays, space);
        let active = displays.as_array().is_some_and(|displays| {
            displays.iter().any(|display| {
                display
                    .pointer("/Current Space/ManagedSpaceID")
                    .and_then(Value::as_u64)
                    == Some(space)
            })
        });
        Ok((exists, active))
    }
    fn membership(
        &self,
        target: cua_driver_core::selected_windows::WindowTarget,
    ) -> Result<Vec<u64>, String> {
        membership(u32::try_from(target.window_id).map_err(|_| "window_id out of range")?)
    }
    fn move_window(
        &self,
        target: cua_driver_core::selected_windows::WindowTarget,
        space: u64,
    ) -> Result<(), String> {
        let pid = i32::try_from(target.pid).map_err(|_| "pid out of range")?;
        let wid = u32::try_from(target.window_id).map_err(|_| "window_id out of range")?;
        if crate::windows::resolve_window_owner(pid, wid) != crate::windows::WindowOwner::SamePid {
            return Err("workspace_window_stale: window owner changed".into());
        }
        let displays = managed_displays()?;
        let original = membership(wid)?;
        let ordinary = |id| {
            displays.as_array().is_some_and(|displays| {
                displays.iter().any(|display| {
                    display
                        .get("Spaces")
                        .and_then(Value::as_array)
                        .is_some_and(|spaces| {
                            spaces.iter().any(|row| {
                                row.get("ManagedSpaceID").and_then(Value::as_u64) == Some(id)
                                    && row.get("type").and_then(Value::as_u64) == Some(0)
                            })
                        })
                })
            })
        };
        if !ordinary(space) || !original.iter().all(|&id| ordinary(id)) {
            return Err("workspace_operation_unsupported: fullscreen or unavailable Space cannot be a movement source or destination".into());
        }
        let active_before: Vec<_> = displays
            .as_array()
            .unwrap()
            .iter()
            .map(|display| display.get("Current Space").cloned())
            .collect();
        move_window(wid, space)?;
        let active_after: Vec<_> = managed_displays()?
            .as_array()
            .ok_or("workspace_state_unavailable")?
            .iter()
            .map(|display| display.get("Current Space").cloned())
            .collect();
        if active_before != active_after {
            return Err("workspace_active_space_changed: display state changed during movement; no corrective switch attempted".into());
        }
        Ok(())
    }
    fn reveal(&self, space: u64) -> Result<(), String> {
        type Reveal = unsafe extern "C" fn(u32, *const c_void, u64);
        let displays = managed_displays()?;
        let display = displays
            .as_array()
            .ok_or("workspace_state_unavailable")?
            .iter()
            .find(|display| contains_space(&Value::Array(vec![(*display).clone()]), space))
            .ok_or("workspace_space_deleted")?;
        let display_id = CFString::new(
            display
                .get("Display Identifier")
                .and_then(Value::as_str)
                .ok_or("workspace_display_unavailable")?,
        );
        let reveal: Reveal = unsafe { symbol(b"SLSManagedDisplaySetCurrentSpace\0")? };
        unsafe {
            reveal(
                connection()?,
                display_id.as_concrete_TypeRef().cast(),
                space,
            );
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            if self.state(space)?.1 {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(
                    "workspace_operation_unsupported: explicit Space switch did not verify".into(),
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
}
