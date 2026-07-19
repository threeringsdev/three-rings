//! Overlay stack — ours (no upstream counterpart; the vendored overlays'
//! per-instance document listeners closed every open overlay on one ESC).
//! Open overlays push their id; ESC handlers act only when their id is on
//! top, so a single keypress closes exactly the topmost overlay.
//!
//! Client-only state: SSR renders overlays closed and never touches this
//! (the hydrate/ssr split mirrors scroll_lock).

#[cfg(feature = "hydrate")]
mod imp {
    use std::cell::RefCell;

    thread_local! {
        static STACK: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    /// Register `id` as the topmost overlay (idempotent: re-push moves it up).
    pub fn push(id: &str) {
        STACK.with(|s| {
            let mut s = s.borrow_mut();
            s.retain(|x| x != id);
            s.push(id.to_string());
        });
    }

    /// Drop `id` from the stack (close or unmount).
    pub fn remove(id: &str) {
        STACK.with(|s| s.borrow_mut().retain(|x| x != id));
    }

    /// Is `id` the topmost open overlay? (ESC gate.)
    pub fn is_top(id: &str) -> bool {
        STACK.with(|s| s.borrow().last().map(|x| x == id).unwrap_or(false))
    }
}

#[cfg(not(feature = "hydrate"))]
mod imp {
    pub fn push(_id: &str) {}
    pub fn remove(_id: &str) {}
    pub fn is_top(_id: &str) -> bool {
        false
    }
}

pub use imp::{is_top, push, remove};
