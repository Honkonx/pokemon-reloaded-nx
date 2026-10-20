// Builds, in memory, the three NCA files of a tiny application title (program, control, meta): what the console needs
// to show a PokeNX icon on the HOME menu. No console API is used here; the caller supplies SHA-256 and the header
// cipher, so the layout can be checked on a PC (tools/home_nca_test) without any key.
//
// Formats (written from the public Switchbrew documentation of NCA, PFS0, RomFS/IVFC and CNMT):
//   NCA3 header 0xC00 bytes (AES-128-XTS, sector 0x200) | sections, each aligned to 0x200
//   program NCA: section 0 = ExeFS (PFS0, SHA-256 hash table, 0x10000 blocks), section 1 = RomFS (IVFC, 6 levels of 0x4000)
//   control NCA: section 0 = RomFS (control.nacp + icon)
//   meta NCA:    section 0 = PFS0 with one .cnmt file (0x1000 blocks)
// Sections are stored without encryption (encryption type "none"), so no title key is involved. The two RSA signatures
// of the header are left empty: the console only accepts such a title when signature patches are installed, like any
// homebrew forwarder.
#pragma once

#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace homenca {

using Bytes = std::vector<uint8_t>;

struct Crypto {
    void (*sha256)(const void* data, size_t size, uint8_t out[32]);
    // Encrypts the 0xC00-byte header in place (AES-128-XTS, Nintendo tweak, sectors of 0x200 counted from 0).
    void (*encrypt_header)(uint8_t* header);
};

struct File {
    std::string name;
    Bytes data;
};

struct Nca {
    Bytes data;
    uint8_t content_id[16];  // the first half of the SHA-256 of the whole file; also its file name
    uint8_t hash[32];
};

// NCA content types (the header field) and ncm content types (the CNMT field) are different numberings.
constexpr uint8_t kNcaProgram = 0, kNcaMeta = 1, kNcaControl = 2;
constexpr uint8_t kNcmMeta = 0, kNcmProgram = 1, kNcmControl = 3;
constexpr size_t kHeaderSize = 0xC00;

inline void put16(uint8_t* p, uint16_t v) { p[0] = static_cast<uint8_t>(v); p[1] = static_cast<uint8_t>(v >> 8); }
inline void put32(uint8_t* p, uint32_t v) { for (int i = 0; i < 4; ++i) p[i] = static_cast<uint8_t>(v >> (8 * i)); }
inline void put64(uint8_t* p, uint64_t v) { for (int i = 0; i < 8; ++i) p[i] = static_cast<uint8_t>(v >> (8 * i)); }

inline void pad_to(Bytes& bytes, size_t alignment) {
    const size_t rest = bytes.size() % alignment;
    if (rest) bytes.resize(bytes.size() + alignment - rest, 0);
}

inline void append(Bytes& bytes, const Bytes& more) { bytes.insert(bytes.end(), more.begin(), more.end()); }

// PFS0: header (16) | entries (0x18 each) | names (padded to 0x20) | file data.
inline Bytes build_pfs0(const std::vector<File>& files) {
    Bytes names;
    std::vector<uint32_t> name_offsets;
    for (const File& file : files) {
        name_offsets.push_back(static_cast<uint32_t>(names.size()));
        names.insert(names.end(), file.name.begin(), file.name.end());
        names.push_back(0);
    }
    while (names.size() % 0x20) names.push_back(0);
    Bytes out(16 + 0x18 * files.size() + names.size(), 0);
    std::memcpy(out.data(), "PFS0", 4);
    put32(&out[4], static_cast<uint32_t>(files.size()));
    put32(&out[8], static_cast<uint32_t>(names.size()));
    uint64_t data_offset = 0;
    for (size_t i = 0; i < files.size(); ++i) {
        uint8_t* entry = &out[16 + 0x18 * i];
        put64(entry, data_offset);
        put64(entry + 8, files[i].data.size());
        put32(entry + 16, name_offsets[i]);
        data_offset += files[i].data.size();
    }
    std::memcpy(&out[16 + 0x18 * files.size()], names.data(), names.size());
    for (const File& file : files) append(out, file.data);
    return out;
}

inline uint32_t romfs_hash_count(uint32_t entries) {
    if (entries < 3) return 3;
    if (entries < 19) return entries | 1;
    uint32_t count = entries;
    while (count % 2 == 0 || count % 3 == 0 || count % 5 == 0 || count % 7 == 0 || count % 11 == 0 || count % 13 == 0 || count % 17 == 0) ++count;
    return count;
}

inline uint32_t romfs_path_hash(uint32_t parent, const uint8_t* name, size_t length) {
    uint32_t hash = parent ^ 123456789u;
    for (size_t i = 0; i < length; ++i) {
        hash = (hash >> 5) | (hash << 27);
        hash ^= name[i];
    }
    return hash;
}

// RomFS with every file in the root directory: header (0x50) | file data (from 0x200, 0x10-aligned) | directory hash
// table | directory table | file hash table | file table.
inline Bytes build_romfs(const std::vector<File>& files) {
    const uint32_t kEmpty = 0xFFFFFFFFu;
    const uint32_t count = static_cast<uint32_t>(files.size());
    std::vector<uint32_t> entry_offsets;
    uint32_t file_table_size = 0;
    for (const File& file : files) {
        entry_offsets.push_back(file_table_size);
        file_table_size += 0x20 + static_cast<uint32_t>((file.name.size() + 3) & ~size_t(3));
    }
    std::vector<uint64_t> data_offsets;
    uint64_t data_size = 0;
    for (const File& file : files) {
        data_size = (data_size + 0xF) & ~uint64_t(0xF);
        data_offsets.push_back(data_size);
        data_size += file.data.size();
    }
    const uint32_t file_hash_count = romfs_hash_count(count), dir_hash_count = romfs_hash_count(1);
    std::vector<uint32_t> file_hash(file_hash_count, kEmpty), dir_hash(dir_hash_count, kEmpty);
    Bytes file_table(file_table_size, 0);
    for (uint32_t i = 0; i < count; ++i) {
        uint8_t* entry = &file_table[entry_offsets[i]];
        const File& file = files[i];
        put32(entry, 0);  // parent: the root directory
        put32(entry + 4, i + 1 < count ? entry_offsets[i + 1] : kEmpty);
        put64(entry + 8, data_offsets[i]);
        put64(entry + 0x10, file.data.size());
        const uint32_t hash = romfs_path_hash(0, reinterpret_cast<const uint8_t*>(file.name.data()), file.name.size()) % file_hash_count;
        put32(entry + 0x18, file_hash[hash]);
        file_hash[hash] = entry_offsets[i];
        put32(entry + 0x1C, static_cast<uint32_t>(file.name.size()));
        std::memcpy(entry + 0x20, file.name.data(), file.name.size());
    }
    Bytes dir_table(0x18, 0);
    put32(&dir_table[0], 0);
    put32(&dir_table[4], kEmpty);
    put32(&dir_table[8], kEmpty);
    put32(&dir_table[0xC], count ? entry_offsets[0] : kEmpty);
    const uint32_t root_hash = romfs_path_hash(0, nullptr, 0) % dir_hash_count;
    put32(&dir_table[0x10], dir_hash[root_hash]);
    dir_hash[root_hash] = 0;
    put32(&dir_table[0x14], 0);

    const uint64_t data_start = 0x200;
    const uint64_t dir_hash_offset = (data_start + data_size + 3) & ~uint64_t(3);
    const uint64_t dir_hash_size = 4ull * dir_hash_count, file_hash_size = 4ull * file_hash_count;
    const uint64_t dir_table_offset = dir_hash_offset + dir_hash_size;
    const uint64_t file_hash_offset = dir_table_offset + dir_table.size();
    const uint64_t file_table_offset = file_hash_offset + file_hash_size;
    Bytes out(static_cast<size_t>(file_table_offset + file_table.size()), 0);
    put64(&out[0x00], 0x50);
    put64(&out[0x08], dir_hash_offset);
    put64(&out[0x10], dir_hash_size);
    put64(&out[0x18], dir_table_offset);
    put64(&out[0x20], dir_table.size());
    put64(&out[0x28], file_hash_offset);
    put64(&out[0x30], file_hash_size);
    put64(&out[0x38], file_table_offset);
    put64(&out[0x40], file_table.size());
    put64(&out[0x48], data_start);
    for (uint32_t i = 0; i < count; ++i) std::memcpy(&out[data_start + data_offsets[i]], files[i].data.data(), files[i].data.size());
    for (uint32_t i = 0; i < dir_hash_count; ++i) put32(&out[dir_hash_offset + 4ull * i], dir_hash[i]);
    std::memcpy(&out[dir_table_offset], dir_table.data(), dir_table.size());
    for (uint32_t i = 0; i < file_hash_count; ++i) put32(&out[file_hash_offset + 4ull * i], file_hash[i]);
    std::memcpy(&out[file_table_offset], file_table.data(), file_table.size());
    return out;
}

// One section of an NCA: a PFS0 (hashed in `block`-byte blocks) or a RomFS (IVFC).
struct Section {
    bool romfs;
    Bytes data;
    uint32_t block;
};

namespace detail {

inline Bytes hash_blocks(const Crypto& crypto, const Bytes& data, size_t block) {
    Bytes out;
    for (size_t at = 0; at < data.size(); at += block) {
        uint8_t digest[32];
        const size_t size = data.size() - at < block ? data.size() - at : block;
        crypto.sha256(data.data() + at, size, digest);
        out.insert(out.end(), digest, digest + 32);
    }
    return out;
}

inline void finish_section(Bytes& nca, uint8_t* header, int index, size_t start, const Crypto& crypto) {
    pad_to(nca, 0x200);
    uint8_t* entry = header + 0x240 + 0x10 * index;
    put32(entry, static_cast<uint32_t>(start / 0x200));
    put32(entry + 4, static_cast<uint32_t>(nca.size() / 0x200));
    put32(entry + 8, 1);
    crypto.sha256(header + 0x400 + 0x200 * index, 0x200, header + 0x280 + 0x20 * index);
}

inline void add_pfs0_section(Bytes& nca, uint8_t* header, int index, const Bytes& pfs0, uint32_t block, const Crypto& crypto) {
    const size_t start = nca.size();
    const Bytes hashes = hash_blocks(crypto, pfs0, block);
    uint8_t master[32];
    crypto.sha256(hashes.data(), hashes.size(), master);
    append(nca, hashes);
    pad_to(nca, 0x200);
    const uint64_t pfs0_offset = nca.size() - start;
    append(nca, pfs0);
    uint8_t* fs = header + 0x400 + 0x200 * index;
    put16(fs, 2);  // version
    fs[2] = 1;     // PFS0
    fs[3] = 2;     // hierarchical SHA-256
    fs[4] = 1;     // no encryption
    std::memcpy(fs + 8, master, 32);
    put32(fs + 8 + 0x20, block);
    put32(fs + 8 + 0x24, 2);  // layers
    put64(fs + 8 + 0x28, 0);
    put64(fs + 8 + 0x30, hashes.size());
    put64(fs + 8 + 0x38, pfs0_offset);
    put64(fs + 8 + 0x40, pfs0.size());
    finish_section(nca, header, index, start, crypto);
}

inline void add_romfs_section(Bytes& nca, uint8_t* header, int index, const Bytes& romfs, const Crypto& crypto) {
    const size_t kBlock = 0x4000;
    const size_t start = nca.size();
    std::vector<Bytes> level(6);
    level[5] = romfs;
    pad_to(level[5], kBlock);
    for (int i = 4; i >= 0; --i) {
        level[i] = hash_blocks(crypto, level[i + 1], kBlock);
        pad_to(level[i], kBlock);
    }
    uint8_t master[32];
    crypto.sha256(level[0].data(), level[0].size(), master);
    uint8_t* fs = header + 0x400 + 0x200 * index;
    put16(fs, 2);
    fs[2] = 0;  // RomFS
    fs[3] = 3;  // hierarchical integrity (IVFC)
    fs[4] = 1;  // no encryption
    uint8_t* ivfc = fs + 8;
    put32(ivfc, 0x43465649);  // "IVFC"
    put32(ivfc + 4, 0x20000);
    put32(ivfc + 8, 32);
    put32(ivfc + 0xC, 7);
    uint64_t logical = 0;
    for (int i = 0; i < 6; ++i) {
        uint8_t* info = ivfc + 0x10 + 0x18 * i;
        put64(info, logical);
        const uint64_t size = i == 5 ? romfs.size() : level[i].size();
        put64(info + 8, size);
        put32(info + 0x10, 14);  // log2 of the 0x4000 block
        logical += level[i].size();
    }
    std::memcpy(ivfc + 0xC0, master, 32);
    for (const Bytes& bytes : level) append(nca, bytes);
    finish_section(nca, header, index, start, crypto);
}

}  // namespace detail

inline Nca build_nca(const Crypto& crypto, uint64_t title_id, uint8_t nca_content_type, const std::vector<Section>& sections) {
    Bytes header(kHeaderSize, 0), nca(kHeaderSize, 0);
    for (size_t i = 0; i < sections.size(); ++i) {
        if (sections[i].romfs) detail::add_romfs_section(nca, header.data(), static_cast<int>(i), sections[i].data, crypto);
        else detail::add_pfs0_section(nca, header.data(), static_cast<int>(i), sections[i].data, sections[i].block, crypto);
    }
    std::memcpy(&header[0x200], "NCA3", 4);
    header[0x204] = 0;  // distribution: download
    header[0x205] = nca_content_type;
    put64(&header[0x208], nca.size());
    put64(&header[0x210], title_id);
    put32(&header[0x21C], 0x000C1100);  // SDK addon version
    Bytes encrypted = header;
    crypto.encrypt_header(encrypted.data());
    std::memcpy(nca.data(), encrypted.data(), kHeaderSize);
    Nca out;
    out.data = nca;
    crypto.sha256(nca.data(), nca.size(), out.hash);
    std::memcpy(out.content_id, out.hash, 16);
    return out;
}

// What the meta NCA and the content database need to know about another NCA.
struct ContentRef {
    uint8_t hash[32];
    uint8_t id[16];
    uint64_t size;
    uint8_t type;  // kNcm*
};

inline ContentRef reference(const Nca& nca, uint8_t ncm_type) {
    ContentRef ref;
    std::memcpy(ref.hash, nca.hash, 32);
    std::memcpy(ref.id, nca.content_id, 16);
    ref.size = nca.data.size();
    ref.type = ncm_type;
    return ref;
}

// NcmContentInfo (0x18): id | size low u32 | size high u8 | attr | type | id offset.
inline void put_content_info(uint8_t* out, const ContentRef& ref) {
    std::memcpy(out, ref.id, 16);
    put32(out + 0x10, static_cast<uint32_t>(ref.size));
    out[0x14] = static_cast<uint8_t>(ref.size >> 32);
    out[0x15] = 0;
    out[0x16] = ref.type;
    out[0x17] = 0;
}

// The CNMT of an application: header (0x20) | application extended header (0x10) | packaged content infos (0x38 each) |
// digest (0x20).
inline Bytes build_cnmt(uint64_t title_id, const ContentRef& program, const ContentRef& control) {
    Bytes out(0x20 + 0x10 + 2 * 0x38 + 0x20, 0);
    put64(&out[0], title_id);
    put32(&out[8], 0);   // version
    out[0xC] = 0x80;     // application
    put16(&out[0xE], 0x10);  // extended header size
    put16(&out[0x10], 2);    // contents: program and control
    put16(&out[0x12], 0);    // content metas
    put64(&out[0x20], title_id | 0x800);  // patch id
    const ContentRef* refs[2] = {&program, &control};
    for (int i = 0; i < 2; ++i) {
        uint8_t* packaged = &out[0x30 + 0x38 * i];
        std::memcpy(packaged, refs[i]->hash, 32);
        put_content_info(packaged + 32, *refs[i]);
    }
    return out;
}

// The record handed to ncm's content meta database: header (8) | extended header (0x10) | content infos (0x18 each):
// meta, program, control.
inline Bytes build_meta_record(uint64_t title_id, const ContentRef& meta, const ContentRef& program, const ContentRef& control) {
    Bytes out(8 + 0x10 + 3 * 0x18, 0);
    put16(&out[0], 0x10);
    put16(&out[2], 3);
    put64(&out[8], title_id | 0x800);
    const ContentRef* refs[3] = {&meta, &program, &control};
    for (int i = 0; i < 3; ++i) put_content_info(&out[0x18 + 0x18 * i], *refs[i]);
    return out;
}

struct Title {
    Nca program, control, meta;
    Bytes meta_record;
};

// The whole title: the ExeFS and RomFS of the forwarder program, the control RomFS (control.nacp and the icon), and the
// meta NCA that ties them together.
inline Title build_title(const Crypto& crypto, uint64_t title_id, const Bytes& exefs_pfs0, const std::vector<File>& program_romfs,
                         const std::vector<File>& control_romfs) {
    Title title;
    title.program = build_nca(crypto, title_id, kNcaProgram, {{false, exefs_pfs0, 0x10000}, {true, build_romfs(program_romfs), 0}});
    title.control = build_nca(crypto, title_id, kNcaControl, {{true, build_romfs(control_romfs), 0}});
    const ContentRef program = reference(title.program, kNcmProgram), control = reference(title.control, kNcmControl);
    char name[48];
    std::snprintf(name, sizeof(name), "Application_%016llx.cnmt", static_cast<unsigned long long>(title_id));
    const Bytes cnmt = build_cnmt(title_id, program, control);
    std::vector<File> cnmt_files;
    cnmt_files.push_back(File{name, cnmt});
    title.meta = build_nca(crypto, title_id, kNcaMeta, {{false, build_pfs0(cnmt_files), 0x1000}});
    title.meta_record = build_meta_record(title_id, reference(title.meta, kNcmMeta), program, control);
    return title;
}

}  // namespace homenca
