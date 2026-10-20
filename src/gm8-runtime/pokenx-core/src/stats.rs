//! Running totals the host reads to explain slow moments: how long the game paused itself (`sleep`), how many room changes
//! and how much time they took, how many rooms were compiled ahead during pauses. Only counters; nothing here changes the
//! game. The host (`pokenx_engine_stats`) takes a snapshot every few seconds and logs the difference.

use core::sync::atomic::{AtomicU64, Ordering};

/// Nanoseconds the game spent inside its own `sleep()` calls (no frame is presented meanwhile).
static PAUSED_NS: AtomicU64 = AtomicU64::new(0);
static PAUSES: AtomicU64 = AtomicU64::new(0);
static ROOM_CHANGES: AtomicU64 = AtomicU64::new(0);
static ROOM_NS: AtomicU64 = AtomicU64::new(0);
static ROOM_MAX_NS: AtomicU64 = AtomicU64::new(0);
static WARMED_ROOMS: AtomicU64 = AtomicU64::new(0);
static WARM_NS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub paused_ns: u64,
    pub pauses: u64,
    pub files_read: u64,
    pub bytes_read: u64,
    pub room_changes: u64,
    pub room_ns: u64,
    pub room_max_ns: u64,
    pub warmed_rooms: u64,
    pub warm_ns: u64,
}

/// A `sleep()` that lasted `nanos` (the part spent compiling rooms ahead is included: for the player it is still a pause).
pub fn note_pause(nanos: u64) {
    PAUSED_NS.fetch_add(nanos, Ordering::Relaxed);
    PAUSES.fetch_add(1, Ordering::Relaxed);
}

/// One room change that took `nanos` in `load_room`.
pub fn note_room_change(nanos: u64) {
    ROOM_CHANGES.fetch_add(1, Ordering::Relaxed);
    ROOM_NS.fetch_add(nanos, Ordering::Relaxed);
    ROOM_MAX_NS.fetch_max(nanos, Ordering::Relaxed);
}

/// A room whose code was fully compiled ahead of time, in `nanos`.
pub fn note_warmed(nanos: u64) {
    WARMED_ROOMS.fetch_add(1, Ordering::Relaxed);
    WARM_NS.fetch_add(nanos, Ordering::Relaxed);
}

/// Time spent compiling ahead that did not finish a room (so it is not counted in `note_warmed`).
pub fn note_warm_time(nanos: u64) {
    WARM_NS.fetch_add(nanos, Ordering::Relaxed);
}

/// The longest room change since the last call (the host reports it per window).
pub fn take_room_max() -> u64 {
    ROOM_MAX_NS.swap(0, Ordering::Relaxed)
}

/// Total pause time so far (for subtracting it from an interval that contained pauses).
pub fn paused_ns() -> u64 {
    PAUSED_NS.load(Ordering::Relaxed)
}

pub fn snapshot() -> Snapshot {
    Snapshot {
        paused_ns: PAUSED_NS.load(Ordering::Relaxed),
        pauses: PAUSES.load(Ordering::Relaxed),
        files_read: crate::gml::file::READ_FILES.load(Ordering::Relaxed),
        bytes_read: crate::gml::file::READ_BYTES.load(Ordering::Relaxed),
        room_changes: ROOM_CHANGES.load(Ordering::Relaxed),
        room_ns: ROOM_NS.load(Ordering::Relaxed),
        room_max_ns: ROOM_MAX_NS.load(Ordering::Relaxed),
        warmed_rooms: WARMED_ROOMS.load(Ordering::Relaxed),
        warm_ns: WARM_NS.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_add_up() {
        let before = snapshot();
        note_pause(500_000_000);
        note_pause(200_000_000);
        note_room_change(120_000_000);
        note_room_change(40_000_000);
        note_warmed(30_000_000);
        let after = snapshot();
        assert_eq!(after.paused_ns - before.paused_ns, 700_000_000);
        assert_eq!(after.pauses - before.pauses, 2);
        assert_eq!(after.room_changes - before.room_changes, 2);
        assert_eq!(after.room_ns - before.room_ns, 160_000_000);
        assert!(after.room_max_ns >= 120_000_000);
        assert_eq!(after.warmed_rooms - before.warmed_rooms, 1);
    }
}
