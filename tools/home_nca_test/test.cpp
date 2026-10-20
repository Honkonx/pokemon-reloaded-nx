// Checks the in-memory NCA builder (app/engine/home_nca.h) with a reader written separately from it: every hash of the
// PFS0 and IVFC trees is recomputed from the bytes of the file, the RomFS is looked up the way the console's RomFS
// reader does (hash table, then name), and the CNMT and the database record are parsed back.
// The header is left unencrypted here (identity cipher) so its fields can be read. No key is needed or used.
#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>

#include "home_nca.h"

using homenca::Bytes;

static int g_failures = 0;
#define CHECK(cond)                                                                  \
    do {                                                                             \
        if (!(cond)) {                                                               \
            std::printf("FAIL %s:%d  %s\n", __FILE__, __LINE__, #cond);              \
            ++g_failures;                                                            \
        }                                                                            \
    } while (0)

// ---- SHA-256 (public-domain style implementation, only for this test) ----------------------------------------------
static uint32_t rotr(uint32_t x, int n) { return (x >> n) | (x << (32 - n)); }
static void sha256(const void* data, size_t size, uint8_t out[32]) {
    static const uint32_t k[64] = {
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
        0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
        0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
        0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
        0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2};
    uint32_t h[8] = {0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};
    std::vector<uint8_t> msg(static_cast<const uint8_t*>(data), static_cast<const uint8_t*>(data) + size);
    msg.push_back(0x80);
    while (msg.size() % 64 != 56) msg.push_back(0);
    const uint64_t bits = static_cast<uint64_t>(size) * 8;
    for (int i = 7; i >= 0; --i) msg.push_back(static_cast<uint8_t>(bits >> (8 * i)));
    for (size_t chunk = 0; chunk < msg.size(); chunk += 64) {
        uint32_t w[64];
        for (int i = 0; i < 16; ++i)
            w[i] = (uint32_t(msg[chunk + 4 * i]) << 24) | (uint32_t(msg[chunk + 4 * i + 1]) << 16) | (uint32_t(msg[chunk + 4 * i + 2]) << 8) | msg[chunk + 4 * i + 3];
        for (int i = 16; i < 64; ++i) {
            const uint32_t s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >> 3);
            const uint32_t s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16] + s0 + w[i - 7] + s1;
        }
        uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4], f = h[5], g = h[6], hh = h[7];
        for (int i = 0; i < 64; ++i) {
            const uint32_t t1 = hh + (rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25)) + ((e & f) ^ (~e & g)) + k[i] + w[i];
            const uint32_t t2 = (rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22)) + ((a & b) ^ (a & c) ^ (b & c));
            hh = g; g = f; f = e; e = d + t1; d = c; c = b; b = a; a = t1 + t2;
        }
        h[0] += a; h[1] += b; h[2] += c; h[3] += d; h[4] += e; h[5] += f; h[6] += g; h[7] += hh;
    }
    for (int i = 0; i < 8; ++i)
        for (int j = 0; j < 4; ++j) out[4 * i + j] = static_cast<uint8_t>(h[i] >> (24 - 8 * j));
}
static std::string hex(const uint8_t* p, size_t n) {
    std::string s;
    char b[3];
    for (size_t i = 0; i < n; ++i) { std::snprintf(b, sizeof(b), "%02x", p[i]); s += b; }
    return s;
}
static void identity_cipher(uint8_t*) {}

// ---- little-endian readers --------------------------------------------------------------------------------------------
static uint32_t rd32(const uint8_t* p) { return p[0] | (p[1] << 8) | (p[2] << 16) | (uint32_t(p[3]) << 24); }
static uint64_t rd64(const uint8_t* p) { return rd32(p) | (uint64_t(rd32(p + 4)) << 32); }
static uint16_t rd16(const uint8_t* p) { return static_cast<uint16_t>(p[0] | (p[1] << 8)); }

static bool digest_equals(const uint8_t* data, size_t size, const uint8_t* expected) {
    uint8_t digest[32];
    sha256(data, size, digest);
    return std::memcmp(digest, expected, 32) == 0;
}

// ---- RomFS reader (the console's lookup: hash the name with its parent, walk the hash chain) -------------------------
static bool romfs_find(const Bytes& romfs, const std::string& name, Bytes& out) {
    const uint64_t dir_hash_off = rd64(&romfs[8]), dir_table_off = rd64(&romfs[0x18]);
    const uint64_t file_hash_off = rd64(&romfs[0x28]), file_hash_size = rd64(&romfs[0x30]), file_table_off = rd64(&romfs[0x38]), data_off = rd64(&romfs[0x48]);
    CHECK(rd64(&romfs[0]) == 0x50);
    CHECK(rd32(&romfs[dir_table_off + 0xC]) != 0xFFFFFFFFu || name.empty());  // the root lists its files
    (void)dir_hash_off;
    const uint32_t buckets = static_cast<uint32_t>(file_hash_size / 4);
    const uint32_t hash = homenca::romfs_path_hash(0, reinterpret_cast<const uint8_t*>(name.data()), name.size()) % buckets;
    uint32_t at = rd32(&romfs[file_hash_off + 4ull * hash]);
    while (at != 0xFFFFFFFFu) {
        const uint8_t* entry = &romfs[file_table_off + at];
        const uint32_t length = rd32(entry + 0x1C);
        if (rd32(entry) == 0 && length == name.size() && std::memcmp(entry + 0x20, name.data(), length) == 0) {
            const uint64_t offset = rd64(entry + 8), size = rd64(entry + 0x10);
            out.assign(romfs.begin() + data_off + offset, romfs.begin() + data_off + offset + size);
            return true;
        }
        at = rd32(entry + 0x18);
    }
    return false;
}

static void test_romfs() {
    for (int count : {1, 2, 5, 25}) {
        std::vector<homenca::File> files;
        for (int i = 0; i < count; ++i) {
            homenca::File file;
            file.name = i == 0 ? "control.nacp" : "icon_" + std::to_string(i) + ".dat";
            file.data.assign(static_cast<size_t>(7 + 100 * i), static_cast<uint8_t>(i + 1));
            files.push_back(file);
        }
        const Bytes romfs = homenca::build_romfs(files);
        for (const auto& file : files) {
            Bytes found;
            CHECK(romfs_find(romfs, file.name, found));
            CHECK(found == file.data);
        }
        Bytes missing;
        CHECK(!romfs_find(romfs, "missing.bin", missing));
        // the sibling chain visits every file once
        const uint64_t file_table_off = rd64(&romfs[0x38]);
        uint32_t at = 0;
        int visited = 0;
        while (at != 0xFFFFFFFFu && visited <= count) {
            ++visited;
            at = rd32(&romfs[file_table_off + at + 4]);
        }
        CHECK(visited == count);
        // data offsets are 0x10-aligned
        for (uint32_t entry = 0, n = 0; n < static_cast<uint32_t>(count); ++n) {
            CHECK(rd64(&romfs[file_table_off + entry + 8]) % 0x10 == 0);
            entry = rd32(&romfs[file_table_off + entry + 4]);
        }
    }
}

static void test_pfs0() {
    const Bytes pfs0 = homenca::build_pfs0({{"main", Bytes(100, 1)}, {"main.npdm", Bytes(33, 2)}});
    CHECK(std::memcmp(pfs0.data(), "PFS0", 4) == 0);
    CHECK(rd32(&pfs0[4]) == 2);
    const uint32_t strings = rd32(&pfs0[8]);
    CHECK(strings % 0x20 == 0);
    const size_t data_start = 16 + 0x18 * 2 + strings;
    CHECK(std::string(reinterpret_cast<const char*>(&pfs0[16 + 0x18 * 2 + rd32(&pfs0[16 + 16])])) == "main");
    CHECK(std::string(reinterpret_cast<const char*>(&pfs0[16 + 0x18 * 2 + rd32(&pfs0[16 + 0x18 + 16])])) == "main.npdm");
    CHECK(rd64(&pfs0[16 + 0x18]) == 100 && rd64(&pfs0[16 + 0x18 + 8]) == 33);
    CHECK(pfs0[data_start] == 1 && pfs0[data_start + 100] == 2 && pfs0.size() == data_start + 133);
}

// Fields of one section as the header declares them.
struct SectionInfo {
    uint64_t start, end;
    const uint8_t* fs;
};

static SectionInfo section(const homenca::Nca& nca, int index) {
    const uint8_t* header = nca.data.data();
    SectionInfo info;
    info.start = uint64_t(rd32(header + 0x240 + 0x10 * index)) * 0x200;
    info.end = uint64_t(rd32(header + 0x240 + 0x10 * index + 4)) * 0x200;
    info.fs = header + 0x400 + 0x200 * index;
    CHECK(rd32(header + 0x240 + 0x10 * index + 8) == 1);
    CHECK(digest_equals(info.fs, 0x200, header + 0x280 + 0x20 * index));  // the header's hash of this fs header
    CHECK(rd16(info.fs) == 2);
    CHECK(info.fs[4] == 1);  // not encrypted
    return info;
}

static void check_header(const homenca::Nca& nca, uint64_t tid, uint8_t content_type) {
    const uint8_t* header = nca.data.data();
    CHECK(std::memcmp(header + 0x200, "NCA3", 4) == 0);
    CHECK(header[0x205] == content_type);
    CHECK(rd64(header + 0x208) == nca.data.size());
    CHECK(rd64(header + 0x210) == tid);
    CHECK(nca.data.size() % 0x200 == 0);
    uint8_t digest[32];
    sha256(nca.data.data(), nca.data.size(), digest);
    CHECK(std::memcmp(digest, nca.hash, 32) == 0 && std::memcmp(digest, nca.content_id, 16) == 0);
}

// Returns the PFS0 stored in a hierarchical-SHA256 section after checking every hash.
static Bytes check_pfs0_section(const homenca::Nca& nca, int index, uint32_t block) {
    const SectionInfo info = section(nca, index);
    CHECK(info.fs[2] == 1 && info.fs[3] == 2);
    CHECK(rd32(info.fs + 8 + 0x20) == block && rd32(info.fs + 8 + 0x24) == 2);
    const uint64_t hash_offset = rd64(info.fs + 8 + 0x28), hash_size = rd64(info.fs + 8 + 0x30);
    const uint64_t data_offset = rd64(info.fs + 8 + 0x38), data_size = rd64(info.fs + 8 + 0x40);
    const uint8_t* base = nca.data.data() + info.start;
    CHECK(digest_equals(base + hash_offset, hash_size, info.fs + 8));  // master hash
    CHECK(data_offset % 0x200 == 0 && data_offset >= hash_offset + hash_size);
    CHECK(hash_size == 32 * ((data_size + block - 1) / block));
    for (uint64_t at = 0, i = 0; at < data_size; at += block, ++i) {
        const uint64_t size = data_size - at < block ? data_size - at : block;
        CHECK(digest_equals(base + data_offset + at, size, base + hash_offset + 32 * i));
    }
    CHECK(info.start + data_offset + data_size <= info.end && info.end - (info.start + data_offset + data_size) < 0x200);
    return Bytes(base + data_offset, base + data_offset + data_size);
}

// Returns the RomFS stored in an IVFC section after checking every level.
static Bytes check_romfs_section(const homenca::Nca& nca, int index) {
    const SectionInfo info = section(nca, index);
    CHECK(info.fs[2] == 0 && info.fs[3] == 3);
    const uint8_t* ivfc = info.fs + 8;
    CHECK(rd32(ivfc) == 0x43465649 && rd32(ivfc + 4) == 0x20000 && rd32(ivfc + 8) == 32 && rd32(ivfc + 0xC) == 7);
    const uint8_t* base = nca.data.data() + info.start;
    uint64_t offset[6], size[6];
    for (int i = 0; i < 6; ++i) {
        offset[i] = rd64(ivfc + 0x10 + 0x18 * i);
        size[i] = rd64(ivfc + 0x10 + 0x18 * i + 8);
        CHECK(rd32(ivfc + 0x10 + 0x18 * i + 0x10) == 14);
    }
    CHECK(offset[0] == 0);
    for (int i = 1; i < 6; ++i) CHECK(offset[i] == offset[i - 1] + size[i - 1]);
    CHECK(digest_equals(base, size[0], ivfc + 0xC0));  // master hash covers level 0
    for (int level = 1; level < 6; ++level) {
        const uint64_t padded = (size[level] + 0x3FFF) & ~uint64_t(0x3FFF);
        for (uint64_t at = 0, i = 0; at < padded; at += 0x4000, ++i) {
            CHECK(digest_equals(base + offset[level] + at, 0x4000, base + offset[level - 1] + 32 * i));
        }
    }
    CHECK(info.start + offset[5] + ((size[5] + 0x3FFF) & ~uint64_t(0x3FFF)) == info.end);
    return Bytes(base + offset[5], base + offset[5] + size[5]);
}

static void test_title() {
    homenca::Crypto crypto{sha256, identity_cipher};
    const uint64_t tid = 0x01007E3A9F2C4000ULL;
    Bytes main_file(86372, 0xAB), npdm(1016, 0xCD);
    for (size_t i = 0; i < main_file.size(); ++i) main_file[i] = static_cast<uint8_t>(i * 31 + 7);
    const Bytes exefs = homenca::build_pfs0({{"main", main_file}, {"main.npdm", npdm}});
    std::vector<homenca::File> program_romfs{{"nextNroPath", Bytes{'s', 'd', 'm', 'c'}}};
    Bytes nacp(0x4000, 0x11), icon(21000, 0x22);
    std::vector<homenca::File> control_romfs{{"control.nacp", nacp}, {"icon_AmericanEnglish.dat", icon}};
    const homenca::Title title = homenca::build_title(crypto, tid, exefs, program_romfs, control_romfs);

    // program: ExeFS then RomFS
    check_header(title.program, tid, homenca::kNcaProgram);
    CHECK(section(title.program, 1).start == section(title.program, 0).end);
    CHECK(section(title.program, 0).start == 0xC00);
    CHECK(check_pfs0_section(title.program, 0, 0x10000) == exefs);
    Bytes found;
    CHECK(romfs_find(check_romfs_section(title.program, 1), "nextNroPath", found) && found == (Bytes{'s', 'd', 'm', 'c'}));
    // control
    check_header(title.control, tid, homenca::kNcaControl);
    const Bytes control = check_romfs_section(title.control, 0);
    CHECK(romfs_find(control, "control.nacp", found) && found == nacp);
    CHECK(romfs_find(control, "icon_AmericanEnglish.dat", found) && found == icon);
    // meta: one .cnmt that names the program and the control
    check_header(title.meta, tid, homenca::kNcaMeta);
    const Bytes meta_pfs0 = check_pfs0_section(title.meta, 0, 0x1000);
    CHECK(rd32(&meta_pfs0[4]) == 1);
    const size_t cnmt_at = 16 + 0x18 + rd32(&meta_pfs0[8]);
    CHECK(std::string(reinterpret_cast<const char*>(&meta_pfs0[16 + 0x18])) == "Application_01007e3a9f2c4000.cnmt");
    const uint8_t* cnmt = &meta_pfs0[cnmt_at];
    CHECK(rd64(&meta_pfs0[16 + 8]) == 0x20 + 0x10 + 2 * 0x38 + 0x20);
    CHECK(rd64(cnmt) == tid && cnmt[0xC] == 0x80 && rd16(cnmt + 0xE) == 0x10 && rd16(cnmt + 0x10) == 2 && rd16(cnmt + 0x12) == 0);
    CHECK(rd64(cnmt + 0x20) == (tid | 0x800));
    const homenca::Nca* contents[2] = {&title.program, &title.control};
    const uint8_t types[2] = {homenca::kNcmProgram, homenca::kNcmControl};
    for (int i = 0; i < 2; ++i) {
        const uint8_t* packaged = cnmt + 0x30 + 0x38 * i;
        CHECK(std::memcmp(packaged, contents[i]->hash, 32) == 0);
        CHECK(std::memcmp(packaged + 32, contents[i]->content_id, 16) == 0);
        CHECK(rd32(packaged + 32 + 0x10) == contents[i]->data.size() && packaged[32 + 0x14] == 0 && packaged[32 + 0x16] == types[i]);
    }
    // database record: header, extended header, meta/program/control infos
    const Bytes& record = title.meta_record;
    CHECK(record.size() == 8 + 0x10 + 3 * 0x18);
    CHECK(rd16(&record[0]) == 0x10 && rd16(&record[2]) == 3 && rd16(&record[4]) == 0);
    CHECK(rd64(&record[8]) == (tid | 0x800));
    CHECK(std::memcmp(&record[0x18], title.meta.content_id, 16) == 0 && record[0x18 + 0x16] == homenca::kNcmMeta);
    CHECK(std::memcmp(&record[0x30], title.program.content_id, 16) == 0 && record[0x30 + 0x16] == homenca::kNcmProgram);
    CHECK(std::memcmp(&record[0x48], title.control.content_id, 16) == 0 && record[0x48 + 0x16] == homenca::kNcmControl);
    CHECK(rd32(&record[0x18 + 0x10]) == title.meta.data.size());
    // distinct files have distinct content ids
    CHECK(std::memcmp(title.program.content_id, title.control.content_id, 16) != 0);
    CHECK(std::memcmp(title.program.content_id, title.meta.content_id, 16) != 0);
    std::printf("program %zu bytes (%s), control %zu bytes (%s), meta %zu bytes (%s)\n", title.program.data.size(), hex(title.program.content_id, 4).c_str(),
                title.control.data.size(), hex(title.control.content_id, 4).c_str(), title.meta.data.size(), hex(title.meta.content_id, 4).c_str());
}

static void test_known_sha256() {
    uint8_t digest[32];
    sha256("abc", 3, digest);
    CHECK(hex(digest, 32) == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

int main() {
    test_known_sha256();
    test_pfs0();
    test_romfs();
    test_title();
    if (g_failures) {
        std::printf("%d check(s) failed\n", g_failures);
        return 1;
    }
    std::printf("home_nca: all checks passed\n");
    return 0;
}
