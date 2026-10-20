// Game updates from the game's official blog, for the menu ("Actualizaciones"). The blog publishes one post per version
// with a Mega link; this checks the newest, downloads and decrypts it to the SD, and the menu installs it with the
// existing ZIP importer (which keeps the player's saves). HTTPS needs sdmc:/switch/pokenx/cacert.pem.
// Console-only (libcurl + mbedtls); tools/menu_preview provides a stand-in for the screenshots.
#pragma once

#include <string>

namespace updater {

constexpr const char* kDownloadsDir = "sdmc:/switch/pokenx/downloads";
constexpr const char* kCaBundle = "sdmc:/switch/pokenx/cacert.pem";

struct Latest {
    bool ok = false;
    std::string error;      // plain Spanish text when !ok
    std::string title;      // blog post title, e.g. "The Last Beta 1.9.2"
    std::string published;  // "2026-08-01"
    std::string notes;      // the post's text, shortened
    std::string file_name;  // name of the file on Mega
    std::string version;    // "1.9.2.2" (from the file name, else the title)
    long long size = 0;
    std::string mega_url;
    std::string page_url;   // the blog, for the manual route
};

// Starts sockets and checks the console is online. Returns "" or the reason it is not.
std::string start_network();
void stop_network();

// Reads the blog feed and asks Mega about the newest post with a download. Takes a few seconds.
Latest check_latest();

// Called with (bytes done, total) while downloading; return true to cancel.
using Progress = bool (*)(unsigned long long done, unsigned long long total, void* user);

// Downloads and decrypts `latest` to `<kDownloadsDir>/<file_name>`, resuming a partial file. Returns "" on success
// (path in `saved_path`) or the reason it failed.
std::string download(const Latest& latest, std::string& saved_path, Progress progress, void* user);

}  // namespace updater
