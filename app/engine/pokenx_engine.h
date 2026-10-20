// C ABI of the Rust engine payload. Keep in sync with
// src/gm8-runtime/pokenx-engine/src/abi.rs.
#pragma once
#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define POKENX_ABI_VERSION 3u
#define POKENX_PAD_CONNECTED 1u

// Pad button bits are the gm8_input::BUTTON_* constants.
#define POKENX_BUTTON_A (1u << 0)
#define POKENX_BUTTON_B (1u << 1)
#define POKENX_BUTTON_X (1u << 2)
#define POKENX_BUTTON_Y (1u << 3)
#define POKENX_BUTTON_L (1u << 4)
#define POKENX_BUTTON_R (1u << 5)
#define POKENX_BUTTON_ZL (1u << 6)
#define POKENX_BUTTON_ZR (1u << 7)
#define POKENX_BUTTON_PLUS (1u << 8)
#define POKENX_BUTTON_MINUS (1u << 9)
#define POKENX_BUTTON_LEFT (1u << 10)
#define POKENX_BUTTON_UP (1u << 11)
#define POKENX_BUTTON_RIGHT (1u << 12)
#define POKENX_BUTTON_DOWN (1u << 13)
#define POKENX_BUTTON_STICK_L (1u << 14)
#define POKENX_BUTTON_STICK_R (1u << 15)

typedef struct PokenxPad PokenxPad;
typedef struct PokenxHost {
    uint32_t abi_version;
    const void* (*get_proc)(const uint8_t* name);  // eglGetProcAddress
    void (*log)(const uint8_t* line);              // one line to the SD log
    void (*swap_buffers)(void);                    // eglSwapBuffers on the window surface
    int32_t (*set_swap_interval)(int32_t interval);  // eglSwapInterval, non-zero on success
    int64_t (*unix_seconds)(void);                 // seconds since the Unix epoch, negative if unknown
    int32_t (*poll_input)(PokenxPad* pad);          // 1 sample, 0 exit, <0 unavailable; game thread only
} PokenxHost;

typedef struct PokenxPad {
    uint32_t flags;
    uint32_t buttons;
    int32_t left_x, left_y, right_x, right_y;
} PokenxPad;

#ifdef __cplusplus
static_assert(sizeof(PokenxHost) == 56 && offsetof(PokenxHost, poll_input) == 48, "PokenxHost ABI mismatch");
static_assert(sizeof(PokenxPad) == 24, "PokenxPad ABI mismatch");
#endif

#define POKENX_RESULT_OK 0
#define POKENX_RESULT_PRESENTED 1  // the core already swapped buffers
#define POKENX_RESULT_FINISHED 2   // the game ended
#define POKENX_RESULT_NO_PRESENT 3 // manual-draw game owns screen_refresh
#define POKENX_RESULT_BAD_ARGUMENT (-1)
#define POKENX_RESULT_BAD_ABI (-2)
#define POKENX_RESULT_NO_GL (-3)
#define POKENX_RESULT_NOT_INITIALIZED (-4)
#define POKENX_RESULT_GL_ERROR (-5)
#define POKENX_RESULT_NO_GAME (-6)
#define POKENX_RESULT_GAME_ERROR (-7)

int32_t pokenx_engine_init(const PokenxHost* host);
int32_t pokenx_engine_load_game(const char* game_dir);
uint32_t pokenx_engine_target_fps(void);
uint32_t pokenx_engine_presentation_hz(void);
void pokenx_engine_reset_timing(void);

// Running totals for the host's periodic report (times in nanoseconds). Mirrors PokenxStats in pokenx-engine/src/abi.rs.
typedef struct PokenxStats {
    uint64_t paused_ns, pauses;           // the game's own sleep() calls
    uint64_t files_read, bytes_read;      // reads from the card (cache misses)
    uint64_t room_changes, room_ns, room_max_ns;  // room_max_ns: longest change since the previous call
    uint64_t warmed_rooms, warm_ns;       // rooms compiled ahead during pauses
} PokenxStats;
void pokenx_engine_stats(PokenxStats* out);
uint64_t pokenx_engine_paused_ns(void);  // total time the game spent in its own sleep() calls
void pokenx_engine_resize(uint32_t width, uint32_t height);
int32_t pokenx_engine_frame(const PokenxPad* pad, uint32_t width, uint32_t height);
int32_t pokenx_engine_frame_elapsed(const PokenxPad* pad, uint32_t width, uint32_t height, uint64_t elapsed_ns);
int32_t pokenx_engine_audio(int16_t* out, uint32_t frames);
void pokenx_engine_shutdown(void);

// ZIP import and game detection for the console menu (implemented in Rust, pokenx-engine/src/zip_ffi.rs).
// The progress callback gets (bytes written, total or 0) and returns non-zero to cancel.
typedef int32_t (*PokenxProgressFn)(uint64_t done, uint64_t total, void* user);
int32_t pokenx_import_zip(const char* zip_path, PokenxProgressFn progress, void* user);  // 0 = ok, else error code
void pokenx_import_error_text(int32_t code, char* out, size_t capacity);
int32_t pokenx_find_game(const char* dir, char* out, size_t capacity);  // 0 = found (exe path in out), else reason

// Game keys, saves backups and the clock hint for the menu (pokenx-engine/src/zip_ffi.rs).
void pokenx_set_unix_seconds(int64_t seconds);  // lets backups made from the menu carry a date
// Reads the key bindings the game sets itself and writes them to out_path. Returns how many, or <0: -1 args, -2 no game,
// -3 unreadable, -4 cannot write the report.
int32_t pokenx_scan_game_keys(const char* game_dir, const char* out_path);
// Like pokenx_scan_game_keys, and the same pass also writes the function-compatibility report (built-in GML functions the game calls that the engine lacks or
// simplifies) to compat_path. The game is parsed once, or not at all when the menu already preloaded it.
int32_t pokenx_scan_game_reports(const char* game_dir, const char* keys_path, const char* compat_path);
int32_t pokenx_backup_create(const char* root, char* name_out, size_t capacity);   // files copied, <0 on failure
int32_t pokenx_backup_list(const char* root, char* out, size_t capacity);          // count; one line per backup: name, files, bytes, unix seconds (tab separated)
int32_t pokenx_backup_restore(const char* root, const char* name);                 // files restored, <0 on failure

// Parses the game on a worker thread while the menu is open (0 = done, <0 = failed); pokenx_engine_load_game then skips that work.
int32_t pokenx_preload_game(const char* game_dir);
void pokenx_preload_clear(void);

#ifdef __cplusplus
}
#endif
