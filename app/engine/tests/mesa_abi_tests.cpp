// SPDX-License-Identifier: GPL-2.0-only
// Native host tests for fixed ARM64 ABI and deadline arithmetic. No NRO/emulator.
#include "../mesa_linux_abi.h"
#include <array>
#include <cstdio>
#include <cstring>

#define CHECK(value) do { if (!(value)) { std::fprintf(stderr, "ABI check failed at line %d\n", __LINE__); return 1; } } while (0)

int main() {
    namespace abi = pokenx_mesa_abi;
    // Verify Linux UAPI byte positions independently of native struct stat.
    abi::Stat metadata{};
    metadata.size = INT64_C(5) << 30;
    metadata.inode = UINT64_C(0x1122334455667788);
    metadata.mode = 0100644;
    metadata.accessed = {-7, 123};
    std::array<unsigned char, 160> bytes{};
    bytes.fill(0xA5);
    std::memcpy(bytes.data() + 16, &metadata, sizeof(metadata));
    auto read64 = [&](size_t offset) {
        uint64_t result = 0;
        for (int i = 7; i >= 0; --i) result = (result << 8) | bytes[16 + offset + i];
        return result;
    };
    CHECK(read64(8) == UINT64_C(0x1122334455667788));
    CHECK(read64(48) == UINT64_C(5368709120));
    CHECK(read64(72) == static_cast<uint64_t>(-7));
    CHECK(read64(80) == 123);
    for (size_t i = 0; i < 16; ++i) CHECK(bytes[i] == 0xA5 && bytes[144 + i] == 0xA5);
    int64_t timeout = 99;
    CHECK(abi::timeout_ns(nullptr, true, 0, timeout) && timeout == -1);
    abi::Timespec t{2, 50};
    CHECK(abi::timeout_ns(&t, false, 999, timeout) && timeout == 2000000050);
    CHECK(abi::timeout_ns(&t, true, 2000000000, timeout) && timeout == 50);
    CHECK(abi::timeout_ns(&t, true, 3000000000, timeout) && timeout == 0);
    CHECK(!abi::timeout_ns(&t, true, -1, timeout));
    t = {-1, 0}; CHECK(!abi::timeout_ns(&t, false, 0, timeout));
    t = {0, 1000000000}; CHECK(!abi::timeout_ns(&t, false, 0, timeout));
    t = {INT64_MAX, 0}; CHECK(abi::timeout_ns(&t, false, 0, timeout) && timeout == INT64_MAX);
    t = {9223372037, 0};
    CHECK(abi::timeout_ns(&t, true, INT64_MAX, timeout) && timeout == 145224193);
    std::puts("Mesa ABI: stat128 (>4GiB), sentinels, deadlines/overflow passed; no Horizon services executed.");
    return 0;
}
