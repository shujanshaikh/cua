//! PiP frame-push hook — registered once by `main.rs` when the
//! `--experimental-pip` flag is on argv.
//!
//! The trait + factory live in the `pip-preview` crate so the platform
//! backends can implement them without depending on `cua-driver-core`.
//! What lives here is just the per-process callback that the tool
//! dispatcher uses to push frames after each successful tool call —
//! a thin shim so `tool.rs` doesn't need to know about `pip-preview`
//! directly and we keep the dependency graph one-directional.
//!
//! The PNG bytes pushed through here come from the existing
//! `SCREENSHOT_FN` callback (the same source `screenshot.png` uses in
//! the recording pipeline), so PiP shows exactly what the recorder
//! captures.

use std::sync::OnceLock;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

type SessionPush = Arc<dyn Fn(PipHookFrame) + Send + Sync>;
static SESSION_PUSH: OnceLock<Mutex<HashMap<String, SessionPush>>> = OnceLock::new();

/// Bind the existing preview renderer to one trusted runtime-private session.
/// This hook is host-only; an agent cannot select the preview recipient.
pub fn set_session_pip_push_fn(
    session: String,
    push: impl Fn(PipHookFrame) + Send + Sync + 'static,
) -> Result<(), &'static str> {
    let mut callbacks = SESSION_PUSH.get_or_init(Mutex::default).lock().unwrap();
    // Session end marks its tombstone before acquiring this callback map.
    // Registration therefore cannot resurrect a renderer after teardown.
    if crate::session::is_session_ended(&session) {
        return Err("session ended before preview registration");
    }
    callbacks.insert(session, Arc::new(push));
    Ok(())
}

pub fn clear_session(session: &str) {
    if let Some(push) = SESSION_PUSH.get() {
        push.lock().unwrap().remove(session);
    }
}

pub fn pip_enabled_for(session: Option<&str>, selected: bool) -> bool {
    if selected {
        session.is_some_and(|session| {
            SESSION_PUSH
                .get()
                .is_some_and(|push| push.lock().unwrap().contains_key(session))
        })
    } else {
        pip_enabled()
    }
}

pub fn push_pip_frame_for(session: Option<&str>, selected: bool, frame: PipHookFrame) {
    if selected {
        let push =
            session.and_then(|session| SESSION_PUSH.get()?.lock().ok()?.get(session).cloned());
        if let Some(push) = push {
            push(frame);
        }
    } else {
        push_pip_frame(frame);
    }
}

/// Synthesized per-call frame payload. Kept structurally identical
/// to `pip_preview::PipFrame` — duplicated here to keep `cua-driver-core`
/// from importing `pip-preview` (the dependency would be circular once
/// platform backends pull both crates in).
pub struct PipHookFrame {
    pub png_bytes: Vec<u8>,
    pub action_label: String,
    pub timestamp_ms: u64,
}

type PipPushFnBox = Box<dyn Fn(PipHookFrame) + Send + Sync>;
static PIP_PUSH_FN: OnceLock<PipPushFnBox> = OnceLock::new();

/// Register the platform-side push callback. `main.rs` calls this
/// once after starting the PiP backend.
pub fn set_pip_push_fn(f: impl Fn(PipHookFrame) + Send + Sync + 'static) {
    let _ = PIP_PUSH_FN.set(Box::new(f));
}

/// True when a PiP backend is wired up. Tool dispatcher uses this to
/// skip the screenshot-bytes path when nothing would consume the
/// frame (avoiding wasted capture work in the common --pip-off case).
pub fn pip_enabled() -> bool {
    PIP_PUSH_FN.get().is_some()
}

/// Push a frame to the PiP window. No-op when no backend is registered.
pub fn push_pip_frame(frame: PipHookFrame) {
    if let Some(f) = PIP_PUSH_FN.get() {
        f(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[test]
    fn selected_preview_routes_only_to_its_session_and_clears_on_release() {
        let a = format!("preview-a-{}", uuid::Uuid::new_v4());
        let b = format!("preview-b-{}", uuid::Uuid::new_v4());
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        set_session_pip_push_fn(a.clone(), move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        let frame = || PipHookFrame {
            png_bytes: vec![1],
            action_label: "fixture".into(),
            timestamp_ms: 0,
        };
        push_pip_frame_for(Some(&b), true, frame());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        push_pip_frame_for(Some(&a), true, frame());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        clear_session(&a);
        assert!(!pip_enabled_for(Some(&a), true));
        push_pip_frame_for(Some(&a), true, frame());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
