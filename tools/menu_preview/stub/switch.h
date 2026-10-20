// Host stand-in for <switch.h>: just enough of libnx for app/engine/gfx.cpp and menu.cpp to compile and draw on a PC.
// Used only by tools/menu_preview/preview.cpp to render the menu screens to PNG files. Not part of the NRO.
#pragma once
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;
typedef int32_t s32;
typedef int64_t s64;
typedef u32 Result;
#define R_FAILED(r) ((r) != 0)
#define R_SUCCEEDED(r) ((r) == 0)

struct NWindow { int unused; };
inline NWindow* nwindowGetDefault() { static NWindow window; return &window; }
enum { PIXEL_FORMAT_RGBA_8888 = 1 };
struct Framebuffer { u32* pixels; int width, height; };
extern Framebuffer* g_preview_fb;
inline Result framebufferCreate(Framebuffer* fb, NWindow*, u32 w, u32 h, int, u32) {
    fb->width = static_cast<int>(w); fb->height = static_cast<int>(h);
    fb->pixels = static_cast<u32*>(std::calloc(static_cast<size_t>(w) * h, 4));
    g_preview_fb = fb;
    return 0;
}
inline Result framebufferMakeLinear(Framebuffer*) { return 0; }
inline void* framebufferBegin(Framebuffer* fb, u32* stride) { *stride = static_cast<u32>(fb->width) * 4; return fb->pixels; }
void preview_frame_end();  // preview.cpp: counts frames and saves the requested screenshot
inline void framebufferEnd(Framebuffer*) { preview_frame_end(); }
inline void framebufferClose(Framebuffer* fb) { std::free(fb->pixels); fb->pixels = nullptr; }

enum PlServiceType { PlServiceType_User = 0 };
enum PlSharedFontType { PlSharedFontType_Standard = 0 };
struct PlFontData { void* address; u32 size; };
Result plInitialize(PlServiceType);
inline void plExit() {}
Result plGetSharedFontByType(PlFontData* out, PlSharedFontType);

struct HidAnalogStickState { s32 x, y; };
struct PadState { u64 down, held; };
enum : u64 {
    HidNpadButton_A = 1ULL << 0, HidNpadButton_B = 1ULL << 1, HidNpadButton_X = 1ULL << 2, HidNpadButton_Y = 1ULL << 3,
    HidNpadButton_StickL = 1ULL << 4, HidNpadButton_StickR = 1ULL << 5, HidNpadButton_L = 1ULL << 6, HidNpadButton_R = 1ULL << 7,
    HidNpadButton_ZL = 1ULL << 8, HidNpadButton_ZR = 1ULL << 9, HidNpadButton_Plus = 1ULL << 10, HidNpadButton_Minus = 1ULL << 11,
    HidNpadButton_Left = 1ULL << 12, HidNpadButton_Up = 1ULL << 13, HidNpadButton_Right = 1ULL << 14, HidNpadButton_Down = 1ULL << 15,
};
enum { HidNpadStyleSet_NpadStandard = 1 };
inline void padConfigureInput(int, int) {}
inline void padInitializeAny(PadState*) {}
void padUpdate(PadState* pad);  // preview.cpp feeds scripted input
inline u64 padGetButtonsDown(const PadState* pad) { return pad->down; }
inline u64 padGetButtons(const PadState* pad) { return pad->held; }
inline HidAnalogStickState padGetStickPos(const PadState*, int) { return {0, 0}; }

enum TimeType { TimeType_UserSystemClock = 0 };
inline Result timeInitialize() { return 0; }
inline void timeExit() {}
inline Result timeGetCurrentTime(TimeType, u64* out) { *out = 1792327000ULL; return 0; }  // a fixed date for the screenshots

struct Thread { int unused; };
typedef void (*ThreadFunc)(void*);
inline Result threadCreate(Thread*, ThreadFunc, void*, void*, size_t, int, int) { return 0; }  // the preview starts no threads
inline Result threadStart(Thread*) { return 0; }
inline Result threadWaitForExit(Thread*) { return 0; }
inline Result threadClose(Thread*) { return 0; }

bool appletMainLoop();  // preview.cpp: false once the requested frames were drawn

enum { InfoType_TotalMemorySize = 6, InfoType_UsedMemorySize = 7 };
#define CUR_PROCESS_HANDLE 0xFFFF8001u
inline Result svcGetInfo(u64* out, int type, u32, u64) { *out = type == InfoType_TotalMemorySize ? (3189ULL << 20) : (3185ULL << 20); return 0; }
inline u64 armGetSystemTick() { static u64 tick = 0; return tick += 19200000 / 60; }
inline u64 armTicksToNs(u64 ticks) { return ticks * 625 / 12; }
