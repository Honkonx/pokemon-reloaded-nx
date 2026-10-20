//! `std::sync` for a runtime that is effectively single-threaded plus a few workers:
//! spin locks (no OS futex is needed), `Arc` from `alloc`, and a simple channel.

use alloc_crate::{collections::VecDeque, sync::Arc as AllocArc};
use core::fmt;

pub use alloc_crate::sync::{Arc, Weak};
pub use core::sync::atomic;

/// Poisoning does not exist here, but the type keeps `lock().unwrap()` compiling.
pub struct PoisonError<T>(T);

impl<T> PoisonError<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}
impl<T> fmt::Debug for PoisonError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PoisonError { .. }")
    }
}
impl<T> fmt::Display for PoisonError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("poisoned lock")
    }
}

pub type LockResult<Guard> = Result<Guard, PoisonError<Guard>>;

pub struct Mutex<T: ?Sized>(spin::Mutex<T>);
pub type MutexGuard<'a, T> = spin::MutexGuard<'a, T>;

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self(spin::Mutex::new(value))
    }
    pub fn into_inner(self) -> LockResult<T> {
        Ok(self.0.into_inner())
    }
}

impl<T: ?Sized> Mutex<T> {
    pub fn lock(&self) -> LockResult<MutexGuard<'_, T>> {
        Ok(self.0.lock())
    }
    pub fn try_lock(&self) -> Result<MutexGuard<'_, T>, ()> {
        self.0.try_lock().ok_or(())
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

pub struct RwLock<T: ?Sized>(spin::RwLock<T>);
pub type RwLockReadGuard<'a, T> = spin::RwLockReadGuard<'a, T>;
pub type RwLockWriteGuard<'a, T> = spin::RwLockWriteGuard<'a, T>;

impl<T> RwLock<T> {
    pub const fn new(value: T) -> Self {
        Self(spin::RwLock::new(value))
    }
}

impl<T: ?Sized> RwLock<T> {
    pub fn read(&self) -> LockResult<RwLockReadGuard<'_, T>> {
        Ok(self.0.read())
    }
    pub fn write(&self) -> LockResult<RwLockWriteGuard<'_, T>> {
        Ok(self.0.write())
    }
}

pub struct Once(spin::Once<()>);

impl Once {
    pub const fn new() -> Self {
        Self(spin::Once::new())
    }
    pub fn call_once<F: FnOnce()>(&self, f: F) {
        self.0.call_once(f);
    }
    pub fn is_completed(&self) -> bool {
        self.0.is_completed()
    }
}

/// `std::sync::OnceLock` over `spin::Once`.
pub struct OnceLock<T>(spin::Once<T>);

impl<T> OnceLock<T> {
    pub const fn new() -> Self {
        Self(spin::Once::new())
    }
    pub fn get(&self) -> Option<&T> {
        self.0.get()
    }
    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> &T {
        self.0.call_once(f)
    }
}

impl<T> Default for OnceLock<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub mod mpsc {
    use super::*;
    use crate::horizon::time::Duration;

    struct Shared<T> {
        queue: spin::Mutex<VecDeque<T>>,
    }

    pub struct Sender<T>(AllocArc<Shared<T>>);
    pub struct Receiver<T>(AllocArc<Shared<T>>);

    #[derive(Debug, PartialEq, Eq)]
    pub struct SendError<T>(pub T);
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub struct RecvError;
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub enum TryRecvError {
        Empty,
        Disconnected,
    }
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub enum RecvTimeoutError {
        Timeout,
        Disconnected,
    }

    pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
        let shared = AllocArc::new(Shared { queue: spin::Mutex::new(VecDeque::new()) });
        (Sender(shared.clone()), Receiver(shared))
    }

    impl<T> Clone for Sender<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl<T> Sender<T> {
        pub fn send(&self, value: T) -> Result<(), SendError<T>> {
            // Disconnected when the receiver is the only other holder gone: strong count counts senders too.
            self.0.queue.lock().push_back(value);
            Ok(())
        }
    }

    impl<T> Receiver<T> {
        fn senders_alive(&self) -> bool {
            AllocArc::strong_count(&self.0) > 1
        }

        pub fn try_recv(&self) -> Result<T, TryRecvError> {
            match self.0.queue.lock().pop_front() {
                Some(value) => Ok(value),
                None if self.senders_alive() => Err(TryRecvError::Empty),
                None => Err(TryRecvError::Disconnected),
            }
        }

        pub fn recv(&self) -> Result<T, RecvError> {
            loop {
                match self.try_recv() {
                    Ok(value) => return Ok(value),
                    Err(TryRecvError::Disconnected) => return Err(RecvError),
                    Err(TryRecvError::Empty) => crate::horizon::thread::sleep(Duration::from_micros(200)),
                }
            }
        }

        pub fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
            let start = crate::horizon::time::Instant::now();
            loop {
                match self.try_recv() {
                    Ok(value) => return Ok(value),
                    Err(TryRecvError::Disconnected) => return Err(RecvTimeoutError::Disconnected),
                    Err(TryRecvError::Empty) if start.elapsed() >= timeout => return Err(RecvTimeoutError::Timeout),
                    Err(TryRecvError::Empty) => crate::horizon::thread::sleep(Duration::from_micros(200)),
                }
            }
        }
    }
}
