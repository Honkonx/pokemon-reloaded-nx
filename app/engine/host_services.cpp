// C services the Rust `pokenx-std` shim links against on Horizon (see
// src/gm8-runtime/pokenx-std/src/horizon/host.rs). The struct layouts of
// `struct stat` and `struct dirent` stay on the C side; Rust only sees these
// small stable types.
#include <switch.h>

#include <dirent.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <unistd.h>
#include <fcntl.h>

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <cerrno>

extern "C" {

struct PokenxHostStat {
    uint64_t size;
    uint32_t kind;  // 0 missing, 1 file, 2 directory
};

int pokenx_fs_read_buffer(void* file, size_t size) {
    return file ? setvbuf(static_cast<FILE*>(file), nullptr, _IOFBF, size) : -1;
}

void* pokenx_fs_create_new(const char* path, int readable, int append, int* already_exists) {
    if (!path || !already_exists) { errno = EINVAL; return nullptr; }
    *already_exists = 0;
    const int flags = O_CREAT | O_EXCL | (readable ? O_RDWR : O_WRONLY) | (append ? O_APPEND : 0);
    const int fd = open(path, flags, 0666);
    if (fd < 0) { *already_exists = (errno == EEXIST); return nullptr; }
    FILE* file = fdopen(fd, append ? (readable ? "a+b" : "ab") : (readable ? "w+b" : "wb"));
    if (!file) {
        const int error = errno;
        close(fd);
        unlink(path); // only the empty file exclusively created by this call
        errno = error;
    }
    return file;
}

int pokenx_fs_set_len(void* handle, uint64_t size) {
    static_assert(sizeof(off_t) == 8);
    if (!handle || size > INT64_MAX) { errno = EINVAL; return -1; }
    auto* file = static_cast<FILE*>(handle);
    const off_t position = ftello(file);
    if (position < 0 || fflush(file) != 0) return -1;
    if (ftruncate(fileno(file), static_cast<off_t>(size)) != 0) return -1;
    // Invalidate stdio buffers/EOF and retain the logical cursor, even beyond
    // the shortened EOF. A subsequent write must not resurrect buffered data.
    return fseeko(file, position, SEEK_SET);
}

int pokenx_fs_sync_all(void* handle) {
    if (!handle) { errno = EINVAL; return -1; }
    auto* file = static_cast<FILE*>(handle);
    if (fflush(file) != 0) return -1;
    return fsync(fileno(file));
}

int pokenx_fs_stat(const char* path, PokenxHostStat* out) {
    if (!path || !out) return -1;
    struct stat info;
    out->size = 0;
    out->kind = 0;
    if (stat(path, &info) != 0) return -1;
    out->size = static_cast<uint64_t>(info.st_size);
    if (S_ISDIR(info.st_mode)) {
        out->kind = 2;
    } else if (S_ISREG(info.st_mode)) {
        out->kind = 1;
    }
    return 0;
}

int pokenx_fs_mkdir(const char* path) { return mkdir(path, 0777) == 0 ? 0 : -1; }

// Free bytes on the volume holding `path` (UINT64_MAX when the volume cannot be queried).
uint64_t pokenx_fs_free_bytes(const char* path) {
    struct statvfs info;
    if (!path || statvfs(path, &info) != 0) return UINT64_MAX;
    return static_cast<uint64_t>(info.f_bavail) * static_cast<uint64_t>(info.f_frsize);
}
int pokenx_fs_remove_file(const char* path) { return unlink(path) == 0 ? 0 : -1; }
int pokenx_fs_remove_dir(const char* path) { return rmdir(path) == 0 ? 0 : -1; }
int pokenx_fs_rename(const char* from, const char* to) { return rename(from, to) == 0 ? 0 : -1; }

void* pokenx_fs_opendir(const char* path) { return opendir(path); }

// 1 = entry written, 0 = end of directory, <0 = error.
int pokenx_fs_readdir(void* dir, char* name, size_t capacity, uint32_t* kind) {
    if (!dir || !name || !kind) return -1;
    struct dirent* entry = readdir(static_cast<DIR*>(dir));
    if (!entry) return 0;
    const size_t length = std::strlen(entry->d_name);
    if (length + 1 > capacity) return -1;
    std::memcpy(name, entry->d_name, length + 1);
    *kind = (entry->d_type == DT_DIR) ? 2u : (entry->d_type == DT_REG ? 1u : 0u);
    return 1;
}

void pokenx_fs_closedir(void* dir) {
    if (dir) closedir(static_cast<DIR*>(dir));
}

void pokenx_host_sleep_ns(uint64_t nanoseconds) {
    // A negative value would mean "yield" to the kernel, not a long sleep.
    svcSleepThread(nanoseconds > INT64_MAX ? INT64_MAX : static_cast<s64>(nanoseconds));
}

}  // extern "C"
