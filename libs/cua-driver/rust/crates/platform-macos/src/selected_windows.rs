//! Native lifetime witnesses for trusted selected-window grants.

use crate::ax::bindings::*;
use core_foundation::{
    base::{CFRelease, CFRetain, CFTypeRef, TCFType},
    data::CFData,
};
use cua_driver_core::selected_windows::{WindowIdentity, WindowSelectionBackend, WindowTarget};
use std::sync::Arc;

pub struct MacosWindowSelection;

/// AXWindows can omit windows on inactive Spaces. Reuse only witnesses owned
/// by this dispatch's trusted selection; never recover a new grant here.
pub(crate) unsafe fn copy_retained_windows(pid: i32) -> Vec<AXUIElementRef> {
    let Some(context) = cua_driver_core::tool::current_dispatch_authorization_context() else {
        return vec![];
    };
    let Ok(Some(selection)) = context.selected_windows() else {
        return vec![];
    };
    selection
        .live_targets()
        .into_iter()
        .filter(|target| target.pid == i64::from(pid))
        .filter_map(|target| {
            let witness = selection.native_identity(target).ok()?;
            let identity = (witness.as_ref() as &dyn std::any::Any).downcast_ref::<Identity>()?;
            CFRetain(identity.element as CFTypeRef);
            Some(identity.element as AXUIElementRef)
        })
        .collect()
}

/// Recover an exact, trusted target omitted by AXWindows. The remote token
/// technique is documented by AltTab #1324 and Paneru's bruteforce_windows.
/// Keep it serialized and time-bounded: broad concurrent AX scans can overload
/// the accessibility server. No candidate's content is read or returned.
unsafe fn recover_window(pid: i32, wid: u32) -> Option<AXUIElementRef> {
    use std::{
        sync::Mutex,
        time::{Duration, Instant},
    };
    static RECOVERY: Mutex<()> = Mutex::new(());
    let _guard = RECOVERY.try_lock().ok()?;
    type Create = unsafe extern "C" fn(*const std::ffi::c_void) -> AXUIElementRef;
    let pointer = crate::input::skylight::find_sym(b"_AXUIElementCreateWithRemoteToken\0")?;
    let create: Create = std::mem::transmute(pointer);
    let deadline = Instant::now() + Duration::from_millis(300);
    let mut token = [0_u8; 20];
    token[..4].copy_from_slice(&pid.to_ne_bytes());
    token[8..12].copy_from_slice(&0x636f636f_u32.to_ne_bytes());
    for candidate in 0..32768_u64 {
        if Instant::now() >= deadline {
            break;
        }
        token[12..20].copy_from_slice(&candidate.to_ne_bytes());
        let data = CFData::from_buffer(&token);
        let window = create(data.as_concrete_TypeRef().cast());
        if window.is_null() {
            continue;
        }
        AXUIElementSetMessagingTimeout(window, 0.05);
        let mut actual_pid = 0;
        if ax_get_window_id(window) == Some(wid)
            && AXUIElementGetPid(window, &mut actual_pid) == kAXErrorSuccess
            && actual_pid == pid
            && copy_string_attr(window, "AXRole").as_deref() == Some("AXWindow")
        {
            return Some(window);
        }
        CFRelease(window.cast());
    }
    None
}

struct Identity {
    target: WindowTarget,
    started: (u64, u64),
    // A retained AX remote object survives numeric CGWindowID reuse. All
    // access is serialized by SelectedWindows; CF retain/release is thread safe.
    element: usize,
}

impl Drop for Identity {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.element as CFTypeRef);
        }
    }
}

fn process_start(pid: i32) -> Result<(u64, u64), String> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if read != size as i32 {
        return Err("selected_window_identity_unavailable: process lifetime unavailable".into());
    }
    let info = unsafe { info.assume_init() };
    Ok((info.pbi_start_tvsec, info.pbi_start_tvusec))
}

impl WindowIdentity for Identity {
    fn validate(&self) -> Result<(), String> {
        let pid = self.target.pid as i32;
        if process_start(pid)? != self.started
            || crate::windows::resolve_window_owner(pid, self.target.window_id as u32)
                != crate::windows::WindowOwner::SamePid
        {
            return Err("selected_window_stale: process or window owner changed".into());
        }
        let element = self.element as AXUIElementRef;
        let valid = unsafe {
            copy_string_attr(element, "AXRole").as_deref() == Some("AXWindow")
                && ax_get_window_id(element) == Some(self.target.window_id as u32)
        };
        valid
            .then_some(())
            .ok_or_else(|| "selected_window_stale: retained AX window is no longer live".into())
    }
}

impl WindowSelectionBackend for MacosWindowSelection {
    fn bind(&self, target: WindowTarget) -> Result<Arc<dyn WindowIdentity>, String> {
        let pid = i32::try_from(target.pid).map_err(|_| "pid out of range")?;
        let wid = u32::try_from(target.window_id).map_err(|_| "window_id out of range")?;
        let started = process_start(pid)?;
        if !unsafe { AXIsProcessTrusted() } {
            return Err("selected_window_identity_unavailable: Accessibility permission is required to bind a lifetime witness".into());
        }
        let element = unsafe {
            let app = AXUIElementCreateApplication(pid);
            if app.is_null() {
                return Err(
                    "selected_window_identity_unavailable: AX application unavailable".into(),
                );
            }
            AXUIElementSetMessagingTimeout(app, 1.0);
            let mut exact = None;
            crate::ax::enablement::ensure_chromium_ax_enabled(pid, app);
            for window in copy_ax_windows_raw(app) {
                AXUIElementSetMessagingTimeout(window, 1.0);
                if ax_get_window_id(window) == Some(wid) && exact.is_none() {
                    exact = Some(window as usize);
                } else {
                    CFRelease(window as CFTypeRef);
                }
            }
            CFRelease(app as CFTypeRef);
            exact
                .or_else(|| recover_window(pid, wid).map(|window| window as usize))
                .ok_or("selected_window_identity_unavailable: exact AX window is unresolved")?
        };
        let identity = Arc::new(Identity {
            target,
            started,
            element,
        });
        identity.validate()?;
        Ok(identity)
    }
}
