// A small QR Code encoder for the menu (byte mode, error correction level M, versions 1-10: up to 213 bytes). It draws the
// official page's address so a phone can open it. Header-only and free of libnx so tools/update_core_test can check it
// against an independent encoder on a PC.
#pragma once

#include <algorithm>
#include <cstdint>
#include <cstdlib>
#include <string>
#include <vector>

namespace qr {

struct Code {
    int size = 0;                  // modules per side; 0 when the text does not fit
    std::vector<uint8_t> modules;  // size*size, 1 = dark
    bool dark(int x, int y) const { return x >= 0 && y >= 0 && x < size && y < size && modules[static_cast<size_t>(y) * size + x] != 0; }
};

namespace detail {

constexpr int kMaxVersion = 10;
constexpr int kEccPerBlock[kMaxVersion + 1] = {0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26};  // level M
constexpr int kBlocks[kMaxVersion + 1] = {0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5};                  // level M

inline int raw_modules(int version) {
    int result = (16 * version + 128) * version + 64;
    if (version >= 2) {
        const int aligns = version / 7 + 2;
        result -= (25 * aligns - 10) * aligns - 55;
        if (version >= 7) result -= 36;
    }
    return result;
}

inline int data_codewords(int version) { return raw_modules(version) / 8 - kEccPerBlock[version] * kBlocks[version]; }

inline std::vector<int> alignment_positions(int version) {
    if (version == 1) return {};
    const int aligns = version / 7 + 2;
    const int size = version * 4 + 17;
    const int step = version == 32 ? 26 : (version * 4 + aligns * 2 + 1) / (aligns * 2 - 2) * 2;
    std::vector<int> positions(static_cast<size_t>(aligns));
    positions[0] = 6;
    for (int i = aligns - 1, pos = size - 7; i >= 1; --i, pos -= step) positions[static_cast<size_t>(i)] = pos;
    return positions;
}

inline uint8_t gf_multiply(uint8_t x, uint8_t y) {
    int z = 0;
    for (int i = 7; i >= 0; --i) {
        z = (z << 1) ^ ((z >> 7) * 0x11D);
        z ^= ((y >> i) & 1) * x;
    }
    return static_cast<uint8_t>(z);
}

inline std::vector<uint8_t> rs_divisor(int degree) {
    std::vector<uint8_t> result(static_cast<size_t>(degree), 0);
    result[static_cast<size_t>(degree) - 1] = 1;
    uint8_t root = 1;
    for (int i = 0; i < degree; ++i) {
        for (int j = 0; j < degree; ++j) {
            result[static_cast<size_t>(j)] = gf_multiply(result[static_cast<size_t>(j)], root);
            if (j + 1 < degree) result[static_cast<size_t>(j)] ^= result[static_cast<size_t>(j) + 1];
        }
        root = gf_multiply(root, 0x02);
    }
    return result;
}

inline std::vector<uint8_t> rs_remainder(const std::vector<uint8_t>& data, const std::vector<uint8_t>& divisor) {
    std::vector<uint8_t> result(divisor.size(), 0);
    for (uint8_t b : data) {
        const uint8_t factor = b ^ result[0];
        result.erase(result.begin());
        result.push_back(0);
        for (size_t i = 0; i < result.size(); ++i) result[i] ^= gf_multiply(divisor[i], factor);
    }
    return result;
}

struct Builder {
    int version, size;
    std::vector<uint8_t> modules, is_function;
    Builder(int v) : version(v), size(v * 4 + 17), modules(static_cast<size_t>(size) * size, 0), is_function(static_cast<size_t>(size) * size, 0) {}
    void set(int x, int y, bool dark) {
        modules[static_cast<size_t>(y) * size + x] = dark;
        is_function[static_cast<size_t>(y) * size + x] = 1;
    }
    bool get(int x, int y) const { return modules[static_cast<size_t>(y) * size + x] != 0; }
    bool func(int x, int y) const { return is_function[static_cast<size_t>(y) * size + x] != 0; }

    void finder(int cx, int cy) {
        for (int dy = -4; dy <= 4; ++dy)
            for (int dx = -4; dx <= 4; ++dx) {
                const int dist = std::max(std::abs(dx), std::abs(dy));
                const int x = cx + dx, y = cy + dy;
                if (x >= 0 && x < size && y >= 0 && y < size) set(x, y, dist != 2 && dist != 4);
            }
    }
    void alignment(int cx, int cy) {
        for (int dy = -2; dy <= 2; ++dy)
            for (int dx = -2; dx <= 2; ++dx) set(cx + dx, cy + dy, std::max(std::abs(dx), std::abs(dy)) != 1);
    }
    void format_bits(int mask) {
        const int data = (0 << 3) | mask;  // level M = 0
        int rem = data;
        for (int i = 0; i < 10; ++i) rem = (rem << 1) ^ ((rem >> 9) * 0x537);
        const int bits = ((data << 10) | rem) ^ 0x5412;
        const auto bit = [&](int i) { return ((bits >> i) & 1) != 0; };
        for (int i = 0; i <= 5; ++i) set(8, i, bit(i));
        set(8, 7, bit(6));
        set(8, 8, bit(7));
        set(7, 8, bit(8));
        for (int i = 9; i < 15; ++i) set(14 - i, 8, bit(i));
        for (int i = 0; i < 8; ++i) set(size - 1 - i, 8, bit(i));
        for (int i = 8; i < 15; ++i) set(8, size - 15 + i, bit(i));
        set(8, size - 8, true);
    }
    void draw_function_patterns() {
        for (int i = 0; i < size; ++i) {
            set(6, i, i % 2 == 0);
            set(i, 6, i % 2 == 0);
        }
        finder(3, 3);
        finder(size - 4, 3);
        finder(3, size - 4);
        const std::vector<int> positions = alignment_positions(version);
        const int count = static_cast<int>(positions.size());
        for (int i = 0; i < count; ++i)
            for (int j = 0; j < count; ++j)
                if (!((i == 0 && j == 0) || (i == 0 && j == count - 1) || (i == count - 1 && j == 0)))
                    alignment(positions[static_cast<size_t>(i)], positions[static_cast<size_t>(j)]);
        format_bits(0);
        if (version >= 7) {
            int rem = version;
            for (int i = 0; i < 12; ++i) rem = (rem << 1) ^ ((rem >> 11) * 0x1F25);
            const long bits = (static_cast<long>(version) << 12) | rem;
            for (int i = 0; i < 18; ++i) {
                const bool bit = ((bits >> i) & 1) != 0;
                const int a = size - 11 + i % 3, b = i / 3;
                set(a, b, bit);
                set(b, a, bit);
            }
        }
    }
    void place_data(const std::vector<uint8_t>& data) {
        size_t i = 0;
        for (int right = size - 1; right >= 1; right -= 2) {
            if (right == 6) right = 5;
            for (int vert = 0; vert < size; ++vert)
                for (int j = 0; j < 2; ++j) {
                    const int x = right - j;
                    const bool upward = ((right + 1) & 2) == 0;
                    const int y = upward ? size - 1 - vert : vert;
                    if (!func(x, y) && i < data.size() * 8) {
                        modules[static_cast<size_t>(y) * size + x] = ((data[i >> 3] >> (7 - (i & 7))) & 1) != 0;
                        ++i;
                    }
                }
        }
    }
    void apply_mask(int mask) {
        for (int y = 0; y < size; ++y)
            for (int x = 0; x < size; ++x) {
                bool invert = false;
                switch (mask) {
                    case 0: invert = (x + y) % 2 == 0; break;
                    case 1: invert = y % 2 == 0; break;
                    case 2: invert = x % 3 == 0; break;
                    case 3: invert = (x + y) % 3 == 0; break;
                    case 4: invert = (x / 3 + y / 2) % 2 == 0; break;
                    case 5: invert = x * y % 2 + x * y % 3 == 0; break;
                    case 6: invert = (x * y % 2 + x * y % 3) % 2 == 0; break;
                    default: invert = ((x + y) % 2 + x * y % 3) % 2 == 0; break;
                }
                if (!func(x, y) && invert) modules[static_cast<size_t>(y) * size + x] ^= 1;
            }
    }
    // The standard four penalty rules (runs, 2x2 blocks, finder-like patterns, dark balance).
    long penalty() const {
        long total = 0;
        for (int pass = 0; pass < 2; ++pass)
            for (int a = 0; a < size; ++a) {
                int run = 1;
                for (int b = 1; b < size; ++b) {
                    const bool same = (pass == 0 ? get(b, a) == get(b - 1, a) : get(a, b) == get(a, b - 1));
                    if (same) {
                        ++run;
                        if (run == 5) total += 3;
                        else if (run > 5) total += 1;
                    } else {
                        run = 1;
                    }
                }
                static const int pattern[] = {1, 0, 1, 1, 1, 0, 1};
                for (int b = 0; b + 7 <= size; ++b) {
                    bool match = true;
                    for (int k = 0; k < 7 && match; ++k) match = (pass == 0 ? get(b + k, a) : get(a, b + k)) == (pattern[k] != 0);
                    if (!match) continue;
                    bool before = b >= 4, after = b + 7 + 4 <= size;
                    for (int k = 1; k <= 4 && (before || after); ++k) {
                        if (before && b - k >= 0 && (pass == 0 ? get(b - k, a) : get(a, b - k))) before = false;
                        if (after && b + 6 + k < size && (pass == 0 ? get(b + 6 + k, a) : get(a, b + 6 + k))) after = false;
                    }
                    if (before || after) total += 40;
                }
            }
        for (int y = 0; y + 1 < size; ++y)
            for (int x = 0; x + 1 < size; ++x) {
                const bool c = get(x, y);
                if (c == get(x + 1, y) && c == get(x, y + 1) && c == get(x + 1, y + 1)) total += 3;
            }
        long dark = 0;
        for (uint8_t m : modules) dark += m;
        const long count = static_cast<long>(size) * size;
        const long k = (std::labs(dark * 20 - count * 10) + count - 1) / count - 1;
        total += (k > 0 ? k : 0) * 10;
        return total;
    }
};

}  // namespace detail

// Encodes `text` (bytes). `forced_mask` 0-7 picks a mask (for tests); -1 chooses the best one.
inline Code encode(const std::string& text, int forced_mask = -1) {
    using namespace detail;
    int version = 1;
    for (; version <= kMaxVersion; ++version) {
        const int count_bits = version <= 9 ? 8 : 16;
        if (4 + count_bits + 8 * static_cast<int>(text.size()) <= data_codewords(version) * 8) break;
    }
    if (version > kMaxVersion || text.size() > 255) return {};
    // data bits
    std::vector<uint8_t> bits;
    const auto put = [&](uint32_t value, int count) {
        for (int i = count - 1; i >= 0; --i) bits.push_back(static_cast<uint8_t>((value >> i) & 1));
    };
    put(0x4, 4);
    put(static_cast<uint32_t>(text.size()), version <= 9 ? 8 : 16);
    for (unsigned char c : text) put(c, 8);
    const size_t capacity = static_cast<size_t>(data_codewords(version)) * 8;
    for (int i = 0; i < 4 && bits.size() < capacity; ++i) bits.push_back(0);
    while (bits.size() % 8) bits.push_back(0);
    std::vector<uint8_t> data(bits.size() / 8, 0);
    for (size_t i = 0; i < bits.size(); ++i) data[i >> 3] |= static_cast<uint8_t>(bits[i] << (7 - (i & 7)));
    for (uint8_t pad = 0xEC; data.size() < static_cast<size_t>(data_codewords(version)); pad ^= 0xEC ^ 0x11) data.push_back(pad);
    // error correction and interleaving
    const int blocks = kBlocks[version], ecc_len = kEccPerBlock[version];
    const int raw_codewords = raw_modules(version) / 8;
    const int short_blocks = blocks - raw_codewords % blocks;
    const int short_len = raw_codewords / blocks;
    const std::vector<uint8_t> divisor = rs_divisor(ecc_len);
    std::vector<std::vector<uint8_t>> block_data, block_ecc;
    size_t at = 0;
    for (int i = 0; i < blocks; ++i) {
        const size_t length = static_cast<size_t>(short_len - ecc_len + (i < short_blocks ? 0 : 1));
        std::vector<uint8_t> chunk(data.begin() + static_cast<long>(at), data.begin() + static_cast<long>(at + length));
        at += length;
        block_ecc.push_back(rs_remainder(chunk, divisor));
        block_data.push_back(chunk);
    }
    std::vector<uint8_t> all;
    for (size_t i = 0; i < block_data.back().size(); ++i)
        for (int b = 0; b < blocks; ++b)
            if (i < block_data[static_cast<size_t>(b)].size()) all.push_back(block_data[static_cast<size_t>(b)][i]);
    for (size_t i = 0; i < static_cast<size_t>(ecc_len); ++i)
        for (int b = 0; b < blocks; ++b) all.push_back(block_ecc[static_cast<size_t>(b)][i]);
    // matrix
    Builder base(version);
    base.draw_function_patterns();
    base.place_data(all);
    int best_mask = forced_mask, chosen = 0;
    long best_penalty = -1;
    for (int mask = 0; mask < 8; ++mask) {
        if (forced_mask >= 0 && mask != forced_mask) continue;
        Builder trial = base;
        trial.apply_mask(mask);
        trial.format_bits(mask);
        const long penalty = trial.penalty();
        if (best_penalty < 0 || penalty < best_penalty) {
            best_penalty = penalty;
            chosen = mask;
        }
    }
    if (best_mask < 0) best_mask = chosen;
    base.apply_mask(best_mask);
    base.format_bits(best_mask);
    Code code;
    code.size = base.size;
    code.modules = base.modules;
    return code;
}

}  // namespace qr
