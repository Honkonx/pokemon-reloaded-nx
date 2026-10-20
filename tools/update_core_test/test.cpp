// Host tests for app/engine/update_core.h (no libnx, no network). Build/run: tools/update_core_test/run.ps1
// With a path argument it also parses that Blogger feed file and prints the releases found (manual check).
#include <cstdio>
#include <cstring>
#include <fstream>
#include <sstream>

#include "../../app/engine/fps_overlay.h"
#include "../../app/engine/qr.h"
#include "../../app/engine/update_core.h"

using namespace update_core;

static int failures = 0;
#define CHECK(condition)                                                        \
    do {                                                                        \
        if (!(condition)) {                                                     \
            std::printf("FAIL line %d: %s\n", __LINE__, #condition);            \
            ++failures;                                                         \
        }                                                                       \
    } while (0)

static void versions() {
    const Version a = parse_version("Proyecto Reloaded The Last Beta 1.9.2.2 Full.exe");
    CHECK(a.count == 4 && a.part[0] == 1 && a.part[1] == 9 && a.part[2] == 2 && a.part[3] == 2);
    CHECK(version_text(a) == "1.9.2.2");
    const Version b = parse_version("The Last Beta 1.9.2");
    CHECK(b.count == 3 && compare(a, b) == 1 && compare(b, a) == -1);
    CHECK(compare(parse_version("1.9.2"), parse_version("1.9.2.0")) == 0);
    CHECK(compare(parse_version("1.10"), parse_version("1.9.9.9")) == 1);
    CHECK(!parse_version("Reloaded 2026 sin version").valid());
    CHECK(!parse_version("").valid());
    CHECK(parse_version("v1.9.3-beta").count == 3);
    CHECK(parse_version("Ampliacion TLB 1.8.5.2 Ampliada").count == 4);
}

static void base64_and_mega() {
    CHECK(base64url_decode("QUJD") == "ABC");
    CHECK(base64url_decode("QUI") == "AB");
    CHECK(base64url_decode("_-8") == std::string("\xFF\xEF", 2));
    CHECK(base64url_decode("a$b").empty());

    // 32-byte key: bytes 0..15 = 0x01, 16..23 = 0x02 (nonce), 24..31 = 0x03.
    // AES key = bytes 0..15 XOR bytes 16..31 = 0x01^0x02 (first 8), 0x01^0x03 (last 8).
    std::string key(32, '\x01');
    for (int i = 16; i < 24; ++i) key[i] = '\x02';
    for (int i = 24; i < 32; ++i) key[i] = '\x03';
    static const char* kAlphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    std::string encoded;
    for (size_t i = 0; i < key.size(); i += 3) {
        uint32_t n = static_cast<uint32_t>(static_cast<uint8_t>(key[i])) << 16;
        const size_t left = key.size() - i;
        if (left > 1) n |= static_cast<uint32_t>(static_cast<uint8_t>(key[i + 1])) << 8;
        if (left > 2) n |= static_cast<uint8_t>(key[i + 2]);
        encoded += kAlphabet[(n >> 18) & 63];
        encoded += kAlphabet[(n >> 12) & 63];
        if (left > 1) encoded += kAlphabet[(n >> 6) & 63];
        if (left > 2) encoded += kAlphabet[n & 63];
    }
    MegaLink link;
    CHECK(parse_mega_link("https://mega.nz/file/AbCdEfGh#" + encoded, link));
    CHECK(link.id == "AbCdEfGh");
    CHECK(link.nonce[0] == 2 && link.nonce[7] == 2);
    CHECK(link.aes_key[0] == (0x01 ^ 0x02) && link.aes_key[7] == (0x01 ^ 0x02) && link.aes_key[8] == (0x01 ^ 0x03));
    CHECK(parse_mega_link("<a href=\"https://mega.nz/file/AbCdEfGh#" + encoded + "\">", link));
    CHECK(!parse_mega_link("https://mega.nz/file/AbCdEfGh", link));
    CHECK(!parse_mega_link("https://mega.nz/file/AbCdEfGh#corta", link));
    CHECK(!parse_mega_link("https://example.com/", link));

    const MegaInfo ok = parse_mega_info("[{\"s\":205855369,\"at\":\"AAAA\",\"msd\":1,\"g\":\"https://gfs.example/dl/XYZ\",\"ip\":[\"1.2.3.4\"]}]");
    CHECK(ok.error == 0 && ok.size == 205855369 && ok.url == "https://gfs.example/dl/XYZ" && ok.attributes == "AAAA");
    CHECK(parse_mega_info("[-9]").error == -9);
    CHECK(parse_mega_info("-17").error == -17);
    CHECK(parse_mega_info("[{\"s\":0}]").error != 0);
    CHECK(mega_error_text(-17).find("cuota") != std::string::npos);
    CHECK(mega_file_name("MEGA{\"n\":\"Juego 1.9.2.2 Full.zip\"}") == "Juego 1.9.2.2 Full.zip");
    CHECK(mega_file_name("basura").empty());
}

static void feed() {
    // The JSON escapes are written with a doubled backslash so the C++ compiler leaves them for the parser to read.
    const std::string json =
        "{\"feed\":{\"title\":{\"type\":\"text\",\"$t\":\"El blog\"},\"entry\":["
        "{\"published\":{\"$t\":\"2026-10-18T22:48:39.618+02:00\"},\"title\":{\"type\":\"text\",\"$t\":\"The Last Fakemon\"},"
        "\"content\":{\"type\":\"html\",\"$t\":\"Concurso &amp; premios\"},\"link\":[{\"rel\":\"x\",\"title\":\"Comentarios\"}]},"
        "{\"published\":{\"$t\":\"2026-08-01T03:03:05.893+02:00\"},\"category\":[{\"term\":\"Novedades\"}],"
        "\"title\":{\"type\":\"text\",\"$t\":\"The Last Beta 1.9.2\"},"
        "\"content\":{\"type\":\"html\",\"$t\":\"\\u003cp\\u003eNovedades \\u00e1\\u00e9:\\u003c/p\\u003e\\u003cul\\u003e\\u003cli\\u003eUno\\u003c/li\\u003e\\u003cli\\u003eDos\\u003c/li\\u003e\\u003c/ul\\u003e"
        "\\u003ca href=\\\"https://mega.nz/file/RrsDjDQY#5hOsjq7HIYsNX30JkeHzPGCl12ZO8yugdcz_f7gqLCA\\\"\\u003eLink\\u003c/a\\u003e "
        "\\u003ca href=\\\"https://store.gofile.io/download/web/id/Juego.zip\\\"\\u003eAlt\\u003c/a\\u003e\"}}]}}";
    const std::vector<Release> releases = parse_feed(json);
    CHECK(releases.size() == 2);
    CHECK(releases[0].title == "The Last Fakemon" && releases[0].mega_url.empty());
    CHECK(releases[0].notes == "Concurso & premios");
    CHECK(releases.size() > 1 && releases[1].title == "The Last Beta 1.9.2" && releases[1].published == "2026-08-01");
    CHECK(releases.size() > 1 && releases[1].mega_url == "https://mega.nz/file/RrsDjDQY#5hOsjq7HIYsNX30JkeHzPGCl12ZO8yugdcz_f7gqLCA");
    CHECK(releases.size() > 1 && releases[1].gofile_url.find("gofile.io") != std::string::npos);
    CHECK(releases.size() > 1 && releases[1].notes.find("Novedades \xC3\xA1\xC3\xA9:") == 0 && releases[1].notes.find("Uno") != std::string::npos);
    const Release* best = newest_with_download(releases);
    CHECK(best && best->title == "The Last Beta 1.9.2");
    MegaLink link;
    CHECK(best && parse_mega_link(best->mega_url, link) && link.id == "RrsDjDQY");
    CHECK(parse_feed("").empty() && parse_feed("{}").empty());

    // A surrogate pair (an emoji) in a JSON string.
    const std::string surrogate = "\"\\ud83d\\ude00!\"";
    size_t pos = 1;
    CHECK(read_json_string(surrogate, pos) == "\xF0\x9F\x98\x80!");
}

static void overlay() {
    fps_overlay::Meter meter;
    uint64_t now = 1'000'000'000ULL;
    bool changed = false;
    for (int i = 0; i < 40; ++i) {
        now += (i == 20) ? 40'000'000ULL : 16'666'667ULL;  // one 40 ms hiccup among 60 Hz frames
        changed = meter.on_frame(now) || changed;
    }
    CHECK(changed);
    CHECK(meter.fps() >= 55 && meter.fps() <= 61);
    CHECK(meter.peak_ms() == 40 || meter.peak_ms() == 17);
    CHECK(meter.quality() == 0);
    fps_overlay::Meter slow;
    uint64_t t = 1;
    for (int i = 0; i < 20; ++i) slow.on_frame(t += 50'000'000ULL);  // 20 fps
    CHECK(slow.fps() >= 19 && slow.fps() <= 21 && slow.quality() == 2);
    // A 2 s pause of the game (sleep) between two frames of an otherwise steady 60 Hz is not a drop: the FPS stays high,
    // the longest gap stays near one frame, and the pause is reported on the third line.
    {
        fps_overlay::Meter paused;
        uint64_t clock = 1'000'000'000ULL, pause_total = 0;
        for (int i = 0; i < 90; ++i) {
            clock += 16'666'667ULL;
            if (i == 30) {
                clock += 2'000'000'000ULL;
                pause_total += 2'000'000'000ULL;
            }
            paused.on_frame(clock, pause_total);
        }
        CHECK(paused.fps() >= 55 && paused.fps() <= 61);
        CHECK(paused.peak_ms() <= 20);
        CHECK(paused.quality() == 0);
        CHECK(paused.pause_ms() == 0 || paused.text().find("\nP ") != std::string::npos);
        // Without the pause total the same frames read as a collapse (what the meter did before).
        fps_overlay::Meter naive;
        uint64_t naive_clock = 1'000'000'000ULL;
        int worst = 0;
        for (int i = 0; i < 90; ++i) {
            naive_clock += 16'666'667ULL;
            if (i == 30) naive_clock += 2'000'000'000ULL;
            if (naive.on_frame(naive_clock) && naive.peak_ms() > worst) worst = naive.peak_ms();
        }
        CHECK(worst >= 2000);
        // A window that contained a pause reports it on a third line; one that did not stays on two lines.
        fps_overlay::Meter shown;
        uint64_t c = 1'000'000'000ULL, p = 0;
        bool saw_pause_line = false;
        for (int i = 0; i < 120; ++i) {
            c += 16'666'667ULL;
            if (i == 10) {
                c += 700'000'000ULL;
                p += 700'000'000ULL;
            }
            shown.on_frame(c, p);
            if (shown.text().find("\nP ") != std::string::npos) saw_pause_line = true;
        }
        CHECK(saw_pause_line);
        CHECK(meter.pause_ms() == 0 && meter.text().find("\nP ") == std::string::npos);
        // A long pause by itself never closes a window with a bogus number.
        fps_overlay::Meter lone;
        lone.on_frame(1'000'000'000ULL, 0);
        lone.on_frame(4'000'000'000ULL, 3'000'000'000ULL);
        CHECK(lone.fps() == 0);
    }
    int w = 0, h = 0;
    fps_overlay::measure("59 FPS\n21 MS", 3, w, h);
    CHECK(w == (6 * 6 - 1) * 3 && h == (2 * 9 - 2) * 3);
    const std::vector<fps_overlay::Rect> rects = fps_overlay::build_rects("59 FPS", 10, 20, 3);
    CHECK(!rects.empty() && rects.size() < 60);
    for (const fps_overlay::Rect& r : rects) CHECK(r.x >= 10 && r.y >= 20 && r.x + r.w <= 10 + w && r.y + r.h <= 20 + 7 * 3);
    CHECK(fps_overlay::build_rects("   ", 0, 0, 3).empty());
    CHECK(meter.text().find("FPS") != std::string::npos);
}

static void qrcode() {
    const qr::Code code = qr::encode("https://pokemon-reloaded.blogspot.com/");
    CHECK(code.size == 29);  // version 3
    CHECK(code.dark(0, 0) && code.dark(6, 0) && !code.dark(1, 1) && code.dark(2, 2));  // top-left finder pattern
    CHECK(code.dark(code.size - 1, 0) && code.dark(0, code.size - 1));
    CHECK(qr::encode(std::string(300, 'x')).size == 0);  // does not fit
    CHECK(qr::encode("a").size == 21);
}

int main(int argc, char** argv) {
    versions();
    overlay();
    qrcode();
    base64_and_mega();
    feed();
    if (argc > 1) {
        std::ifstream file(argv[1], std::ios::binary);
        std::stringstream text;
        text << file.rdbuf();
        for (const Release& release : parse_feed(text.str())) {
            MegaLink link;
            std::printf("%s | %s | mega=%s | key_ok=%d | notes=%zu chars\n", release.published.c_str(), release.title.c_str(),
                        release.mega_url.empty() ? "no" : "si", release.mega_url.empty() ? 0 : parse_mega_link(release.mega_url, link),
                        release.notes.size());
        }
    }
    std::printf(failures ? "update_core: %d FAILED\n" : "update_core: all checks passed\n", failures);
    return failures ? 1 : 0;
}
