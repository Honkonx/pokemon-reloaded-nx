// SPDX-License-Identifier: GPL-2.0-only
// Native Horizon services for the Linux ABI imports of Mesa's Rust objects.
// Only those imports are renamed at link preparation; C/newlib callers retain
// their own errno values, clock IDs, flags and stat layout.
#include "mesa_linux_abi.h"
#include <switch.h>
#include <cerrno>
#include <cstdarg>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <malloc.h>
#include <pthread.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

// Pinned Rust Linux/AArch64 treats pthread_attr_t as opaque 64-byte, aligned
// to eight bytes. All attribute operations stay in the same native libc.
// Reject an SDK change that would overflow that caller-owned storage.
static_assert(sizeof(pthread_attr_t) <= 64 && alignof(pthread_attr_t) <= 8);
static_assert(sizeof(pthread_t) == sizeof(uint64_t));
static_assert(sizeof(pthread_key_t) == sizeof(uint32_t));

namespace abi = pokenx_mesa_abi;
extern "C" void* __aarch64_read_tp();
extern "C" void pokenx_mesa_bind_thread() {
    // libnx allocates the linked ELF TLS image for each native thread. GCC
    // accesses it through the soft-TP helper; pinned Rust std uses TPIDR_EL0.
    void* base = __aarch64_read_tp();
    __asm__ volatile("msr tpidr_el0, %0" :: "r"(base) : "memory");
}
extern "C" int __real_pthread_create(pthread_t*, const pthread_attr_t*, void* (*)(void*), void*);
namespace {
struct ThreadStart { void* (*entry)(void*); void* argument; };
void* thread_start(void* raw) {
    const ThreadStart start = *static_cast<ThreadStart*>(raw);
    free(raw);
    pokenx_mesa_bind_thread();
    return start.entry(start.argument);
}
thread_local int linux_error = 0;
long fail(int error) { linux_error = error; return -1; }
int native_error(int error) {
    if (error == ENOSYS) return abi::NotImplemented;
    if (error == ETIMEDOUT) return abi::TimedOut;
    if (error == EOVERFLOW) return abi::Overflow;
    if (error == EILSEQ) return 84;
    if (error == ENAMETOOLONG) return 36;
    if (error == ENOTEMPTY) return 39;
    if (error == ELOOP) return 40;
    if (error == EDEADLK) return 35;
    if (error == ENOLCK) return 37;
    if (error == ECANCELED) return 125;
    if (error == ENOMSG) return 42;
    if (error == EIDRM) return 43;
    if (error == EOPNOTSUPP) return abi::NotSupported;
    return error >= 1 && error <= 34 ? error : abi::Io;
}

extern "C" int __wrap_pthread_create(pthread_t* thread, const pthread_attr_t* attr,
                                     void* (*entry)(void*), void* argument) {
    auto* start = static_cast<ThreadStart*>(malloc(sizeof(ThreadStart)));
    if (!start) return ENOMEM;
    *start = {entry, argument};
    const int result = __real_pthread_create(thread, attr, thread_start, start);
    if (result != 0) free(start);
    return result;
}
long native_failure() { return fail(native_error(errno)); }
int clock_id(int id) {
    switch (id) { case 0: return CLOCK_REALTIME; case 1: return CLOCK_MONOTONIC; default: return -1; }
}
long kernel_failure(Result rc) {
    if (R_VALUE(rc) == KERNELRESULT(TimedOut)) return fail(abi::TimedOut);
    if (R_VALUE(rc) == KERNELRESULT(InvalidState)) return fail(abi::Again);
    if (R_VALUE(rc) == KERNELRESULT(Cancelled)) return fail(abi::Interrupted);
    return fail(abi::Invalid);
}
void convert_stat(const struct stat& from, abi::Stat& to) {
    to = {};
    to.device = from.st_dev; to.inode = from.st_ino; to.mode = from.st_mode;
    to.links = from.st_nlink; to.uid = from.st_uid; to.gid = from.st_gid;
    to.rdevice = from.st_rdev; to.size = from.st_size;
    to.block_size = from.st_blksize > INT32_MAX ? INT32_MAX : static_cast<int32_t>(from.st_blksize);
    to.blocks = from.st_blocks;
    to.accessed = {from.st_atim.tv_sec, from.st_atim.tv_nsec};
    to.modified = {from.st_mtim.tv_sec, from.st_mtim.tv_nsec};
    to.changed = {from.st_ctim.tv_sec, from.st_ctim.tv_nsec};
}
}

extern "C" int* pokenx_mesa_linux___errno_location() { return &linux_error; }
extern "C" int pokenx_mesa_linux_clock_gettime(int id, abi::Timespec* out) {
    const int native_id = clock_id(id);
    if (!out) return static_cast<int>(fail(abi::Fault));
    if (native_id < 0) return static_cast<int>(fail(abi::Invalid));
    struct timespec result{};
    if (clock_gettime(native_id, &result) != 0) return static_cast<int>(native_failure());
    *out = {result.tv_sec, result.tv_nsec};
    return 0;
}
extern "C" ssize_t pokenx_mesa_linux_getrandom(void* out, size_t count, unsigned flags) {
    if ((flags & ~7u) != 0) return fail(abi::Invalid);
    if (!out && count) return fail(abi::Fault);
    if (count > static_cast<size_t>(SSIZE_MAX)) return fail(abi::Invalid);
    randomGet(out, count);
    return static_cast<ssize_t>(count);
}
extern "C" long pokenx_mesa_linux_syscall(long number, ...) {
    va_list args; va_start(args, number);
    long result = -1;
    if (number == abi::RandomCall) {
        void* buffer = va_arg(args, void*);
        const size_t size = va_arg(args, size_t);
        const unsigned flags = va_arg(args, unsigned);
        result = pokenx_mesa_linux_getrandom(buffer, size, flags);
    } else if (number == abi::FutexCall) {
        void* address = va_arg(args, void*);
        const int operation = va_arg(args, int);
        const uint32_t value = va_arg(args, uint32_t);
        const int command = operation & ~(abi::Private | abi::Realtime);
        if (!address || (reinterpret_cast<uintptr_t>(address) & 3)) result = fail(abi::Invalid);
        else if (command == abi::Wake) {
            if ((operation & abi::Realtime) || value > INT32_MAX) result = fail(abi::Invalid);
            else if (value == 0) result = 0;
            else {
                const Result rc = svcSignalToAddress(address, SignalType_Signal, 0, static_cast<int32_t>(value));
                // Horizon does not report the number woken. Rust documents a
                // conservative false wake result on some supported platforms.
                result = R_SUCCEEDED(rc) ? 0 : kernel_failure(rc);
            }
        } else if (command == abi::Wait || command == abi::WaitBitset) {
            const abi::Timespec* timeout = va_arg(args, const abi::Timespec*);
            bool full_mask = true;
            if (command == abi::WaitBitset) {
                (void)va_arg(args, void*);
                full_mask = va_arg(args, uint32_t) == UINT32_MAX;
            }
            abi::Timespec now{}; int64_t duration = 0;
            const bool absolute = command == abi::WaitBitset;
            const int selected_clock = operation & abi::Realtime ? 0 : 1;
            if (!full_mask || (command == abi::Wait && (operation & abi::Realtime))) result = fail(abi::Invalid);
            else if (absolute && timeout && pokenx_mesa_linux_clock_gettime(selected_clock, &now) != 0) result = -1;
            else if (!abi::timeout_ns(timeout, absolute, now.seconds * 1000000000 + now.nanoseconds, duration)) result = fail(abi::Invalid);
            else {
                const Result rc = svcWaitForAddress(address, ArbitrationType_WaitIfEqual,
                    static_cast<int32_t>(value), duration);
                result = R_SUCCEEDED(rc) ? 0 : kernel_failure(rc);
            }
        } else result = fail(abi::NotImplemented);
    } else if (number == abi::YieldCall) { svcSleepThread(-1); result = 0; }
    else if (number == abi::GetTidCall || number == abi::GetPidCall) {
        uint64_t id = 0;
        const Result rc = number == abi::GetTidCall ? svcGetThreadId(&id, CUR_THREAD_HANDLE) : svcGetProcessId(&id, CUR_PROCESS_HANDLE);
        result = R_FAILED(rc) ? kernel_failure(rc) : id > INT32_MAX ? fail(abi::Overflow) : static_cast<long>(id);
    } else result = fail(abi::NotImplemented);
    va_end(args);
    return result;
}

extern "C" int pokenx_mesa_linux_open64(const char* path, int flags, ...) {
    constexpr int accepted = 3 | abi::Create | abi::Exclusive | abi::NoCtty | abi::Truncate | abi::Append |
        abi::Nonblock | abi::Dsync | abi::Directory | abi::NoFollow | abi::LargeFile | abi::NoAtime | abi::CloseExec | abi::Sync;
    if (flags & abi::Direct) return static_cast<int>(fail(abi::NotSupported));
    if ((flags & ~accepted) || (flags & 3) == 3) return static_cast<int>(fail(abi::Invalid));
    int native_flags = (flags & 3) == 0 ? O_RDONLY : (flags & 3) == 1 ? O_WRONLY : O_RDWR;
    if (flags & abi::Create) native_flags |= O_CREAT;
    if (flags & abi::Exclusive) native_flags |= O_EXCL;
    if (flags & abi::Truncate) native_flags |= O_TRUNC;
    if (flags & abi::Append) native_flags |= O_APPEND;
    if (flags & abi::Nonblock) native_flags |= O_NONBLOCK;
    if (flags & (abi::Dsync | abi::Sync)) native_flags |= O_SYNC;
    if (flags & abi::NoFollow) {
        struct stat previous{};
        if (lstat(path, &previous) == 0 && S_ISLNK(previous.st_mode)) return static_cast<int>(fail(40));
        // fsdev/SD does not support symlinks. This does not promise a POSIX
        // atomic nofollow open on other device implementations.
    }
    unsigned mode = 0;
    if (flags & abi::Create) { va_list args; va_start(args, flags); mode = va_arg(args, unsigned); va_end(args); }
    const int fd = open(path, native_flags, mode);
    if (fd < 0) return static_cast<int>(native_failure());
    if (flags & abi::Directory) {
        struct stat info{};
        if (fstat(fd, &info) != 0) { const int error = errno; close(fd); return static_cast<int>(fail(native_error(error))); }
        if (!S_ISDIR(info.st_mode)) { close(fd); return static_cast<int>(fail(20)); }
    }
    return fd;
}
extern "C" int pokenx_mesa_linux_fstat64(int fd, abi::Stat* out) {
    if (!out) return static_cast<int>(fail(abi::Fault));
    struct stat info{};
    if (fstat(fd, &info) != 0) return static_cast<int>(native_failure());
    convert_stat(info, *out); return 0;
}
extern "C" int pokenx_mesa_linux_stat64(const char* path, abi::Stat* out) {
    if (!out) return static_cast<int>(fail(abi::Fault));
    struct stat info{};
    if (stat(path, &info) != 0) return static_cast<int>(native_failure());
    convert_stat(info, *out); return 0;
}
extern "C" int64_t pokenx_mesa_linux_lseek64(int fd, int64_t offset, int whence) {
    static_assert(sizeof(off_t) == 8);
    const off_t result = lseek(fd, static_cast<off_t>(offset), whence);
    return result < 0 ? native_failure() : result;
}
extern "C" ssize_t pokenx_mesa_linux_read(int fd, void* out, size_t size) {
    const ssize_t result = read(fd, out, size); return result < 0 ? native_failure() : result;
}
extern "C" ssize_t pokenx_mesa_linux_write(int fd, const void* in, size_t size) {
    const ssize_t result = write(fd, in, size); return result < 0 ? native_failure() : result;
}
extern "C" int pokenx_mesa_linux_close(int fd) { return close(fd) < 0 ? static_cast<int>(native_failure()) : 0; }
extern "C" ssize_t pokenx_mesa_linux_writev(int fd, const abi::Iovec* vectors, int count) {
    if (count < 0 || (!vectors && count)) return fail(abi::Invalid);
    ssize_t total = 0;
    for (int i = 0; i < count; ++i) {
        if (vectors[i].iov_len > static_cast<size_t>(SSIZE_MAX - total)) return total ? total : fail(abi::Invalid);
        const ssize_t written = pokenx_mesa_linux_write(fd, vectors[i].iov_base, vectors[i].iov_len);
        if (written < 0) return total ? total : -1;
        total += written;
        if (static_cast<size_t>(written) != vectors[i].iov_len) break;
    }
    return total;
}
extern "C" int pokenx_mesa_linux_posix_memalign(void** out, size_t align, size_t size) {
    if (!out || align < sizeof(void*) || (align & (align - 1))) return abi::Invalid;
    void* allocation = memalign(align, size);
    if (!allocation) return abi::NoMemory;
    *out = allocation; return 0;
}
extern "C" unsigned long pokenx_mesa_linux_getauxval(unsigned long) { linux_error = 2; return 0; }
extern "C" void* pokenx_mesa_linux_mmap64(void*, size_t, int, int, int, int64_t) {
    fail(abi::NotImplemented); return reinterpret_cast<void*>(-1);
}
extern "C" int pokenx_mesa_linux_munmap(void*, size_t) { return static_cast<int>(fail(abi::NotImplemented)); }
