//! Native lifetime witnesses for trusted selected-window grants.

use crate::ax::bindings::*;
use core_foundation::base::{CFRelease, CFTypeRef};
use cua_driver_core::selected_windows::{WindowIdentity, WindowSelectionBackend, WindowTarget};
use std::sync::Arc;

pub struct MacosWindowSelection;

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
            for window in copy_ax_windows(app) {
                AXUIElementSetMessagingTimeout(window, 1.0);
                if ax_get_window_id(window) == Some(wid) && exact.is_none() {
                    exact = Some(window as usize);
                } else {
                    CFRelease(window as CFTypeRef);
                }
            }
            CFRelease(app as CFTypeRef);
            exact.ok_or("selected_window_identity_unavailable: exact AX window is unresolved")?
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
