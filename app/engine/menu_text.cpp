#include "menu.h"  // text-console fallback menu (run_text_menu); the graphical menu is menu.cpp

#include <switch.h>

#include <sys/iosupport.h>

#include <dirent.h>
#include <sys/stat.h>

#include <algorithm>
#include <cctype>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include "pokenx_engine.h"

namespace {

constexpr const char* kRoot = "sdmc:/switch/pokenx";
constexpr const char* kGameDir = "sdmc:/switch/pokenx/game";
constexpr const char* kResolutionFile = "sdmc:/switch/pokenx/resolution.txt";
constexpr const char* kScalingFile = "sdmc:/switch/pokenx/scaling.txt";

const char* const kGreen = "\x1b[32;1m";
const char* const kYellow = "\x1b[33;1m";
const char* const kRed = "\x1b[31;1m";
const char* const kDim = "\x1b[37;2m";
const char* const kReset = "\x1b[0m";

PadState g_pad;

// One frame of input: button presses plus the left stick turned into up/down/left/right presses.
struct Input {
    u64 down = 0;
};

Input poll_input() {
    static int stick_vertical = 0;
    static int stick_horizontal = 0;
    padUpdate(&g_pad);
    Input input;
    input.down = padGetButtonsDown(&g_pad);
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
    struct stat info;
    return stat(path, &info) == 0;
}

std::string lower(std::string text) {
    std::transform(text.begin(), text.end(), text.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return text;
}

// ---- Game detection ---------------------------------------------------------------------------------------------
struct GameState {
    bool found = false;
    std::string text;  // exe path when found, otherwise the reason
};

GameState detect_game() {
    char buffer[600] = {0};
    GameState state;
    state.found = pokenx_find_game(kGameDir, buffer, sizeof(buffer)) == 0;
    state.text = buffer;
    return state;
}

std::string short_name(const std::string& path) {
    const size_t slash = path.find_last_of('/');
    return slash == std::string::npos ? path : path.substr(slash + 1);
}

// ---- Screens ----------------------------------------------------------------------------------------------------
void header() {
    std::printf("%sPokeNX%s - runtime OpenGMK nativo para Nintendo Switch\n", kGreen, kReset);
    std::printf("%s------------------------------------------------------------------------%s\n", kDim, kReset);
}

void memory_line() {
    u64 total = 0, used = 0;
    svcGetInfo(&total, InfoType_TotalMemorySize, CUR_PROCESS_HANDLE, 0);
    svcGetInfo(&used, InfoType_UsedMemorySize, CUR_PROCESS_HANDLE, 0);
    std::printf("Memoria del proceso: %llu MiB (usada %llu MiB)  Modo: %s\n", static_cast<unsigned long long>(total >> 20),
                static_cast<unsigned long long>(used >> 20), total < (1024ULL << 20) ? "applet (poca RAM)" : "aplicacion");
}

const char* const kMainItems[] = {"Jugar", "Importar un ZIP del juego", "Opciones", "Diagnostico", "Salir"};
constexpr int kMainCount = 5;

void draw_main(int selected, const GameState& game, const MenuInfo& info, const std::string& note) {
    consoleClear();
    header();
    memory_line();
    if (game.found) {
        std::printf("Juego: %sdetectado%s  %s\n", kGreen, kReset, short_name(game.text).c_str());
    } else {
        std::printf("Juego: %sno detectado%s  (%s)\n", kRed, kReset, game.text.c_str());
        std::printf("%sCopia el juego (el .exe y su carpeta data/) a sdmc:/switch/pokenx/game/ o importa un ZIP.%s\n", kDim,
                    kReset);
    }
    std::printf("\n");
    for (int i = 0; i < kMainCount; ++i) {
        const bool blocked = i == 0 && !info.play_allowed;
        std::printf(" %s%s%s%s\n", i == selected ? kYellow : "", i == selected ? "> " : "  ", kMainItems[i],
                    (blocked ? "  [no disponible aqui]" : ""));
        std::printf("%s", kReset);
    }
    std::printf("\n");
    if (!info.message.empty()) std::printf("%s%s%s\n\n", kYellow, info.message.c_str(), kReset);
    if (!note.empty()) std::printf("%s%s%s\n\n", kYellow, note.c_str(), kReset);
    std::printf("%sArriba/Abajo: mover   A: elegir   +: salir%s\n", kDim, kReset);
    consoleUpdate(nullptr);
}

void show_options() {
    int selected = 0;
    bool dirty = true;
    while (appletMainLoop()) {
        const Input input = poll_input();
        const std::string resolution = read_token(kResolutionFile, "720p");
        const std::string scaling = read_token(kScalingFile, "full");
        if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_Plus)) return;
        if (pressed(input, HidNpadButton_Down)) { selected = (selected + 1) % 2; dirty = true; }
        if (pressed(input, HidNpadButton_Up)) { selected = (selected + 1) % 2; dirty = true; }
        if (pressed(input, HidNpadButton_A) || pressed(input, HidNpadButton_Left) || pressed(input, HidNpadButton_Right)) {
            if (selected == 0) write_token(kResolutionFile, resolution == "720p" ? "1080p" : "720p");
            if (selected == 1) write_token(kScalingFile, scaling == "full" ? "aspect" : "full");
            dirty = true;
        }
        if (dirty) {
            consoleClear();
            header();
            std::printf("Opciones (se aplican al siguiente arranque del juego)\n\n");
            const std::string values[2] = {read_token(kResolutionFile, "720p"), read_token(kScalingFile, "full")};
            const char* labels[2] = {"Resolucion de salida (16:9)", "Escalado de la imagen"};
            const char* hints[2] = {"720p (recomendado) / 1080p", "full = rellena 16:9 / aspect = 4:3 con bandas"};
            for (int i = 0; i < 2; ++i) {
                std::printf(" %s%s%-30s %s%s\n", i == selected ? kYellow : "", i == selected ? "> " : "  ", labels[i],
                            values[i].c_str(), kReset);
                if (i == selected) std::printf("     %s%s%s\n", kDim, hints[i], kReset);
            }
            std::printf("\n%sArriba/Abajo: mover   A: cambiar   B: volver%s\n", kDim, kReset);
            consoleUpdate(nullptr);
            dirty = false;
        }
        consoleUpdate(nullptr);
    }
}

void print_log_line(const char* name, const char* path) {
    const unsigned long long size = file_size(path);
    std::printf("  %-14s %s\n", name, file_exists(path) ? (size ? "con contenido" : "vacio") : "no existe");
}

void print_tail(const char* path, int lines) {
    FILE* file = std::fopen(path, "r");
    if (!file) return;
    std::fseek(file, 0, SEEK_END);
    long size = std::ftell(file);
    long start = size > 2400 ? size - 2400 : 0;
    std::fseek(file, start, SEEK_SET);
    std::vector<std::string> collected;
    char line[200];
    while (std::fgets(line, sizeof(line), file)) {
        std::string text(line);
        while (!text.empty() && (text.back() == '\n' || text.back() == '\r')) text.pop_back();
        if (text.size() > 76) text.resize(76);
        collected.push_back(text);
    }
    std::fclose(file);
    const int begin = static_cast<int>(collected.size()) > lines ? static_cast<int>(collected.size()) - lines : 0;
    for (size_t i = begin; i < collected.size(); ++i) std::printf("    %s\n", collected[i].c_str());
}

void show_diagnostics(const GameState& game) {
    consoleClear();
    header();
    memory_line();
    std::printf("Juego: %s\n", game.found ? game.text.c_str() : game.text.c_str());
    std::printf("\nRegistros en sdmc:/switch/pokenx/logs/\n");
    print_log_line("engine.log", "sdmc:/switch/pokenx/logs/engine.log");
    print_log_line("engine.log.1", "sdmc:/switch/pokenx/logs/engine.log.1");
    print_log_line("boot.log", "sdmc:/switch/pokenx/logs/boot.log");
    print_log_line("crash.log", "sdmc:/switch/pokenx/logs/crash.log");
    std::printf("\nUltimas lineas de engine.log.1 (ejecucion anterior):\n");
    print_tail("sdmc:/switch/pokenx/logs/engine.log.1", 8);
    std::printf("\nUltimas lineas de engine.log (esta ejecucion):\n");
    print_tail("sdmc:/switch/pokenx/logs/engine.log", 6);
    std::printf("\n%sB: volver%s\n", kDim, kReset);
    consoleUpdate(nullptr);
    while (appletMainLoop()) {
        const Input input = poll_input();
        if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_Plus) || pressed(input, HidNpadButton_A)) return;
        consoleUpdate(nullptr);
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
};

int32_t on_progress(uint64_t done, uint64_t total, void* user) {
    auto* context = static_cast<ProgressContext*>(user);
    const u64 now = armTicksToNs(armGetSystemTick());
    if (now - context->last_update > 150'000'000ULL) {
        context->last_update = now;
        std::printf("\r  Copiando... %3u%%  (%llu / %llu MiB)   B: cancelar   ",
                    total ? static_cast<unsigned>(done * 100 / total) : 0u, static_cast<unsigned long long>(done >> 20),
                    static_cast<unsigned long long>(total >> 20));
        consoleUpdate(nullptr);
        padUpdate(&g_pad);
        if (padGetButtonsDown(&g_pad) & (HidNpadButton_B | HidNpadButton_Plus)) return 1;
    }
    return 0;
}

// Returns a message for the main menu.
std::string import_zip(const std::string& zip_path) {
    consoleClear();
    header();
    std::printf("Importando %s\n", short_name(zip_path).c_str());
    std::printf("%sSe reemplaza sdmc:/switch/pokenx/game/ (la versión anterior se borra; tus partidas pasan a la nueva).%s\n\n", kDim, kReset);
    consoleUpdate(nullptr);
    ProgressContext context;
    const int32_t code = pokenx_import_zip(zip_path.c_str(), on_progress, &context);
    std::printf("\n");
    if (code == 0) return "Importacion completada: el juego quedo en sdmc:/switch/pokenx/game/.";
    char text[200] = {0};
    pokenx_import_error_text(code, text, sizeof(text));
    return std::string("Importacion no instalada: ") + text + ".";
}

std::string browse_zip() {
    std::string path = kRoot;
    int selected = 0;
    bool dirty = true;
    std::vector<DirEntry> entries = list_directory(path);
    while (appletMainLoop()) {
        const Input input = poll_input();
        const int count = static_cast<int>(entries.size());
        if (pressed(input, HidNpadButton_Plus)) return "";
        if (pressed(input, HidNpadButton_Down) && count) { selected = (selected + 1) % count; dirty = true; }
        if (pressed(input, HidNpadButton_Up) && count) { selected = (selected + count - 1) % count; dirty = true; }
        if (pressed(input, HidNpadButton_B) || pressed(input, HidNpadButton_Left)) {
            const size_t slash = path.find_last_of('/');
            if (path == "sdmc:/" || path.size() <= 6 || slash == std::string::npos) return "";
            path = slash <= 5 ? std::string("sdmc:/") : path.substr(0, slash);
            entries = list_directory(path);
            selected = 0;
            dirty = true;
        }
        if (pressed(input, HidNpadButton_A) && count) {
            const DirEntry& entry = entries[selected];
            const std::string full = path + (path.back() == '/' ? "" : "/") + entry.name;
            if (entry.is_dir) {
                path = full;
                entries = list_directory(path);
                selected = 0;
                dirty = true;
            } else {
                return import_zip(full);
            }
        }
        if (dirty) {
            consoleClear();
            header();
            std::printf("Elige el ZIP del juego.  Carpeta: %s\n\n", path.c_str());
            if (entries.empty()) std::printf("  (sin carpetas ni .zip aqui)\n");
            const int window = 30;
            const int first = std::max(0, std::min(selected - window / 2, count - window));
            for (int i = first; i < count && i < first + window; ++i) {
                std::printf(" %s%s%s%s", i == selected ? kYellow : "", i == selected ? "> " : "  ",
                            entries[i].is_dir ? "[carpeta] " : "", entries[i].name.c_str());
                if (!entries[i].is_dir) std::printf("  (%llu MiB)", entries[i].size >> 20);
                std::printf("%s\n", kReset);
            }
            std::printf("\n%sA: abrir/elegir   B: subir de carpeta   +: cancelar%s\n", kDim, kReset);
            consoleUpdate(nullptr);
            dirty = false;
        }
        consoleUpdate(nullptr);
    }
    return "";
}

}  // namespace

MenuResult run_text_menu(const MenuInfo& info) {
    // consoleInit points stdout at the text console; consoleExit does not undo it. The console has no framebuffer after
    // consoleExit, so any later printf (or Rust println!) would write through a null pointer and kill the process. The
    // original stdout (the null device) is put back when the menu closes. run_menu runs once per process: libnx only
    // sets the devoptab on the first consoleInit.
    const devoptab_t* const previous_stdout = devoptab_list[STD_OUT];
    PrintConsole* const console = consoleInit(nullptr);
    {
        // boot.log is the file the user is asked for: say whether the menu could draw. An invisible menu (framebuffer or
        // NWindow creation failed) would otherwise look exactly like a black screen.
        const bool ready = console && console->consoleInitialised;
        if (FILE* boot = std::fopen("sdmc:/switch/pokenx/logs/boot.log", "a")) {
            std::fprintf(boot, ready ? "4. text menu console ready\n" : "4. text menu console FAILED to initialise (menu not visible)\n");
            std::fclose(boot);
        }
    }
    padConfigureInput(8, HidNpadStyleSet_NpadStandard);
    padInitializeAny(&g_pad);

    GameState game = detect_game();
    std::string note;
    int selected = game.found && info.play_allowed ? 0 : 1;
    bool dirty = true;
    MenuResult result = MenuResult::Quit;
    while (appletMainLoop()) {
        const Input input = poll_input();
        if (pressed(input, HidNpadButton_Plus)) break;
        if (pressed(input, HidNpadButton_Down)) { selected = (selected + 1) % kMainCount; dirty = true; }
        if (pressed(input, HidNpadButton_Up)) { selected = (selected + kMainCount - 1) % kMainCount; dirty = true; }
        if (pressed(input, HidNpadButton_A)) {
            note.clear();
            dirty = true;
            if (selected == 0) {
                if (!game.found) note = "No hay juego detectado: copialo a game/ o importa un ZIP.";
                else if (!info.play_allowed) note = info.blocked_reason;
                else { result = MenuResult::Play; break; }
            } else if (selected == 1) {
                const std::string outcome = browse_zip();
                if (!outcome.empty()) note = outcome;
                game = detect_game();
            } else if (selected == 2) {
                show_options();
            } else if (selected == 3) {
                show_diagnostics(game);
            } else {
                break;
            }
        }
        if (dirty) {
            draw_main(selected, game, info, note);
            dirty = false;
        }
        consoleUpdate(nullptr);
    }
    consoleExit(nullptr);
    devoptab_list[STD_OUT] = previous_stdout;
    return result;
}
