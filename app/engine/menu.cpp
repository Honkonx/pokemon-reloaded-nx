// Graphical menu of PokeNX (mockups in bocetos/v2). Drawn with gfx.cpp on a libnx framebuffer, before EGL/Mesa start.
// Screens: Inicio (mosaicos), Actualizaciones (blog oficial, ZIP, copias), Opciones, Mandos (botones + capa), Teclas del juego,
// Acceso HOME, Diagnóstico, Avisos.
// If gfx cannot start, run_menu falls back to the text console menu (menu_text.cpp).
#include "menu.h"

#include <switch.h>

#include <dirent.h>
#include <sys/stat.h>

#include <algorithm>
#include <cctype>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <string>
#include <vector>

#include "gfx.h"
#include "pokenx_engine.h"
#include "qr.h"
#include "update_core.h"
#include "updater.h"
#include "home_install.h"

extern "C" uint64_t pokenx_fs_free_bytes(const char* path);  // host_services.cpp

namespace {

using gfx::rgba;

constexpr const char* kRoot = "sdmc:/switch/pokenx";
constexpr const char* kGameDir = "sdmc:/switch/pokenx/game";
constexpr const char* kLogDir = "sdmc:/switch/pokenx/logs";
constexpr const char* kControlsFile = "sdmc:/switch/pokenx/controls.txt";

const u32 kGold = rgba(242, 199, 68), kGoldDark = rgba(224, 138, 30), kText = rgba(232, 236, 245), kDim = rgba(141, 150, 171),
          kOk = rgba(88, 214, 141), kBad = rgba(236, 112, 99), kWarn = rgba(245, 176, 65), kPanel = rgba(21, 26, 43),
          kLine = rgba(43, 50, 71), kTile = rgba(26, 31, 46), kRowSel = rgba(31, 39, 64), kInk = rgba(16, 19, 28),
          kChip = rgba(255, 255, 255, 24);

// ---- Input ------------------------------------------------------------------------------------------------------
PadState g_pad;

struct Input {
    u64 down = 0;
    u64 held = 0;
};

Input poll_input() {
    static int stick_vertical = 0;
    static int stick_horizontal = 0;
    padUpdate(&g_pad);
    Input input;
    input.down = padGetButtonsDown(&g_pad);
    input.held = padGetButtons(&g_pad);
    const HidAnalogStickState stick = padGetStickPos(&g_pad, 0);
    const int vertical = stick.y > 18000 ? -1 : (stick.y < -18000 ? 1 : 0);  // -1 = up
    const int horizontal = stick.x > 18000 ? 1 : (stick.x < -18000 ? -1 : 0);
    if (vertical != 0 && vertical != stick_vertical) input.down |= vertical < 0 ? HidNpadButton_Up : HidNpadButton_Down;
    if (horizontal != 0 && horizontal != stick_horizontal)
        input.down |= horizontal < 0 ? HidNpadButton_Left : HidNpadButton_Right;
    stick_vertical = vertical;
    stick_horizontal = horizontal;
    return input;
}

bool pressed(const Input& input, u64 button) { return (input.down & button) != 0; }

// ---- Files ------------------------------------------------------------------------------------------------------
std::string read_token(const char* path, const char* fallback) {
    char value[24] = {0};
    if (FILE* file = std::fopen(path, "r")) {
        if (std::fscanf(file, "%23s", value) != 1) value[0] = 0;
        std::fclose(file);
    }
    return value[0] ? std::string(value) : std::string(fallback);
}

void write_token(const char* path, const char* value) {
    if (FILE* file = std::fopen(path, "w")) {
        std::fprintf(file, "%s\n", value);
        std::fclose(file);
    }
}

unsigned long long file_size(const char* path) {
    struct stat info;
    return stat(path, &info) == 0 ? static_cast<unsigned long long>(info.st_size) : 0ULL;
}

bool file_exists(const char* path) {
#ifdef POKENX_PREVIEW
    if (std::strstr(path, "pokenx.nro")) return true;  // the PC preview has no SD card: pretend the NRO is in place
#endif
    struct stat info;
    return stat(path, &info) == 0;
}

std::string lower(std::string text) {
    std::transform(text.begin(), text.end(), text.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return text;
}

std::string short_name(const std::string& path) {
    const size_t slash = path.find_last_of('/');
    return slash == std::string::npos ? path : path.substr(slash + 1);
}

struct GameState {
    bool found = false;
    std::string text;  // exe path when found, otherwise the reason
};

void preload_join();  // below

GameState detect_game() {
    preload_join();  // find_game is a Rust call: not while the worker parses
    char buffer[600] = {0};
    GameState state;
    state.found = pokenx_find_game(kGameDir, buffer, sizeof(buffer)) == 0;
    state.text = buffer;
    return state;
}

struct Memory {
    u64 total = 0, used = 0;
    bool applet() const { return total < (1024ULL << 20); }
};

Memory read_memory() {
    Memory memory;
    svcGetInfo(&memory.total, InfoType_TotalMemorySize, CUR_PROCESS_HANDLE, 0);
    svcGetInfo(&memory.used, InfoType_UsedMemorySize, CUR_PROCESS_HANDLE, 0);
    return memory;
}

std::string mib(unsigned long long bytes) { return std::to_string(bytes >> 20) + " MiB"; }

// install-source.txt says that the app itself installed the current game from a ZIP ("zip", then the size of the game's
// .exe at that moment). The boot log compares it with the .exe that is there now: equal means imported by the app,
// anything else means the folder was filled by hand (or by a build from before this marker). Written once per import and
// read once per start, never per frame.
constexpr const char* kInstallSourcePath = "sdmc:/switch/pokenx/install-source.txt";

long long file_size_of(const std::string& path) {
    struct stat info;
    return stat(path.c_str(), &info) == 0 ? static_cast<long long>(info.st_size) : -1;
}

// ---- Preloading the game while the menu is open -------------------------------------------------------------------
// Reading and parsing the .exe takes about 6 s; a worker thread starts it when the menu opens so that "Jugar" can skip it
// (loader::launch takes the result). Whatever else calls into the Rust side (import, backups, key reading) first waits for
// it with preload_join(): those calls are not made while the worker runs. Never in applet mode (memory), and "preload.txt"
// = off turns it off.
Thread g_preload_thread;
bool g_preload_running = false;

void preload_entry(void*) { pokenx_preload_game(kGameDir); }

void preload_join() {
    if (!g_preload_running) return;
    threadWaitForExit(&g_preload_thread);
    threadClose(&g_preload_thread);
    g_preload_running = false;
}

void preload_start(const GameState& game, const Memory& memory) {
    if (g_preload_running || !game.found || memory.applet() || read_token("sdmc:/switch/pokenx/preload.txt", "on") == "off") return;
    if (R_FAILED(threadCreate(&g_preload_thread, preload_entry, nullptr, nullptr, 0x400000, 0x2C, -2))) return;
    if (R_FAILED(threadStart(&g_preload_thread))) {
        threadClose(&g_preload_thread);
        return;
    }
    g_preload_running = true;
}

// Last lines of a log file (the whole file is never loaded: only its final ~16 KiB).
std::vector<std::string> tail_lines(const char* path, size_t count, size_t max_chars = 110) {
    std::vector<std::string> lines;
    FILE* file = std::fopen(path, "r");
    if (!file) return lines;
    std::fseek(file, 0, SEEK_END);
    const long size = std::ftell(file);
    std::fseek(file, size > 16384 ? size - 16384 : 0, SEEK_SET);
    char line[400];
    while (std::fgets(line, sizeof(line), file)) {
        std::string text(line);
        while (!text.empty() && (text.back() == '\n' || text.back() == '\r')) text.pop_back();
        if (text.size() > max_chars) text.resize(max_chars);
        lines.push_back(text);
    }
    std::fclose(file);
    if (lines.size() > count) lines.erase(lines.begin(), lines.end() - static_cast<long>(count));
    return lines;
}

// "[  26479] text" -> 26479 (or -1).
long log_ms(const std::string& line) {
    if (line.size() < 3 || line[0] != '[') return -1;
    return std::atol(line.c_str() + 1);
}

// Milliseconds the last game load took, from the previous run's log (engine.log.1: the log is rotated at start-up):
// "game started" minus "Memory (before load_game)". -1 when unknown.
long last_load_seconds_x10() {
    static long cached = -2;  // the previous run's log does not change while the menu is open
    if (cached != -2) return cached;
    cached = -1;
    FILE* file = std::fopen("sdmc:/switch/pokenx/logs/engine.log.1", "r");
    if (!file) return -1;
    long begin = -1, done = -1;
    char line[400];
    while (std::fgets(line, sizeof(line), file)) {
        if (begin < 0 && std::strstr(line, "Memory (before load_game)")) begin = log_ms(line);
        else if (std::strstr(line, "game started")) { done = log_ms(line); break; }
    }
    std::fclose(file);
    cached = (begin >= 0 && done >= begin) ? (done - begin) / 100 : -1;
    return cached;
}

std::string seconds_text(long tenths) {
    char text[24];
    std::snprintf(text, sizeof(text), "%ld,%ld s", tenths / 10, tenths % 10);
    return text;
}

std::string size_text(const char* path) {
    if (!file_exists(path)) return "no existe";
    const unsigned long long size = file_size(path);
    if (size == 0) return "vacío";
    return size >= (1ULL << 20) ? mib(size) : std::to_string((size + 1023) >> 10) + " KiB";
}

// ---- Drawing helpers --------------------------------------------------------------------------------------------
enum class Icon { Play, Import, Update, Options, Pad, Keys, Home, Diag, Exit };

void draw_icon(Icon icon, int cx, int cy, int s, u32 color) {  // s: half-size in pixels
    switch (icon) {
        case Icon::Play:
            gfx::triangle(cx - s * 5 / 10, cy - s, cx - s * 5 / 10, cy + s, cx + s, cy, color);
            break;
        case Icon::Import:
            gfx::triangle(cx, cy - s, cx - s * 7 / 10, cy - s * 2 / 10, cx + s * 7 / 10, cy - s * 2 / 10, color);
            gfx::fill(cx - s * 2 / 10, cy - s * 2 / 10, s * 4 / 10, s * 8 / 10, color);
            gfx::fill(cx - s, cy + s * 8 / 10, s * 2, s * 2 / 10 + 1, color);
            break;
        case Icon::Update:
            gfx::circle(cx, cy, s, color);
            gfx::triangle(cx, cy + s * 55 / 100, cx - s * 4 / 10, cy, cx + s * 4 / 10, cy, kInk);
            gfx::fill(cx - s * 1 / 10, cy - s * 5 / 10, s * 2 / 10 + 1, s * 5 / 10 + 1, kInk);
            break;
        case Icon::Keys:
            gfx::rrect(cx - s, cy - s * 6 / 10, s * 2, s * 12 / 10, s / 5, color);
            for (int row = 0; row < 2; ++row)
                for (int col = 0; col < 4; ++col)
                    gfx::fill(cx - s * 8 / 10 + col * s * 45 / 100, cy - s * 4 / 10 + row * s * 4 / 10, s * 3 / 10, s * 2 / 10, kInk);
            gfx::fill(cx - s * 5 / 10, cy + s * 3 / 10, s * 10 / 10, s * 2 / 10, kInk);
            break;
        case Icon::Options:
            for (int i = 0; i < 3; ++i) {
                const int y = cy - s * 6 / 10 + i * s * 6 / 10;
                gfx::fill(cx - s, y - 1, s * 2, 3, color);
                gfx::circle(cx - s * 4 / 10 + (i == 1 ? s * 8 / 10 : (i == 2 ? s * 2 / 10 : 0)), y, std::max(3, s / 5), color);
            }
            break;
        case Icon::Pad:
            gfx::rrect(cx - s, cy - s * 5 / 10, s * 2, s * 10 / 10, s / 2, color);
            gfx::fill(cx - s * 6 / 10, cy - s * 1 / 10, s * 5 / 10 + 1, 3, kInk);
            gfx::fill(cx - s * 4 / 10 - 1, cy - s * 3 / 10, 3, s * 5 / 10 + 1, kInk);
            gfx::circle(cx + s * 4 / 10, cy - s * 1 / 10, std::max(2, s / 8), kInk);
            gfx::circle(cx + s * 7 / 10, cy + s * 1 / 10, std::max(2, s / 8), kInk);
            break;
        case Icon::Home:
            gfx::triangle(cx - s, cy - s * 1 / 10, cx, cy - s, cx + s, cy - s * 1 / 10, color);
            gfx::fill(cx - s * 7 / 10, cy - s * 1 / 10, s * 14 / 10, s * 9 / 10, color);
            gfx::fill(cx - s * 15 / 100, cy + s * 3 / 10, s * 3 / 10, s * 5 / 10, kInk);
            break;
        case Icon::Diag:
            for (int i = 0; i < 3; ++i) {
                const int y = cy - s * 6 / 10 + i * s * 6 / 10;
                gfx::circle(cx - s * 7 / 10, y, std::max(2, s / 9), color);
                gfx::fill(cx - s * 3 / 10, y - 1, s * 13 / 10, 3, color);
            }
            break;
        case Icon::Exit:
            gfx::line(cx - s * 7 / 10, cy - s * 7 / 10, cx + s * 7 / 10, cy + s * 7 / 10, std::max(4, s / 4), color);
            gfx::line(cx - s * 7 / 10, cy + s * 7 / 10, cx + s * 7 / 10, cy - s * 7 / 10, std::max(4, s / 4), color);
            break;
    }
}

struct Chip {
    std::string label;
    u32 color;
    bool dot = false;  // a status dot drawn before the text (the system font has no reliable bullet glyph)
};

int draw_chip(int right, int y, const Chip& chip) {  // returns the new right edge
    const int dot = chip.dot ? 18 : 0;
    const int width = gfx::text_width(chip.label, 15) + 26 + dot;
    gfx::rrect(right - width, y, width, 28, 14, kChip);
    if (chip.dot) gfx::circle(right - width + 20, y + 14, 5, chip.color);
    gfx::text(right - width + 13 + dot, y + 4, chip.label, 15, chip.dot ? kText : chip.color);
    return right - width - 10;
}

void draw_top(const std::string& crumb, const std::vector<Chip>& chips) {
    int x = 64;
    const std::string title = "POKENX";
    for (char c : title) {
        const std::string one(1, c);
        gfx::text(x, 28, one, 30, kGold);
        x += gfx::text_width(one, 30) + 4;
    }
    if (!crumb.empty()) gfx::text(x + 18, 40, crumb, 20, kDim);
    int right = gfx::kWidth - 64;
    for (const Chip& chip : chips) right = draw_chip(right, 34, chip);
}

struct Hint {
    std::string key;
    std::string label;
};

void draw_foot(const std::vector<Hint>& hints) {
    int total = 0;
    for (const Hint& hint : hints) total += gfx::text_width(hint.key, 15) + 22 + 8 + gfx::text_width(hint.label, 16) + 30;
    int x = (gfx::kWidth - total) / 2;
    for (const Hint& hint : hints) {
        const int key_w = gfx::text_width(hint.key, 15) + 22;
        gfx::rrect(x, 672, key_w, 26, 13, kLine);
        gfx::text(x + 11, 675, hint.key, 15, kText);
        x += key_w + 8;
        gfx::text(x, 674, hint.label, 16, kDim);
        x += gfx::text_width(hint.label, 16) + 30;
    }
}

void draw_panel(int x, int y, int w, int h) {
    gfx::rrect(x, y, w, h, 14, kLine);
    gfx::rrect(x + 1, y + 1, w - 2, h - 2, 13, kPanel);
}

void draw_row(int x, int y, int w, const std::string& label, const std::string& value, const std::string& hint, bool selected,
              u32 value_color = kGold) {
    if (selected) {
        gfx::rrect(x, y, w, 46, 10, kRowSel);
        gfx::rrect(x, y + 4, 5, 38, 2, kGold);
    }
    gfx::text(x + 22, y + 10, gfx::ellipsize(label, 22, w / 2), 22, selected ? kText : rgba(201, 208, 226));
    if (!value.empty()) gfx::text_right(x + w - 20, y + 10, gfx::ellipsize(value, 22, w / 2 - 20), 22, value_color);
    if (!hint.empty() && selected) {
        const int hint_x = x + 22 + gfx::text_width(label, 22) + 18;
        const int room = x + w - 24 - gfx::text_width(value, 22) - hint_x;
        if (room > 60) gfx::text(hint_x, y + 15, gfx::ellipsize(hint, 14, room), 14, kDim);
    }
}

// A frame: background + header + footer, then `body()`.
template <typename F>
void frame(const std::string& crumb, const std::vector<Chip>& chips, const std::vector<Hint>& hints, F body) {
    gfx::begin();
    draw_top(crumb, chips);
    body();
    draw_foot(hints);
    gfx::end();
}

std::vector<Chip> game_chips(const GameState& game, const Memory& memory) {
    std::vector<Chip> chips;
    chips.push_back({mib(memory.total) + (memory.applet() ? " · modo applet" : " · modo título"), memory.applet() ? kWarn : kText});
    chips.push_back({game.found ? "Juego detectado" : "Sin juego", game.found ? kOk : kBad, true});
    return chips;  // drawn right to left: the last one ends up leftmost
}

// ---- Modal card -------------------------------------------------------------------------------------------------
// `text` wrapped into lines of at most `max_width` pixels at `size`.
std::vector<std::string> wrap_lines(const std::string& text, int size, int max_width) {
    std::vector<std::string> lines;
    std::string line, word;
    for (char c : text + " ") {
        if (c != ' ') {
            word += c;
            continue;
        }
        const std::string candidate = line.empty() ? word : line + " " + word;
        if (gfx::text_width(candidate, size) > max_width && !line.empty()) {
            lines.push_back(line);
            line = word;
        } else {
            line = candidate;
        }
        word.clear();
    }
    if (!line.empty()) lines.push_back(line);
    return lines;
}

// Returns the chosen option index, or -1 when cancelled with B. The card grows with its text.
int show_card(const std::string& title, const std::string& text, const std::vector<std::string>& options, u32 title_color) {
    int selected = 0;
    const int w = 860, x = (gfx::kWidth - w) / 2, y = 150;
    const std::vector<std::string> lines = wrap_lines(text, 20, w - 72);
    const int text_height = static_cast<int>(lines.size()) * 28;
    const int h = 84 + text_height + 24 + static_cast<int>(options.size()) * 52 + 24;
    while (appletMainLoop()) {
        const Input input = poll_input();
        if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_Plus)) return -1;
        if (pressed(input, HidNpadButton_Down) && !options.empty()) selected = (selected + 1) % static_cast<int>(options.size());
        if (pressed(input, HidNpadButton_Up) && !options.empty())
            selected = (selected + static_cast<int>(options.size()) - 1) % static_cast<int>(options.size());
        if (pressed(input, HidNpadButton_A)) return selected;
        frame("Aviso", {}, {{"A", "elegir"}, {"B", "volver"}}, [&] {
            draw_panel(x, y, w, h);
            gfx::text(x + 36, y + 26, title, 30, title_color);
            int ty = y + 84;
            for (const std::string& line : lines) {
                gfx::text(x + 36, ty, line, 20, rgba(201, 208, 226));
                ty += 28;
            }
            for (size_t i = 0; i < options.size(); ++i)
                draw_row(x + 24, y + h - 24 - static_cast<int>(options.size() - i) * 52, w - 48, options[i], "", "", static_cast<int>(i) == selected);
        });
    }
    return -1;
}

// ---- Options ----------------------------------------------------------------------------------------------------
struct Choice {
    const char* token;
    const char* label;
};

struct Setting {
    const char* title;
    const char* file;
    std::vector<Choice> choices;
    const char* help;
};

const std::vector<Setting>& settings() {
    static const std::vector<Setting> list = {
        {"Resolución", "sdmc:/switch/pokenx/resolution.txt", {{"720p", "720p"}, {"1080p", "1080p"}},
         "720p es la salida recomendada: 1280×720, 16:9, con presentación objetivo de 60 Hz. 1080p exige más a la GPU en portátil."},
        {"Escalado", "sdmc:/switch/pokenx/scaling.txt", {{"full", "Pantalla completa"}, {"aspect", "4:3 con bandas"}},
         "Pantalla completa estira el juego a 16:9. 4:3 con bandas conserva la forma para la que se dibujó el juego."},
        // The graphics driver (nvc0 / zink) is not offered here: Zink does not present a frame yet. The code and
        // graphics-driver.txt still work for development (see docs/arquitectura/zink.md).
        {"Rendimiento", "sdmc:/switch/pokenx/performance.txt", {{"max", "Máximo"}, {"balanced", "Equilibrado"}, {"stock", "Sin cambios"}},
         "Máximo sube CPU a 1785 MHz y GPU según el modo. Equilibrado usa menos batería y calor. Sin cambios no toca los relojes."},
        {"Caché de archivos", "sdmc:/switch/pokenx/filecache.txt", {{"on", "Activada"}, {"off", "Desactivada"}},
         "El juego relee los mismos archivos de datos cada vez que cambia de escena. La caché los guarda en memoria: la primera vez tarda, las siguientes casi no."},
        {"Zona muerta del palo", "sdmc:/switch/pokenx/stickzone.txt", {{"small", "Pequeña"}, {"normal", "Normal"}, {"large", "Grande"}},
         "Cuánto hay que inclinar el palo izquierdo para que cuente como una flecha. Pequeña responde antes pero puede dar pulsaciones sin querer; Grande exige inclinarlo más."},
        {"Precargar el juego", "sdmc:/switch/pokenx/preload.txt", {{"on", "Activada"}, {"off", "Desactivada"}},
         "Mientras estás en el menú se lee y analiza el juego en segundo plano, y «Jugar» se ahorra esos segundos. Solo en modo título (necesita memoria). Se aplica al volver a abrir el programa."},
        {"Medidor de FPS", "sdmc:/switch/pokenx/fps.txt", {{"off", "Apagado"}, {"on", "Encendido"}},
         "Muestra en una esquina del juego los fotogramas por segundo y el hueco más largo entre dos fotogramas (en milisegundos). Verde: va bien; amarillo o rojo: por debajo de 60. Las pausas del propio juego (diálogos y escenas) no cuentan como caída: si hubo, una tercera línea «P» da sus milisegundos."},
    };
    return list;
}

const char* const kSettingDefaults[] = {"720p", "full", "max", "on", "normal", "on", "off"};

size_t current_choice(const Setting& setting, size_t index) {
    const std::string value = read_token(setting.file, kSettingDefaults[index]);
    for (size_t i = 0; i < setting.choices.size(); ++i)
        if (value == setting.choices[i].token) return i;
    return 0;
}

void show_options(const GameState& game) {
    int selected = 0;
    const int count = static_cast<int>(settings().size());
    const bool soundfont = file_exists("sdmc:/switch/pokenx/soundfont.sf2");
    std::vector<size_t> values;  // current choice per setting; files are read once and after each change
    const auto reload = [&] {
        values.clear();
        for (int i = 0; i < count; ++i) values.push_back(current_choice(settings()[i], static_cast<size_t>(i)));
    };
    reload();
    while (appletMainLoop()) {
        const Input input = poll_input();
        if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_Plus)) return;
        if (pressed(input, HidNpadButton_Down)) selected = (selected + 1) % count;
        if (pressed(input, HidNpadButton_Up)) selected = (selected + count - 1) % count;
        const Setting& setting = settings()[selected];
        if (pressed(input, HidNpadButton_A) || pressed(input, HidNpadButton_Right)) {
            const size_t next = (values[selected] + 1) % setting.choices.size();
            write_token(setting.file, setting.choices[next].token);
            reload();
        } else if (pressed(input, HidNpadButton_Left)) {
            const size_t n = setting.choices.size();
            write_token(setting.file, setting.choices[(values[selected] + n - 1) % n].token);
            reload();
        }
        if (pressed(input, HidNpadButton_Y)) {
            for (int i = 0; i < count; ++i) write_token(settings()[i].file, kSettingDefaults[i]);
            reload();
        }
        const Memory memory = read_memory();
        frame("Opciones", game_chips(game, memory), {{"Arriba/Abajo", "mover"}, {"Izq/Der · A", "cambiar"}, {"Y", "restablecer"}, {"B", "volver"}}, [&] {
            const int px = 64, py = 100, pw = 700, ph = 540;
            draw_panel(px, py, pw, ph);
            for (int i = 0; i < count; ++i) {
                const Setting& item = settings()[i];
                draw_row(px + 16, py + 14 + i * 50, pw - 32, item.title, item.choices[values[i]].label, "", i == selected);
            }
            draw_row(px + 16, py + 14 + count * 50, pw - 32, "Soundfont MIDI",
                     soundfont ? "Encontrada" : "No encontrada", "", false, soundfont ? kOk : kWarn);
            gfx::text(px + 38, py + ph - 52, "Los cambios se aplican la próxima vez que pulses Jugar.", 15, kDim);
            draw_panel(790, 100, 426, 540);
            gfx::text(816, 124, setting.title, 24, kGold);
            // wrapped help
            std::string line, word;
            int ty = 170;
            for (char c : std::string(setting.help) + " ") {
                if (c == ' ') {
                    const std::string candidate = line.empty() ? word : line + " " + word;
                    if (gfx::text_width(candidate, 18) > 374 && !line.empty()) {
                        gfx::text(816, ty, line, 18, rgba(201, 208, 226));
                        ty += 26;
                        line = word;
                    } else {
                        line = candidate;
                    }
                    word.clear();
                } else {
                    word += c;
                }
            }
            if (!line.empty()) gfx::text(816, ty, line, 18, rgba(201, 208, 226));
            gfx::text(816, 590, std::string("Archivo: ") + short_name(setting.file), 14, kDim);
        });
    }
}

int draw_wrapped(int x, int y, int max_width, const std::string& text, int size, u32 color, int line_height, int max_lines = 0);

// ---- Controls ---------------------------------------------------------------------------------------------------
// Profiles: "Por defecto" (the factory layout, read-only), "Perfil 1" (controls.txt) and "Perfil 2" (controls-2.txt);
// profile.txt says which one the game uses. Four tabs: "Botones" (the 12 buttons and the key each sends), "Capa 1" (hold a
// modifier button, ZL by default, and press another button to send a key no single button can: F1-F12, Supr, Inicio, Fin,
// Re Pág, Av Pág...), "Capa 2" (the same with ZR: the digits and the letters of menus and field moves) and "Vista"
// (the controller drawn with the keys on each button).
// The file is controls.txt; gm8-input's ButtonMap::parse reads the same lines (LAYER / LAYER2).
struct PadButton {
    const char* name;      // controls.txt name
    const char* label;     // shown
    u64 hid;
    const char* def_key;
};

const PadButton kPadButtons[] = {
    {"A", "A", HidNpadButton_A, "Z"},          {"B", "B", HidNpadButton_B, "X"},
    {"X", "X", HidNpadButton_X, "C"},          {"Y", "Y", HidNpadButton_Y, "V"},
    {"L", "L", HidNpadButton_L, "M"},          {"R", "R", HidNpadButton_R, "B"},
    {"ZL", "ZL", HidNpadButton_ZL, "S"},       {"ZR", "ZR", HidNpadButton_ZR, "O"},
    {"PLUS", "+", HidNpadButton_Plus, "SPACE"}, {"MINUS", "-", HidNpadButton_Minus, "ENTER"},
    {"STICK_L", "Clic palo izq.", HidNpadButton_StickL, "E"}, {"STICK_R", "Clic palo der.", HidNpadButton_StickR, "ESC"},
};
constexpr int kPadButtonCount = 12;

// Buttons that can carry a layer key, with the default of gm8-input's LayerMap::DEFAULT ("NONE" = nothing).
struct LayerButton {
    const char* name;
    const char* label;
    u64 hid;
    const char* def_key;   // first layer (ZL)
    const char* def_key2;  // second layer (ZR)
};

const LayerButton kLayerButtons[] = {
    {"A", "A", HidNpadButton_A, "F5", "1"},         {"B", "B", HidNpadButton_B, "F6", "2"},
    {"X", "X", HidNpadButton_X, "F11", "3"},        {"Y", "Y", HidNpadButton_Y, "F12", "4"},
    {"L", "L", HidNpadButton_L, "F10", "5"},        {"R", "R", HidNpadButton_R, "DELETE", "6"},
    {"ZL", "ZL", HidNpadButton_ZL, "NONE", "NONE"}, {"ZR", "ZR", HidNpadButton_ZR, "NONE", "NONE"},
    {"PLUS", "+", HidNpadButton_Plus, "HOME", "T"}, {"MINUS", "-", HidNpadButton_Minus, "END", "N"},
    {"UP", "Cruz arriba", HidNpadButton_Up, "F1", "Q"}, {"DOWN", "Cruz abajo", HidNpadButton_Down, "F9", "P"},
    {"LEFT", "Cruz izquierda", HidNpadButton_Left, "PAGEUP", "L"}, {"RIGHT", "Cruz derecha", HidNpadButton_Right, "PAGEDOWN", "F"},
    {"STICK_L", "Clic palo izq.", HidNpadButton_StickL, "NONE", "H"}, {"STICK_R", "Clic palo der.", HidNpadButton_StickR, "NONE", "K"},
};
constexpr int kLayerButtonCount = 16;
constexpr int kLayerCount = 2;
const char* const kModifierNames[] = {"ZL", "ZR", "L", "R", "NONE"};
constexpr int kModifierCount = 5;

// Same vocabulary as gm8_input::Key::from_name.
const char* const kKeyNames[] = {"Z", "X", "C", "V", "B", "N", "M", "A", "S", "D", "F", "G", "H", "J", "K", "L", "Q", "W", "E", "R",
                                  "T", "Y", "U", "I", "O", "P", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
                                  "SPACE", "ENTER", "ESC", "TAB", "SHIFT", "CTRL", "ALT", "BACKSPACE", "DELETE", "INSERT",
                                  "HOME", "END", "PAGEUP", "PAGEDOWN", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9",
                                  "F10", "F11", "F12"};
constexpr int kKeyCount = sizeof(kKeyNames) / sizeof(kKeyNames[0]);

std::string key_label(const std::string& name) {
    if (name == "SPACE") return "Espacio";
    if (name == "ENTER") return "Intro";
    if (name == "ESC") return "Esc";
    if (name == "TAB") return "Tab";
    if (name == "SHIFT") return "Mayús";
    if (name == "CTRL") return "Ctrl";
    if (name == "ALT") return "Alt";
    if (name == "BACKSPACE") return "Retroceso";
    if (name == "DELETE") return "Supr";
    if (name == "INSERT") return "Ins";
    if (name == "HOME") return "Inicio";
    if (name == "END") return "Fin";
    if (name == "PAGEUP") return "Re Pág";
    if (name == "PAGEDOWN") return "Av Pág";
    if (name == "NONE") return "Ninguna";
    if (name == "UP") return "Flecha arriba";
    if (name == "DOWN") return "Flecha abajo";
    if (name == "LEFT") return "Flecha izq.";
    if (name == "RIGHT") return "Flecha der.";
    if (!name.empty() && name[0] == '#') return "Código " + name.substr(1);
    return name;
}

bool is_known_key(const std::string& key) {
    for (const char* known : kKeyNames)
        if (key == known) return true;
    return false;
}

struct ControlMap {
    std::string key[kPadButtonCount];
    std::string modifier[kLayerCount] = {"ZL", "ZR"};  // ZL, ZR, L, R or NONE
    std::string layer[kLayerCount][kLayerButtonCount];  // indexed like kLayerButtons
};

ControlMap default_controls() {
    ControlMap map;
    for (int i = 0; i < kPadButtonCount; ++i) map.key[i] = kPadButtons[i].def_key;
    for (int i = 0; i < kLayerButtonCount; ++i) {
        map.layer[0][i] = kLayerButtons[i].def_key;
        map.layer[1][i] = kLayerButtons[i].def_key2;
    }
    return map;
}

std::string trim_upper(std::string value) {
    while (!value.empty() && std::isspace(static_cast<unsigned char>(value.back()))) value.pop_back();
    size_t start = 0;
    while (start < value.size() && std::isspace(static_cast<unsigned char>(value[start]))) ++start;
    value = value.substr(start);
    for (char& c : value) c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
    return value;
}

constexpr const char* kProfileFile = "sdmc:/switch/pokenx/profile.txt";
constexpr const char* kControlsFile2 = "sdmc:/switch/pokenx/controls-2.txt";

// "default", "1" or "2". Profile 1 is the original controls.txt, so existing layouts keep working.
std::string active_profile() {
    const std::string token = read_token(kProfileFile, "1");
    return token == "default" || token == "2" ? token : "1";
}

const char* profile_file(const std::string& profile) { return profile == "2" ? kControlsFile2 : (profile == "1" ? kControlsFile : nullptr); }

std::string profile_label(const std::string& profile) { return profile == "default" ? "Por defecto" : "Perfil " + profile; }

ControlMap load_controls() {
    ControlMap map = default_controls();
    const char* file_path = profile_file(active_profile());
    if (!file_path) return map;
    if (FILE* file = std::fopen(file_path, "r")) {
        char line[96];
        while (std::fgets(line, sizeof(line), file)) {
            std::string text(line);
            const size_t hash = text.find('#');
            if (hash != std::string::npos) text.resize(hash);
            const size_t eq = text.find('=');
            if (eq == std::string::npos) continue;
            const std::string button = trim_upper(text.substr(0, eq)), key = trim_upper(text.substr(eq + 1));
            const int which = button.compare(0, 6, "LAYER2") == 0 ? 1 : (button.compare(0, 5, "LAYER") == 0 ? 0 : -1);
            const size_t head = which == 1 ? 6 : 5;
            if (which >= 0 && button.size() == head) {
                for (const char* name : kModifierNames)
                    if (key == name) map.modifier[which] = key;
            } else if (which >= 0 && button.size() > head && button[head] == '.') {
                for (int i = 0; i < kLayerButtonCount; ++i)
                    if (button.substr(head + 1) == kLayerButtons[i].name && (key == "NONE" || is_known_key(key))) map.layer[which][i] = key;
            } else {
                for (int i = 0; i < kPadButtonCount; ++i)
                    if (button == kPadButtons[i].name && is_known_key(key)) map.key[i] = key;
            }
        }
        std::fclose(file);
    }
    return map;
}

void save_controls(const ControlMap& map) {
    const char* file_path = profile_file(active_profile());
    if (!file_path) return;  // the default profile is never written
    if (FILE* file = std::fopen(file_path, "w")) {
        std::fprintf(file, "# PokeNX: BOTON=TECLA del juego (la tecla es la que GameMaker recibe). Se edita desde Mandos.\n");
        for (int i = 0; i < kPadButtonCount; ++i) std::fprintf(file, "%s=%s\n", kPadButtons[i].name, map.key[i].c_str());
        std::fprintf(file, "# Capa: mantén el botón LAYER y pulsa otro para enviar su tecla (NONE = ninguna).\n");
        std::fprintf(file, "LAYER=%s\n", map.modifier[0].c_str());
        for (int i = 0; i < kLayerButtonCount; ++i) std::fprintf(file, "LAYER.%s=%s\n", kLayerButtons[i].name, map.layer[0][i].c_str());
        std::fprintf(file, "# Capa 2: lo mismo con otro botón (por defecto ZR): cifras y letras de menús.\n");
        std::fprintf(file, "LAYER2=%s\n", map.modifier[1].c_str());
        for (int i = 0; i < kLayerButtonCount; ++i) std::fprintf(file, "LAYER2.%s=%s\n", kLayerButtons[i].name, map.layer[1][i].c_str());
        std::fclose(file);
    }
}

const char* modifier_label(const std::string& name) {
    if (name == "NONE") return "Sin capa";
    for (const PadButton& button : kPadButtons)
        if (name == button.name) return button.label;
    return "ZL";
}

u64 modifier_hid(const std::string& name) {
    for (const PadButton& button : kPadButtons)
        if (name == button.name) return button.hid;
    return 0;
}

// Which Switch input produces `key`, as short text ("A", "ZL + A", "Cruz / palo"), or "" when none can.
std::string producing(const ControlMap& map, const std::string& key) {
    if (key == "UP" || key == "DOWN" || key == "LEFT" || key == "RIGHT") return "Cruz / palo";
    std::string result;
    for (int i = 0; i < kPadButtonCount; ++i) {
        if (map.key[i] != key) continue;
        result += (result.empty() ? "" : ", ") + std::string(kPadButtons[i].label);
    }
    for (int which = 0; which < kLayerCount; ++which) {
        if (map.modifier[which] == "NONE") continue;
        for (int i = 0; i < kLayerButtonCount; ++i) {
            if (map.layer[which][i] != key || modifier_hid(map.modifier[which]) == kLayerButtons[i].hid) continue;
            result += (result.empty() ? "" : ", ") + std::string(modifier_label(map.modifier[which])) + " + " + kLayerButtons[i].label;
        }
    }
    return result;
}

// The buttons of layer `which` that can carry a key: every button except the modifiers of both layers (a modifier held
// together with the other one never sends a layer key).
std::vector<int> layer_items(const ControlMap& map, int which) {
    (void)which;
    std::vector<int> items;
    for (int i = 0; i < kLayerButtonCount; ++i) {
        bool is_modifier = false;
        for (int w = 0; w < kLayerCount; ++w) is_modifier = is_modifier || (map.modifier[w] != "NONE" && modifier_hid(map.modifier[w]) == kLayerButtons[i].hid);
        if (!is_modifier) items.push_back(i);
    }
    return items;
}

// The next modifier for layer `which`, skipping the one the other layer already uses.
std::string next_modifier(const ControlMap& map, int which) {
    int at = 0;
    for (int i = 0; i < kModifierCount; ++i)
        if (map.modifier[which] == kModifierNames[i]) at = i;
    for (int step = 1; step <= kModifierCount; ++step) {
        const std::string candidate = kModifierNames[(at + step) % kModifierCount];
        if (candidate == "NONE" || candidate != map.modifier[1 - which]) return candidate;
    }
    return "NONE";
}

const u32 kLayer2Color = rgba(120, 180, 255);

// One button of the controller drawing: its name, the key it sends and, below, the key it sends under each layer
// (green: first layer, blue: second layer).
void draw_button_box(int x, int y, int w, int h, const std::string& name, const std::string& key, const std::string& layer_key,
                     const std::string& layer2_key, bool held) {
    gfx::rrect(x, y, w, h, 10, held ? kOk : kLine);
    gfx::rrect(x + 2, y + 2, w - 4, h - 4, 9, held ? rgba(30, 70, 50) : kTile);
    gfx::text_center(x + w / 2, y + 3, name, 12, kDim);
    gfx::text_center(x + w / 2, y + 15, gfx::ellipsize(key, 18, w - 8), 18, kGold);
    if (!layer_key.empty()) gfx::text_center(x + w / 2, y + h - 33, gfx::ellipsize(layer_key, 13, w - 8), 13, kOk);
    if (!layer2_key.empty()) gfx::text_center(x + w / 2, y + h - 18, gfx::ellipsize(layer2_key, 13, w - 8), 13, kLayer2Color);
}

void draw_pad_view(const ControlMap& map, u64 held) {
    const auto base_of = [&](const char* name) -> std::string {
        for (int i = 0; i < kPadButtonCount; ++i)
            if (std::string(kPadButtons[i].name) == name) return key_label(map.key[i]);
        return "";
    };
    const auto layer_of = [&](const char* name, int which) -> std::string {
        if (map.modifier[which] == "NONE") return "";
        for (int i = 0; i < kLayerButtonCount; ++i)
            if (std::string(kLayerButtons[i].name) == name && map.layer[which][i] != "NONE" && modifier_hid(map.modifier[which]) != kLayerButtons[i].hid)
                return std::string(modifier_label(map.modifier[which])) + "+ " + key_label(map.layer[which][i]);
        return "";
    };
    const auto hid_of = [&](const char* name) -> u64 {
        for (int i = 0; i < kPadButtonCount; ++i)
            if (std::string(kPadButtons[i].name) == name) return kPadButtons[i].hid;
        for (int i = 0; i < kLayerButtonCount; ++i)
            if (std::string(kLayerButtons[i].name) == name) return kLayerButtons[i].hid;
        return 0;
    };
    const auto button = [&](int x, int y, int w, int h, const char* name, const char* label, bool arrows = false) {
        const std::string base = arrows ? key_label(name == std::string("UP") ? "UP" : name == std::string("DOWN") ? "DOWN" : name == std::string("LEFT") ? "LEFT" : "RIGHT") : base_of(name);
        draw_button_box(x, y, w, h, label, base, layer_of(name, 0), layer_of(name, 1), (held & hid_of(name)) != 0);
    };
    const int top = 112;
    button(82, top, 112, 84, "ZL", "ZL");
    button(202, top, 112, 84, "L", "L");
    button(504, top, 112, 84, "R", "R");
    button(624, top, 112, 84, "ZR", "ZR");
    button(328, top, 80, 84, "MINUS", "-");
    button(410, top, 80, 84, "PLUS", "+");
    // left stick (arrows) and its click
    gfx::circle(170, 300, 46, kLine);
    gfx::circle(170, 300, 42, kTile);
    gfx::text_center(170, 286, "Palo", 14, kDim);
    gfx::text_center(170, 304, "flechas", 15, kGold);
    button(120, 352, 100, 70, "STICK_L", "Clic palo");
    // d-pad
    button(120, 430, 100, 70, "UP", "Cruz arriba", true);
    button(66 + 4, 504, 100, 70, "LEFT", "Cruz izq.", true);
    button(176, 504, 100, 70, "RIGHT", "Cruz der.", true);
    button(120, 578, 100, 70, "DOWN", "Cruz abajo", true);
    // face buttons: X top, Y left, A right, B bottom
    button(540, 236, 100, 84, "X", "X");
    button(484, 326, 100, 84, "Y", "Y");
    button(596, 326, 100, 84, "A", "A");
    button(540, 416, 100, 84, "B", "B");
    button(360, 520, 120, 70, "STICK_R", "Clic palo der.");
    gfx::text(330, 236, "Dorado: la tecla normal.", 14, kDim);
    gfx::text(330, 258, "Verde: con la capa 1.", 14, kOk);
    gfx::text(330, 280, "Azul: con la capa 2.", 14, kLayer2Color);
}

void show_controls(const GameState& game) {
    ControlMap map = load_controls();
    int tab = 0;  // 0 = Botones, 1 = Capa 1, 2 = Capa 2, 3 = Vista
    int selected = 0;
    bool capturing = false;
    int choice = 0;
    const auto row_count = [&] {
        if (tab == 0) return kPadButtonCount;
        if (tab == 3) return 1;
        return 1 + static_cast<int>(layer_items(map, tab - 1).size());
    };
    // The button of kLayerButtons that row `selected` of a layer tab stands for.
    const auto layer_item = [&](int row) { return layer_items(map, tab - 1)[static_cast<size_t>(row - 1)]; };
    std::string profile = active_profile();
    while (appletMainLoop()) {
        const Input input = poll_input();
        if (selected >= row_count()) selected = 0;
        const int count = row_count();
        const bool editable = profile != "default";
        const int choices = tab == 0 ? kKeyCount : kKeyCount + 1;  // the layer picker also offers "Ninguna"
        if (capturing) {
            if (pressed(input, HidNpadButton_B)) capturing = false;
            if (pressed(input, HidNpadButton_Right) || pressed(input, HidNpadButton_Down)) choice = (choice + 1) % choices;
            if (pressed(input, HidNpadButton_Left) || pressed(input, HidNpadButton_Up)) choice = (choice + choices - 1) % choices;
            if (pressed(input, HidNpadButton_A)) {
                const std::string picked = choice < kKeyCount ? kKeyNames[choice] : "NONE";
                if (tab == 0) map.key[selected] = picked;
                else map.layer[tab - 1][layer_item(selected)] = picked;
                save_controls(map);
                capturing = false;
            }
        } else {
            if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_Plus)) return;
            if (pressed(input, HidNpadButton_L)) {
                tab = (tab + 3) % 4;
                selected = 0;
            }
            if (pressed(input, HidNpadButton_R)) {
                tab = (tab + 1) % 4;
                selected = 0;
            }
            if (pressed(input, HidNpadButton_ZL) || pressed(input, HidNpadButton_ZR)) {
                const char* order[] = {"default", "1", "2"};
                int at = 0;
                for (int i = 0; i < 3; ++i)
                    if (profile == order[i]) at = i;
                at = (at + (pressed(input, HidNpadButton_ZR) ? 1 : 2)) % 3;
                profile = order[at];
                write_token(kProfileFile, profile.c_str());
                map = load_controls();
                selected = 0;
            }
            if (pressed(input, HidNpadButton_Down)) selected = (selected + 1) % count;
            if (pressed(input, HidNpadButton_Up)) selected = (selected + count - 1) % count;
            if (pressed(input, HidNpadButton_A) && tab == 3) {
                // the view is read-only
            } else if (pressed(input, HidNpadButton_A) && !editable) {
                show_card("El perfil por defecto no se edita", "Es la distribución de fábrica. Cambia a «Perfil 1» o «Perfil 2» con ZL o ZR y modifica ahí lo que quieras.", {"Entendido"}, kWarn);
            } else if (pressed(input, HidNpadButton_A)) {
                if ((tab == 1 || tab == 2) && selected == 0) {
                    map.modifier[tab - 1] = next_modifier(map, tab - 1);
                    save_controls(map);
                } else {
                    capturing = true;
                    const std::string& current = tab == 0 ? map.key[selected] : map.layer[tab - 1][layer_item(selected)];
                    choice = current == "NONE" ? kKeyCount : 0;
                    for (int i = 0; i < kKeyCount; ++i)
                        if (current == kKeyNames[i]) choice = i;
                }
            }
            if (pressed(input, HidNpadButton_Y)) {
                if (const char* file_path = profile_file(profile)) std::remove(file_path);
                map = load_controls();
            }
        }
        const Memory memory = read_memory();
        std::vector<Hint> hints = capturing ? std::vector<Hint>{{"Izq/Der", "cambiar tecla"}, {"A", "aceptar"}, {"B", "cancelar"}}
                                            : std::vector<Hint>{{"Arriba/Abajo", "mover"}, {"A", "reasignar"}, {"L/R", "pestaña"}, {"ZL/ZR", "perfil"}, {"Y", "restablecer"}, {"B", "volver"}};
        std::vector<Chip> chips = game_chips(game, memory);
        chips.push_back({profile_label(profile), editable ? kGold : kDim});
        const char* titles[] = {"Mandos · Botones", "Mandos · Capa 1", "Mandos · Capa 2", "Mandos · Vista"};
        frame(titles[tab], chips, hints, [&] {
            if (tab == 3) {
                draw_panel(64, 100, 690, 560);
                draw_pad_view(map, input.held);
                draw_panel(780, 100, 436, 560);
                gfx::text(806, 124, profile_label(profile), 24, kGold);
                int y = draw_wrapped(806, 170, 384, editable ? "Así queda tu mando con este perfil. Cambia de perfil con ZL o ZR; edita en las pestañas Botones, Capa 1 y Capa 2." : "La distribución de fábrica; no se edita. Cambia a Perfil 1 o Perfil 2 con ZL o ZR para personalizar.", 17, rgba(201, 208, 226), 25);
                y = draw_wrapped(806, y + 12, 384, "La cruz y el palo izquierdo siempre son las flechas. Pulsa botones del mando: se iluminan en verde.", 17, kDim, 25);
                draw_wrapped(806, y + 12, 384, "El juego guarda sus propias teclas dentro de cada partida; esta vista es lo que el mando envía, no lo que tengas configurado en el juego.", 15, kDim, 22);
                gfx::text(806, 620, "Salir del juego: L + R + +", 15, kDim);
                return;
            }
            draw_panel(64, 100, 690, 560);
            const int visible = 12;
            const int first = std::max(0, std::min(selected - visible / 2, count - visible));
            for (int i = first; i < count && i < first + visible; ++i) {
                const int y = 112 + (i - first) * 44;
                if (tab == 0) {
                    const bool is_held = (input.held & kPadButtons[i].hid) != 0;
                    draw_row(80, y, 658, kPadButtons[i].label, key_label(map.key[i]), "", i == selected, is_held ? kOk : kGold);
                } else if (i == 0) {
                    draw_row(80, y, 658, tab == 1 ? "Botón de la capa 1" : "Botón de la capa 2", modifier_label(map.modifier[tab - 1]), "", i == selected);
                } else {
                    const int which = tab - 1;
                    const LayerButton& item = kLayerButtons[layer_item(i)];
                    const std::string& key = map.layer[which][layer_item(i)];
                    const bool is_held = map.modifier[which] != "NONE" && (input.held & item.hid) != 0 && (input.held & modifier_hid(map.modifier[which])) != 0;
                    draw_row(80, y, 658, std::string(modifier_label(map.modifier[which])) + " + " + item.label, key_label(key), "", i == selected,
                             is_held ? kOk : (key == "NONE" ? kDim : kGold));
                }
            }
            draw_panel(780, 100, 436, 560);
            if (capturing) {
                const std::string title = tab == 0 ? std::string("Cambiar «") + kPadButtons[selected].label + "»"
                                                   : std::string("Cambiar «") + modifier_label(map.modifier[tab - 1]) + " + " + kLayerButtons[layer_item(selected)].label + "»";
                gfx::text(806, 124, title, 24, kGold);
                gfx::text(806, 170, "Elige la tecla del juego que debe", 17, rgba(201, 208, 226));
                gfx::text(806, 194, "producir este botón.", 17, rgba(201, 208, 226));
                int x = 806, y = 234;
                for (int i = 0; i < choices; ++i) {
                    const std::string label = key_label(i < kKeyCount ? kKeyNames[i] : "NONE");
                    const int w = std::max(30, gfx::text_width(label, 15) + 16);
                    if (x + w > 1196) { x = 806; y += 36; }
                    gfx::rrect(x, y, w, 30, 8, i == choice ? kGold : kLine);
                    gfx::text_center(x + w / 2, y + 5, label, 15, i == choice ? kInk : kText);
                    x += w + 5;
                }
            } else if (tab == 0) {
                gfx::text(806, 124, "Botones", 24, kGold);
                gfx::text(806, 170, "Pulsa botones del mando: se iluminan", 17, rgba(201, 208, 226));
                gfx::text(806, 194, "en verde y muestran la tecla del juego.", 17, rgba(201, 208, 226));
                int y = 250;
                int in_use = -1;  // the first layer whose modifier is held
                for (int w = 0; w < kLayerCount && in_use < 0; ++w)
                    if (modifier_hid(map.modifier[w]) != 0 && (input.held & modifier_hid(map.modifier[w])) != 0) in_use = w;
                for (int i = 0; i < kLayerButtonCount && in_use >= 0; ++i) {
                    if (!(input.held & kLayerButtons[i].hid) || map.layer[in_use][i] == "NONE" || kLayerButtons[i].hid == modifier_hid(map.modifier[in_use])) continue;
                    gfx::text(806, y, std::string(modifier_label(map.modifier[in_use])) + " + " + kLayerButtons[i].label + "  =  " + key_label(map.layer[in_use][i]), 21, kOk);
                    y += 32;
                }
                for (int i = 0; i < kPadButtonCount && in_use < 0; ++i) {
                    if (!(input.held & kPadButtons[i].hid)) continue;
                    gfx::text(806, y, std::string(kPadButtons[i].label) + "  =  " + key_label(map.key[i]), 22, kOk);
                    y += 34;
                }
                gfx::text(806, 548, "La cruz y el palo izquierdo siempre son las flechas.", 15, kDim);
                gfx::text(806, 570, "Las pestañas Capa 1 y 2 dan F1-F12, cifras, letras...", 15, kDim);
                gfx::text(806, 592, "Salir del juego: L + R + +", 15, kDim);
                gfx::text(806, 614, std::string("Archivo: ") + (profile_file(profile) ? short_name(profile_file(profile)) : std::string("(de fábrica)")), 14, kDim);
            } else {
                const int which = tab - 1;
                gfx::text(806, 124, which == 0 ? "Capa 1 · función y páginas" : "Capa 2 · cifras y letras", 24, kGold);
                const std::string base = map.modifier[which] == "NONE" ? std::string() : std::string(modifier_label(map.modifier[which]));
                std::string help;
                if (map.modifier[which] == "NONE") {
                    help = "Esta capa está desactivada. Elige un botón de capa en la primera fila para usarla.";
                } else if (which == 0) {
                    help = "El juego usa teclas que ningún botón da por sí solo (guardar F5, cargar F6, ayuda F1, páginas...). Mantén " + base +
                           " y pulsa otro botón: se envía la tecla de esta lista en lugar de la normal. Un toque corto de " + base +
                           " sigue enviando su tecla de siempre.";
                } else {
                    help = "Para las cifras y las letras que el mando no alcanza: 1-6 cambian de compañero o usan movimientos de campo, y las letras abren menús (misiones, entrenador, áreas, buscar...). Mantén " +
                           base + " y pulsa otro botón. Un toque corto de " + base + " sigue enviando su tecla de siempre. Si pulsas las dos capas a la vez, gana la primera.";
                }
                draw_wrapped(806, 170, 384, help, 17, rgba(201, 208, 226), 25);
                gfx::text(806, 596, std::string("Se guarda en ") + (profile_file(profile) ? short_name(profile_file(profile)) : std::string("(perfil de fábrica)")) + (which == 0 ? " (líneas LAYER)." : " (líneas LAYER2)."), 14, kDim);
            }
        });
    }
}

// The last `keep` lines of `path` that contain any of the needles (scans the whole file; used only when exporting a report).
std::vector<std::string> grep_lines(const char* path, std::initializer_list<const char*> needles, size_t keep, size_t max_chars = 220) {
    std::vector<std::string> found;
    FILE* file = std::fopen(path, "r");
    if (!file) return found;
    char line[500];
    while (std::fgets(line, sizeof(line), file)) {
        bool match = false;
        for (const char* needle : needles) match = match || std::strstr(line, needle) != nullptr;
        if (!match) continue;
        std::string text(line);
        while (!text.empty() && (text.back() == '\n' || text.back() == '\r')) text.pop_back();
        if (text.size() > max_chars) text.resize(max_chars);
        found.push_back(text);
        if (found.size() > keep) found.erase(found.begin());
    }
    std::fclose(file);
    return found;
}

// ---- Diagnostics ------------------------------------------------------------------------------------------------
void export_report(const GameState& game, const Memory& memory) {
    FILE* file = std::fopen("sdmc:/switch/pokenx/logs/informe.txt", "w");
    if (!file) return;
    std::fprintf(file, "PokeNX - informe de diagnóstico\n");
    std::fprintf(file, "Memoria: %llu MiB (usada %llu MiB), modo %s\n", static_cast<unsigned long long>(memory.total >> 20),
                 static_cast<unsigned long long>(memory.used >> 20), memory.applet() ? "applet" : "título");
    std::fprintf(file, "Juego: %s\n", game.found ? short_name(game.text).c_str() : game.text.c_str());
    const long load = last_load_seconds_x10();
    std::fprintf(file, "Última carga: %s\n", load >= 0 ? seconds_text(load).c_str() : "desconocida");
    for (size_t i = 0; i < settings().size(); ++i)
        std::fprintf(file, "%s: %s\n", settings()[i].title, read_token(settings()[i].file, kSettingDefaults[i]).c_str());
    std::fprintf(file, "\n--- Fases de la última carga (engine.log.1) ---\n");
    for (const std::string& line : grep_lines("sdmc:/switch/pokenx/logs/engine.log.1", {"launch:", "parsing", "game data parsed", "game started", "Memory (before"}, 24))
        std::fprintf(file, "%s\n", line.c_str());
    std::fprintf(file, "\n--- Fotogramas tardíos y rendimiento de vídeo (últimas líneas) ---\n");
    for (const char* name : {"engine.log.1", "engine.log"})
        for (const std::string& line : grep_lines((std::string(kLogDir) + "/" + name).c_str(), {"video:"}, 6))
            std::fprintf(file, "%s: %s\n", name, line.c_str());
    std::fprintf(file, "\n--- Salas que tardaron más de 150 ms (últimas) ---\n");
    for (const char* name : {"engine.log.1", "engine.log"})
        for (const std::string& line : grep_lines((std::string(kLogDir) + "/" + name).c_str(), {" loaded in "}, 8))
            std::fprintf(file, "%s: %s\n", name, line.c_str());
    std::fprintf(file, "\n--- boot.log ---\n");
    for (const std::string& line : tail_lines("sdmc:/switch/pokenx/logs/boot.log", 12, 200)) std::fprintf(file, "%s\n", line.c_str());
    std::fprintf(file, "\n--- engine.log.1 (ejecución anterior, final) ---\n");
    for (const std::string& line : tail_lines("sdmc:/switch/pokenx/logs/engine.log.1", 40, 200)) std::fprintf(file, "%s\n", line.c_str());
    std::fprintf(file, "\n--- engine.log (esta ejecución, final) ---\n");
    for (const std::string& line : tail_lines("sdmc:/switch/pokenx/logs/engine.log", 40, 200)) std::fprintf(file, "%s\n", line.c_str());
    std::fprintf(file, "\n--- crash.log ---\n");
    for (const std::string& line : tail_lines("sdmc:/switch/pokenx/logs/crash.log", 30, 200)) std::fprintf(file, "%s\n", line.c_str());
    std::fclose(file);
}

void show_diagnostics(const GameState& game) {
    int which = 0;  // 0 = engine.log.1 (previous run), 1 = engine.log, 2 = boot.log, 3 = crash.log
    const char* const names[] = {"engine.log.1", "engine.log", "boot.log", "crash.log"};
    std::string notice;
    int loaded_which = -1;
    std::vector<std::string> lines;
    std::vector<std::string> stat_values;
    std::vector<std::string> file_sizes;
    while (appletMainLoop()) {
        const Input input = poll_input();
        if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_Plus)) return;
        if (pressed(input, HidNpadButton_R) || pressed(input, HidNpadButton_Right)) which = (which + 1) % 4;
        if (pressed(input, HidNpadButton_L) || pressed(input, HidNpadButton_Left)) which = (which + 3) % 4;
        if (loaded_which != which) {
            loaded_which = which;
            lines = tail_lines((std::string(kLogDir) + "/" + names[which]).c_str(), 17);
            stat_values = {read_token(settings()[0].file, kSettingDefaults[0]), read_token("sdmc:/switch/pokenx/graphics-driver.txt", "nvc0"),
                           read_token(settings()[2].file, kSettingDefaults[2]), read_token(settings()[3].file, kSettingDefaults[3])};
            file_sizes.clear();
            for (int i = 0; i < 4; ++i) file_sizes.push_back(size_text((std::string(kLogDir) + "/" + names[i]).c_str()));
        }
        const Memory memory = read_memory();
        if (pressed(input, HidNpadButton_X)) {
            export_report(game, memory);
            notice = "Informe guardado en logs/informe.txt";
        }
        const long load = last_load_seconds_x10();
        frame("Diagnóstico", game_chips(game, memory),
              {{"L/R", "cambiar registro"}, {"X", "exportar informe"}, {"B", "volver"}}, [&] {
                  draw_panel(64, 100, 470, 560);
                  struct Stat { const char* k; std::string v; };
                  const Stat stats[] = {
                      {"Memoria", mib(memory.total) + " (usada " + mib(memory.used) + ")"},
                      {"Modo", memory.applet() ? "Applet (poca RAM)" : "Título completo"},
                      {"Última carga", load >= 0 ? seconds_text(load) : "desconocida"},
                      {"Juego", game.found ? short_name(game.text) : "no detectado"},
                      {"Resolución", stat_values[0]},
                      {"Driver", stat_values[1]},
                      {"Rendimiento", stat_values[2]},
                      {"Caché de archivos", stat_values[3]},
                  };
                  int y = 116;
                  for (const Stat& stat : stats) {
                      gfx::text(88, y, stat.k, 17, kDim);
                      gfx::text(88, y + 22, gfx::ellipsize(stat.v, 20, 420), 20, kText);
                      y += 56;
                  }
                  for (int i = 0; i < 4; ++i) {
                      gfx::text(88, 566 + i * 22, names[i], 15, kDim);
                      gfx::text_right(510, 566 + i * 22, file_sizes[i], 15, i == 3 && file_sizes[i] != "vacío" && file_sizes[i] != "no existe" ? kBad : kText);
                  }
                  draw_panel(560, 100, 656, 560);
                  gfx::text(584, 116, names[which], 20, kGold);
                  gfx::text_right(1192, 120, "L/R para cambiar", 14, kDim);
                  int ly = 156;
                  if (lines.empty()) gfx::text(584, ly, "(sin contenido)", 15, kDim);
                  for (const std::string& line : lines) {
                      const bool bad = line.find("rror") != std::string::npos || line.find("ailed") != std::string::npos ||
                                       line.find("anic") != std::string::npos;
                      gfx::text(584, ly, gfx::ellipsize(line, 15, 610), 15, bad ? kBad : rgba(170, 180, 200));
                      ly += 26;
                  }
                  if (!notice.empty()) gfx::text(584, 626, notice, 16, kOk);
              });
    }
}

// ---- Home shortcut -----------------------------------------------------------------------------------------------
// The forwarder title is created from here (home_install.cpp): the same window the emulators show to put a game on the
// desktop, here a card that explains what is written and then does it.
void draw_home_progress(const char* step, int index, int total) {
    frame("Creando el acceso en HOME", {}, {}, [&] {
        gfx::text_center(gfx::kWidth / 2, 210, "Creando el acceso directo", 30, kText);
        gfx::rrect(210, 290, 860, 22, 11, kLine);
        gfx::rrect(210, 290, std::max(22, static_cast<int>(860LL * index / total)), 22, 11, kGold);
        gfx::text(210, 322, std::string("Paso ") + std::to_string(index) + " de " + std::to_string(total), 18, kDim);
        gfx::text_center(gfx::kWidth / 2, 420, step, 22, kGold);
        gfx::text_center(gfx::kWidth / 2, 600, "No apagues la consola ni saques la tarjeta SD.", 16, kDim);
    });
}

void show_home_result(const home_install::Outcome& outcome, const char* done_text) {
    if (outcome.ok) {
        show_card("Hecho", done_text, {"Entendido"}, kOk);
        return;
    }
    std::string text = "Falló el paso «" + outcome.step + "»";
    if (outcome.code) text += " (código " + home_install::code_text(outcome.code) + ", 0x" + [&] {
        char buffer[16];
        std::snprintf(buffer, sizeof(buffer), "%X", outcome.code);
        return std::string(buffer);
    }() + ")";
    text += ".";
    if (!outcome.detail.empty()) text += " " + outcome.detail;
    text += " El acceso no quedó en HOME; puedes volver a intentarlo.";
    show_card("No se pudo", text, {"Volver"}, kBad);
}

void show_home_shortcut(const GameState& game) {
    const bool nro_here = file_exists("sdmc:/switch/pokenx/pokenx.nro");
    bool installed = home_install::is_installed();
    while (appletMainLoop()) {
        const Input input = poll_input();
        if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_Plus)) return;
        if (pressed(input, HidNpadButton_A)) {
            if (!nro_here) {
                show_card("Falta pokenx.nro", "El acceso arranca sdmc:/switch/pokenx/pokenx.nro y no está en esa ruta. Copia ahí el NRO y vuelve.", {"Volver"}, kWarn);
            } else if (show_card(installed ? "Actualizar el acceso en HOME" : "Crear el acceso en HOME",
                                 "Se escribirán tres archivos de contenido en la tarjeta SD (programa, datos y registro) y PokeNX aparecerá en HOME como una aplicación, con toda la memoria. "
                                 "Hace falta Atmosphère con sigpatches. El juego y las partidas no se tocan.",
                                 {installed ? "Actualizar" : "Crear el acceso", "Cancelar"}, kGold) == 0) {
                const home_install::Outcome outcome = home_install::install(draw_home_progress);
                installed = home_install::is_installed();
                show_home_result(outcome, "PokeNX debería aparecer ahora en la pantalla de inicio de la consola. Si la consola no lo abre, comprueba que Atmosphère tenga los sigpatches.");
            }
        }
        if (pressed(input, HidNpadButton_Y) && installed) {
            if (show_card("Quitar el acceso de HOME", "Se borra el icono y su contenido de la tarjeta SD. El NRO, el juego y las partidas no se tocan.", {"Quitar", "Cancelar"}, kWarn) == 0) {
                const home_install::Outcome outcome = home_install::uninstall();
                installed = home_install::is_installed();
                show_home_result(outcome, "El acceso ya no está en HOME.");
            }
        }
        const Memory memory = read_memory();
        frame("Acceso directo en HOME", game_chips(game, memory), {{"A", installed ? "actualizar el acceso" : "crear el acceso"}, {"Y", "quitar"}, {"B", "volver"}}, [&] {
            draw_panel(64, 100, 560, 560);
            gfx::rrect_gradient(92, 128, 110, 110, 22, kGold, kGoldDark);
            draw_icon(Icon::Play, 147, 183, 34, kInk);
            gfx::text(226, 138, "PokeNX", 32, kText);
            gfx::text(226, 182, "Un acceso en la pantalla de inicio", 17, kDim);
            gfx::text(226, 204, "de la Switch (un «forwarder»).", 17, kDim);
            gfx::text(92, 270, "Estado", 20, kGold);
            gfx::circle(98, 314, 5, installed ? kOk : kDim);
            gfx::text(112, 304, installed ? "El acceso está en HOME" : "El acceso aún no está en HOME", 17, installed ? kOk : kDim);
            gfx::circle(98, 342, 5, nro_here ? kOk : kWarn);
            gfx::text(112, 332, nro_here ? "pokenx.nro en la ruta fija" : "pokenx.nro no está en sdmc:/switch/pokenx/", 17, nro_here ? kOk : kWarn);
            gfx::circle(98, 370, 5, memory.applet() ? kWarn : kOk);
            gfx::text(112, 360, memory.applet() ? "Ahora: modo applet (poca memoria)" : "Ahora: modo título (memoria completa)", 17,
                      memory.applet() ? kWarn : kOk);
            gfx::text(92, 412, "Qué consigue", 20, kGold);
            gfx::text(92, 446, "- Icono en HOME de la consola.", 17, rgba(201, 208, 226));
            gfx::text(92, 474, "- Arranca como aplicación: toda la memoria,", 17, rgba(201, 208, 226));
            gfx::text(92, 498, "  sin mantener R al abrir.", 17, rgba(201, 208, 226));
            gfx::text(92, 526, "- Si mueves el .nro, deja de funcionar.", 17, rgba(201, 208, 226));
            gfx::text(92, 574, "Requiere Atmosphère con sigpatches.", 16, kWarn);
            draw_panel(650, 100, 566, 560);
            gfx::text(676, 124, "Crear el acceso", 24, kGold);
            const bool nsp_here = file_exists("sdmc:/switch/pokenx/PokeNX-HOME.nsp");
            int y = 168;
            y = draw_wrapped(676, y, 516, "Pulsa A: PokeNX crea el acceso él mismo, como la ventana de «crear acceso directo» de los emuladores. Escribe tres archivos de contenido en la tarjeta SD y registra la aplicación.", 17, rgba(201, 208, 226), 24);
            y = draw_wrapped(676, y + 8, 516, "Pulsa Y para quitarlo. Actualizar el NRO no obliga a repetirlo: el acceso siempre arranca el mismo archivo.", 17, rgba(201, 208, 226), 24);
            y = draw_wrapped(676, y + 8, 516, "Si algo falla, el mensaje dice en qué paso y con qué código; antes del último paso no se registra nada.", 17, rgba(201, 208, 226), 24);
            gfx::text(676, y + 22, "Otras formas", 20, kGold);
            y = draw_wrapped(676, y + 56, 516, "En el PC: scripts/build-nsp.ps1 genera PokeNX-HOME.nsp (necesita tu prod.keys y hacbrewpack) para instalarlo con DBI, Tinfoil o Goldleaf. En la consola: Sphaira también crea forwarders.", 16, kDim, 22);
            gfx::circle(682, y + 22, 5, nsp_here ? kOk : kDim);
            gfx::text(696, y + 12, nsp_here ? "PokeNX-HOME.nsp está en switch/pokenx/" : "PokeNX-HOME.nsp no está en switch/pokenx/", 15, nsp_here ? kOk : kDim);
            draw_wrapped(676, y + 44, 516, "Nada de esto está verificado en una consola todavía.", 15, kDim, 22);
        });
    }
}

// ---- ZIP import -------------------------------------------------------------------------------------------------
struct DirEntry {
    std::string name;
    bool is_dir = false;
    unsigned long long size = 0;
};

std::vector<DirEntry> list_directory(const std::string& path) {
    std::vector<DirEntry> entries;
    DIR* dir = opendir(path.c_str());
    if (!dir) return entries;
    while (dirent* item = readdir(dir)) {
        const std::string name = item->d_name;
        if (name == "." || name == "..") continue;
        struct stat info;
        const std::string full = path + (path.back() == '/' ? "" : "/") + name;
        if (stat(full.c_str(), &info) != 0) continue;
        DirEntry entry;
        entry.name = name;
        entry.is_dir = S_ISDIR(info.st_mode);
        entry.size = static_cast<unsigned long long>(info.st_size);
        if (!entry.is_dir) {
            const std::string low = lower(name);
            if (low.size() < 5 || low.compare(low.size() - 4, 4, ".zip") != 0) continue;
        }
        entries.push_back(entry);
    }
    closedir(dir);
    std::sort(entries.begin(), entries.end(), [](const DirEntry& a, const DirEntry& b) {
        if (a.is_dir != b.is_dir) return a.is_dir;
        return lower(a.name) < lower(b.name);
    });
    return entries;
}

struct ProgressContext {
    u64 last_update = 0;
    std::string name;
};

void draw_import_progress(const std::string& name, unsigned long long done, unsigned long long total) {
    const unsigned percent = total ? static_cast<unsigned>(done * 100 / total) : 0;
    frame("Importando", {}, {{"B", "cancelar"}}, [&] {
        gfx::text_center(gfx::kWidth / 2, 210, gfx::ellipsize(name, 30, 1000), 30, kText);
        gfx::rrect(210, 290, 860, 22, 11, kLine);
        if (percent > 0) gfx::rrect(210, 290, std::max(22, static_cast<int>(860ULL * percent / 100)), 22, 11, kGold);
        gfx::text(210, 322, mib(done) + " / " + mib(total), 18, kDim);
        gfx::text_right(1070, 322, std::to_string(percent) + " %", 18, kDim);
        const char* steps[] = {"Comprobar rutas y cabecera", "Extraer a una carpeta temporal", "Validar CRC y estructura GM8",
                               "Sustituir game/ y borrar la versión anterior (tus partidas pasan a la nueva)"};
        for (int i = 0; i < 4; ++i) {
            const bool now = (i == 1 && percent < 100) || (i == 2 && percent >= 100);
            const bool done_step = i == 0 || (i == 1 && percent >= 100);
            gfx::circle(420, 410 + i * 40 + 10, 9, done_step ? kOk : (now ? kGold : kLine));
            gfx::text(446, 410 + i * 40, steps[i], 20, now ? kText : kDim);
        }
        gfx::text_center(gfx::kWidth / 2, 600, "No se toca game/ hasta que la validación termina: cancelar no daña la instalación actual.", 16, kDim);
    });
}

int32_t on_progress(uint64_t done, uint64_t total, void* user) {
    auto* context = static_cast<ProgressContext*>(user);
    const u64 now = armTicksToNs(armGetSystemTick());
    if (now - context->last_update > 100'000'000ULL) {
        context->last_update = now;
        draw_import_progress(context->name, done, total);
        padUpdate(&g_pad);
        if (padGetButtonsDown(&g_pad) & (HidNpadButton_B | HidNpadButton_Plus)) return 1;
    }
    return 0;
}

std::string import_zip(const std::string& zip_path) {
    preload_join();
    pokenx_preload_clear();  // the game is about to change: whatever was parsed is stale
    ProgressContext context;
    context.name = short_name(zip_path);
    draw_import_progress(context.name, 0, 0);
    const int32_t code = pokenx_import_zip(zip_path.c_str(), on_progress, &context);
    if (code == 0) {
        const GameState installed = detect_game();
        if (installed.found) {
            if (FILE* marker = std::fopen(kInstallSourcePath, "w")) {
                std::fprintf(marker, "zip\n%lld\n", file_size_of(installed.text));
                std::fclose(marker);
            }
        }
        return "Importación completada: el juego quedó en sdmc:/switch/pokenx/game/.";
    }
    char text[200] = {0};
    pokenx_import_error_text(code, text, sizeof(text));
    return std::string("Importación no instalada: ") + text + ".";
}

std::string browse_zip(const GameState& game) {
    std::string path = kRoot;
    int selected = 0;
    std::vector<DirEntry> entries = list_directory(path);
    while (appletMainLoop()) {
        const Input input = poll_input();
        const int count = static_cast<int>(entries.size());
        if (pressed(input, HidNpadButton_Plus)) return "";
        if (pressed(input, HidNpadButton_Down) && count) selected = (selected + 1) % count;
        if (pressed(input, HidNpadButton_Up) && count) selected = (selected + count - 1) % count;
        if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_Left)) {
            const size_t slash = path.find_last_of('/');
            if (path == "sdmc:/" || path.size() <= 6 || slash == std::string::npos) return "";
            path = slash <= 5 ? std::string("sdmc:/") : path.substr(0, slash);
            entries = list_directory(path);
            selected = 0;
        }
        if (pressed(input, HidNpadButton_A) && count) {
            const DirEntry& entry = entries[selected];
            const std::string full = path + (path.back() == '/' ? "" : "/") + entry.name;
            if (entry.is_dir) {
                path = full;
                entries = list_directory(path);
                selected = 0;
            } else {
                return import_zip(full);
            }
        }
        const Memory memory = read_memory();
        std::vector<Chip> chips = game_chips(game, memory);
        chips.push_back({gfx::ellipsize(path, 15, 360), kText});
        frame("Importar un ZIP del juego", chips, {{"Arriba/Abajo", "mover"}, {"A", "abrir / importar"}, {"B", "subir"}, {"+", "cancelar"}}, [&] {
            draw_panel(64, 100, 700, 560);
            const int visible = 10;
            const int first = std::max(0, std::min(selected - visible / 2, count - visible));
            if (entries.empty()) gfx::text(96, 130, "(sin carpetas ni archivos .zip aquí)", 20, kDim);
            for (int i = first; i < count && i < first + visible; ++i) {
                const DirEntry& entry = entries[i];
                draw_row(80, 112 + (i - first) * 54, 668, (entry.is_dir ? "> " : "") + entry.name, entry.is_dir ? "" : mib(entry.size), "", i == selected);
            }
            draw_panel(790, 100, 426, 560);
            gfx::text(816, 124, "Vista previa", 24, kGold);
            if (count && !entries[selected].is_dir) {
                const DirEntry& entry = entries[selected];
                gfx::text(816, 176, gfx::ellipsize(entry.name, 18, 376), 18, kText);
                gfx::text(816, 206, mib(entry.size), 17, kDim);
                gfx::text(816, 260, "Se importará a:", 17, kDim);
                gfx::text(816, 286, "sdmc:/switch/pokenx/game/", 17, kText);
                gfx::text(816, 326, "La versión actual se sustituye y se", 17, kDim);
                gfx::text(816, 350, "borra; tus partidas pasan a la nueva.", 17, kDim);
                gfx::text(816, 410, "Se validan rutas, CRC, cabecera PE,", 17, kDim);
                gfx::text(816, 434, "data/ y estructura GM8 antes de", 17, kDim);
                gfx::text(816, 458, "sustituir nada.", 17, kDim);
                gfx::rrect(816, 560, 150, 40, 12, kGold);
                gfx::text_center(891, 568, "A  Importar", 18, kInk);
            } else if (count) {
                gfx::text(816, 176, "Carpeta", 18, kText);
                gfx::text(816, 206, "A para entrar.", 17, kDim);
            }
        });
    }
    return "";
}

// ---- Wrapped text helper ------------------------------------------------------------------------------------------
// Draws `text` wrapped at `max_width` pixels starting at (x, y); returns the y after the last line. `max_lines` 0 = all.
int draw_wrapped(int x, int y, int max_width, const std::string& text, int size, u32 color, int line_height, int max_lines) {
    std::string line, word;
    int lines = 0;
    const auto flush = [&] {
        gfx::text(x, y, line, size, color);
        y += line_height;
        ++lines;
    };
    for (char c : text + " ") {
        if (c == ' ' || c == '\n') {
            const std::string candidate = line.empty() ? word : line + " " + word;
            if (!line.empty() && gfx::text_width(candidate, size) > max_width) {
                if (max_lines && lines + 1 >= max_lines) {
                    line = gfx::ellipsize(line + " " + word, size, max_width);
                    flush();
                    return y;
                }
                flush();
                line = word;
            } else {
                line = candidate;
            }
            word.clear();
            if (c == '\n' && !line.empty()) {
                flush();
                line.clear();
            }
        } else {
            word += c;
        }
    }
    if (!line.empty() && (!max_lines || lines < max_lines)) flush();
    return y;
}

long long now_unix() {
    if (R_FAILED(timeInitialize())) return 0;
    u64 now = 0;
    const bool ok = R_SUCCEEDED(timeGetCurrentTime(TimeType_UserSystemClock, &now));
    timeExit();
    return ok ? static_cast<long long>(now) : 0;
}

std::string date_text(long long unix_seconds) {
    if (unix_seconds <= 0) return "fecha desconocida";
    const time_t stamp = static_cast<time_t>(unix_seconds);
    const struct tm* parts = gmtime(&stamp);
    if (!parts) return "fecha desconocida";
    char text[40];
    std::snprintf(text, sizeof(text), "%04d-%02d-%02d %02d:%02d", parts->tm_year + 1900, parts->tm_mon + 1, parts->tm_mday, parts->tm_hour, parts->tm_min);
    return text;
}

// ---- Game keys ----------------------------------------------------------------------------------------------------
// What the game itself uses: read once from its scripts into cache/game-keys.txt (pokenx_scan_game_keys) and shown with
// the Switch input that produces each key, so keys no button can send (F5, Re Pág...) stand out. While the game runs it
// also writes the keys it uses at that moment (the ones the player changed inside the game) to cache/game-keys-live.txt;
// when that file exists its keys replace the defaults and the ones that differ are marked.
#ifndef POKENX_KEYS_REPORT_PATH
#define POKENX_KEYS_REPORT_PATH "sdmc:/switch/pokenx/cache/game-keys.txt"  // the PC preview points this at a sample file
#endif
#ifndef POKENX_LIVE_KEYS_PATH
#define POKENX_LIVE_KEYS_PATH "sdmc:/switch/pokenx/cache/game-keys-live.txt"
#endif
#ifndef POKENX_COMPAT_REPORT_PATH
#define POKENX_COMPAT_REPORT_PATH "sdmc:/switch/pokenx/cache/game-compat.txt"
#endif
constexpr const char* kKeysReport = POKENX_KEYS_REPORT_PATH;
constexpr const char* kLiveKeysReport = POKENX_LIVE_KEYS_PATH;
constexpr const char* kCompatReport = POKENX_COMPAT_REPORT_PATH;

void make_dir(const char* path) {
#ifdef __SWITCH__
    mkdir(path, 0777);
#else
    mkdir(path);  // MinGW (the PC preview)
#endif
}

struct GameKey {
    std::string group, action, key;
    std::string original;  // the game's own default when the player changed it ("" = unchanged)
};

struct KeysReport {
    bool ok = false;
    std::string exe;
    unsigned long long size = 0;
    std::vector<GameKey> keys;
    bool live = false;  // keys read from the game while it ran
    int changed = 0;    // how many of them differ from the defaults
};

KeysReport load_keys_report() {
    KeysReport report;
    FILE* file = std::fopen(kKeysReport, "r");
    if (!file) return report;
    char line[300];
    while (std::fgets(line, sizeof(line), file)) {
        std::string text(line);
        while (!text.empty() && (text.back() == '\n' || text.back() == '\r')) text.pop_back();
        if (text.compare(0, 4, "exe=") == 0) report.exe = text.substr(4);
        else if (text.compare(0, 5, "size=") == 0) report.size = std::strtoull(text.c_str() + 5, nullptr, 10);
        else if (!text.empty() && text[0] != '#') {
            const size_t a = text.find('|'), b = a == std::string::npos ? a : text.find('|', a + 1);
            if (b != std::string::npos) report.keys.push_back({text.substr(0, a), text.substr(a + 1, b - a - 1), text.substr(b + 1)});
        }
    }
    std::fclose(file);
    report.ok = !report.exe.empty() && !report.keys.empty();
    return report;
}

// Replaces the defaults with the keys the game reported while it ran (cache/game-keys-live.txt), when that file exists.
void apply_live_keys(KeysReport& report) {
    report.live = false;
    report.changed = 0;
    FILE* file = std::fopen(kLiveKeysReport, "r");
    if (!file) return;
    char line[300];
    while (std::fgets(line, sizeof(line), file)) {
        std::string text(line);
        while (!text.empty() && (text.back() == '\n' || text.back() == '\r')) text.pop_back();
        if (text.empty() || text[0] == '#') continue;
        const size_t a = text.find('|'), b = a == std::string::npos ? a : text.find('|', a + 1);
        if (b == std::string::npos) continue;
        const std::string group = text.substr(0, a), action = text.substr(a + 1, b - a - 1), key = text.substr(b + 1);
        for (GameKey& known : report.keys) {
            if (known.group != group || known.action != action) continue;
            report.live = true;
            if (known.original.empty() && known.key != key) {
                known.original = known.key;
                known.key = key;
                ++report.changed;
            }
            break;
        }
    }
    std::fclose(file);
}

bool report_is_current(const KeysReport& report, const GameState& game) {
    return report.ok && game.found && report.exe == short_name(game.text) && report.size == file_size(game.text.c_str());
}

std::string group_title(const std::string& code) {
    static const struct { const char* code; const char* title; } kTitles[] = {
        {"bt", "Combate"}, {"caja", "Caja"}, {"captura", "Capturas de pantalla"}, {"comando", "Comandos"},
        {"eliminatorias", "Eliminatorias"}, {"entrenador", "Tarjeta de entrenador"}, {"ex", "Exploración"},
        {"global", "Global (teclas F)"}, {"ini", "Pantalla de título"}, {"menu", "Menú principal"}, {"menump", "Menú del móvil"},
        {"mision", "Misiones"}, {"misiones", "Misiones"}, {"movil", "Móvil"}, {"mp", "Menú del móvil"}, {"obj", "Objetos y mochila"},
        {"opciones", "Opciones del juego"}, {"p1", "Combate local: jugador 1"}, {"p2", "Combate local: jugador 2"},
        {"pkdex", "Pokédex"}, {"pkmn", "Equipo"}, {"poker", "Póker"}, {"prestamo", "Préstamos"}, {"puzzle", "Puzzle"},
        {"random", "Aleatorio"}, {"search", "Búsqueda"}, {"vicio", "Minijuego «vicio»"}, {"vuelo", "Mapa de vuelo"},
        {"xpod", "Reproductor de música"}, {"otros", "Otras"},
    };
    for (const auto& item : kTitles)
        if (code == item.code) return item.title;
    std::string title = code;
    if (!title.empty()) title[0] = static_cast<char>(std::toupper(static_cast<unsigned char>(title[0])));
    return title;
}

std::string action_title(std::string action) {
    for (char& c : action)
        if (c == '_') c = ' ';
    return action;
}

struct KeyGroup {
    std::string code;
    std::vector<size_t> items;  // indices into the report
    int unreachable = 0;
};

std::vector<KeyGroup> build_groups(const KeysReport& report, const ControlMap& map) {
    std::vector<KeyGroup> groups;
    for (size_t i = 0; i < report.keys.size(); ++i) {
        auto found = std::find_if(groups.begin(), groups.end(), [&](const KeyGroup& group) { return group.code == report.keys[i].group; });
        if (found == groups.end()) {
            groups.push_back({report.keys[i].group, {}, 0});
            found = groups.end() - 1;
        }
        found->items.push_back(i);
        if (producing(map, report.keys[i].key).empty()) ++found->unreachable;
    }
    return groups;
}

// Compatibility report of the installed game version (cache/game-compat.txt): which built-in GameMaker functions it calls
// that the engine lacks or simplifies. It is written by the same pass that reads the keys.
struct CompatItem {
    std::string name;
    int count = 0;
    std::string impact;
};

struct CompatReport {
    bool ok = false;
    std::string exe;
    unsigned long long size = 0;
    int scripts = 0, builtin = 0;
    std::vector<CompatItem> missing, simplified;
};

CompatReport load_compat_report() {
    CompatReport report;
    FILE* file = std::fopen(kCompatReport, "r");
    if (!file) return report;
    char line[400];
    while (std::fgets(line, sizeof(line), file)) {
        std::string text(line);
        while (!text.empty() && (text.back() == '\n' || text.back() == '\r')) text.pop_back();
        if (text.compare(0, 4, "exe=") == 0) report.exe = text.substr(4);
        else if (text.compare(0, 5, "size=") == 0) report.size = std::strtoull(text.c_str() + 5, nullptr, 10);
        else if (text.compare(0, 8, "scripts=") == 0) report.scripts = std::atoi(text.c_str() + 8);
        else if (text.compare(0, 8, "builtin=") == 0) report.builtin = std::atoi(text.c_str() + 8);
        else if (text.compare(0, 8, "missing|") == 0 || text.compare(0, 11, "simplified|") == 0) {
            const bool is_missing = text[0] == 'm';
            std::vector<std::string> parts;
            size_t at = text.find('|') + 1;
            for (int i = 0; i < 3; ++i) {
                const size_t bar = text.find('|', at);
                if (bar == std::string::npos || i == 2) {
                    parts.push_back(text.substr(at));
                    break;
                }
                parts.push_back(text.substr(at, bar - at));
                at = bar + 1;
            }
            if (parts.size() >= 2) {
                CompatItem item{parts[0], std::atoi(parts[1].c_str()), parts.size() > 2 ? parts[2] : std::string()};
                (is_missing ? report.missing : report.simplified).push_back(item);
            }
        }
    }
    std::fclose(file);
    report.ok = !report.exe.empty();
    return report;
}

bool compat_is_current(const CompatReport& report, const GameState& game) {
    return report.ok && game.found && report.exe == short_name(game.text) && report.size == file_size(game.text.c_str());
}

// Reads the game's data once and writes both reports (keys and compatibility).
bool scan_keys(const GameState& game) {
    gfx::begin();
    draw_top("Datos del juego", {});
    gfx::text_center(gfx::kWidth / 2, 280, "Leyendo los datos del juego...", 34, kText);
    gfx::text_center(gfx::kWidth / 2, 340, "Teclas y compatibilidad de esta versión (unos segundos). Después queda guardado.", 20, kDim);
    draw_foot({});
    gfx::end();
    make_dir("sdmc:/switch/pokenx/cache");
    const int32_t code = pokenx_scan_game_reports(kGameDir, kKeysReport, kCompatReport);
    if (code >= 0) return true;
    show_card("No se pudieron leer los datos del juego",
              code == -2 ? "No hay un juego válido en game/." : code == -4 ? "No se pudo guardar el informe en la tarjeta SD."
                                                                           : "No se pudieron analizar los datos del juego (¿poca memoria? ábrelo como aplicación).",
              {"Volver"}, kBad);
    (void)game;
    return false;
}

void show_game_keys(const GameState& game, const Memory& memory0) {
    preload_join();
    if (!game.found) {
        show_card("No hay juego instalado", "Instala el juego (Actualizaciones) para ver las teclas que usa.", {"Volver"}, kBad);
        return;
    }
    KeysReport report = load_keys_report();
    if (!report_is_current(report, game) || !compat_is_current(load_compat_report(), game)) {
        if (memory0.applet()) {
            show_card("Hace falta más memoria", "Leer las teclas analiza el juego entero. Abre PokeNX como aplicación (abriendo cualquier juego con R pulsado) y vuelve aquí.",
                      {"Volver"}, kWarn);
            return;
        }
        if (show_card("Leer los datos del juego", "El juego guarda sus teclas en su propio código. Se analiza una vez (también las funciones que usa, para el informe de compatibilidad), tarda unos segundos y no cambia nada del juego.",
                      {"Leer ahora", "Volver"}, kGold) != 0)
            return;
        if (!scan_keys(game)) return;
        report = load_keys_report();
        if (!report.ok) return;
    }
    apply_live_keys(report);
    ControlMap map = load_controls();
    std::vector<KeyGroup> groups = build_groups(report, map);
    int selected = 0;
    int open_group = -1;  // -1 = the list of groups, otherwise the group whose keys are listed
    int item = 0;
    int total_unreachable = 0;
    for (const KeyGroup& group : groups) total_unreachable += group.unreachable;
    while (appletMainLoop()) {
        const Input input = poll_input();
        const int group_count = static_cast<int>(groups.size());
        if (pressed(input, HidNpadButton_Plus)) return;
        if (open_group < 0) {
            if (pressed(input, HidNpadButton_B)) return;
            if (pressed(input, HidNpadButton_Down)) selected = (selected + 1) % group_count;
            if (pressed(input, HidNpadButton_Up)) selected = (selected + group_count - 1) % group_count;
            if (pressed(input, HidNpadButton_A)) {
                open_group = selected;
                item = 0;
            }
            if (pressed(input, HidNpadButton_X)) {
                show_controls(game);
                map = load_controls();
                groups = build_groups(report, map);
                total_unreachable = 0;
                for (const KeyGroup& group : groups) total_unreachable += group.unreachable;
            }
        } else {
            const int count = static_cast<int>(groups[open_group].items.size());
            if (pressed(input, HidNpadButton_B)) open_group = -1;
            if (pressed(input, HidNpadButton_Down)) item = (item + 1) % count;
            if (pressed(input, HidNpadButton_Up)) item = (item + count - 1) % count;
            if (pressed(input, HidNpadButton_X)) {
                show_controls(game);
                map = load_controls();
                groups = build_groups(report, map);
                total_unreachable = 0;
                for (const KeyGroup& group : groups) total_unreachable += group.unreachable;
            }
        }
        const Memory memory = read_memory();
        std::vector<Chip> chips = game_chips(game, memory);
        chips.push_back({std::to_string(total_unreachable) + " sin botón", total_unreachable ? kBad : kOk});
        if (report.changed) chips.push_back({std::to_string(report.changed) + " cambiadas por ti", kGold});
        frame(open_group < 0 ? "Teclas del juego" : "Teclas del juego · " + group_title(groups[open_group].code), chips,
              open_group < 0 ? std::vector<Hint>{{"Arriba/Abajo", "mover"}, {"A", "abrir"}, {"X", "ir a Mandos"}, {"B", "volver"}}
                             : std::vector<Hint>{{"Arriba/Abajo", "mover"}, {"X", "ir a Mandos"}, {"B", "grupos"}},
              [&] {
                  if (open_group < 0) {
                      draw_panel(64, 100, 690, 560);
                      const int visible = 12;
                      const int first = std::max(0, std::min(selected - visible / 2, group_count - visible));
                      for (int i = first; i < group_count && i < first + visible; ++i) {
                          const KeyGroup& group = groups[i];
                          const std::string value = group.unreachable ? std::to_string(group.unreachable) + " sin botón" : std::to_string(group.items.size()) + " teclas";
                          draw_row(80, 112 + (i - first) * 44, 658, group_title(group.code), value, "", i == selected, group.unreachable ? kBad : kGold);
                      }
                      const KeyGroup& group = groups[selected];
                      draw_panel(780, 100, 436, 560);
                      gfx::text(806, 124, group_title(group.code), 24, kGold);
                      gfx::text(806, 170, std::to_string(group.items.size()) + " teclas definidas por el juego", 18, rgba(201, 208, 226));
                      gfx::text(806, 198, group.unreachable ? std::to_string(group.unreachable) + " no tienen botón en la Switch" : "Todas se pueden pulsar con el mando", 18,
                                group.unreachable ? kBad : kOk);
                      int y = 252;
                      for (size_t n = 0; n < group.items.size() && n < 6; ++n) {
                          const GameKey& key = report.keys[group.items[n]];
                          gfx::text(806, y, gfx::ellipsize(action_title(key.action), 16, 230), 16, kDim);
                          gfx::text_right(1190, y, key_label(key.key), 16, kText);
                          y += 28;
                      }
                      if (report.live) {
                          gfx::text(806, 574, "Son las teclas que usa tu juego ahora (de tu", 14, kOk);
                          gfx::text(806, 596, "última partida). Las cambiadas llevan «antes».", 14, kOk);
                      } else {
                          gfx::text(806, 574, "Teclas por defecto del propio juego. Si las", 14, kDim);
                          gfx::text(806, 596, "cambias dentro del juego, aparecerán aquí", 14, kDim);
                          gfx::text(806, 618, "después de jugar.", 14, kDim);
                      }
                  } else {
                      const KeyGroup& group = groups[open_group];
                      const int count = static_cast<int>(group.items.size());
                      draw_panel(64, 100, 1152, 560);
                      gfx::text(96, 118, "Acción", 15, kDim);
                      gfx::text(560, 118, "Tecla", 15, kDim);
                      gfx::text(760, 118, "La produce", 15, kDim);
                      const int visible = 10;
                      const int first = std::max(0, std::min(item - visible / 2, count - visible));
                      for (int i = first; i < count && i < first + visible; ++i) {
                          const GameKey& key = report.keys[group.items[i]];
                          const std::string by = producing(map, key.key);
                          const int y = 140 + (i - first) * 50;
                          if (i == item) {
                              gfx::rrect(80, y, 1120, 46, 10, kRowSel);
                              gfx::rrect(80, y + 4, 5, 38, 2, kGold);
                          }
                          gfx::text(104, y + 10, gfx::ellipsize(action_title(key.action), 21, 430), 21, i == item ? kText : rgba(201, 208, 226));
                          gfx::text(560, y + 10, key_label(key.key), 21, key.original.empty() ? kGold : kOk);
                          if (!key.original.empty()) gfx::text(668, y + 16, "antes " + key_label(key.original), 15, kDim);
                          gfx::text(760, y + 10, by.empty() ? "sin botón" : gfx::ellipsize(by, 21, 420), 21, by.empty() ? kBad : kOk);
                      }
                  }
              });
    }
}

std::string calls_text(int count) { return std::to_string(count) + (count == 1 ? " llamada" : " llamadas"); }

// ---- Compatibility of the installed version ----------------------------------------------------------------------
void show_compatibility(const GameState& game, const Memory& memory0) {
    preload_join();
    if (!game.found) {
        show_card("No hay juego instalado", "Instala el juego (Actualizaciones) para ver su compatibilidad.", {"Volver"}, kBad);
        return;
    }
    CompatReport report = load_compat_report();
    const auto scan = [&]() -> bool {
        if (memory0.applet()) {
            show_card("Hace falta más memoria", "Comprobar la compatibilidad analiza el juego entero. Abre PokeNX como aplicación (abriendo cualquier juego con R pulsado) y vuelve aquí.",
                      {"Volver"}, kWarn);
            return false;
        }
        if (!scan_keys(game)) return false;
        report = load_compat_report();
        return report.ok;
    };
    if (!compat_is_current(report, game)) {
        if (show_card("Comprobar la compatibilidad", "Se analiza el código del juego instalado (unos segundos) y se cuentan las funciones de GameMaker que llama y que el motor no tiene. No cambia nada del juego.",
                      {"Comprobar ahora", "Volver"}, kGold) != 0 || !scan())
            return;
    }
    while (appletMainLoop()) {
        const Input input = poll_input();
        if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_Plus) || pressed(input, HidNpadButton_A)) return;
        if (pressed(input, HidNpadButton_Y)) scan();
        const Memory memory = read_memory();
        const bool clean = report.missing.empty();
        std::vector<Chip> chips = game_chips(game, memory);
        chips.push_back({clean ? "Sin funciones que falten" : std::to_string(report.missing.size()) + " que faltan", clean ? kOk : kBad});
        frame("Compatibilidad de la versión instalada", chips, {{"Y", "comprobar de nuevo"}, {"B", "volver"}}, [&] {
            draw_panel(64, 100, 560, 560);
            gfx::text(92, 124, gfx::ellipsize(report.exe, 16, 500), 16, kDim);
            draw_row(80, 156, 528, "Funciones que llama", std::to_string(report.builtin), "integradas de GameMaker", false, kGold);
            draw_row(80, 210, 528, "Que el motor no tiene", clean ? "ninguna" : std::to_string(report.missing.size()), "", false, clean ? kOk : kBad);
            draw_row(80, 264, 528, "Simplificadas", std::to_string(report.simplified.size()), "funcionan con menos detalle", false, report.simplified.empty() ? kOk : kWarn);
            draw_row(80, 318, 528, "Scripts propios", std::to_string(report.scripts), "del propio juego", false, kGold);
            draw_wrapped(92, 400, 500, "Es un análisis del código del juego, no una prueba de que todo funcione. No ve las llamadas que el juego calcula mientras corre, y que una función exista no garantiza que se comporte como en GameMaker 8.", 15, kDim, 22);
            draw_panel(650, 100, 566, 560);
            gfx::text(676, 124, clean ? "Nada falta" : "Funciones que el motor no tiene", 24, clean ? kOk : kBad);
            int y = 168;
            if (clean) {
                y = draw_wrapped(676, y, 516, "Todas las funciones integradas que llama esta versión existen en el motor.", 17, rgba(201, 208, 226), 24);
            } else {
                for (size_t i = 0; i < report.missing.size() && i < 8; ++i) {
                    gfx::text(676, y, gfx::ellipsize(report.missing[i].name, 17, 400), 17, kBad);
                    gfx::text_right(1190, y, calls_text(report.missing[i].count), 15, kDim);
                    y += 28;
                }
                if (report.missing.size() > 8) gfx::text(676, y, "y " + std::to_string(report.missing.size() - 8) + " más", 15, kDim);
                y += 28;
            }
            y += 14;
            gfx::text(676, y, "Con menos detalle", 20, kGold);
            y += 36;
            if (report.simplified.empty()) gfx::text(676, y, "Ninguna.", 16, kDim);
            for (size_t i = 0; i < report.simplified.size() && i < 6; ++i) {
                gfx::text(676, y, gfx::ellipsize(report.simplified[i].name, 16, 300), 16, kWarn);
                gfx::text_right(1190, y, calls_text(report.simplified[i].count), 14, kDim);
                y += 22;
                gfx::text(690, y, gfx::ellipsize(report.simplified[i].impact, 13, 490), 13, kDim);
                y += 24;
            }
        });
    }
}

// ---- Backups ------------------------------------------------------------------------------------------------------
struct BackupEntry {
    std::string name;
    unsigned long long files = 0, bytes = 0;
    long long unix_seconds = 0;
};

std::vector<BackupEntry> list_backups() {
    std::vector<BackupEntry> entries;
    char buffer[2048] = {0};
    if (pokenx_backup_list(kRoot, buffer, sizeof(buffer)) <= 0) return entries;
    std::string text(buffer);
    size_t at = 0;
    while (at < text.size()) {
        const size_t end = text.find('\n', at);
        const std::string line = text.substr(at, end == std::string::npos ? std::string::npos : end - at);
        at = end == std::string::npos ? text.size() : end + 1;
        BackupEntry entry;
        char name[40] = {0};
        unsigned long long files = 0, bytes = 0;
        long long stamp = 0;
        if (std::sscanf(line.c_str(), "%39[^\t]\t%llu\t%llu\t%lld", name, &files, &bytes, &stamp) == 4) {
            entry.name = name;
            entry.files = files;
            entry.bytes = bytes;
            entry.unix_seconds = stamp;
            entries.push_back(entry);
        }
    }
    return entries;
}

void show_backups(const GameState& game, std::string& note) {
    preload_join();
    pokenx_set_unix_seconds(now_unix());
    std::vector<BackupEntry> entries = list_backups();
    int selected = 0;
    std::string notice;
    while (appletMainLoop()) {
        const Input input = poll_input();
        const int count = static_cast<int>(entries.size());
        if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_Plus)) return;
        if (pressed(input, HidNpadButton_Down) && count) selected = (selected + 1) % count;
        if (pressed(input, HidNpadButton_Up) && count) selected = (selected + count - 1) % count;
        if (pressed(input, HidNpadButton_X)) {
            char name[40] = {0};
            const int32_t files = pokenx_backup_create(kRoot, name, sizeof(name));
            notice = files >= 0 ? std::string("Copia creada: ") + name + " (" + std::to_string(files) + " archivos)" : "No se pudo crear la copia (¿hay partidas guardadas?)";
            entries = list_backups();
            selected = 0;
        }
        if (pressed(input, HidNpadButton_A) && count) {
            const BackupEntry& entry = entries[selected];
            const int choice = show_card("Restaurar " + entry.name,
                                         "Se reemplazan las partidas y los ajustes actuales por los de esta copia (" + date_text(entry.unix_seconds) +
                                             "). Antes se guarda una copia nueva de lo actual, así que se puede deshacer.",
                                         {"Restaurar", "Cancelar"}, kGold);
            if (choice == 0) {
                const int32_t restored = pokenx_backup_restore(kRoot, entry.name.c_str());
                notice = restored >= 0 ? "Restaurada " + entry.name + " (" + std::to_string(restored) + " archivos)" : "No se pudo restaurar la copia";
                if (restored >= 0) note = notice;
                entries = list_backups();
                selected = 0;
            }
        }
        const Memory memory = read_memory();
        frame("Actualizaciones · Copias de seguridad", game_chips(game, memory),
              {{"Arriba/Abajo", "mover"}, {"A", "restaurar"}, {"X", "copia nueva"}, {"B", "volver"}}, [&] {
                  draw_panel(64, 100, 690, 560);
                  if (entries.empty()) {
                      gfx::text(96, 130, "Todavía no hay copias.", 22, kDim);
                      gfx::text(96, 168, "X crea una ahora; también se hace una sola al instalar una versión nueva.", 17, kDim);
                  }
                  for (int i = 0; i < count && i < 10; ++i)
                      draw_row(80, 112 + i * 54, 658, entries[i].name, std::to_string(entries[i].files) + " archivos · " + mib(entries[i].bytes), date_text(entries[i].unix_seconds), i == selected);
                  draw_panel(780, 100, 436, 560);
                  gfx::text(806, 124, "Qué se guarda", 24, kGold);
                  int y = draw_wrapped(806, 170, 384, "Tus partidas guardadas, los ajustes del juego (teclas y sonido), las capturas de pantalla y el registro de combate.", 17, rgba(201, 208, 226), 25);
                  y = draw_wrapped(806, y + 14, 384, "Se conservan las últimas 5 copias; las más antiguas se borran solas.", 17, kDim, 25);
                  y = draw_wrapped(806, y + 14, 384, "Al instalar una versión nueva del juego se crea una copia y tus datos pasan a la versión nueva.", 17, kDim, 25);
                  if (!notice.empty()) draw_wrapped(806, 560, 384, notice, 16, kOk, 24, 3);
              });
    }
}

// ---- Updates ------------------------------------------------------------------------------------------------------
std::string installed_version(const GameState& game) {
    if (!game.found) return "";
    const update_core::Version version = update_core::parse_version(short_name(game.text));
    return version.valid() ? update_core::version_text(version) : "";
}

struct DownloadContext {
    std::string name;
    u64 last_update = 0;
};

void draw_download_progress(const std::string& name, unsigned long long done, unsigned long long total, int step) {
    const unsigned percent = total ? static_cast<unsigned>(done * 100 / total) : 0;
    frame("Actualizaciones · Descargando", {}, {{"B", "cancelar"}}, [&] {
        gfx::text_center(gfx::kWidth / 2, 200, gfx::ellipsize(name, 28, 1000), 28, kText);
        gfx::rrect(210, 280, 860, 22, 11, kLine);
        if (percent > 0) gfx::rrect(210, 280, std::max(22, static_cast<int>(860ULL * percent / 100)), 22, 11, kGold);
        gfx::text(210, 312, mib(done) + " / " + mib(total), 18, kDim);
        gfx::text_right(1070, 312, std::to_string(percent) + " %", 18, kDim);
        const char* steps[] = {"Comprobar el espacio libre", "Descargar y descifrar el ZIP", "Instalar: extraer, comprobar y traer tus partidas",
                               "Borrar el ZIP descargado"};
        for (int i = 0; i < 4; ++i) {
            gfx::circle(420, 400 + i * 40 + 10, 9, i < step ? kOk : (i == step ? kGold : kLine));
            gfx::text(446, 400 + i * 40, steps[i], 20, i == step ? kText : kDim);
        }
        gfx::text_center(gfx::kWidth / 2, 600, "Tus partidas no se tocan: antes de instalar se hace una copia y pasan a la versión nueva.", 16, kDim);
    });
}

bool on_download_progress(unsigned long long done, unsigned long long total, void* user) {
    auto* context = static_cast<DownloadContext*>(user);
    const u64 now = armTicksToNs(armGetSystemTick());
    if (now - context->last_update > 150'000'000ULL) {
        context->last_update = now;
        draw_download_progress(context->name, done, total, 1);
        padUpdate(&g_pad);
        if (padGetButtonsDown(&g_pad) & (HidNpadButton_B | HidNpadButton_Plus)) return true;
    }
    return false;
}

// Downloads `latest` and installs it with the ZIP importer. Returns the message for the home screen.
std::string download_and_install(const updater::Latest& latest) {
    const unsigned long long needed = static_cast<unsigned long long>(latest.size) * 5 / 2 + (64ULL << 20);
    draw_download_progress(latest.file_name, 0, static_cast<unsigned long long>(latest.size), 0);
    if (pokenx_fs_free_bytes(kRoot) < needed)
        return "No hay espacio suficiente en la tarjeta SD: hacen falta unos " + mib(needed) + " libres.";
    DownloadContext context;
    context.name = latest.file_name.empty() ? latest.title : latest.file_name;
    std::string path;
    const std::string error = updater::download(latest, path, on_download_progress, &context);
    if (!error.empty()) return error + ".";
    draw_download_progress(context.name, static_cast<unsigned long long>(latest.size), static_cast<unsigned long long>(latest.size), 2);
    pokenx_set_unix_seconds(now_unix());
    const std::string outcome = import_zip(path);
    if (outcome.find("completada") != std::string::npos) {
        draw_download_progress(context.name, static_cast<unsigned long long>(latest.size), static_cast<unsigned long long>(latest.size), 3);
        std::remove(path.c_str());
        return "Actualizado a la versión " + (latest.version.empty() ? latest.title : latest.version) + ". Tus partidas pasaron a la versión nueva.";
    }
    return outcome + " El ZIP descargado sigue en downloads/.";
}

// "Descargar a mano": the address of the official page as a QR code and as text, with the three steps.
void show_manual_download(const GameState& game) {
    const std::string address = "https://pokemon-reloaded.blogspot.com/";
    const qr::Code code = qr::encode(address);
    while (appletMainLoop()) {
        const Input input = poll_input();
        if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_A) || pressed(input, HidNpadButton_Plus)) return;
        const Memory memory = read_memory();
        frame("Actualizaciones · Descargar a mano", game_chips(game, memory), {{"B", "volver"}}, [&] {
            draw_panel(64, 100, 560, 560);
            const int module = code.size ? 360 / (code.size + 8) : 1;  // 4 modules of quiet zone on every side
            const int side = (code.size + 8) * module;
            const int left = 64 + (560 - side) / 2, top = 130;
            gfx::rrect(left, top, side, side, 8, rgba(255, 255, 255));
            for (int y = 0; y < code.size; ++y)
                for (int x = 0; x < code.size; ++x)
                    if (code.dark(x, y)) gfx::fill(left + (x + 4) * module, top + (y + 4) * module, module, module, rgba(10, 12, 20));
            gfx::text_center(64 + 280, top + side + 24, "Apunta la cámara del móvil", 20, kText);
            gfx::text_center(64 + 280, top + side + 54, "pokemon-reloaded.blogspot.com", 18, kGold);
            draw_panel(650, 100, 566, 560);
            gfx::text(676, 124, "Descargar a mano", 24, kGold);
            int y = draw_wrapped(676, 170, 516, "Úsalo si la descarga desde la consola no funciona (sin internet, cuota de Mega agotada...).", 17, rgba(201, 208, 226), 25);
            const char* steps[] = {"1. Abre la página con el móvil o el PC y descarga el ZIP de la última versión.",
                                   "2. Copia el ZIP a la tarjeta SD (cualquier carpeta).",
                                   "3. Vuelve a Actualizaciones y elige «Instalar desde un ZIP de la SD»."};
            y += 14;
            for (const char* step : steps) y = draw_wrapped(676, y, 516, step, 18, kText, 26) + 10;
            draw_wrapped(676, y + 10, 516, "Tus partidas se conservan al instalar: pasan a la versión nueva y queda una copia en Copias de seguridad.", 16, kDim, 23);
        });
    }
}

void show_updates(GameState& game, std::string& note) {
    int selected = 0;
    updater::Latest latest;
    bool checked = false;
    std::string status;
    const std::string installed = installed_version(game);
    const auto compare_text = [&]() -> std::string {
        if (!checked) return "";
        if (!latest.ok) return "Sin respuesta";
        const update_core::Version a = update_core::parse_version(installed), b = update_core::parse_version(latest.version);
        if (!a.valid() || !b.valid()) return "Versión " + latest.version;
        return update_core::compare(a, b) < 0 ? "Hay una nueva" : "Estás al día";
    };
    constexpr int kRows = 6;
    // After a new version is installed, offer to check what it calls that the engine lacks (a few seconds).
    const auto offer_compatibility = [&]() {
        const Memory memory_now = read_memory();
        if (!game.found || memory_now.applet() || compat_is_current(load_compat_report(), game)) return;
        if (show_card("Comprobar esta versión", "Se ha cambiado el juego instalado. ¿Quieres comprobar ahora qué funciones de GameMaker llama que el motor no tiene? Tarda unos segundos y no cambia nada.",
                      {"Comprobar ahora", "Más tarde"}, kGold) == 0)
            show_compatibility(game, memory_now);
    };
    while (appletMainLoop()) {
        const Input input = poll_input();
        if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_Plus)) break;
        if (pressed(input, HidNpadButton_Down)) selected = (selected + 1) % kRows;
        if (pressed(input, HidNpadButton_Up)) selected = (selected + kRows - 1) % kRows;
        if (pressed(input, HidNpadButton_Y)) selected = 0;
        if (pressed(input, HidNpadButton_A)) {
            if (selected == 0) {
                gfx::begin();
                draw_top("Actualizaciones", {});
                gfx::text_center(gfx::kWidth / 2, 300, "Consultando el blog oficial...", 34, kText);
                gfx::text_center(gfx::kWidth / 2, 356, "Puede tardar unos segundos.", 20, kDim);
                gfx::end();
                const std::string network = updater::start_network();
                if (!network.empty()) {
                    latest = updater::Latest();
                    latest.error = network;
                } else {
                    latest = updater::check_latest();
                }
                checked = true;
                status = latest.ok ? "" : latest.error;
            } else if (selected == 1) {
                if (!checked || !latest.ok) {
                    show_card("Primero busca una actualización", "Elige «Buscar actualización» para saber qué versión hay en el blog oficial.", {"Volver"}, kWarn);
                } else {
                    const update_core::Version a = update_core::parse_version(installed), b = update_core::parse_version(latest.version);
                    const bool older_or_same = a.valid() && b.valid() && update_core::compare(a, b) >= 0;
                    const std::string version = latest.version.empty() ? latest.title : latest.version;
                    const int choice = show_card(older_or_same ? "Ya tienes esta versión" : "Instalar la versión " + version,
                                                 older_or_same ? "La versión del blog (" + version + ") no es más nueva que la instalada. Se puede descargar de nuevo igualmente; tus partidas se conservan."
                                                               : "Se descargan " + mib(static_cast<unsigned long long>(latest.size)) + ", se instalan y tus partidas pasan a la versión nueva (se hace una copia antes). La versión actual se borra al terminar (no quedan dos copias del juego).",
                                                 {older_or_same ? "Descargar de nuevo" : "Descargar e instalar", "Cancelar"}, kGold);
                    if (choice == 0) {
                        note = download_and_install(latest);
                        game = detect_game();
                        updater::stop_network();
                        offer_compatibility();
                        break;
                    }
                }
            } else if (selected == 2) {
                const std::string outcome = browse_zip(game);
                if (!outcome.empty()) {
                    note = outcome;
                    game = detect_game();
                    offer_compatibility();
                    break;
                }
            } else if (selected == 3) {
                show_backups(game, note);
            } else if (selected == 4) {
                show_manual_download(game);
            } else {
                show_compatibility(game, read_memory());
            }
        }
        const Memory memory = read_memory();
        const std::string verdict = compare_text();
        frame("Actualizaciones", game_chips(game, memory), {{"Arriba/Abajo", "mover"}, {"A", "elegir"}, {"B", "volver"}}, [&] {
            draw_panel(64, 100, 690, 560);
            draw_row(80, 112, 658, "Buscar actualización", verdict, "consulta el blog oficial", selected == 0, verdict == "Hay una nueva" ? kWarn : (verdict == "Estás al día" ? kOk : kGold));
            draw_row(80, 166, 658, "Descargar e instalar", checked && latest.ok ? mib(static_cast<unsigned long long>(latest.size)) : "", "baja el ZIP y lo instala", selected == 1, kGold);
            draw_row(80, 220, 658, "Instalar desde un ZIP de la SD", "explorador", "sin internet", selected == 2);
            draw_row(80, 274, 658, "Copias de seguridad", "partidas y ajustes", "restaurar", selected == 3);
            draw_row(80, 328, 658, "Descargar a mano", "código QR y enlace", "sin descarga desde la consola", selected == 4);
            draw_row(80, 382, 658, "Compatibilidad de esta versión", "funciones que faltan", "análisis del código", selected == 5);
            gfx::text(96, 600, "Tus partidas se conservan al actualizar.", 16, kDim);
            draw_panel(780, 100, 436, 560);
            gfx::text(806, 124, "Juego instalado", 24, kGold);
            gfx::text(806, 170, installed.empty() ? (game.found ? "Versión no reconocida" : "Ninguno") : "Versión " + installed, 21, kText);
            if (game.found) gfx::text(806, 200, gfx::ellipsize(short_name(game.text), 14, 392), 14, kDim);
            if (checked && latest.ok) {
                gfx::text(806, 250, "En el blog oficial", 20, kGold);
                gfx::text(806, 282, gfx::ellipsize(latest.title + (latest.version.empty() ? "" : "  ·  " + latest.version), 18, 392), 18, kText);
                gfx::text(806, 310, latest.published + "  ·  " + mib(static_cast<unsigned long long>(latest.size)), 15, kDim);
                draw_wrapped(806, 342, 392, latest.notes, 15, rgba(201, 208, 226), 22, 8);
            } else if (checked) {
                gfx::text(806, 250, "No se pudo consultar", 20, kBad);
                draw_wrapped(806, 284, 392, status, 17, rgba(201, 208, 226), 25, 6);
                gfx::text(806, 560, "Prueba «Descargar a mano» (QR) o el ZIP de la SD.", 16, kDim);
            } else {
                draw_wrapped(806, 250, 392, "Pulsa «Buscar actualización» para ver si el blog oficial tiene una versión más nueva.", 17, rgba(201, 208, 226), 25);
            }
        });
    }
    updater::stop_network();
}

// ---- Home -------------------------------------------------------------------------------------------------------
struct Tile {
    const char* label;
    const char* sub;
    Icon icon;
    const char* description;
};

const Tile kTiles[] = {
    {"Jugar", "", Icon::Play, "Carga el juego y empieza a jugar."},
    {"Actualizaciones", "juego y versión", Icon::Update, "Busca versiones nuevas en el blog oficial, instala un ZIP sin perder tus partidas y gestiona copias."},
    {"Opciones", "vídeo · rendimiento", Icon::Options, "Resolución, escalado, rendimiento, caché de archivos, precarga y medidor de FPS."},
    {"Mandos", "botones y capa", Icon::Pad, "Elige qué tecla produce cada botón y la capa para F1-F12, Supr, Re Pág..."},
    {"Teclas", "del juego", Icon::Keys, "Las teclas que usa el juego, qué botón las produce y cuáles no tienen botón."},
    {"Acceso HOME", "crear acceso", Icon::Home, "Un icono de PokeNX en la pantalla de inicio de la consola (forwarder)."},
    {"Diagnóstico", "registros", Icon::Diag, "Memoria, modo, tiempos de carga y registros; exporta un informe."},
    {"Salir", "a hbmenu", Icon::Exit, "Cierra PokeNX y vuelve al menú de homebrew."},
};
constexpr int kTileCount = 8;

void draw_tiles(int selected, bool play_ok) {
    const int small_w = 120, small_h = 170, big_w = 262, big_h = 250, gap = 12;
    const int total = big_w + (kTileCount - 1) * small_w + (kTileCount - 1) * gap;
    int x = (gfx::kWidth - total) / 2;
    const int center_y = 330;
    for (int i = 0; i < kTileCount; ++i) {
        const bool big = i == 0;
        const int w = big ? big_w : small_w, h = big ? big_h : small_h;
        const int y = center_y - h / 2;
        const bool sel = i == selected;
        if (sel) gfx::rrect(x - 5, y - 5, w + 10, h + 10, 20, kGold);
        if (big) {
            gfx::rrect_gradient(x, y, w, h, 16, play_ok ? kGold : rgba(120, 112, 90), play_ok ? kGoldDark : rgba(80, 74, 62));
            draw_icon(kTiles[i].icon, x + w / 2, y + 100, 46, kInk);
            gfx::text_center(x + w / 2, y + 182, kTiles[i].label, 30, kInk);
            if (!play_ok) gfx::text_center(x + w / 2, y + 220, "no disponible", 15, kInk);
        } else {
            gfx::rrect(x, y, w, h, 14, sel ? rgba(35, 43, 68) : kTile);
            draw_icon(kTiles[i].icon, x + w / 2, y + 58, 24, sel ? kGold : rgba(201, 208, 226));
            gfx::text_center(x + w / 2, y + 108, gfx::ellipsize(kTiles[i].label, 19, w - 8), 19, kText);
            gfx::text_center(x + w / 2, y + 138, gfx::ellipsize(kTiles[i].sub, 13, w - 12), 13, kDim);
        }
        x += w + gap;
    }
}

void draw_loading(const GameState& game) {
    gfx::begin();
    draw_top("", {});
    gfx::text_center(gfx::kWidth / 2, 250, "Preparando el juego...", 40, kText);
    gfx::text_center(gfx::kWidth / 2, 310, game.found ? short_name(game.text) : "", 22, kDim);
    gfx::rrect(320, 380, 640, 18, 9, kLine);
    gfx::rrect(320, 380, 180, 18, 9, kGold);
    gfx::text_center(gfx::kWidth / 2, 430, "Puede tardar unos segundos. Pulsa + para cancelar.", 20, kDim);
    gfx::text_center(gfx::kWidth / 2, 462, "Con la caché de archivos activada, las cargas siguientes son más rápidas.", 17, kDim);
    gfx::end();
}

// One line in boot.log per start: which build, handheld or docked, memory and how the game got onto the SD. It is what
// makes a "it worked for me" or "it did not" report usable.
void log_session(const GameState& game) {
    FILE* boot = std::fopen("sdmc:/switch/pokenx/logs/boot.log", "a");
    if (!boot) return;
    const Memory memory = read_memory();
    const bool docked = appletGetOperationMode() == AppletOperationMode_Console;
    const char* source = "no game found";
    if (game.found) {
        source = "a folder (copied by hand, or installed before this marker existed)";
        if (FILE* marker = std::fopen(kInstallSourcePath, "r")) {
            char how[16] = {0};
            long long marked = -1;
            if (std::fscanf(marker, "%15s %lld", how, &marked) == 2 && std::strcmp(how, "zip") == 0 &&
                marked == file_size_of(game.text))
                source = "a ZIP imported by the app";
            std::fclose(marker);
        }
    }
    std::fprintf(boot, "5. session: build %s %s, %s, memory total %llu MiB used %llu MiB, game installed from %s\n", __DATE__,
                 __TIME__, docked ? "docked" : "handheld", static_cast<unsigned long long>(memory.total >> 20),
                 static_cast<unsigned long long>(memory.used >> 20), source);
    std::fclose(boot);
}

MenuResult run_graphical(const MenuInfo& info) {
    GameState game = detect_game();
    log_session(game);
    preload_start(game, read_memory());
    std::string note;
    int selected = game.found && info.play_allowed ? 0 : 1;
    MenuResult result = MenuResult::Quit;
    while (appletMainLoop()) {
        const Input input = poll_input();
        if (pressed(input, HidNpadButton_Plus)) break;
        if (pressed(input, HidNpadButton_Right)) selected = (selected + 1) % kTileCount;
        if (pressed(input, HidNpadButton_Left)) selected = (selected + kTileCount - 1) % kTileCount;
        const bool play_ok = game.found && info.play_allowed;
        if (pressed(input, HidNpadButton_A)) {
            note.clear();
            if (selected == 0) {
                if (play_ok) {
                    draw_loading(game);
                    result = MenuResult::Play;
                    break;
                }
                if (!game.found) {
                    const int choice = show_card("No hay juego instalado",
                                                 "El menú no encontró un .exe GameMaker 8.0 válido en sdmc:/switch/pokenx/game/. "
                                                 "Copia el juego (el .exe y su carpeta data/) o instálalo desde Actualizaciones.",
                                                 {"Buscar o instalar el juego", "Volver"}, kBad);
                    if (choice == 0) {
                        show_updates(game, note);
                        game = detect_game();
                        preload_start(game, read_memory());
                    }
                } else {
                    show_card("«Jugar» no está disponible ahora", info.blocked_reason.empty() ? "El proceso no tiene memoria suficiente." : info.blocked_reason,
                              {"Volver"}, kWarn);
                }
            } else if (selected == 1) {
                show_updates(game, note);
                game = detect_game();
                preload_start(game, read_memory());
            } else if (selected == 2) {
                show_options(game);
            } else if (selected == 3) {
                show_controls(game);
            } else if (selected == 4) {
                show_game_keys(game, read_memory());
            } else if (selected == 5) {
                show_home_shortcut(game);
            } else if (selected == 6) {
                show_diagnostics(game);
            } else {
                break;
            }
        }
        const Memory memory = read_memory();
        frame("", game_chips(game, memory), {{"Izq/Der", "mover"}, {"A", "elegir"}, {"+", "salir"}}, [&] {
            draw_tiles(selected, play_ok);
            gfx::text_center(gfx::kWidth / 2, 484, kTiles[selected].description, 22, rgba(201, 208, 226));
            if (selected == 0 && game.found) {
                const long load = last_load_seconds_x10();
                gfx::text_center(gfx::kWidth / 2, 520,
                                 short_name(game.text) + (load >= 0 ? "  ·  última carga: " + seconds_text(load) : ""), 17, kDim);
            }
            const std::string& banner = !note.empty() ? note : info.message;
            if (!banner.empty()) {
                const int w = 1000, x = (gfx::kWidth - w) / 2;
                gfx::rrect(x, 572, w, 64, 14, rgba(245, 176, 65, 40));
                gfx::text_center(gfx::kWidth / 2, 592, gfx::ellipsize(banner, 18, w - 40), 18, kWarn);
            }
        });
    }
    preload_join();  // finish (or never start) the background parse before the engine takes over
    return result;
}

}  // namespace

MenuResult run_menu(const MenuInfo& info) {
    padConfigureInput(8, HidNpadStyleSet_NpadStandard);
    padInitializeAny(&g_pad);
    const bool ready = gfx::init();
    if (FILE* boot = std::fopen("sdmc:/switch/pokenx/logs/boot.log", "a")) {
        std::fprintf(boot, ready ? "4. graphical menu ready\n" : "4. graphical menu FAILED to start (framebuffer or system font): using the text console\n");
        std::fclose(boot);
    }
    if (!ready) return run_text_menu(info);
    const MenuResult result = run_graphical(info);
    gfx::shutdown();  // releases the window before EGL/Mesa take it
    return result;
}
