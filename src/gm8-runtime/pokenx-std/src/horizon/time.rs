//! `std::time` on Horizon: `Duration` from `core`; `Instant` from the ARM generic counter.

use core::{
    ops::{Add, AddAssign, Sub, SubAssign},
    time::Duration as CoreDuration,
};

pub use core::time::Duration;

#[cfg(target_arch = "aarch64")]
fn counter() -> u64 {
    let value: u64;
    // The PHYSICAL count, exactly as libnx's armGetSystemTick reads it (nx/include/switch/arm/counter.h:16). Horizon
    // does not let a program read the virtual count (cntvct_el0): halo-ce-universal measured on hardware that it faults
    // (port/switch/host/host_switch.h:78-89), and that fault would kill the process at the first Instant::now().
    // SAFETY: reading the physical counter is permitted at EL0 on Horizon (libnx does it).
    unsafe { core::arch::asm!("isb", "mrs {}, cntpct_el0", out(reg) value, options(nomem, nostack)) };
    value
}

#[cfg(target_arch = "aarch64")]
fn frequency() -> u64 {
    let value: u64;
    // SAFETY: as above.
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) value, options(nomem, nostack)) };
    value.max(1)
}

// Only the host test build compiles this module off AArch64: nanoseconds from a monotonic clock.
#[cfg(not(target_arch = "aarch64"))]
fn counter() -> u64 {
    static START: real_std::sync::OnceLock<real_std::time::Instant> = real_std::sync::OnceLock::new();
    START.get_or_init(real_std::time::Instant::now).elapsed().as_nanos() as u64
}

#[cfg(not(target_arch = "aarch64"))]
fn frequency() -> u64 {
    1_000_000_000
}

fn ticks_to_duration(ticks: u64) -> Duration {
    let freq = frequency();
    let secs = ticks / freq;
    let nanos = ((ticks % freq) as u128 * 1_000_000_000 / freq as u128) as u32;
    CoreDuration::new(secs, nanos)
}

fn duration_to_ticks(duration: Duration) -> u64 {
    let freq = frequency() as u128;
    (duration.as_nanos() * freq / 1_000_000_000) as u64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(u64);

impl Instant {
    pub fn now() -> Instant {
        Instant(counter())
    }
    pub fn elapsed(&self) -> Duration {
        Instant::now().duration_since(*self)
    }
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        self.checked_duration_since(earlier).unwrap_or_default()
    }
    pub fn checked_duration_since(&self, earlier: Instant) -> Option<Duration> {
        self.0.checked_sub(earlier.0).map(ticks_to_duration)
    }
    pub fn saturating_duration_since(&self, earlier: Instant) -> Duration {
        self.duration_since(earlier)
    }
    pub fn checked_add(&self, duration: Duration) -> Option<Instant> {
        self.0.checked_add(duration_to_ticks(duration)).map(Instant)
    }
    pub fn checked_sub(&self, duration: Duration) -> Option<Instant> {
        self.0.checked_sub(duration_to_ticks(duration)).map(Instant)
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, other: Duration) -> Instant {
        self.checked_add(other).expect("overflow when adding duration to instant")
    }
}
impl AddAssign<Duration> for Instant {
    fn add_assign(&mut self, other: Duration) {
        *self = *self + other;
    }
}
impl Sub<Duration> for Instant {
    type Output = Instant;
    fn sub(self, other: Duration) -> Instant {
        self.checked_sub(other).expect("overflow when subtracting duration from instant")
    }
}
impl SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, other: Duration) {
        *self = *self - other;
    }
}
impl Sub<Instant> for Instant {
    type Output = Duration;
    fn sub(self, other: Instant) -> Duration {
        self.duration_since(other)
    }
}

/// This build has no wall clock: `SystemTime` is monotonic time since boot and exists only so
/// code that measures intervals keeps compiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SystemTime(Duration);

pub const UNIX_EPOCH: SystemTime = SystemTime(Duration::ZERO);

#[derive(Clone, Debug)]
pub struct SystemTimeError(Duration);

impl SystemTime {
    pub const UNIX_EPOCH: SystemTime = UNIX_EPOCH;
    pub fn now() -> SystemTime {
        SystemTime(ticks_to_duration(counter()))
    }
    pub fn duration_since(&self, earlier: SystemTime) -> Result<Duration, SystemTimeError> {
        self.0.checked_sub(earlier.0).ok_or(SystemTimeError(earlier.0 - self.0))
    }
    pub fn elapsed(&self) -> Result<Duration, SystemTimeError> {
        SystemTime::now().duration_since(*self)
    }
}
