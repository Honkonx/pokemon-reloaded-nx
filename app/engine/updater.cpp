// Console-only half of the game updater: HTTPS with libcurl + mbedtls, Mega's public-link download protocol, AES-CTR.
// The parsing it relies on is in update_core.h (tested on a PC). Compiles and links for the Switch; the network path is
// not proven on a console.
#include "updater.h"

#include <switch.h>

#include <curl/curl.h>
#include <mbedtls/aes.h>

#include <sys/stat.h>

#include <cstdio>
#include <cstring>
#include <vector>

#include "update_core.h"

namespace updater {

namespace {

constexpr const char* kFeedUrl = "https://pokemon-reloaded.blogspot.com/feeds/posts/default?alt=json&max-results=8";
constexpr const char* kPageUrl = "https://pokemon-reloaded.blogspot.com/";
constexpr const char* kMegaApi = "https://g.api.mega.co.nz/cs";
constexpr unsigned long long kChunk = 8ULL << 20;  // multiple of 16, as AES-CTR resume needs
constexpr size_t kMaxFeedBytes = 2u << 20;

bool g_network_started = false;
bool g_curl_ready = false;
unsigned g_request_id = 1;

bool file_exists(const char* path) {
    struct stat info;
    return stat(path, &info) == 0;
}

unsigned long long file_size(const char* path) {
    struct stat info;
    return stat(path, &info) == 0 ? static_cast<unsigned long long>(info.st_size) : 0ULL;
}

struct Transfer {
    std::string body;                     // for API/feed replies
    FILE* out = nullptr;                  // for the download (decrypted on the fly)
    mbedtls_aes_context* aes = nullptr;
    unsigned char counter[16] = {};
    unsigned char stream_block[16] = {};
    size_t nc_off = 0;
    unsigned long long done = 0, total = 0;
    Progress progress = nullptr;
    void* user = nullptr;
    bool cancelled = false;
    bool write_failed = false;
};

size_t on_body(char* data, size_t size, size_t count, void* target) {
    auto* transfer = static_cast<Transfer*>(target);
    const size_t bytes = size * count;
    if (transfer->body.size() + bytes > kMaxFeedBytes) return 0;
    transfer->body.append(data, bytes);
    return bytes;
}

size_t on_download(char* data, size_t size, size_t count, void* target) {
    auto* transfer = static_cast<Transfer*>(target);
    const size_t bytes = size * count;
    std::vector<unsigned char> plain(bytes);
    mbedtls_aes_crypt_ctr(transfer->aes, bytes, &transfer->nc_off, transfer->counter, transfer->stream_block,
                          reinterpret_cast<const unsigned char*>(data), plain.data());
    if (std::fwrite(plain.data(), 1, bytes, transfer->out) != bytes) {
        transfer->write_failed = true;
        return 0;
    }
    transfer->done += bytes;
    if (transfer->progress && transfer->progress(transfer->done, transfer->total, transfer->user)) {
        transfer->cancelled = true;
        return 0;
    }
    return bytes;
}

CURL* make_handle(std::string& error) {
    if (!g_curl_ready) {
        curl_global_init(CURL_GLOBAL_DEFAULT);
        g_curl_ready = true;
    }
    if (!file_exists(kCaBundle)) {
        error = "Falta cacert.pem en sdmc:/switch/pokenx/ (viene en el paquete); sin él no se puede verificar la conexión segura";
        return nullptr;
    }
    CURL* handle = curl_easy_init();
    if (!handle) {
        error = "No se pudo iniciar la red";
        return nullptr;
    }
    curl_easy_setopt(handle, CURLOPT_CAINFO, kCaBundle);
    curl_easy_setopt(handle, CURLOPT_FOLLOWLOCATION, 1L);
    curl_easy_setopt(handle, CURLOPT_MAXREDIRS, 5L);
    curl_easy_setopt(handle, CURLOPT_USERAGENT, "PokeNX");
    curl_easy_setopt(handle, CURLOPT_NOSIGNAL, 1L);
    curl_easy_setopt(handle, CURLOPT_CONNECTTIMEOUT, 15L);
    curl_easy_setopt(handle, CURLOPT_LOW_SPEED_LIMIT, 1024L);
    curl_easy_setopt(handle, CURLOPT_LOW_SPEED_TIME, 30L);
    return handle;
}

std::string curl_error_text(CURLcode code, long http) {
    if (http == 509) return "Mega limitó la descarga de hoy (cuota agotada)";
    if (code == CURLE_PEER_FAILED_VERIFICATION || code == CURLE_SSL_CACERT_BADFILE || code == CURLE_SSL_CERTPROBLEM ||
        code == CURLE_SSL_CONNECT_ERROR)
        return "No se pudo verificar la conexión segura (¿reloj de la consola o cacert.pem?)";
    if (code == CURLE_COULDNT_RESOLVE_HOST || code == CURLE_COULDNT_CONNECT || code == CURLE_OPERATION_TIMEDOUT)
        return "No hay respuesta del servidor (revisa la conexión)";
    if (http >= 400) return "El servidor respondió con el error " + std::to_string(http);
    return std::string("Falló la descarga: ") + curl_easy_strerror(code);
}

// GET/POST a small reply into `reply`. Returns "" or the reason.
std::string request(const std::string& url, const std::string* post_body, std::string& reply) {
    std::string error;
    CURL* handle = make_handle(error);
    if (!handle) return error;
    Transfer transfer;
    curl_easy_setopt(handle, CURLOPT_URL, url.c_str());
    curl_easy_setopt(handle, CURLOPT_WRITEFUNCTION, on_body);
    curl_easy_setopt(handle, CURLOPT_WRITEDATA, &transfer);
    curl_easy_setopt(handle, CURLOPT_TIMEOUT, 60L);
    curl_slist* headers = nullptr;
    if (post_body) {
        headers = curl_slist_append(headers, "Content-Type: application/json");
        curl_easy_setopt(handle, CURLOPT_HTTPHEADER, headers);
        curl_easy_setopt(handle, CURLOPT_POSTFIELDS, post_body->c_str());
        curl_easy_setopt(handle, CURLOPT_POSTFIELDSIZE, static_cast<long>(post_body->size()));
    }
    const CURLcode code = curl_easy_perform(handle);
    long http = 0;
    curl_easy_getinfo(handle, CURLINFO_RESPONSE_CODE, &http);
    if (headers) curl_slist_free_all(headers);
    curl_easy_cleanup(handle);
    if (code != CURLE_OK || http >= 400) return curl_error_text(code, http);
    reply = std::move(transfer.body);
    return "";
}

std::string mega_request(const update_core::MegaLink& link, update_core::MegaInfo& info) {
    const std::string body = "[{\"a\":\"g\",\"g\":1,\"ssl\":2,\"p\":\"" + link.id + "\"}]";
    std::string reply;
    const std::string error = request(std::string(kMegaApi) + "?id=" + std::to_string(g_request_id++), &body, reply);
    if (!error.empty()) return error;
    info = update_core::parse_mega_info(reply);
    if (info.error != 0) return update_core::mega_error_text(info.error);
    return "";
}

std::string decrypt_file_name(const update_core::MegaLink& link, const std::string& attributes_b64) {
    std::string cipher = update_core::base64url_decode(attributes_b64);
    if (cipher.empty() || cipher.size() % 16 != 0) return {};
    mbedtls_aes_context aes;
    mbedtls_aes_init(&aes);
    std::string plain(cipher.size(), '\0');
    unsigned char iv[16] = {};
    const int status = mbedtls_aes_setkey_dec(&aes, link.aes_key, 128);
    if (status == 0)
        mbedtls_aes_crypt_cbc(&aes, MBEDTLS_AES_DECRYPT, cipher.size(), iv, reinterpret_cast<const unsigned char*>(cipher.data()),
                              reinterpret_cast<unsigned char*>(&plain[0]));
    mbedtls_aes_free(&aes);
    return status == 0 ? update_core::mega_file_name(plain) : std::string();
}

bool safe_file_name(const std::string& name) {
    if (name.empty() || name.size() > 120 || name[0] == '.') return false;
    for (char c : name)
        if (c == '/' || c == '\\' || c == ':' || c == '"' || c == '<' || c == '>' || c == '|' || c == '*' || c == '?') return false;
    return true;
}

}  // namespace

std::string start_network() {
    if (!g_network_started) {
        if (R_FAILED(socketInitializeDefault())) return "No se pudo iniciar la red de la consola";
        if (R_FAILED(nifmInitialize(NifmServiceType_User))) {
            socketExit();
            return "No se pudo consultar la conexión de la consola";
        }
        g_network_started = true;
    }
    NifmInternetConnectionType type;
    u32 strength;
    NifmInternetConnectionStatus status;
    if (R_FAILED(nifmGetInternetConnectionStatus(&type, &strength, &status)) || status != NifmInternetConnectionStatus_Connected)
        return "La consola no está conectada a internet";
    return "";
}

void stop_network() {
    if (g_curl_ready) {
        curl_global_cleanup();
        g_curl_ready = false;
    }
    if (g_network_started) {
        nifmExit();
        socketExit();
        g_network_started = false;
    }
}

Latest check_latest() {
    Latest latest;
    latest.page_url = kPageUrl;
    std::string feed;
    std::string error = request(kFeedUrl, nullptr, feed);
    if (!error.empty()) {
        latest.error = error;
        return latest;
    }
    const std::vector<update_core::Release> releases = update_core::parse_feed(feed);
    const update_core::Release* release = update_core::newest_with_download(releases);
    if (!release) {
        latest.error = "El blog no tiene ninguna entrada con enlace de descarga";
        return latest;
    }
    latest.title = release->title;
    latest.published = release->published;
    latest.notes = release->notes;
    latest.mega_url = release->mega_url;
    update_core::MegaLink link;
    if (!update_core::parse_mega_link(release->mega_url, link)) {
        latest.error = "El enlace de Mega del blog no tiene el formato esperado";
        return latest;
    }
    update_core::MegaInfo info;
    error = mega_request(link, info);
    if (!error.empty()) {
        latest.error = error;
        return latest;
    }
    latest.size = info.size;
    latest.file_name = decrypt_file_name(link, info.attributes);
    const update_core::Version version = update_core::parse_version(latest.file_name.empty() ? latest.title : latest.file_name);
    latest.version = version.valid() ? update_core::version_text(version) : std::string();
    latest.ok = true;
    return latest;
}

std::string download(const Latest& latest, std::string& saved_path, Progress progress, void* user) {
    update_core::MegaLink link;
    if (!update_core::parse_mega_link(latest.mega_url, link)) return "El enlace de Mega no es válido";
    update_core::MegaInfo info;
    std::string error = mega_request(link, info);  // a fresh temporary URL: the one from the check may have expired
    if (!error.empty()) return error;
    const std::string name = safe_file_name(latest.file_name) ? latest.file_name : std::string("juego-actualizado.zip");
    mkdir("sdmc:/switch", 0777);
    mkdir("sdmc:/switch/pokenx", 0777);
    mkdir(kDownloadsDir, 0777);
    const std::string final_path = std::string(kDownloadsDir) + "/" + name;
    const std::string part_path = final_path + ".part";
    const unsigned long long total = static_cast<unsigned long long>(info.size);

    if (file_size(final_path.c_str()) == total) {  // already downloaded completely
        saved_path = final_path;
        return "";
    }
    // Resume at a multiple of 16 bytes: the CTR counter then starts clean at a block boundary.
    unsigned long long offset = file_size(part_path.c_str()) / 16 * 16;
    if (offset > total) offset = 0;
    FILE* out = std::fopen(part_path.c_str(), offset ? "r+b" : "wb");
    if (!out) return "No se pudo crear el archivo en la tarjeta SD";
    if (offset) std::fseek(out, static_cast<long>(offset), SEEK_SET);

    mbedtls_aes_context aes;
    mbedtls_aes_init(&aes);
    mbedtls_aes_setkey_enc(&aes, link.aes_key, 128);
    Transfer transfer;
    transfer.out = out;
    transfer.aes = &aes;
    transfer.total = total;
    transfer.done = offset;
    transfer.progress = progress;
    transfer.user = user;
    std::memcpy(transfer.counter, link.nonce, 8);
    const unsigned long long block = offset / 16;
    for (int i = 0; i < 8; ++i) transfer.counter[15 - i] = static_cast<unsigned char>(block >> (8 * i));

    error.clear();
    while (offset < total && error.empty()) {
        const unsigned long long last = std::min(offset + kChunk, total) - 1;
        CURL* handle = make_handle(error);
        if (!handle) break;
        const std::string url = info.url + "/" + std::to_string(offset) + "-" + std::to_string(last);
        curl_easy_setopt(handle, CURLOPT_URL, url.c_str());
        curl_easy_setopt(handle, CURLOPT_WRITEFUNCTION, on_download);
        curl_easy_setopt(handle, CURLOPT_WRITEDATA, &transfer);
        const CURLcode code = curl_easy_perform(handle);
        long http = 0;
        curl_easy_getinfo(handle, CURLINFO_RESPONSE_CODE, &http);
        curl_easy_cleanup(handle);
        if (transfer.cancelled) {
            error = "Descarga cancelada (lo descargado se conserva para continuar)";
        } else if (transfer.write_failed) {
            error = "No se pudo escribir en la tarjeta SD (¿está llena?)";
        } else if (code != CURLE_OK || http >= 400) {
            error = curl_error_text(code, http);
        } else if (transfer.done != last + 1) {
            error = "La descarga llegó incompleta";
        }
        offset = transfer.done;
    }
    mbedtls_aes_free(&aes);
    std::fclose(out);
    if (!error.empty()) return error;
    std::remove(final_path.c_str());
    if (std::rename(part_path.c_str(), final_path.c_str()) != 0) return "No se pudo terminar el archivo descargado";
    saved_path = final_path;
    return "";
}

}  // namespace updater
