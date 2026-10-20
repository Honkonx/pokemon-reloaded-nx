// PokeNX engine payload: libnx entrypoint that owns startup, the EGL/Mesa
// context, HID and audout, and hosts the Rust engine through pokenx_engine.h.
//
// This is the whole of pokenx.nro: the graphical menu (menu.cpp), this host and the Rust engine are one binary; there
// is no second executable. Build success is not evidence that it runs.
#include <switch.h>

#include <EGL/egl.h>
#include <EGL/eglext.h>

#ifdef POKENX_MESA26
extern "C" void pokenx_mesa_bind_thread();
#endif

#include <algorithm>
#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

#include <string>
#include <vector>

#include "fps_overlay.h"
#include "menu.h"
#include "pokenx_engine.h"

namespace {

constexpr const char* kDataRoot = "sdmc:/switch/pokenx";
constexpr const char* kLogPath = "sdmc:/switch/pokenx/logs/engine.log";
constexpr const char* kDriverPath = "sdmc:/switch/pokenx/graphics-driver.txt";
constexpr const char* kScalingPath = "sdmc:/switch/pokenx/scaling.txt";
constexpr const char* kResolutionPath = "sdmc:/switch/pokenx/resolution.txt";
// 16:9 output. 720p by default (handheld native, scaled by the system when docked); `resolution.txt` with
// "1080p" renders at 1920x1080 instead (more GPU work, sharper when docked).
uint32_t kRenderWidth = 1280;
uint32_t kRenderHeight = 720;
void log_line(const char* line);
void select_resolution() {
    char value[16] = "720p";
    if (FILE* config = std::fopen(kResolutionPath, "r")) {
        if (std::fscanf(config, "%15s", value) != 1) std::snprintf(value, sizeof(value), "720p");
        std::fclose(config);
    }
    if (std::strcmp(value, "1080p") == 0) {
        kRenderWidth = 1920;
        kRenderHeight = 1080;
    } else {
        kRenderWidth = 1280;
        kRenderHeight = 720;
    }
    log_line(kRenderHeight == 1080 ? "Resolution: 1080p (1920x1080)" : "Resolution: 720p (1280x720)");
}
constexpr const char* kGameDir = "sdmc:/switch/pokenx/game";
constexpr u32 kAudioFrames = 1024;  // ~21 ms at 48 kHz (th07-style ring of 4)
constexpr u32 kAudioBytes = kAudioFrames * 2 * sizeof(s16);
constexpr u32 kAudioAligned = (kAudioBytes + 0xFFF) & ~0xFFFu;
constexpr int kAudioBuffers = 4;
constexpr u64 kNanosPerSecond = 1000000000ULL;

FILE* g_log = nullptr;

// Writes only to the SD log. It must not touch stdout: while the text menu is up stdout is libnx's console, and after
// consoleExit libnx leaves that console attached to stdout with no framebuffer (the next printf writes through a null
// pointer and the process dies; see menu.cpp run_menu).
//
// Every line carries the milliseconds since the first line. The SD flush is throttled (the GM8 parser emits tens of
// thousands of " + Added ..." lines and one SD write per line made loading very slow).
u64 g_log_start_tick = 0;
u64 g_log_last_flush_ms = 0;
unsigned g_log_lines = 0;
void loading_pump();  // defined after the pad state; called while the game is being parsed
bool g_loading = false;

// Progress seen by the watchdog thread: every log line and every presented frame counts. When it stops moving for a while,
// the watchdog writes where it stopped (the last line), which a throttled flush used to lose (see Watchdog below).
std::atomic<u64> g_progress{0};
Mutex g_last_line_mutex = {};
char g_last_line[160] = {0};
u64 g_last_line_ms = 0;

void log_line(const char* line) {
    if (!g_log_start_tick) g_log_start_tick = armGetSystemTick();
    const u64 ms = armTicksToNs(armGetSystemTick() - g_log_start_tick) / 1000000ULL;
    ++g_log_lines;
    g_progress.fetch_add(1, std::memory_order_relaxed);
    if (std::strncmp(line, "mark:", 5) == 0) {  // checkpoints worth keeping the text of (not the thousands of " + Added" lines)
        mutexLock(&g_last_line_mutex);
        std::snprintf(g_last_line, sizeof(g_last_line), "%s", line);
        g_last_line_ms = ms;
        mutexUnlock(&g_last_line_mutex);
    }
    if (g_log) {
        std::fprintf(g_log, "[%7llu] %s\n", static_cast<unsigned long long>(ms), line);
        // One SD write costs ~4 ms (measured: consecutive log lines 4 ms apart), and the game logs many tiny pieces.
        // Flush at most every 100 ms, except for lines that report a problem (a crash must leave them on the card) and
        // the "mark:" checkpoints (a few per session: they say where a stall happened, so they must reach the card).
        const bool urgent = std::strstr(line, "rror") || std::strstr(line, "ailed") || std::strstr(line, "anic") ||
                            std::strncmp(line, "mark:", 5) == 0;
        if (urgent || ms - g_log_last_flush_ms >= 100) {
            std::fflush(g_log);
            g_log_last_flush_ms = ms;
        }
    }
    if (g_loading) loading_pump();
}

void log_fmt(const char* format, unsigned value) {
    char buffer[160];
    std::snprintf(buffer, sizeof(buffer), format, value);
    log_line(buffer);
}

void log_i32(const char* format, int value) {
    char buffer[160];
    std::snprintf(buffer, sizeof(buffer), format, value);
    log_line(buffer);
}

// Process memory at a named checkpoint: the game needs ~200 MiB while loading, and in applet mode the limit is low.
void log_memory(const char* label) {
    u64 total = 0, used = 0;
    svcGetInfo(&total, InfoType_TotalMemorySize, CUR_PROCESS_HANDLE, 0);
    svcGetInfo(&used, InfoType_UsedMemorySize, CUR_PROCESS_HANDLE, 0);
    char line[128];
    std::snprintf(line, sizeof(line), "Memory (%s): total %llu MiB, used %llu MiB", label,
                  static_cast<unsigned long long>(total >> 20), static_cast<unsigned long long>(used >> 20));
    log_line(line);
}

// A Zink session that never presents its first game frame (a stall inside the driver looks like a black screen and the only
// way out is to close the program) must not repeat by itself. Starting a Zink session writes this marker; the first
// presented game frame, or any clean end of the session, removes it. The next start finds it only after a stall or a crash:
// before the menu shows, the driver goes back to nvc0 and the menu says so. Choosing Zink again in Opciones starts over.
constexpr const char* kZinkPendingPath = "sdmc:/switch/pokenx/zink-pending.txt";
bool g_zink_pending = false;

void zink_pending_set() {
    if (FILE* marker = std::fopen(kZinkPendingPath, "w")) {
        std::fputs("Zink session started; removed when the first game frame is presented.\n", marker);
        std::fclose(marker);
        g_zink_pending = true;
    }
}

void zink_pending_clear() {
    if (!g_zink_pending) return;
    g_zink_pending = false;
    std::remove(kZinkPendingPath);
}

// Returns a sentence for the menu when the previous Zink session never presented a frame (and puts nvc0 in
// graphics-driver.txt), or an empty string.
std::string zink_fall_back_if_stalled() {
    struct stat marker;
    if (stat(kZinkPendingPath, &marker) != 0) return std::string();
    std::remove(kZinkPendingPath);
    char requested[16] = "nvc0";
    if (FILE* config = std::fopen(kDriverPath, "r")) {
        if (std::fscanf(config, "%15s", requested) != 1) std::snprintf(requested, sizeof(requested), "nvc0");
        std::fclose(config);
    }
    if (std::strcmp(requested, "zink") != 0) return std::string();
    if (FILE* config = std::fopen(kDriverPath, "w")) {
        std::fputs("nvc0\n", config);
        std::fclose(config);
    }
    log_line("Zink: the previous session never presented a game frame; graphics driver set back to nvc0");
    return "La sesion anterior con Zink no llego a mostrar ni un fotograma: el driver grafico vuelve a nvc0. Puedes elegir Zink otra vez en Opciones.";
}

// Developer file, not in the menu: `sdmc:/switch/pokenx/dev-env.txt`, one NAME=VALUE per line (# starts a comment), applied
// before EGL starts. It lets the Mesa/NVK knobs and the engine's POKENX_* settings be tried on the console without
// rebuilding (for example NVK_SWITCH_MAPPED_COMPLETION=0, MESA_SHADER_CACHE_DISABLE=true, POKENX_ATLAS_MAX=2048). Only
// names starting with NVK_, MESA_, NOUVEAU_, ZINK_ or POKENX_ are accepted, and each applied line is written to engine.log.
void apply_dev_env() {
    FILE* file = std::fopen("sdmc:/switch/pokenx/dev-env.txt", "r");
    if (!file) return;
    char line[200];
    while (std::fgets(line, sizeof(line), file)) {
        line[std::strcspn(line, "\r\n")] = 0;
        char* equals = std::strchr(line, '=');
        if (!equals || line[0] == '#') continue;
        *equals = 0;
        const char* name = line;
        const char* value = equals + 1;
        static const char* const kAllowed[] = {"NVK_", "MESA_", "NOUVEAU_", "ZINK_", "POKENX_"};
        bool allowed = false;
        for (const char* prefix : kAllowed)
            if (std::strncmp(name, prefix, std::strlen(prefix)) == 0) allowed = true;
        if (!allowed || !*value) {
            log_line("dev-env.txt: a line was ignored (name not allowed or empty value)");
            continue;
        }
        setenv(name, value, 1);
        char text[240];
        std::snprintf(text, sizeof(text), "dev-env.txt: %s=%s", name, value);
        log_line(text);
    }
    std::fclose(file);
}

// nvc0 or zink, chosen before eglInitialize through MESA_SWITCH_GL_DRIVER. Only Mesa 26.x
// (mesa-switch) reads it; the devkitPro switch-mesa 20.1.0 package ignores it.
void select_graphics_driver() {
    char requested[16] = "nvc0";
    if (FILE* config = std::fopen(kDriverPath, "r")) {
        if (std::fscanf(config, "%15s", requested) != 1) std::snprintf(requested, sizeof(requested), "nvc0");
        std::fclose(config);
    }
    if (std::strcmp(requested, "nvc0") != 0 && std::strcmp(requested, "zink") != 0) {
        log_line("graphics-driver.txt invalid; using nvc0");
        std::snprintf(requested, sizeof(requested), "nvc0");
    }
    // The menu no longer offers Zink (it does not present a frame yet). It still runs for development: graphics-driver.txt
    // says zink AND dev-env.txt exists. A leftover "zink" from an older version of the menu is therefore ignored.
    struct stat dev_probe;
    if (std::strcmp(requested, "zink") == 0 && stat("sdmc:/switch/pokenx/dev-env.txt", &dev_probe) != 0) {
        log_line("Zink is not offered in the menu and runs only with dev-env.txt present; using nvc0");
        std::snprintf(requested, sizeof(requested), "nvc0");
    }
#ifdef POKENX_MESA26
    log_line("Linked Mesa 26 unified: NVC0 and Zink/NVK compiled; hardware behavior unverified");
#else
    log_line("Linked Mesa 20.1: NVC0 only");
    if (std::strcmp(requested, "zink") == 0) {
        log_line("Zink is unavailable in this build; selecting NVC0");
        std::snprintf(requested, sizeof(requested), "nvc0");
    }
#endif
    setenv("MESA_SWITCH_GL_DRIVER", requested, 1);
    if (std::strcmp(requested, "zink") == 0) zink_pending_set();
    apply_dev_env();
    // Keep Mesa's shader cache inside our data folder instead of sdmc:/.mesa.
    setenv("MESA_SHADER_CACHE_DIR", "sdmc:/switch/pokenx/shader", 0);
    // Mesa's on-disk cache grows without a practical bound (default 1 GB). Keep it small on the SD card; `shader/`
    // only ever holds Mesa's own cache files (the game ships no shader files).
    setenv("MESA_SHADER_CACHE_MAX_SIZE", "64M", 0);
    char buffer[80];
    std::snprintf(buffer, sizeof(buffer), "Requested Mesa GL driver: %s", requested);
    log_line(buffer);
}

struct Egl {
    EGLDisplay display = EGL_NO_DISPLAY;
    EGLSurface surface = EGL_NO_SURFACE;
    EGLContext context = EGL_NO_CONTEXT;

    bool init() {
#ifdef POKENX_MESA26
        pokenx_mesa_bind_thread();
#endif
        log_line("EGL: eglGetDisplay/eglInitialize");
        display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
        if (display == EGL_NO_DISPLAY || eglInitialize(display, nullptr, nullptr) != EGL_TRUE) {
            log_fmt("eglInitialize failed: 0x%04x", eglGetError());
            return false;
        }
        EGLConfig config = nullptr;
        EGLint count = 0;
        const EGLint attributes[] = {EGL_SURFACE_TYPE, EGL_WINDOW_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_BIT,
                                     EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_ALPHA_SIZE, 8,
                                     EGL_DEPTH_SIZE, 24, EGL_STENCIL_SIZE, 8, EGL_NONE};
        if (eglBindAPI(EGL_OPENGL_API) != EGL_TRUE) {
            log_fmt("eglBindAPI failed: 0x%04x", eglGetError());
            return false;
        }
        log_line("EGL: eglChooseConfig");
        bool chosen = eglChooseConfig(display, attributes, &config, 1, &count) == EGL_TRUE && count > 0;
        if (!chosen) {
            // Some drivers offer no 24-bit depth plus 8-bit stencil together; retry with a plain depth buffer.
            const EGLint fallback[] = {EGL_SURFACE_TYPE, EGL_WINDOW_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_BIT,
                                       EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_ALPHA_SIZE, 8,
                                       EGL_DEPTH_SIZE, 16, EGL_NONE};
            log_line("eglChooseConfig with stencil failed; retrying without stencil");
            chosen = eglChooseConfig(display, fallback, &config, 1, &count) == EGL_TRUE && count > 0;
        }
        if (!chosen) {
            log_fmt("eglChooseConfig failed: 0x%04x", eglGetError());
            return false;
        }
        // Fixed 16:9 size chosen by select_resolution(), the same in handheld and docked mode.
        nwindowSetDimensions(nwindowGetDefault(), kRenderWidth, kRenderHeight);
        log_line("EGL: eglCreateWindowSurface");
        surface = eglCreateWindowSurface(display, config, nwindowGetDefault(), nullptr);
        if (surface == EGL_NO_SURFACE) {
            log_fmt("eglCreateWindowSurface failed: 0x%04x", eglGetError());
            return false;
        }
        // OpenGMK's renderer requires OpenGL 3.3 core.
        const EGLint context_attributes[] = {EGL_CONTEXT_MAJOR_VERSION_KHR, 3, EGL_CONTEXT_MINOR_VERSION_KHR, 3,
                                             EGL_CONTEXT_OPENGL_PROFILE_MASK_KHR,
                                             EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT_KHR, EGL_NONE};
        log_line("EGL: eglCreateContext 3.3 core");
        context = eglCreateContext(display, config, EGL_NO_CONTEXT, context_attributes);
        if (context == EGL_NO_CONTEXT || eglMakeCurrent(display, surface, surface, context) != EGL_TRUE) {
            log_fmt("eglCreateContext/eglMakeCurrent failed: 0x%04x", eglGetError());
            return false;
        }
        if (eglSwapInterval(display, 1) != EGL_TRUE) log_fmt("eglSwapInterval failed: 0x%04x", eglGetError());
        log_line("EGL: context ready");
        return true;
    }

    void shutdown() {
        if (display == EGL_NO_DISPLAY) return;
        eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
        if (context != EGL_NO_CONTEXT) eglDestroyContext(display, context);
        if (surface != EGL_NO_SURFACE) eglDestroySurface(display, surface);
        eglTerminate(display);
        display = EGL_NO_DISPLAY;
    }
};

Egl g_egl;
u64 g_successful_swaps = 0;
u64 g_swap_failures = 0;
u64 g_next_swap_ns = 0;

// FPS meter (Opciones > Medidor de FPS, fps.txt = on). The numbers come from fps_overlay.h; the pixels are scissored
// clears made right before eglSwapBuffers, so no program, texture or buffer of the game is touched. Only the framebuffer
// binding, scissor, clear colour and colour mask are changed, and they are put back.
struct OverlayGl {
    void (*enable)(unsigned) = nullptr;
    void (*disable)(unsigned) = nullptr;
    unsigned char (*is_enabled)(unsigned) = nullptr;
    void (*scissor)(int, int, int, int) = nullptr;
    void (*clear_color)(float, float, float, float) = nullptr;
    void (*clear)(unsigned) = nullptr;
    void (*get_integerv)(unsigned, int*) = nullptr;
    void (*get_floatv)(unsigned, float*) = nullptr;
    void (*get_booleanv)(unsigned, unsigned char*) = nullptr;
    void (*color_mask)(unsigned char, unsigned char, unsigned char, unsigned char) = nullptr;
    void (*bind_framebuffer)(unsigned, unsigned) = nullptr;
};
OverlayGl g_overlay_gl;
bool g_overlay_on = false;
fps_overlay::Meter g_overlay_meter;
std::vector<fps_overlay::Rect> g_overlay_rects;
fps_overlay::Rect g_overlay_box = {0, 0, 0, 0};
int g_overlay_scale = 3;

void overlay_prepare() {
    g_overlay_on = false;
    if (FILE* file = std::fopen("sdmc:/switch/pokenx/fps.txt", "r")) {
        char token[16] = {0};
        if (std::fscanf(file, "%15s", token) == 1) g_overlay_on = std::strcmp(token, "on") == 0;
        std::fclose(file);
    }
    if (!g_overlay_on) return;
    const auto load = [](const char* name) { return reinterpret_cast<void*>(eglGetProcAddress(name)); };
    g_overlay_gl.enable = reinterpret_cast<void (*)(unsigned)>(load("glEnable"));
    g_overlay_gl.disable = reinterpret_cast<void (*)(unsigned)>(load("glDisable"));
    g_overlay_gl.is_enabled = reinterpret_cast<unsigned char (*)(unsigned)>(load("glIsEnabled"));
    g_overlay_gl.scissor = reinterpret_cast<void (*)(int, int, int, int)>(load("glScissor"));
    g_overlay_gl.clear_color = reinterpret_cast<void (*)(float, float, float, float)>(load("glClearColor"));
    g_overlay_gl.clear = reinterpret_cast<void (*)(unsigned)>(load("glClear"));
    g_overlay_gl.get_integerv = reinterpret_cast<void (*)(unsigned, int*)>(load("glGetIntegerv"));
    g_overlay_gl.get_floatv = reinterpret_cast<void (*)(unsigned, float*)>(load("glGetFloatv"));
    g_overlay_gl.get_booleanv = reinterpret_cast<void (*)(unsigned, unsigned char*)>(load("glGetBooleanv"));
    g_overlay_gl.color_mask = reinterpret_cast<void (*)(unsigned char, unsigned char, unsigned char, unsigned char)>(load("glColorMask"));
    g_overlay_gl.bind_framebuffer = reinterpret_cast<void (*)(unsigned, unsigned)>(load("glBindFramebuffer"));
    const void* const required[] = {reinterpret_cast<void*>(g_overlay_gl.enable), reinterpret_cast<void*>(g_overlay_gl.disable),
                                    reinterpret_cast<void*>(g_overlay_gl.is_enabled), reinterpret_cast<void*>(g_overlay_gl.scissor),
                                    reinterpret_cast<void*>(g_overlay_gl.clear_color), reinterpret_cast<void*>(g_overlay_gl.clear),
                                    reinterpret_cast<void*>(g_overlay_gl.get_integerv), reinterpret_cast<void*>(g_overlay_gl.get_floatv),
                                    reinterpret_cast<void*>(g_overlay_gl.get_booleanv), reinterpret_cast<void*>(g_overlay_gl.color_mask),
                                    reinterpret_cast<void*>(g_overlay_gl.bind_framebuffer)};
    for (const void* entry : required) {
        if (!entry) {
            g_overlay_on = false;
            log_line("FPS meter: a GL entry point is missing; the meter stays off");
            return;
        }
    }
    log_line("FPS meter: on");
}

void overlay_paint(int window_w, int window_h) {
    constexpr unsigned kScissorTest = 0x0C11, kScissorBox = 0x0C10, kClearColor = 0x0C22, kColorMask = 0x0C23, kFramebufferBinding = 0x8CA6,
                       kFramebuffer = 0x8D40, kColorBufferBit = 0x4000;
    (void)window_w;
    int framebuffer = 0, box[4] = {0, 0, 0, 0};
    float color[4] = {0, 0, 0, 0};
    unsigned char mask[4] = {1, 1, 1, 1};
    g_overlay_gl.get_integerv(kFramebufferBinding, &framebuffer);
    g_overlay_gl.get_integerv(kScissorBox, box);
    g_overlay_gl.get_floatv(kClearColor, color);
    g_overlay_gl.get_booleanv(kColorMask, mask);
    const unsigned char scissor_was_on = g_overlay_gl.is_enabled(kScissorTest);
    if (framebuffer != 0) g_overlay_gl.bind_framebuffer(kFramebuffer, 0);
    g_overlay_gl.color_mask(1, 1, 1, 1);
    g_overlay_gl.enable(kScissorTest);
    const auto fill = [&](const fps_overlay::Rect& rect, float r, float g, float b) {
        g_overlay_gl.scissor(rect.x, window_h - rect.y - rect.h, rect.w, rect.h);
        g_overlay_gl.clear_color(r, g, b, 1.0f);
        g_overlay_gl.clear(kColorBufferBit);
    };
    fill(g_overlay_box, 0.0f, 0.0f, 0.0f);
    const int quality = g_overlay_meter.quality();
    for (const fps_overlay::Rect& rect : g_overlay_rects)
        fill(rect, quality == 0 ? 0.35f : 1.0f, quality == 0 ? 1.0f : (quality == 1 ? 0.85f : 0.3f), quality == 0 ? 0.45f : 0.2f);
    if (!scissor_was_on) g_overlay_gl.disable(kScissorTest);
    g_overlay_gl.scissor(box[0], box[1], box[2], box[3]);
    g_overlay_gl.clear_color(color[0], color[1], color[2], color[3]);
    g_overlay_gl.color_mask(mask[0], mask[1], mask[2], mask[3]);
    if (framebuffer != 0) g_overlay_gl.bind_framebuffer(kFramebuffer, static_cast<unsigned>(framebuffer));
}

void overlay_frame(u64 now) {
    EGLint width = 0, height = 0;
    eglQuerySurface(g_egl.display, g_egl.surface, EGL_WIDTH, &width);
    eglQuerySurface(g_egl.display, g_egl.surface, EGL_HEIGHT, &height);
    if (width <= 0 || height <= 0) return;
    if (g_overlay_meter.on_frame(now, pokenx_engine_paused_ns()) || g_overlay_rects.empty()) {
        g_overlay_scale = std::max(2, height / 240);
        const std::string text = g_overlay_meter.text();
        int text_w = 0, text_h = 0;
        fps_overlay::measure(text, g_overlay_scale, text_w, text_h);
        const int margin = g_overlay_scale * 3, pad = g_overlay_scale * 2;
        g_overlay_box = {margin - pad, margin - pad, text_w + pad * 2, text_h + pad * 2};
        g_overlay_rects = fps_overlay::build_rects(text, margin, margin, g_overlay_scale);
    }
    overlay_paint(width, height);
}

// Host services handed to the Rust side.
const void* host_get_proc(const uint8_t* name) {
    return reinterpret_cast<const void*>(eglGetProcAddress(reinterpret_cast<const char*>(name)));
}
void host_log(const uint8_t* line) { log_line(reinterpret_cast<const char*>(line)); }
void host_swap_buffers() {
    // GML screen_refresh can run inside a dialog loop. Apply the same cadence
    // there, rather than allowing those swaps to bypass the outer 60 Hz loop.
    u64 now = armTicksToNs(armGetSystemTick());
    if (g_next_swap_ns > now) svcSleepThread(static_cast<s64>(g_next_swap_ns - now));
    const u64 slot_start = armTicksToNs(armGetSystemTick());
    g_next_swap_ns = slot_start + kNanosPerSecond / pokenx_engine_presentation_hz();
    if (g_overlay_on) overlay_frame(slot_start);
    // The first presentations are written down one by one: with Zink a swap that never returns (no error, no further log)
    // looks exactly like a black screen, and the log must say it was this call.
    static unsigned swap_number = 0;
    const bool traced = swap_number < 3;
    ++swap_number;
    if (traced) log_fmt("mark: eglSwapBuffers #%u begins", swap_number);
    const bool swapped = eglSwapBuffers(g_egl.display, g_egl.surface) == EGL_TRUE;
    g_progress.fetch_add(1, std::memory_order_relaxed);
    if (traced) {
        char text[96];
        std::snprintf(text, sizeof(text), "mark: eglSwapBuffers #%u %s in %llu ms", swap_number, swapped ? "done" : "FAILED",
                      static_cast<unsigned long long>((armTicksToNs(armGetSystemTick()) - slot_start) / 1000000ULL));
        log_line(text);
    }
    if (swapped) {
        if (++g_successful_swaps == 1) {
            log_line("mark: first game frame presented");
            zink_pending_clear();
        }
    } else if (++g_swap_failures == 1) {
        log_fmt("eglSwapBuffers failed: 0x%04x", eglGetError());
    }
}
int32_t host_set_swap_interval(int32_t interval) { return eglSwapInterval(g_egl.display, interval) == EGL_TRUE; }
bool g_time_ready = false;
int64_t host_unix_seconds() {
    if (!g_time_ready) return -1;
    u64 now = 0;
    return R_SUCCEEDED(timeGetCurrentTime(TimeType_UserSystemClock, &now)) ? static_cast<int64_t>(now) : -1;
}

// Audio: a ring of buffers refilled by a dedicated thread (the game frame can take longer than a
// buffer lasts, so refilling from the render loop would underrun).
struct Audio {
    AudioOutBuffer buffers[kAudioBuffers];
    void* memory[kAudioBuffers] = {};
    Thread thread = {};
    std::atomic<bool> stop{false};
    std::atomic<bool> paused{false};
    bool started = false;
    bool thread_started = false;

    static void thread_main(void* arg) {
        Audio* audio = static_cast<Audio*>(arg);
        while (!audio->stop.load(std::memory_order_acquire)) {
            AudioOutBuffer* released = nullptr;
            u32 count = 0;
            if (R_FAILED(audoutWaitPlayFinish(&released, &count, 100000000ULL)) || count == 0 || !released) continue;
            if (audio->paused.load(std::memory_order_acquire)) {
                std::memset(released->buffer, 0, kAudioBytes);
            } else {
                pokenx_engine_audio(static_cast<int16_t*>(released->buffer), kAudioFrames);
            }
            released->data_size = kAudioBytes;
            audoutAppendAudioOutBuffer(released);
        }
    }

    bool init() {
        const Result init_rc = audoutInitialize();
        if (R_FAILED(init_rc)) {
            log_fmt("audoutInitialize failed: 0x%x", init_rc);
            return false;
        }
        const Result start_rc = audoutStartAudioOut();
        if (R_FAILED(start_rc)) {
            log_fmt("audoutStartAudioOut failed: 0x%x", start_rc);
            audoutExit();
            return false;
        }
        started = true;
        for (int i = 0; i < kAudioBuffers; ++i) {
            memory[i] = aligned_alloc(0x1000, kAudioAligned);
            if (!memory[i]) {
                log_line("audio: aligned_alloc failed");
                return false;
            }
            std::memset(memory[i], 0, kAudioAligned);
            std::memset(&buffers[i], 0, sizeof(buffers[i]));
            buffers[i].buffer = memory[i];
            buffers[i].buffer_size = kAudioAligned;
            buffers[i].data_size = kAudioBytes;
            buffers[i].data_offset = 0;
            audoutAppendAudioOutBuffer(&buffers[i]);
        }
        // Priority 0x2B on core 1, as the Switch ports in  do.
        if (R_SUCCEEDED(threadCreate(&thread, thread_main, this, nullptr, 0x10000, 0x2B, 1)) &&
            R_SUCCEEDED(threadStart(&thread))) {
            thread_started = true;
        } else {
            log_line("audio thread could not start");
            threadClose(&thread);
            return false;
        }
        return true;
    }

    void shutdown() {
        stop.store(true, std::memory_order_release);
        if (thread_started) {
            threadWaitForExit(&thread);
            threadClose(&thread);
            thread_started = false;
        }
        if (started) {
            audoutStopAudioOut();
            audoutExit();
            started = false;
        }
        for (void*& block : memory) {
            std::free(block);
            block = nullptr;
        }
    }
};

u32 map_buttons(u64 held) {
    struct Pair { u64 hid; u32 bit; };
    static const Pair table[] = {
        {HidNpadButton_A, POKENX_BUTTON_A},         {HidNpadButton_B, POKENX_BUTTON_B},
        {HidNpadButton_X, POKENX_BUTTON_X},         {HidNpadButton_Y, POKENX_BUTTON_Y},
        {HidNpadButton_L, POKENX_BUTTON_L},         {HidNpadButton_R, POKENX_BUTTON_R},
        {HidNpadButton_ZL, POKENX_BUTTON_ZL},       {HidNpadButton_ZR, POKENX_BUTTON_ZR},
        {HidNpadButton_Plus, POKENX_BUTTON_PLUS},   {HidNpadButton_Minus, POKENX_BUTTON_MINUS},
        {HidNpadButton_Left, POKENX_BUTTON_LEFT},   {HidNpadButton_Up, POKENX_BUTTON_UP},
        {HidNpadButton_Right, POKENX_BUTTON_RIGHT}, {HidNpadButton_Down, POKENX_BUTTON_DOWN},
        {HidNpadButton_StickL, POKENX_BUTTON_STICK_L}, {HidNpadButton_StickR, POKENX_BUTTON_STICK_R},
    };
    u32 out = 0;
    for (const Pair& pair : table) {
        if (held & pair.hid) out |= pair.bit;
    }
    return out;
}

// A single Joy-Con held sideways has no L/R/ZL/ZR/second stick. The layout (face buttons rotated
// 90 degrees, SL/SR as shoulders, stick click and Plus/Minus as start/select) is written from the
// behaviour described by SDL's Switch joystick driver (zlib licence), not copied from it.
u32 map_single_joycon(u64 held, bool left_joycon) {
    u32 out = 0;
    auto set = [&](u64 hid, u32 bit) { if (held & hid) out |= bit; };
    if (left_joycon) {
        set(HidNpadButton_Down, POKENX_BUTTON_A);   set(HidNpadButton_Left, POKENX_BUTTON_B);
        set(HidNpadButton_Right, POKENX_BUTTON_X);  set(HidNpadButton_Up, POKENX_BUTTON_Y);
        set(HidNpadButton_LeftSL, POKENX_BUTTON_ZL); set(HidNpadButton_LeftSR, POKENX_BUTTON_ZR);
        set(HidNpadButton_StickL, POKENX_BUTTON_PLUS); set(HidNpadButton_Minus, POKENX_BUTTON_MINUS);
    } else {
        set(HidNpadButton_X, POKENX_BUTTON_A);      set(HidNpadButton_A, POKENX_BUTTON_B);
        set(HidNpadButton_Y, POKENX_BUTTON_X);      set(HidNpadButton_B, POKENX_BUTTON_Y);
        set(HidNpadButton_RightSL, POKENX_BUTTON_ZL); set(HidNpadButton_RightSR, POKENX_BUTTON_ZR);
        set(HidNpadButton_StickR, POKENX_BUTTON_PLUS); set(HidNpadButton_Plus, POKENX_BUTTON_MINUS);
    }
    return out;
}

// Watchdog: a second thread that wakes every second and, when nothing has moved for a while (no log line, no presented
// frame, no pad poll), writes where the program is stuck. The main thread cannot do it: a driver stall (Zink/NVK waiting on
// a fence, a swap that never returns) leaves it inside one call, and the log lines written just before were still in the
// stdio buffer because the flush is throttled, so a force-closed session used to end with a log that stops at an earlier
// line with no hint. It writes to engine.log and flushes at once. Reports come at 10 s, 30 s, 1, 2, 3, 5, 10 and 15 min.
std::atomic<bool> g_watchdog_stop{false};
std::atomic<bool> g_watchdog_hold{false};  // the system took the focus away: the game is paused on purpose

struct Watchdog {
    Thread thread = {};
    bool started = false;

    static void run(void*) {
        static const unsigned kReportAfterSeconds[] = {10, 30, 60, 120, 180, 300, 600, 900};
        u64 seen = g_progress.load(std::memory_order_relaxed);
        unsigned quiet = 0, reports = 0;
        while (!g_watchdog_stop.load(std::memory_order_acquire)) {
            svcSleepThread(1'000'000'000LL);
            const u64 progress = g_progress.load(std::memory_order_relaxed);
            if (g_watchdog_hold.load(std::memory_order_relaxed) || progress != seen) {
                seen = progress;
                quiet = 0;
                reports = 0;
                continue;
            }
            ++quiet;
            if (reports >= sizeof(kReportAfterSeconds) / sizeof(kReportAfterSeconds[0]) || quiet < kReportAfterSeconds[reports]) continue;
            ++reports;
            char last[sizeof(g_last_line)];
            u64 last_ms = 0;
            mutexLock(&g_last_line_mutex);
            std::memcpy(last, g_last_line, sizeof(last));
            last_ms = g_last_line_ms;
            mutexUnlock(&g_last_line_mutex);
            last[sizeof(last) - 1] = 0;
            const u64 ms = armTicksToNs(armGetSystemTick() - g_log_start_tick) / 1000000ULL;
            if (g_log) {
                std::fprintf(g_log, "[%7llu] mark: WATCHDOG: no progress for %u s; %llu frame(s) presented so far; last checkpoint at %llu ms: %s\n",
                             static_cast<unsigned long long>(ms), quiet, static_cast<unsigned long long>(g_successful_swaps),
                             static_cast<unsigned long long>(last_ms), last[0] ? last : "(none yet)");
                std::fflush(g_log);
            }
        }
    }

    void start() {
        // Lowest priority, any core: it only sleeps, and must not take time from the game or the audio thread.
        if (R_SUCCEEDED(threadCreate(&thread, run, nullptr, nullptr, 0x4000, 0x3B, -2)) && R_SUCCEEDED(threadStart(&thread))) {
            started = true;
        } else {
            threadClose(&thread);
            log_line("watchdog thread could not start; a stall will not be reported");
        }
    }

    void stop() {
        g_watchdog_stop.store(true, std::memory_order_release);
        if (started) {
            threadWaitForExit(&thread);
            threadClose(&thread);
            started = false;
        }
    }
};

Audio* g_audio = nullptr;
PadState g_pad;
bool g_pad_ready = false;
bool g_clock_discontinuity = false;

// Polling is also called from GML io_handle/keyboard_wait, so dialogs keep
// receiving HID, lifecycle and exit requests while inside Game::start/step.
int32_t poll_pad_snapshot(PokenxPad* output) {
    if (!output || !g_pad_ready) return -1;
    g_progress.fetch_add(1, std::memory_order_relaxed);  // GML dialogs poll here while they wait: that is not a stall
    if (g_log) {  // lines held back by the throttle in log_line reach the card within a second
        static u64 last_flush_ns = 0;
        const u64 now = armTicksToNs(armGetSystemTick());
        if (now - last_flush_ns >= 1000000000ULL) {
            std::fflush(g_log);
            last_flush_ns = now;
        }
    }
    if (g_swap_failures) {
        static bool reported = false;
        if (!reported) {
            reported = true;
            log_fmt("Closing: %u failed buffer swap(s) (eglSwapBuffers)", static_cast<unsigned>(g_swap_failures));
        }
        return 0;
    }
    u32 waited_ms = 0;
    while (true) {
        if (!appletMainLoop()) return 0;
        const AppletFocusState focus = appletGetFocusState();
        if (focus == AppletFocusState_InFocus) break;
        // A silent wait here looks exactly like a black screen: say so at 1 s, then every 10 s.
        if (waited_ms == 1000 || (waited_ms > 1000 && waited_ms % 10000 == 0)) {
            char text[96];
            std::snprintf(text, sizeof(text), "Waiting for focus: state %d for %u ms (the game is paused)", static_cast<int>(focus), waited_ms);
            log_line(text);
        }
        waited_ms += 50;
        g_watchdog_hold.store(true, std::memory_order_relaxed);
        g_clock_discontinuity = true;
        g_next_swap_ns = 0;
        if (g_audio) g_audio->paused.store(true, std::memory_order_release);
        svcSleepThread(50'000'000LL);
    }
    if (g_audio) g_audio->paused.store(false, std::memory_order_release);
    g_watchdog_hold.store(false, std::memory_order_relaxed);
    PadState& pad = g_pad;
        padUpdate(&pad);
        const u64 held = padGetButtons(&pad);
        PokenxPad snapshot = {};
        snapshot.flags = (padIsConnected(&pad) || padIsHandheld(&pad)) ? POKENX_PAD_CONNECTED : 0;
        const u32 style = padGetStyleSet(&pad);
        const bool single_left = (style & HidNpadStyleTag_NpadJoyLeft) != 0;
        const bool single_right = (style & HidNpadStyleTag_NpadJoyRight) != 0;
        const HidAnalogStickState left = padGetStickPos(&pad, 0);
        const HidAnalogStickState right = padGetStickPos(&pad, 1);
        if (single_left || single_right) {
            snapshot.buttons = map_single_joycon(held, single_left);
            // A lone Joy-Con is held sideways: rotate its stick into the game's frame.
            const HidAnalogStickState s = single_left ? left : right;
            snapshot.left_x = single_left ? -s.y : s.y;
            snapshot.left_y = single_left ? s.x : -s.x;
        } else {
            snapshot.buttons = map_buttons(held);
            snapshot.left_x = left.x;
            snapshot.left_y = left.y;
            snapshot.right_x = right.x;
            snapshot.right_y = right.y;
        }

        // Exit combo: L + R + Plus. HOME and the applet exit request also end the loop.
        const bool exit_full = (held & HidNpadButton_L) && (held & HidNpadButton_R) && (held & HidNpadButton_Plus);
        const bool exit_single = (single_left || single_right) &&
            (snapshot.buttons & POKENX_BUTTON_ZL) && (snapshot.buttons & POKENX_BUTTON_ZR) &&
            (snapshot.buttons & (POKENX_BUTTON_PLUS | POKENX_BUTTON_MINUS));
        if (exit_full || exit_single) return 0;

    *output = snapshot;
    return 1;
}

u64 now_ns() { return armTicksToNs(armGetSystemTick()); }

// Why the last session ended; shown by the menu when it comes back.
std::string g_status;
void write_engine_status(const char* text) { g_status = text; }

// While the GM8 data is parsed (tens of thousands of assets, no event pumping inside the Rust parser) nothing else
// would read the pad or paint the screen, which is exactly a black screen that ignores +. Called from log_line at most
// every 100 ms: handles HOME/sleep, lets + (or L+R+Plus) cancel the load, and shows a progress bar.
void loading_pump() {
    static u64 last_ns = 0;
    static void (*clear_color)(float, float, float, float) = nullptr;
    static void (*clear)(unsigned) = nullptr;
    static void (*enable)(unsigned) = nullptr;
    static void (*disable)(unsigned) = nullptr;
    static void (*scissor)(int, int, int, int) = nullptr;
    static void (*viewport)(int, int, int, int) = nullptr;
    static bool resolved = false;
    if (!g_pad_ready) return;
    const u64 now = armTicksToNs(armGetSystemTick());
    if (now - last_ns < 100000000ULL) return;
    last_ns = now;
    if (!appletMainLoop()) {
        g_loading = false;
        g_watchdog_stop.store(true, std::memory_order_release);
        zink_pending_clear();
        log_line("Load cancelled: the system asked the application to close");
        write_engine_status("La carga se cancelo: el sistema pidio cerrar la aplicacion.");
        std::exit(0);
    }
    padUpdate(&g_pad);
    const u64 held = padGetButtons(&g_pad);
    if ((padGetButtonsDown(&g_pad) & HidNpadButton_Plus) || ((held & HidNpadButton_L) && (held & HidNpadButton_R) && (held & HidNpadButton_Plus))) {
        g_loading = false;
        g_watchdog_stop.store(true, std::memory_order_release);
        zink_pending_clear();
        log_line("Load cancelled by the user with +");
        write_engine_status("Carga del juego cancelada con +.");
        std::exit(0);
    }
    if (appletGetFocusState() != AppletFocusState_InFocus) return;
    if (!resolved) {
        resolved = true;
        clear_color = reinterpret_cast<decltype(clear_color)>(eglGetProcAddress("glClearColor"));
        clear = reinterpret_cast<decltype(clear)>(eglGetProcAddress("glClear"));
        enable = reinterpret_cast<decltype(enable)>(eglGetProcAddress("glEnable"));
        disable = reinterpret_cast<decltype(disable)>(eglGetProcAddress("glDisable"));
        scissor = reinterpret_cast<decltype(scissor)>(eglGetProcAddress("glScissor"));
        viewport = reinterpret_cast<decltype(viewport)>(eglGetProcAddress("glViewport"));
    }
    if (!clear_color || !clear || !enable || !disable || !scissor || !viewport) return;
    const int w = static_cast<int>(kRenderWidth), h = static_cast<int>(kRenderHeight);
    float progress = static_cast<float>(g_log_lines) / 22000.0f;
    if (progress > 0.98f) progress = 0.98f;
    viewport(0, 0, w, h);
    clear_color(0.05f, 0.07f, 0.16f, 1.0f);
    clear(0x4000);  // GL_COLOR_BUFFER_BIT
    enable(0x0C11);  // GL_SCISSOR_TEST
    scissor(w / 8, h / 2 - 12, w * 3 / 4, 24);
    clear_color(0.18f, 0.20f, 0.30f, 1.0f);
    clear(0x4000);
    scissor(w / 8, h / 2 - 12, static_cast<int>(w * 3 / 4 * progress), 24);
    clear_color(0.95f, 0.78f, 0.15f, 1.0f);
    clear(0x4000);
    disable(0x0C11);
    eglSwapBuffers(g_egl.display, g_egl.surface);
}

// ---- Crash report -------------------------------------------------------------------------------------------
// An unhandled CPU exception (bad access, undefined instruction, misaligned PC...) used to close the program with
// no trace. libnx calls __libnx_exception_handler on its own stack; it writes the registers to logs/crash.log through
// a file descriptor opened at startup (no stdio locks, no malloc) so the address can be mapped to a symbol in
// pokenx-engine.elf: offset = pc - module_base. Atmosphere also writes its own report in atmosphere/crash_reports/.
int g_crash_fd = -1;

void crash_write(const char* text) {
    if (g_crash_fd >= 0) (void)!write(g_crash_fd, text, std::strlen(text));
}

void crash_hex(const char* label, u64 value) {
    char line[96];
    std::snprintf(line, sizeof(line), "%s = 0x%016llx\n", label, static_cast<unsigned long long>(value));
    crash_write(line);
}

}  // namespace

// ---- Boot markers -------------------------------------------------------------------------------------------
// logs/boot.log records how far the process got: libnx finished its own initialisation (userAppInit), the static
// constructors ran, and main() started. If engine.log is missing after "Abrir juego", the last marker in boot.log
// says whether the process died while libnx/the loader was setting it up, in a constructor, or inside main().
static void boot_mark(const char* text) {
    int fd = open("sdmc:/switch/pokenx/logs/boot.log", O_WRONLY | O_CREAT | O_APPEND, 0666);
    if (fd < 0) return;
    (void)!write(fd, text, std::strlen(text));
    close(fd);
}

// libnx's default heap takes ALL free process memory minus 2 MiB (init.c __libnx_initheap). Mesa then starts threads,
// opens service sessions and maps GPU memory, which also need process memory outside the newlib heap; with 2 MiB left
// those requests can fail when EGL starts. The nova3-nx ports (nova3-nx/runtime/source/
// nx_init.c, RT_GFX_RESERVE_MB = 16 in all of them) hold memory back for the graphics driver; we hold back more because
// Mesa 26 with Zink/NVK is larger. A heap override from the loader (envHasHeapOverride) is respected as is.
constexpr u64 kGraphicsReserve = 32ULL << 20;

extern "C" void __libnx_initheap(void) {
    extern char* fake_heap_start;
    extern char* fake_heap_end;
    void* addr = nullptr;
    u64 size = 0;
    if (envHasHeapOverride()) {
        addr = envGetHeapOverrideAddr();
        size = envGetHeapOverrideSize();
    } else {
        u64 total = 0, used = 0;
        svcGetInfo(&total, InfoType_TotalMemorySize, CUR_PROCESS_HANDLE, 0);
        svcGetInfo(&used, InfoType_UsedMemorySize, CUR_PROCESS_HANDLE, 0);
        u64 want = total > used + 0x200000 ? total - used - 0x200000 : 0;
        want = want > kGraphicsReserve + (64ULL << 20) ? want - kGraphicsReserve : want;  // keep the reserve only when it fits
        want &= ~0x1FFFFFULL;
        if (want == 0) want = 0x2000000ULL * 16;
        // Retry smaller instead of dying before any log exists.
        Result rc = svcSetHeapSize(&addr, want);
        size = want;
        for (u64 next = want >> 1; R_FAILED(rc) && next >= (32ULL << 20); next >>= 1) {
            size = next & ~0x1FFFFFULL;
            rc = svcSetHeapSize(&addr, size);
        }
        if (R_FAILED(rc)) diagAbortWithResult(MAKERESULT(Module_Libnx, LibnxError_HeapAllocFailed));
    }
    fake_heap_start = static_cast<char*>(addr);
    fake_heap_end = static_cast<char*>(addr) + size;
}

extern "C" void userAppInit(void) {
    mkdir("sdmc:/switch", 0777);
    mkdir("sdmc:/switch/pokenx", 0777);
    mkdir("sdmc:/switch/pokenx/logs", 0777);
    std::remove("sdmc:/switch/pokenx/logs/boot.log");
    u64 total = 0, used = 0;
    svcGetInfo(&total, InfoType_TotalMemorySize, CUR_PROCESS_HANDLE, 0);
    svcGetInfo(&used, InfoType_UsedMemorySize, CUR_PROCESS_HANDLE, 0);
    extern char* fake_heap_start;
    extern char* fake_heap_end;
    char line[200];
    std::snprintf(line, sizeof(line),
                  "1. libnx initialised (userAppInit). memory total %llu MiB, used %llu MiB, newlib heap %llu MiB (reserve %llu MiB)\n",
                  static_cast<unsigned long long>(total >> 20), static_cast<unsigned long long>(used >> 20),
                  static_cast<unsigned long long>((fake_heap_end - fake_heap_start) >> 20),
                  static_cast<unsigned long long>(kGraphicsReserve >> 20));
    boot_mark(line);
}

__attribute__((constructor)) static void boot_constructors_ran() { boot_mark("2. static constructors ran\n"); }

extern "C" {
// Bigger than libnx's default 1 KiB so snprintf and the register dump fit.
alignas(16) u8 __nx_exception_stack[0x4000];
u64 __nx_exception_stack_size = sizeof(__nx_exception_stack);

void __libnx_exception_handler(ThreadExceptionDump* ctx) {
    crash_write("=== PokeNX engine: unhandled CPU exception ===\n");
    crash_hex("error_desc (0x100 instruction abort, 0x101 other, 0x102 misaligned PC, 0x103 misaligned SP, 0x104 trap)",
              ctx->error_desc);
    crash_hex("pc", ctx->pc.x);
    crash_hex("lr", ctx->lr.x);
    crash_hex("sp", ctx->sp.x);
    crash_hex("fp", ctx->fp.x);
    crash_hex("far (faulting address)", ctx->far.x);
    crash_hex("esr", ctx->esr);
    crash_hex("pstate", ctx->pstate);
    MemoryInfo info = {};
    u32 page = 0;
    if (R_SUCCEEDED(svcQueryMemory(&info, &page, reinterpret_cast<u64>(&crash_write)))) {
        crash_hex("module_text_base (pc - this = offset in the ELF)", info.addr);
    }
    crash_hex("&crash_write", reinterpret_cast<u64>(&crash_write));
    for (int i = 0; i < 29; ++i) {
        char label[16];
        std::snprintf(label, sizeof(label), "x%d", i);
        crash_hex(label, ctx->cpu_gprs[i].x);
    }
    // A short stack dump helps when lr is not enough; the stack belongs to the faulting thread.
    crash_write("stack words at sp:\n");
    // Only when sp looks like a valid, aligned address: reading a bad sp here would fault inside the handler itself.
    if (ctx->sp.x > 0x100000 && (ctx->sp.x & 7) == 0) {
        const u64* stack = reinterpret_cast<const u64*>(ctx->sp.x);
        for (int i = 0; i < 24; ++i) crash_hex("  [sp+..]", stack[i]);
    } else {
        crash_write("  (sp is not a valid aligned address; stack not dumped)\n");
    }
    crash_write("=== end of crash report ===\n");
    if (g_crash_fd >= 0) fsync(g_crash_fd);
}
}  // extern "C"

// Static host service used by the Rust text bridge. Input strings are UTF-8,
// output includes NUL; 1 confirmed, 0 dismissed, -1 unavailable/error.
// This is a system applet, not an overlay of virtual gameplay controls.
extern "C" int32_t pokenx_host_text_input(const char* prompt, const char* initial,
                                          uint8_t* output, size_t capacity) {
    if (!prompt || !initial || !output || capacity < 1025) return -1;
    output[0] = 0;
    SwkbdConfig keyboard = {};
    Result rc = swkbdCreate(&keyboard, 0);
    if (R_FAILED(rc)) {
        swkbdClose(&keyboard);
        log_fmt("System keyboard creation failed: 0x%x", rc);
        return -1;
    }
    swkbdConfigMakePresetDefault(&keyboard);
    swkbdConfigSetHeaderText(&keyboard, "PokeNX");
    swkbdConfigSetGuideText(&keyboard, prompt);
    swkbdConfigSetInitialText(&keyboard, initial);
    swkbdConfigSetStringLenMax(&keyboard, 240);
    swkbdConfigSetStringLenMin(&keyboard, 0);
    swkbdConfigSetReturnButtonFlag(&keyboard, 0);
    const bool was_paused = g_audio && g_audio->paused.exchange(true, std::memory_order_acq_rel);
    rc = swkbdShow(&keyboard, reinterpret_cast<char*>(output), capacity);
    swkbdClose(&keyboard);
    if (g_audio) g_audio->paused.store(was_paused, std::memory_order_release);
    g_clock_discontinuity = true;
    g_next_swap_ns = 0;
    if (g_egl.display != EGL_NO_DISPLAY &&
        eglMakeCurrent(g_egl.display, g_egl.surface, g_egl.surface, g_egl.context) != EGL_TRUE) {
        ++g_swap_failures;
        log_line("EGL context restore after keyboard failed");
        return -1;
    }
    if (R_SUCCEEDED(rc)) return 1;
    if (rc == MAKERESULT(Module_Libnx, LibnxError_LibAppletBadExit)) return 0;
    log_fmt("System keyboard failed: 0x%x", rc);
    return -1;
}

// Standard output of the Rust side (pokenx-std println!/eprintln!) lands in the SD log.
extern "C" void pokenx_host_log(const char* line) { log_line(line); }

// Randomness for the Rust side (getrandom custom backend).
extern "C" void pokenx_host_random(unsigned char* buffer, size_t length) { randomGet(buffer, length); }

// ---- Clocks ---------------------------------------------------------------------------------------------------
// A homebrew starts at the handheld defaults (CPU 1020 MHz, GPU 307 MHz). The game thread, the GL calls and Mesa's
// driver all run on one CPU core, so the clocks matter. Rates mirror halo-ce-universal
// (halo-ce-universal/port/switch/host/host_sdl2.c:300-345, CC0): CPU 1785 MHz, EMC 1600 MHz,
// GPU 768 MHz docked / 460.8 MHz handheld; the rates found are put back when the session ends. Atmosphere on
// firmware >= 8 exposes this through clkrst. `performance.txt` containing "stock" skips it. Not measured here.
struct ClockState {
    bool ready = false;
    u32 cpu = 0, gpu = 0, emc = 0;
};
ClockState g_clocks;

u32 clock_get(PcvModuleId module) {
    ClkrstSession session;
    u32 hz = 0;
    if (R_FAILED(clkrstOpenSession(&session, module, 3))) return 0;
    clkrstGetClockRate(&session, &hz);
    clkrstCloseSession(&session);
    return hz;
}

void clock_set(PcvModuleId module, u32 hz) {
    ClkrstSession session;
    if (!hz || R_FAILED(clkrstOpenSession(&session, module, 3))) return;
    clkrstSetClockRate(&session, hz);
    clkrstCloseSession(&session);
}

void clocks_raise() {
    char mode[16] = "max";
    if (FILE* config = std::fopen("sdmc:/switch/pokenx/performance.txt", "r")) {
        if (std::fscanf(config, "%15s", mode) != 1) std::snprintf(mode, sizeof(mode), "max");
        std::fclose(config);
    }
    if (std::strcmp(mode, "stock") == 0) {
        log_line("Clocks: stock (performance.txt)");
        return;
    }
    const bool balanced = std::strcmp(mode, "balanced") == 0;
    if (!hosversionAtLeast(8, 0, 0) || R_FAILED(clkrstInitialize())) {
        log_line("Clocks: clkrst unavailable; staying at the console defaults");
        return;
    }
    g_clocks.cpu = clock_get(PcvModuleId_CpuBus);
    g_clocks.gpu = clock_get(PcvModuleId_GPU);
    g_clocks.emc = clock_get(PcvModuleId_EMC);
    const bool docked = appletGetOperationMode() == AppletOperationMode_Console;
    // max: CPU 1785 / GPU 768 docked, 460.8 handheld. balanced: CPU 1224 / GPU 614.4 docked, 384 handheld (less heat and
    // battery use; the game's own load is mostly the CPU interpreter, so max is the default).
    clock_set(PcvModuleId_CpuBus, balanced ? 1224000000u : 1785000000u);
    clock_set(PcvModuleId_EMC, 1600000000u);
    clock_set(PcvModuleId_GPU, docked ? (balanced ? 614400000u : 768000000u) : (balanced ? 384000000u : 460800000u));
    g_clocks.ready = true;
    char text[160];
    std::snprintf(text, sizeof(text), "Clocks: before cpu %u gpu %u emc %u Hz; now cpu %u gpu %u emc %u Hz (%s)", g_clocks.cpu,
                  g_clocks.gpu, g_clocks.emc, clock_get(PcvModuleId_CpuBus), clock_get(PcvModuleId_GPU),
                  clock_get(PcvModuleId_EMC), docked ? "docked" : "handheld");
    log_line(text);
}

void clocks_restore() {
    if (!g_clocks.ready) return;
    clock_set(PcvModuleId_CpuBus, g_clocks.cpu);
    clock_set(PcvModuleId_GPU, g_clocks.gpu);
    clock_set(PcvModuleId_EMC, g_clocks.emc);
    clkrstExit();
    g_clocks.ready = false;
    log_line("Clocks restored");
}

int main(int, char**) {
    MenuInfo info;
    boot_mark("3. main() started\n");
    // libnx's __appInit already mounted sdmc: before userAppInit (init.c); mounting again fails with a false error and
    // leaks a filesystem handle. Check that the mount works instead.
    struct stat sd_probe;
    const Result mount_rc = stat("sdmc:/", &sd_probe) == 0 ? 0 : MAKERESULT(Module_Libnx, LibnxError_NotInitialized);
    mkdir("sdmc:/switch", 0777);
    mkdir(kDataRoot, 0777);
    mkdir("sdmc:/switch/pokenx/logs", 0777);
    mkdir("sdmc:/switch/pokenx/shader", 0777);
    mkdir("sdmc:/switch/pokenx/cache", 0777);
    // Keep the previous run's log next to the new one (a crash is usually read after the next start).
    std::remove("sdmc:/switch/pokenx/logs/engine.log.1");
    std::rename(kLogPath, "sdmc:/switch/pokenx/logs/engine.log.1");
    // "a" (append) on purpose: the file is new after the rotation above, and Horizon only lets a file grow when it
    // was opened in append mode (see app/src/sd_file.rs).
    g_log = std::fopen(kLogPath, "a");
    std::remove("sdmc:/switch/pokenx/logs/crash.log");
    g_crash_fd = open("sdmc:/switch/pokenx/logs/crash.log", O_WRONLY | O_CREAT | O_APPEND, 0666);
    log_line("PokeNX engine payload starting");
    if (g_crash_fd < 0) log_line("crash.log could not be opened; an exception will leave no report");
    if (R_FAILED(mount_rc)) log_fmt("fsdevMountSdmc failed: 0x%x (no game and no log without the SD card)", mount_rc);
    if (!g_log) log_line("engine.log could not be opened; messages go only to stdout");
    {
        u64 total = 0, used = 0;
        svcGetInfo(&total, InfoType_TotalMemorySize, CUR_PROCESS_HANDLE, 0);
        svcGetInfo(&used, InfoType_UsedMemorySize, CUR_PROCESS_HANDLE, 0);
        char memory[96];
        std::snprintf(memory, sizeof(memory), "Process memory: total %llu MiB, used %llu MiB",
                      static_cast<unsigned long long>(total >> 20), static_cast<unsigned long long>(used >> 20));
        log_line(memory);
        // Applet mode (hbmenu opened from the Album) leaves a homebrew only ~380-512 MiB in total (measured on hardware
        // by nx.js); an application-mode launch has several GiB. This game needs roughly 215-260 MiB of CPU memory while
        // loading plus ~190 MiB of textures that also come out of the process memory, so in applet mode it cannot fit
        // and the process would simply die. Say so and go back to the menu instead. `allow-low-memory.txt` in
        // sdmc:/switch/pokenx/ skips the check (for testing).
        constexpr u64 kMinGameMemory = 1024ULL << 20;
        struct stat override_stat;
        const bool allow_low_memory = stat("sdmc:/switch/pokenx/allow-low-memory.txt", &override_stat) == 0;
        if (total < kMinGameMemory && !allow_low_memory) {
            char text[220];
            std::snprintf(text, sizeof(text),
                          "Memoria insuficiente: el proceso solo tiene %llu MiB (modo applet). Abre PokeNX manteniendo R al lanzar un juego instalado.",
                          static_cast<unsigned long long>(total >> 20));
            log_line(text);
            info.play_allowed = false;
            info.blocked_reason = text;
            info.message = text;
        }
    }

    if (FILE* last = std::fopen("sdmc:/switch/pokenx/logs/last-session.txt", "r")) {
        char text[300] = {0};
        if (std::fgets(text, sizeof(text), last)) {
            std::string previous = std::string("Ultima sesion: ") + text;
            while (!previous.empty() && (previous.back() == '\n' || previous.back() == '\r')) previous.pop_back();
            if (!info.message.empty()) info.message += "\n";
            info.message += previous;
        }
        std::fclose(last);
        std::remove("sdmc:/switch/pokenx/logs/last-session.txt");
    }
    {
        const std::string stalled = zink_fall_back_if_stalled();
        if (!stalled.empty()) {
            if (!info.message.empty()) info.message += '\n';
            info.message += stalled;
        }
    }
    if (run_menu(info) == MenuResult::Quit) {
        log_line("Menu: quit");
        if (g_log) std::fclose(g_log);
        fsdevUnmountAll();
        return 0;
    }

    clocks_raise();
    g_time_ready = R_SUCCEEDED(timeInitialize());
    select_graphics_driver();
    select_resolution();
    hidSetNpadJoyHoldType(HidNpadJoyHoldType_Horizontal);
    padConfigureInput(8, HidNpadStyleSet_NpadStandard | HidNpadStyleTag_NpadHandheld);
    padInitializeAny(&g_pad);
    g_pad_ready = true;
    Audio audio;
    g_audio = &audio;
    Watchdog watchdog;
    watchdog.start();
    bool ok = g_egl.init();
    if (ok) overlay_prepare();
    // nx.js measured on hardware that HID can stop delivering input when EGL takes the NWindow after the pads were
    // configured; configure them again now that the graphics stack owns the window.
    padConfigureInput(8, HidNpadStyleSet_NpadStandard | HidNpadStyleTag_NpadHandheld);
    padInitializeAny(&g_pad);

    PokenxHost host = {POKENX_ABI_VERSION, host_get_proc,     host_log,
                       host_swap_buffers,  host_set_swap_interval, host_unix_seconds, poll_pad_snapshot};
    if (ok) {
        const int32_t init_result = pokenx_engine_init(&host);
        if (init_result != POKENX_RESULT_OK) {
            log_i32("pokenx_engine_init failed: result %d", init_result);
            ok = false;
        }
    }

    const bool audio_ok = ok && audio.init();
    if (ok && !audio_ok) log_line("Audio unavailable; continuing without sound");

    bool game_running = false;
    if (ok) {
        // Default: fill the 16:9 screen. `scaling.txt` containing "aspect" keeps the game's own shape with bands.
        char scaling[16] = "full";
        if (FILE* config = std::fopen(kScalingPath, "r")) {
            if (std::fscanf(config, "%15s", scaling) != 1) std::snprintf(scaling, sizeof(scaling), "full");
            std::fclose(config);
        }
        if (std::strcmp(scaling, "aspect") != 0) std::snprintf(scaling, sizeof(scaling), "full");
        setenv("OPENGMK_SCALING", scaling, 0);
        {
            // Budget of the engine's read cache for game data files (see pokenx-core gml/file.rs). filecache.txt: "off"
            // disables it. In applet mode (little RAM) the budget is smaller.
            char cache_mode[16] = "on";
            if (FILE* config = std::fopen("sdmc:/switch/pokenx/filecache.txt", "r")) {
                if (std::fscanf(config, "%15s", cache_mode) != 1) std::snprintf(cache_mode, sizeof(cache_mode), "on");
                std::fclose(config);
            }
            u64 total_memory = 0;
            svcGetInfo(&total_memory, InfoType_TotalMemorySize, CUR_PROCESS_HANDLE, 0);
            const char* budget = std::strcmp(cache_mode, "off") == 0 ? "0" : (total_memory >= (1024ULL << 20) ? "64" : "16");
            setenv("POKENX_FILE_CACHE_MB", budget, 1);
            log_fmt("File cache budget: %u MiB", static_cast<unsigned>(std::atoi(budget)));
        }
        log_line(scaling[0] == 'a' ? "Scaling: aspect (bands)" : "Scaling: full (16:9)");
        log_memory("before load_game");
        g_loading = true;
        const int32_t loaded = pokenx_engine_load_game(kGameDir);
        g_loading = false;
        log_i32("pokenx_engine_load_game result: %d", loaded);
        log_memory("after load_game");
        game_running = loaded == POKENX_RESULT_OK;
        log_line(game_running ? "Game loaded" : "No game running: returning to the menu (details above)");
        if (!game_running) {
            char text[200];
            // load_game returns NO_GAME for every load error (missing game, bad data, or a renderer/GL failure inside
            // Game::launch), so do not claim the game is missing: the exact cause is in engine.log.
            const char* reason = loaded == POKENX_RESULT_NO_GAME
                                     ? "no se pudo cargar el juego: no esta en game/, sus datos fallan o el motor/OpenGL no inicio"
                                     : (loaded == POKENX_RESULT_NO_GL ? "no se pudo iniciar OpenGL"
                                                                      : "error al cargar los datos del juego");
            std::snprintf(text, sizeof(text), "El motor no pudo cargar el juego (%s; codigo %d). Detalle en logs/engine.log.",
                          reason, static_cast<int>(loaded));
            write_engine_status(text);
            ok = false;  // skip the loop: control goes back to the launcher
        }
    }


    EGLint width = static_cast<EGLint>(kRenderWidth), height = static_cast<EGLint>(kRenderHeight);
    if (ok) {
        eglQuerySurface(g_egl.display, g_egl.surface, EGL_WIDTH, &width);
        eglQuerySurface(g_egl.display, g_egl.surface, EGL_HEIGHT, &height);
        pokenx_engine_resize(static_cast<uint32_t>(width), static_cast<uint32_t>(height));
    }

    unsigned frames = 0;
    const u32 presentation_hz = pokenx_engine_presentation_hz();
    const u64 frame_ns = kNanosPerSecond / presentation_hz;
    u64 previous_start = now_ns();
    u64 deadline = previous_start;
    u64 stats_start = previous_start;
    u64 stats_swaps = g_successful_swaps;
    u64 stats_work_max = 0;
    unsigned stats_frames = 0, stats_late = 0;
    PokenxStats previous_stats = {};
    pokenx_engine_stats(&previous_stats);
    log_fmt("Presentation target: %u Hz (simulation keeps room_speed)", presentation_hz);
    int32_t last_result = POKENX_RESULT_OK;
    while (ok) {
        PokenxPad snapshot = {};
        if (poll_pad_snapshot(&snapshot) <= 0) break;
        const u64 frame_start = now_ns();
        if (g_clock_discontinuity) {
            pokenx_engine_reset_timing();
            previous_start = deadline = stats_start = frame_start;
            stats_swaps = g_successful_swaps;
            stats_frames = stats_late = 0;
            stats_work_max = 0;
            g_clock_discontinuity = false;
            log_line("Presentation/simulation clocks reset after focus loss");
        }
        const u64 elapsed_ns = frame_start - previous_start;
        previous_start = frame_start;

        // Docked/handheld changes alter the drawable size.
        EGLint new_width = width, new_height = height;
        eglQuerySurface(g_egl.display, g_egl.surface, EGL_WIDTH, &new_width);
        eglQuerySurface(g_egl.display, g_egl.surface, EGL_HEIGHT, &new_height);
        if (new_width != width || new_height != height) {
            width = new_width;
            height = new_height;
            pokenx_engine_resize(static_cast<uint32_t>(width), static_cast<uint32_t>(height));
        }

        if (frames < 2) log_fmt("mark: game frame %u begins (simulation and drawing)", frames);
        last_result = pokenx_engine_frame_elapsed(&snapshot, static_cast<uint32_t>(width), static_cast<uint32_t>(height), elapsed_ns);
        if (frames < 2) log_fmt("mark: game frame %u computed", frames);
        if (last_result == POKENX_RESULT_FINISHED) {
            log_line("Game finished");
            break;
        }
        if (last_result < 0) {
            log_fmt("pokenx_engine_frame error %d; stopping", static_cast<int>(last_result));
            {
                char text[200];
                std::snprintf(text, sizeof(text), "El motor se detuvo por un error en el juego (fotograma %u, codigo %d). Detalle en logs/engine.log.",
                              frames, static_cast<int>(last_result));
                write_engine_status(text);
            }
            break;
        }
        if (last_result == POKENX_RESULT_OK) host_swap_buffers();
        if (g_swap_failures) { ok = false; break; }
        ++frames;
        ++stats_frames;

        // One display opportunity at 60 Hz; simulation ticks are accumulated by Rust.
        // Work time includes simulation, GL submission and any wait inside eglSwapBuffers.
        const u64 now = now_ns();
        const u64 work_ns = now - frame_start;
        if (work_ns > stats_work_max) stats_work_max = work_ns;
        deadline += frame_ns;
        if (deadline > now) {
            svcSleepThread(static_cast<s64>(deadline - now));
        } else {
            if (now - deadline > 500'000ULL) ++stats_late; // ignore sub-0.5 ms wake-up jitter
            deadline = now;
        }
        const u64 stats_now = now_ns();
        const u64 duration_ns = stats_now - stats_start;
        if (duration_ns >= 5 * kNanosPerSecond) {
            const u64 hz_milli = static_cast<u64>(stats_frames) * 1'000'000'000'000ULL / duration_ns;
            const u64 swaps_milli = (g_successful_swaps - stats_swaps) * 1'000'000'000'000ULL / duration_ns;
            char report[240];
            std::snprintf(report, sizeof(report),
                "video: host %llu.%03llu Hz, successful swaps %llu.%03llu/s, room_speed %u, max host work %llu us, late %u/%u",
                static_cast<unsigned long long>(hz_milli / 1000), static_cast<unsigned long long>(hz_milli % 1000),
                static_cast<unsigned long long>(swaps_milli / 1000), static_cast<unsigned long long>(swaps_milli % 1000),
                pokenx_engine_target_fps(), static_cast<unsigned long long>(stats_work_max / 1000), stats_late, stats_frames);
            log_line(report);
            // What the game itself did in the same window, to tell "the engine is slow" from "the game paused itself"
            // (sleep), "it read files" or "it changed room" without guessing.
            PokenxStats current_stats = {};
            pokenx_engine_stats(&current_stats);
            const auto delta = [&](uint64_t PokenxStats::*field) {
                return static_cast<unsigned long long>(current_stats.*field - previous_stats.*field);
            };
            char pace[300];
            std::snprintf(pace, sizeof(pace),
                "pace: game paused %llu ms in %llu pause(s); %llu file(s) read (%llu KiB); %llu room change(s), engine time %llu ms, longest %llu ms; %llu room(s) compiled ahead (%llu ms)",
                delta(&PokenxStats::paused_ns) / 1000000ULL, delta(&PokenxStats::pauses), delta(&PokenxStats::files_read),
                delta(&PokenxStats::bytes_read) / 1024ULL, delta(&PokenxStats::room_changes), delta(&PokenxStats::room_ns) / 1000000ULL,
                static_cast<unsigned long long>(current_stats.room_max_ns / 1000000ULL), delta(&PokenxStats::warmed_rooms),
                delta(&PokenxStats::warm_ns) / 1000000ULL);
            log_line(pace);
            previous_stats = current_stats;
            stats_start = stats_now;
            stats_swaps = g_successful_swaps;
            stats_frames = stats_late = 0;
            stats_work_max = 0;
        }
    }
    log_fmt("Main loop ended after %u frames", frames);

    watchdog.stop();
    audio.shutdown();
    g_audio = nullptr;
    g_pad_ready = false;
    pokenx_engine_shutdown();
    g_egl.shutdown();
    clocks_restore();
    if (g_time_ready) timeExit();
    log_line("Game session ended");
    zink_pending_clear();  // a session that ends by itself is not a stall
    // The text console must not take the window back once Mesa used it: Mesa registers 3 buffer slots, the console 2, and
    // the console's third frame then fails to dequeue and libnx aborts (0x2B59 BadGfxDequeueBuffer; measured on
    // hardware by the nova3-nx port, nova3-nx/runtime/source/util.c:268-275). So the reason is
    // saved and the menu shows it the next time PokeNX starts; the process returns to hbmenu.
    if (FILE* last = std::fopen("sdmc:/switch/pokenx/logs/last-session.txt", "w")) {
        std::fprintf(last, "%s\n", g_status.empty() ? "La sesion de juego anterior termino sin error registrado." : g_status.c_str());
        std::fclose(last);
    }
    if (g_log) std::fclose(g_log);
    fsdevUnmountAll();
    return ok ? 0 : 1;
}
