// Pure parts of the game updater (no libnx, no network): version numbers, the blog feed, Mega links and replies.
// Header-only so tools/update_core_test can run them on a PC. The console-only parts (HTTPS, AES) live in updater.cpp.
#pragma once

#include <cstdint>
#include <cstdlib>
#include <string>
#include <vector>

namespace update_core {

// ---- Versions ---------------------------------------------------------------------------------------------------
struct Version {
    int part[4] = {0, 0, 0, 0};
    int count = 0;  // how many parts were written ("1.9.2" has 3); 0 = no version found
    bool valid() const { return count > 0; }
};

// The first dotted number sequence in `text` ("... Beta 1.9.2.2 Full.exe" -> 1.9.2.2). Needs at least one dot, so a lone
// number such as a year is not taken for a version.
inline Version parse_version(const std::string& text) {
    for (size_t i = 0; i < text.size(); ++i) {
        if (!(text[i] >= '0' && text[i] <= '9') || (i > 0 && ((text[i - 1] >= '0' && text[i - 1] <= '9') || text[i - 1] == '.'))) continue;
        Version version;
        size_t at = i;
        while (version.count < 4) {
            size_t end = at;
            int value = 0;
            while (end < text.size() && text[end] >= '0' && text[end] <= '9' && end - at < 4) value = value * 10 + (text[end++] - '0');
            if (end == at) break;
            version.part[version.count++] = value;
            if (end + 1 < text.size() && text[end] == '.' && text[end + 1] >= '0' && text[end + 1] <= '9') at = end + 1;
            else break;
        }
        if (version.count >= 2) return version;
    }
    return {};
}

// Missing trailing parts count as 0 ("1.9.2" == "1.9.2.0"). Returns -1, 0 or 1.
inline int compare(const Version& a, const Version& b) {
    for (int i = 0; i < 4; ++i) {
        if (a.part[i] != b.part[i]) return a.part[i] < b.part[i] ? -1 : 1;
    }
    return 0;
}

inline std::string version_text(const Version& version) {
    std::string text;
    for (int i = 0; i < version.count; ++i) text += (i ? "." : "") + std::to_string(version.part[i]);
    return text;
}

// ---- Base64 (URL-safe, unpadded, as Mega uses) -----------------------------------------------------------------
inline std::string base64url_decode(const std::string& text) {
    std::string out;
    uint32_t buffer = 0;
    int bits = 0;
    for (char c : text) {
        int value;
        if (c >= 'A' && c <= 'Z') value = c - 'A';
        else if (c >= 'a' && c <= 'z') value = c - 'a' + 26;
        else if (c >= '0' && c <= '9') value = c - '0' + 52;
        else if (c == '-' || c == '+') value = 62;
        else if (c == '_' || c == '/') value = 63;
        else if (c == '=') break;
        else return {};
        buffer = (buffer << 6) | static_cast<uint32_t>(value);
        bits += 6;
        if (bits >= 8) {
            bits -= 8;
            out.push_back(static_cast<char>((buffer >> bits) & 0xFF));
        }
    }
    return out;
}

// ---- Mega -------------------------------------------------------------------------------------------------------
struct MegaLink {
    std::string id;
    uint8_t aes_key[16] = {};  // the file key folded to 128 bits
    uint8_t nonce[8] = {};     // first half of the CTR counter block
};

// `https://mega.nz/file/<id>#<key>`: the key is 32 bytes, the AES key is its two halves XORed and the CTR nonce is bytes 16-23.
inline bool parse_mega_link(const std::string& url, MegaLink& link) {
    const size_t file = url.find("/file/");
    const size_t hash = url.find('#', file == std::string::npos ? 0 : file);
    if (file == std::string::npos || hash == std::string::npos || hash <= file + 6) return false;
    link.id = url.substr(file + 6, hash - file - 6);
    std::string key_text = url.substr(hash + 1);
    const size_t stop = key_text.find_first_of("\"' <&?");
    if (stop != std::string::npos) key_text.resize(stop);
    const std::string key = base64url_decode(key_text);
    if (key.size() != 32 || link.id.empty() || link.id.size() > 16) return false;
    for (int i = 0; i < 16; ++i) link.aes_key[i] = static_cast<uint8_t>(key[i] ^ key[i + 16]);
    for (int i = 0; i < 8; ++i) link.nonce[i] = static_cast<uint8_t>(key[16 + i]);
    return true;
}

// Reads the value after `"name":` (a number, or a string without escapes) from a flat Mega reply.
inline bool json_number(const std::string& json, const char* name, long long& value) {
    const std::string pattern = std::string("\"") + name + "\":";
    const size_t at = json.find(pattern);
    if (at == std::string::npos) return false;
    char* end = nullptr;
    value = std::strtoll(json.c_str() + at + pattern.size(), &end, 10);
    return end != json.c_str() + at + pattern.size();
}

inline bool json_plain_string(const std::string& json, const char* name, std::string& value) {
    const std::string pattern = std::string("\"") + name + "\":\"";
    const size_t at = json.find(pattern);
    if (at == std::string::npos) return false;
    const size_t start = at + pattern.size();
    const size_t end = json.find('"', start);
    if (end == std::string::npos) return false;
    value = json.substr(start, end - start);
    return true;
}

struct MegaInfo {
    long long size = 0;
    std::string url;         // temporary download URL (append "/<from>-<to>" for a range)
    std::string attributes;  // encrypted attribute block, base64url
    int error = 0;           // negative Mega error code when the reply is one
};

// `[{"s":..,"at":"..","g":".."}]` or `[-9]`.
inline MegaInfo parse_mega_info(const std::string& reply) {
    MegaInfo info;
    size_t digits = 0;
    while (digits < reply.size() && (reply[digits] == '[' || reply[digits] == ' ')) ++digits;
    if (digits < reply.size() && reply[digits] == '-') {
        info.error = std::atoi(reply.c_str() + digits);
        return info;
    }
    json_number(reply, "s", info.size);
    json_plain_string(reply, "g", info.url);
    json_plain_string(reply, "at", info.attributes);
    if (info.error == 0 && (info.url.empty() || info.size <= 0)) info.error = -1;
    return info;
}

inline std::string mega_error_text(int code) {
    switch (code) {
        case -3: return "Mega pide reintentar más tarde";
        case -9: return "El archivo ya no existe en Mega";
        case -16: return "Mega bloqueó el enlace";
        case -17: return "Mega limitó la descarga de hoy (cuota agotada)";
        case -18: return "El archivo de Mega no está disponible ahora";
        default: return "Mega respondió con un error (" + std::to_string(code) + ")";
    }
}

// The attribute block decrypts to `MEGA{"n":"file name"}`; this returns the name from that plain text.
inline std::string mega_file_name(const std::string& attributes_plain) {
    const size_t at = attributes_plain.find("\"n\":\"");
    if (attributes_plain.compare(0, 4, "MEGA") != 0 || at == std::string::npos) return {};
    const size_t start = at + 5;
    const size_t end = attributes_plain.find('"', start);
    return end == std::string::npos ? std::string() : attributes_plain.substr(start, end - start);
}

// ---- Blog feed (Blogger JSON) -----------------------------------------------------------------------------------
struct Release {
    std::string title;      // "The Last Beta 1.9.2"
    std::string published;  // "2026-08-01"
    std::string mega_url;
    std::string gofile_url;
    std::string notes;      // plain text, shortened
};

inline void append_utf8(std::string& out, uint32_t code) {
    if (code < 0x80) out.push_back(static_cast<char>(code));
    else if (code < 0x800) { out.push_back(static_cast<char>(0xC0 | (code >> 6))); out.push_back(static_cast<char>(0x80 | (code & 0x3F))); }
    else if (code < 0x10000) {
        out.push_back(static_cast<char>(0xE0 | (code >> 12))); out.push_back(static_cast<char>(0x80 | ((code >> 6) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | (code & 0x3F)));
    } else {
        out.push_back(static_cast<char>(0xF0 | (code >> 18))); out.push_back(static_cast<char>(0x80 | ((code >> 12) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | ((code >> 6) & 0x3F))); out.push_back(static_cast<char>(0x80 | (code & 0x3F)));
    }
}

// Reads the JSON string that starts right after its opening quote at `pos`; leaves `pos` after the closing quote.
inline std::string read_json_string(const std::string& json, size_t& pos) {
    std::string out;
    auto hex4 = [&](size_t at) {
        uint32_t value = 0;
        for (size_t i = 0; i < 4 && at + i < json.size(); ++i) {
            const char c = json[at + i];
            value = value * 16 + static_cast<uint32_t>(c <= '9' ? c - '0' : (c | 0x20) - 'a' + 10);
        }
        return value;
    };
    while (pos < json.size() && json[pos] != '"') {
        if (json[pos] != '\\') { out.push_back(json[pos++]); continue; }
        if (++pos >= json.size()) break;
        const char escape = json[pos++];
        switch (escape) {
            case 'n': out.push_back('\n'); break;
            case 't': out.push_back('\t'); break;
            case 'r': break;
            case 'u': {
                uint32_t code = hex4(pos);
                pos += 4;
                if (code >= 0xD800 && code < 0xDC00 && pos + 5 < json.size() && json[pos] == '\\' && json[pos + 1] == 'u') {
                    const uint32_t low = hex4(pos + 2);
                    pos += 6;
                    code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                }
                append_utf8(out, code);
                break;
            }
            default: out.push_back(escape);  // \" \\ \/
        }
    }
    if (pos < json.size()) ++pos;
    return out;
}

inline std::string strip_tags(const std::string& html) {
    std::string text;
    bool in_tag = false;
    for (size_t i = 0; i < html.size(); ++i) {
        const char c = html[i];
        if (c == '<') {
            in_tag = true;
            const bool breaks = html.compare(i, 3, "<br") == 0 || html.compare(i, 4, "</p>") == 0 || html.compare(i, 4, "</di") == 0 ||
                                html.compare(i, 3, "<li") == 0 || html.compare(i, 3, "<p>") == 0;
            if (breaks && !text.empty() && text.back() != '\n') text.push_back('\n');
        } else if (c == '>') {
            in_tag = false;
        } else if (!in_tag) {
            if (c == '&') {
                const size_t semi = html.find(';', i);
                if (semi != std::string::npos && semi - i <= 6) {
                    const std::string entity = html.substr(i, semi - i + 1);
                    text += entity == "&amp;" ? "&" : entity == "&lt;" ? "<" : entity == "&gt;" ? ">" : entity == "&quot;" ? "\"" : entity == "&nbsp;" ? " " : "";
                    i = semi;
                    continue;
                }
            }
            if (c == '\r' || c == '\t') text.push_back(' ');
            else if (c != '\n' || (!text.empty() && text.back() != '\n')) text.push_back(c);
        }
    }
    while (!text.empty() && (text.back() == '\n' || text.back() == ' ')) text.pop_back();
    return text;
}

inline std::string first_link(const std::string& html, const char* needle) {
    size_t at = 0;
    while ((at = html.find("href=\"", at)) != std::string::npos) {
        at += 6;
        const size_t end = html.find('"', at);
        if (end == std::string::npos) break;
        const std::string url = html.substr(at, end - at);
        if (url.find(needle) != std::string::npos) return url;
        at = end;
    }
    return {};
}

// Releases from a Blogger `?alt=json` feed, newest first as the blog returns them. Every entry is returned (with empty
// links when the post has none); the caller picks the first with a Mega link.
inline std::vector<Release> parse_feed(const std::string& json) {
    std::vector<Release> releases;
    const std::string published_key = "\"published\":{\"$t\":\"";
    size_t at = json.find(published_key);
    while (at != std::string::npos) {
        Release release;
        size_t pos = at + published_key.size();
        release.published = read_json_string(json, pos).substr(0, 10);
        const size_t next = json.find(published_key, pos);
        const std::string limit_text = json.substr(pos, next == std::string::npos ? std::string::npos : next - pos);
        const std::string title_key = "\"title\":{\"type\":\"text\",\"$t\":\"", content_key = "\"content\":{\"type\":\"html\",\"$t\":\"";
        const size_t title_at = limit_text.find(title_key);
        if (title_at != std::string::npos) {
            size_t p = title_at + title_key.size();
            release.title = read_json_string(limit_text, p);
        }
        const size_t content_at = limit_text.find(content_key);
        if (content_at != std::string::npos) {
            size_t p = content_at + content_key.size();
            const std::string html = read_json_string(limit_text, p);
            release.mega_url = first_link(html, "mega.nz/file/");
            release.gofile_url = first_link(html, "gofile.io");
            release.notes = strip_tags(html);
            if (release.notes.size() > 700) release.notes.resize(700);
        }
        if (!release.title.empty()) releases.push_back(release);
        at = next;
    }
    return releases;
}

// The newest release that has a Mega link.
inline const Release* newest_with_download(const std::vector<Release>& releases) {
    for (const Release& release : releases)
        if (!release.mega_url.empty()) return &release;
    return nullptr;
}

}  // namespace update_core
