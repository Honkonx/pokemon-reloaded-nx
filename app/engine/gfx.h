// Tiny software renderer for the menu: a linear libnx framebuffer (1280x720 RGBA8888), rounded rectangles, circles,
// triangles and text from the system's shared font (stb_truetype, public domain / MIT). It exists so the menu can look
// like the mockups in bocetos/v2 without OpenGL: the menu runs before EGL/Mesa take the window, and gfx::shutdown()
// releases it before that, exactly as the old text console did.
#pragma once

#include <switch.h>

#include <string>

namespace gfx {

constexpr int kWidth = 1280;
constexpr int kHeight = 720;

constexpr u32 rgba(u32 r, u32 g, u32 b, u32 a = 255) { return (a << 24) | (b << 16) | (g << 8) | r; }

// false: no framebuffer or no system font; the caller falls back to the text console.
bool init();
void shutdown();

void begin();  // acquires the back buffer and paints the background
void end();    // presents it (waits for the next vertical blank)

void fill(int x, int y, int w, int h, u32 color);
void rrect(int x, int y, int w, int h, int radius, u32 color);
void rrect_gradient(int x, int y, int w, int h, int radius, u32 top, u32 bottom);  // vertical gradient
void circle(int cx, int cy, int radius, u32 color);
void ring(int cx, int cy, int radius, int thickness, u32 color);
void triangle(int x0, int y0, int x1, int y1, int x2, int y2, u32 color);
void line(int x0, int y0, int x1, int y1, int thickness, u32 color);

// Text. `y` is the top of the line box. Sizes are pixel heights.
int text_width(const std::string& utf8, int size);
void text(int x, int y, const std::string& utf8, int size, u32 color);
void text_center(int cx, int y, const std::string& utf8, int size, u32 color);
void text_right(int right, int y, const std::string& utf8, int size, u32 color);
std::string ellipsize(const std::string& utf8, int size, int max_width);

}  // namespace gfx
