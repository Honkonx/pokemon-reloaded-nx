// SPDX-License-Identifier: GPL-2.0-only
// Layouts consumed by Mesa's pinned aarch64-unknown-linux-gnu Rust libraries.
// Values describe their ABI; native newlib constants must never be substituted.
#pragma once
#include <cstddef>
#include <cstdint>
#include <climits>

namespace pokenx_mesa_abi {
struct Timespec { int64_t seconds; int64_t nanoseconds; };
struct Iovec { void* iov_base; size_t iov_len; };
struct Stat {
    uint64_t device, inode;
    uint32_t mode, links, uid, gid;
    uint64_t rdevice, pad1;
    int64_t size;
    int32_t block_size, pad2;
    int64_t blocks;
    Timespec accessed, modified, changed;
    uint32_t unused[2];
};
static_assert(sizeof(Timespec) == 16);
static_assert(sizeof(Stat) == 128);
static_assert(offsetof(Stat, size) == 48);
static_assert(offsetof(Stat, accessed) == 72);
constexpr int Interrupted = 4, Io = 5, Again = 11, NoMemory = 12, Fault = 14;
constexpr int Invalid = 22, NotImplemented = 38, Overflow = 75, NotSupported = 95, TimedOut = 110;
constexpr int Wait = 0, Wake = 1, WaitBitset = 9, Private = 128, Realtime = 256;
constexpr long FutexCall = 98, YieldCall = 124, GetPidCall = 172, GetTidCall = 178, RandomCall = 278;
constexpr int Create = 1 << 6, Exclusive = 1 << 7, NoCtty = 1 << 8, Truncate = 1 << 9;
constexpr int Append = 1 << 10, Nonblock = 1 << 11, Dsync = 1 << 12;
constexpr int Directory = 1 << 14, NoFollow = 1 << 15, Direct = 1 << 16, LargeFile = 1 << 17;
constexpr int NoAtime = 1 << 18, CloseExec = 1 << 19, Sync = 1 << 20;

// Return a relative Horizon timeout. -1 means infinite; invalid values are
// distinguished from a valid past deadline (0), without signed overflow.
inline bool timeout_ns(const Timespec* value, bool absolute, int64_t now, int64_t& out) {
    if (!value) { out = -1; return true; }
    if (absolute && now < 0) return false;
    if (value->seconds < 0 || value->nanoseconds < 0 || value->nanoseconds >= 1000000000) return false;
    const uint64_t sec = static_cast<uint64_t>(value->seconds);
    if (sec > UINT64_MAX / 1000000000) { out = INT64_MAX; return true; }
    const uint64_t seconds_ns = sec * 1000000000;
    if (static_cast<uint64_t>(value->nanoseconds) > UINT64_MAX - seconds_ns) { out = INT64_MAX; return true; }
    const uint64_t ns = seconds_ns + static_cast<uint64_t>(value->nanoseconds);
    const uint64_t relative = absolute ? (ns <= static_cast<uint64_t>(now) ? 0 : ns - static_cast<uint64_t>(now)) : ns;
    out = relative > INT64_MAX ? INT64_MAX : static_cast<int64_t>(relative);
    return true;
}
}
