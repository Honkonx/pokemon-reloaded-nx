// Renders the graphical menu screens to PNG on a PC, with libnx replaced by stub/switch.h and a Windows TrueType font
// standing in for the Switch shared font. Layout/overflow check only: it proves nothing about the console.
// Build/run: tools/menu_preview/run.ps1 (output in tools/menu_preview/out/).
#include <algorithm>
#include <string>
#include <vector>
#include <cstdint>

#include "../../app/engine/fps_overlay.h"
#include "../../app/engine/menu.cpp"  // anonymous-namespace screens are used directly

Framebuffer* g_preview_fb = nullptr;
static int g_frames_left = 0;
static std::string g_shot;
static std::vector<u64> g_script;  // buttons "down" per frame
static size_t g_script_at = 0;
static u64 g_held = 0;

static std::vector<unsigned char> g_font_bytes;
Result plInitialize(PlServiceType) {
    FILE* f = std::fopen("C:/Windows/Fonts/segoeui.ttf", "rb");
    if (!f) return 1;
    std::fseek(f, 0, SEEK_END);
    g_font_bytes.resize(static_cast<size_t>(std::ftell(f)));
    std::fseek(f, 0, SEEK_SET);
    std::fread(g_font_bytes.data(), 1, g_font_bytes.size(), f);
    std::fclose(f);
    return 0;
}
Result plGetSharedFontByType(PlFontData* out, PlSharedFontType) {
    out->address = g_font_bytes.data();
    out->size = static_cast<u32>(g_font_bytes.size());
    return 0;
}

void padUpdate(PadState* pad) {
    pad->down = g_script_at < g_script.size() ? g_script[g_script_at++] : 0;
    pad->held = g_held;
}
bool appletMainLoop() { return g_frames_left > 0; }

static uint32_t crc32_of(const unsigned char* data, size_t size) {
    uint32_t crc = 0xFFFFFFFFu;
    for (size_t i = 0; i < size; ++i) {
        crc ^= data[i];
        for (int k = 0; k < 8; ++k) crc = (crc >> 1) ^ (0xEDB88320u & (0u - (crc & 1u)));
    }
    return ~crc;
}
// zlib stream made of stored (uncompressed) deflate blocks: large but trivial, and needs no zlib.
static std::vector<unsigned char> zlib_stored(const std::vector<unsigned char>& raw) {
    std::vector<unsigned char> out = {0x78, 0x01};
    size_t pos = 0;
    do {
        const size_t n = std::min<size_t>(65535, raw.size() - pos);
        const bool last = pos + n >= raw.size();
        out.push_back(last ? 1 : 0);
        out.push_back(n & 0xFF); out.push_back(n >> 8); out.push_back(~n & 0xFF); out.push_back((~n >> 8) & 0xFF);
        out.insert(out.end(), raw.begin() + pos, raw.begin() + pos + n);
        pos += n;
    } while (pos < raw.size());
    uint32_t a = 1, b = 0;
    for (unsigned char c : raw) { a = (a + c) % 65521; b = (b + a) % 65521; }
    const uint32_t adler = (b << 16) | a;
    for (int i = 3; i >= 0; --i) out.push_back(static_cast<unsigned char>(adler >> (8 * i)));
    return out;
}

static void put32(std::vector<unsigned char>& v, uint32_t x) { for (int i = 3; i >= 0; --i) v.push_back(static_cast<unsigned char>(x >> (8 * i))); }
static void chunk(std::vector<unsigned char>& out, const char* type, const std::vector<unsigned char>& data) {
    put32(out, static_cast<uint32_t>(data.size()));
    std::vector<unsigned char> body(type, type + 4);
    body.insert(body.end(), data.begin(), data.end());
    out.insert(out.end(), body.begin(), body.end());
    put32(out, crc32_of(body.data(), body.size()));
}
static void save_png(const std::string& path) {
    const int w = g_preview_fb->width, h = g_preview_fb->height;
    std::vector<unsigned char> raw;
    for (int y = 0; y < h; ++y) {
        raw.push_back(0);
        for (int x = 0; x < w; ++x) {
            const u32 p = g_preview_fb->pixels[static_cast<size_t>(y) * w + x];
            raw.push_back(p & 0xFF); raw.push_back((p >> 8) & 0xFF); raw.push_back((p >> 16) & 0xFF);
        }
    }
    const std::vector<unsigned char> packed = zlib_stored(raw);
    std::vector<unsigned char> png = {0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A};
    std::vector<unsigned char> ihdr;
    put32(ihdr, w); put32(ihdr, h);
    ihdr.insert(ihdr.end(), {8, 2, 0, 0, 0});
    chunk(png, "IHDR", ihdr);
    chunk(png, "IDAT", packed);
    chunk(png, "IEND", {});
    if (FILE* f = std::fopen(path.c_str(), "wb")) { std::fwrite(png.data(), 1, png.size(), f); std::fclose(f); }
}
void preview_frame_end() {
    if (--g_frames_left == 0 && !g_shot.empty()) save_png(g_shot);
}

extern "C" {
int32_t pokenx_find_game(const char*, char* out, size_t capacity) {
    std::snprintf(out, capacity, "sdmc:/switch/pokenx/game/Proyecto Reloaded The Last Beta 1.9.2.2 Full.exe");
    return 0;
}
int32_t pokenx_import_zip(const char*, PokenxProgressFn, void*) { return 0; }
void pokenx_import_error_text(int32_t, char* out, size_t capacity) { std::snprintf(out, capacity, "error de prueba"); }
void pokenx_set_unix_seconds(int64_t) {}
int32_t pokenx_preload_game(const char*) { return 0; }
void pokenx_preload_clear() {}
int32_t pokenx_scan_game_keys(const char*, const char*) { return 0; }
int32_t pokenx_scan_game_reports(const char*, const char*, const char*) { return 0; }
int32_t pokenx_backup_create(const char*, char* out, size_t capacity) { std::snprintf(out, capacity, "copia-004"); return 3; }
int32_t pokenx_backup_list(const char*, char* out, size_t capacity) {
    std::snprintf(out, capacity, "copia-003\t14\t2306048\t1792240000\ncopia-002\t12\t2101248\t1791900000\ncopia-001\t9\t1048576\t1791300000\n");
    return 3;
}
int32_t pokenx_backup_restore(const char*, const char*) { return 14; }
uint64_t pokenx_fs_free_bytes(const char*) { return 18ULL << 30; }
}

// Stand-in for updater.cpp (HTTPS and Mega only exist on the console): fixed answers for the screenshots.
static bool g_preview_offline = false;  // true: the console "is not online"
static bool g_preview_newer = false;  // true: the blog "has" a newer version than the installed one
namespace updater {
std::string start_network() { return g_preview_offline ? "La consola no está conectada a internet" : ""; }
void stop_network() {}
Latest check_latest() {
    Latest latest;
    latest.ok = true;
    latest.title = g_preview_newer ? "The Last Beta 1.9.3" : "The Last Beta 1.9.2";
    latest.version = g_preview_newer ? "1.9.3" : "1.9.2.2";
    latest.published = g_preview_newer ? "2026-11-12" : "2026-08-01";
    latest.file_name = std::string("Proyecto Reloaded ") + latest.title + " Full.zip";
    latest.size = 205855369LL;
    latest.notes = "Novedades de la versión (aquí va el texto de la entrada del blog oficial, ya sin formato).\nSe muestran las primeras líneas.";
    latest.page_url = "https://pokemon-reloaded.blogspot.com/";
    return latest;
}
std::string download(const Latest&, std::string& saved_path, Progress, void*) { saved_path = "sdmc:/switch/pokenx/downloads/x.zip"; return ""; }
}  // namespace updater
// Stand-in for home_install.cpp (it talks to the console's ncm/ns services): the shortcut "installs" instantly, or fails on demand.
static bool g_preview_home_installed = false;
static bool g_preview_home_fails = false;
namespace home_install {
bool is_installed() { return g_preview_home_installed; }
Outcome install(Progress progress) {
    Outcome outcome;
    if (progress) progress("Escribiendo el contenido en la tarjeta SD", 5, 6);
    if (g_preview_home_fails) {
        outcome.step = "escribir un contenido (ncm)";
        outcome.code = (1u << 0) | (5u << 9);  // module 5 (ncm), description 0 -> "2005-0000"
        outcome.detail = "No se pudo escribir en la tarjeta SD.";
        return outcome;
    }
    g_preview_home_installed = true;
    outcome.ok = true;
    return outcome;
}
Outcome uninstall() { Outcome outcome; g_preview_home_installed = false; outcome.ok = true; return outcome; }
std::string code_text(unsigned int code) { char text[24]; std::snprintf(text, sizeof(text), "2%03u-%04u", code & 0x1FF, (code >> 9) & 0x1FFF); return text; }
}  // namespace home_install
MenuResult run_text_menu(const MenuInfo&) { return MenuResult::Quit; }

static void shot(const char* name, int frames, std::vector<u64> script, u64 held = 0) {
    g_frames_left = frames; g_shot = std::string("out/") + name + ".png"; g_script = std::move(script); g_script_at = 0; g_held = held;
}

// A small sample report so the keys screen can be drawn without a game. POKENX_PREVIEW_KEYS=<real report> uses a real one
// (its size line is zeroed because the stub game file does not exist on the PC).
static void write_sample_report() {
    std::string text;
    if (const char* real = std::getenv("POKENX_PREVIEW_KEYS")) {
        if (FILE* f = std::fopen(real, "r")) {
            char line[400];
            while (std::fgets(line, sizeof(line), f)) text += std::strncmp(line, "size=", 5) == 0 ? "size=0\n" : line;
            std::fclose(f);
        }
    }
    if (text.empty())
        text = "exe=Proyecto Reloaded The Last Beta 1.9.2.2 Full.exe\nsize=0\nbt|mov_arriba|UP\nbt|ataque1|Z\nbt|cambio|S\nglobal|guardar|F5\nglobal|cargar|F6\nglobal|salir|ESC\nmenu|ayuda|F1\nmenu|pokedex|F12\n";
    if (FILE* f = std::fopen("out/game-keys.txt", "w")) { std::fputs(text.c_str(), f); std::fclose(f); }
}

// Sample compatibility report (cache/game-compat.txt). With `missing` it shows what a version with gaps would look like.
static void write_sample_compat(bool missing) {
    if (FILE* f = std::fopen("out/game-compat.txt", "w")) {
        std::fputs("exe=Proyecto Reloaded The Last Beta 1.9.2.2 Full.exe\nsize=0\nscripts=5075\nbuiltin=218\n", f);
        if (missing) std::fputs("missing|draw_set_alpha_test|3\nmissing|sprite_collision_mask|1\nmissing|external_define|2\n", f);
        std::fputs("simplified|message_alpha|1|solo el aspecto de los cuadros de mensaje\n"
                   "simplified|window_set_fullscreen|20|irrelevante en la Switch (siempre pantalla completa)\n"
                   "simplified|chr|26|solo importa con texto fuera de ASCII y Latin-1\n"
                   "simplified|get_open_filename|1|función del PC; el juego ya maneja que se cancele\n", f);
        std::fclose(f);
    }
}

// The keys the game reported while it ran (cache/game-keys-live.txt): a few differ from the defaults.
static void write_sample_live() {
    if (FILE* f = std::fopen("out/game-keys-live.txt", "w")) {
        std::fputs("# live\nbt|ataque1|A\nbt|Ataque1|A\nglobal|guardar|G\nglobal|cargar|F6\nbt|cambio|S\nbt|mov_arriba|UP\n", f);
        std::fclose(f);
    }
}

int main() {
    std::remove("out/game-keys-live.txt");
    std::remove("out/game-compat.txt");
    write_sample_report();
    if (!gfx::init()) { std::fprintf(stderr, "gfx::init failed\n"); return 1; }
    GameState game = detect_game();
    MenuInfo info;
    info.message = "La carga se canceló: el sistema pidió cerrar la aplicación.";
    shot("01-inicio", 4, {}); run_graphical(info);
    shot("01b-inicio-opciones", 6, {0, HidNpadButton_Right, HidNpadButton_Right}); run_graphical(MenuInfo{});
    shot("02-cargando", 1, {}); draw_loading(game); g_frames_left = 0;
    shot("03-opciones", 4, {}); show_options(game);
    shot("04-mandos", 4, {}); show_controls(game);
    shot("04b-mandos-reasignar", 6, {0, HidNpadButton_A, HidNpadButton_Right, HidNpadButton_Right}); show_controls(game);
    shot("04c-mandos-prueba", 4, {}, HidNpadButton_A | HidNpadButton_ZL); show_controls(game);
    shot("05-importar", 4, {}); (void)browse_zip(game);
    // Screens added with the updater and the game-keys screen. Button scripts are "down" events per frame.
    shot("10-actualizaciones", 3, {}); { std::string note; GameState g = game; show_updates(g, note); }
    g_preview_newer = true;
    shot("10b-actualizaciones-hay-nueva", 8, {0, HidNpadButton_A}); { std::string note; GameState g = game; show_updates(g, note); }
    shot("10c-actualizaciones-instalar", 9, {0, HidNpadButton_A, 0, HidNpadButton_Down, HidNpadButton_A}); { std::string note; GameState g = game; show_updates(g, note); }
    g_preview_newer = false;
    g_preview_offline = true;
    shot("10e-actualizaciones-sin-conexion", 8, {0, HidNpadButton_A}); { std::string note; GameState g = game; show_updates(g, note); }
    g_preview_offline = false;
    // The FPS meter as it is painted over the game (a stand-in picture here: the real one is drawn with scissored GL clears).
    shot("13-medidor-fps", 1, {});
    {
        gfx::begin();
        gfx::rrect_gradient(0, 0, gfx::kWidth, gfx::kHeight, 0, rgba(70, 120, 60), rgba(30, 60, 30));
        gfx::text_center(gfx::kWidth / 2, 330, "(el juego, a pantalla completa)", 28, rgba(220, 240, 220));
        fps_overlay::Meter meter;
        uint64_t t = 1;
        for (int i = 0; i < 40; ++i) meter.on_frame(t += (i == 20 ? 33000000ULL : 16666667ULL));
        const std::string text = meter.text();
        int text_w = 0, text_h = 0;
        fps_overlay::measure(text, 3, text_w, text_h);
        gfx::fill(3, 3, text_w + 12, text_h + 12, rgba(0, 0, 0));
        for (const fps_overlay::Rect& rect : fps_overlay::build_rects(text, 9, 9, 3)) gfx::fill(rect.x, rect.y, rect.w, rect.h, rgba(90, 255, 115));
        gfx::end();
    }
    shot("13b-medidor-fps-pausa", 1, {});
    {
        // A dialog pause (sleep 700 ms) in the middle of a steady 60 Hz: the meter leaves it out and reports it on the P line.
        gfx::begin();
        gfx::rrect_gradient(0, 0, gfx::kWidth, gfx::kHeight, 0, rgba(70, 120, 60), rgba(30, 60, 30));
        gfx::text_center(gfx::kWidth / 2, 330, "(un diálogo con pausa del propio juego)", 28, rgba(220, 240, 220));
        fps_overlay::Meter meter;
        uint64_t t = 1, paused = 0;
        for (int i = 0; i < 70; ++i) {
            t += 16666667ULL;
            if (i == 12) {
                t += 700000000ULL;
                paused += 700000000ULL;
            }
            meter.on_frame(t, paused);
            if (meter.pause_ms() > 0) break;
        }
        const std::string text = meter.text();
        int text_w = 0, text_h = 0;
        fps_overlay::measure(text, 3, text_w, text_h);
        gfx::fill(3, 3, text_w + 12, text_h + 12, rgba(0, 0, 0));
        for (const fps_overlay::Rect& rect : fps_overlay::build_rects(text, 9, 9, 3)) gfx::fill(rect.x, rect.y, rect.w, rect.h, rgba(90, 255, 115));
        gfx::end();
    }
    shot("10f-descargar-a-mano", 3, {}); show_manual_download(game);
    shot("10d-descargando", 1, {}); draw_download_progress("Proyecto Reloaded The Last Beta 1.9.3 Full.zip", 92ULL << 20, 196ULL << 20, 1); g_frames_left = 0;
    shot("11-copias", 4, {}); { std::string note; show_backups(game, note); }
    std::remove("out/game-keys.txt");
    shot("12a-teclas-leer", 3, {}); show_game_keys(game, read_memory());
    write_sample_report();
    shot("12-teclas-grupos", 4, {}); show_game_keys(game, read_memory());
    shot("12b-teclas-grupo-global", 12, {0, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_A}); show_game_keys(game, read_memory());
    write_sample_compat(false);
    shot("14-compatibilidad", 12, {0, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_A}); { std::string note; GameState g = game; show_updates(g, note); }
    write_sample_compat(true);
    shot("14b-compatibilidad-con-faltas", 12, {0, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_A}); { std::string note; GameState g = game; show_updates(g, note); }
    std::remove("out/game-compat.txt");
    shot("14c-compatibilidad-comprobar", 12, {0, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_A}); { std::string note; GameState g = game; show_updates(g, note); }
    write_sample_compat(false);
    write_sample_live();
    shot("12c-teclas-grupos-tuyas", 4, {}); show_game_keys(game, read_memory());
    shot("12d-teclas-grupo-global-tuyas", 12, {0, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_Down, HidNpadButton_A}); show_game_keys(game, read_memory());
    std::remove("out/game-keys-live.txt");
    shot("04d-mandos-capa", 4, {0, HidNpadButton_R}); show_controls(game);
    shot("04e-mandos-capa-elegir", 7, {0, HidNpadButton_R, HidNpadButton_Down, HidNpadButton_A}); show_controls(game);
    shot("04f-mandos-capa-prueba", 4, {0, HidNpadButton_R}, HidNpadButton_ZL | HidNpadButton_A); show_controls(game);
    shot("04j-mandos-capa2", 5, {0, HidNpadButton_R, HidNpadButton_R}); show_controls(game);
    shot("04k-mandos-capa2-prueba", 5, {0, HidNpadButton_R, HidNpadButton_R}, HidNpadButton_ZR | HidNpadButton_A); show_controls(game);
    shot("04g-mandos-vista", 6, {0, HidNpadButton_R, HidNpadButton_R, HidNpadButton_R}); show_controls(game);
    shot("04h-mandos-vista-prueba", 6, {0, HidNpadButton_R, HidNpadButton_R, HidNpadButton_R}, HidNpadButton_ZL | HidNpadButton_A); show_controls(game);
    shot("04i-mandos-perfil-defecto", 5, {0, HidNpadButton_ZL}); show_controls(game);
    shot("06-importando", 1, {}); draw_import_progress("Proyecto Reloaded The Last Beta 1.9.2.2 Full.zip", 790ULL << 20, 2140ULL << 20); g_frames_left = 0;
    shot("07-acceso-home", 3, {}); show_home_shortcut(game);
    shot("07b-acceso-home-crear", 5, {0, HidNpadButton_A}); show_home_shortcut(game);
    shot("07c-acceso-home-progreso", 1, {}); draw_home_progress("Escribiendo el contenido en la tarjeta SD", 5, 6); g_frames_left = 0;
    shot("07d-acceso-home-hecho", 8, {0, HidNpadButton_A, 0, HidNpadButton_A}); show_home_shortcut(game);
    shot("07e-acceso-home-instalado", 3, {}); show_home_shortcut(game);
    shot("07f-acceso-home-quitar", 5, {0, HidNpadButton_Y}); show_home_shortcut(game);
    g_preview_home_installed = false;
    g_preview_home_fails = true;
    shot("07g-acceso-home-error", 8, {0, HidNpadButton_A, 0, HidNpadButton_A}); show_home_shortcut(game);
    g_preview_home_fails = false;
    shot("08-diagnostico", 3, {}); show_diagnostics(game);
    shot("09-aviso", 3, {}); (void)show_card("No hay juego instalado", "El menú no encontró un .exe GameMaker 8.0 válido en sdmc:/switch/pokenx/game/. Copia el juego (el .exe y su carpeta data/) o importa el ZIP.", {"Importar un ZIP del juego", "Volver"}, kBad);
    gfx::shutdown();
    return 0;
}
