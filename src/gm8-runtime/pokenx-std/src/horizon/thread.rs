//! `std::thread`: `sleep` is supported. Worker threads are not provided by this shim yet;
//! the Horizon runtime replaces the code that spawned them (the audio pump runs from the host).

use crate::horizon::time::Duration;
use alloc_crate::boxed::Box;
use core::any::Any;

pub fn sleep(duration: Duration) {
    let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;
    // SAFETY: plain call into the host's sleep helper.
    unsafe { super::host::pokenx_host_sleep_ns(nanos) };
}

pub fn yield_now() {
    sleep(Duration::from_nanos(1));
}

pub struct JoinHandle<T>(Option<T>);

impl<T> JoinHandle<T> {
    pub fn join(mut self) -> Result<T, Box<dyn Any + Send + 'static>> {
        Ok(self.0.take().expect("JoinHandle already consumed"))
    }
}

/// Runs the closure to completion on the calling thread and hands back its result. This keeps
/// code that spawns short, joinable work compiling; detached long-running workers must be
/// replaced by the host, not routed through here.
pub fn spawn<F, T>(f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    JoinHandle(Some(f()))
}
