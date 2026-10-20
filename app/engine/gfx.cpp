#include "gfx.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <map>
#include <vector>

#define STB_TRUETYPE_IMPLEMENTATION
#include "third_party/stb_truetype.h"

namespace gfx {
namespace {

Framebuffer g_fb;
bool g_fb_ready = false;
bool g_pl_ready = false;
u32* g_pixels = nullptr;
u32 g_stride = 0;  // in pixels
std::vector<u32> g_background;

stbtt_fontinfo g_font;
bool g_font_ready = false;

struct Glyph {
    int w = 0, h = 0, xoff = 0, yoff = 0;
    float advance = 0;
    std::vector<u8> alpha;
};
std::map<uint64_t, Glyph> g_glyphs;

inline void blend(u32* dst, u32 color, int coverage) {
    const u32 a = (((color >> 24) & 0xFF) * static_cast<u32>(coverage)) / 255;
    if (a == 0) return;
    if (a >= 255) {
        *dst = color | 0xFF000000u;
        return;
    }
    const u32 d = *dst;
    const u32 inv = 255 - a;
    const u32 r = ((color & 0xFF) * a + (d & 0xFF) * inv) / 255;
    const u32 g = (((color >> 8) & 0xFF) * a + ((d >> 8) & 0xFF) * inv) / 255;
    const u32 b = (((color >> 16) & 0xFF) * a + ((d >> 16) & 0xFF) * inv) / 255;
    *dst = 0xFF000000u | (b << 16) | (g << 8) | r;
}

inline void hspan(int y, int x0, int x1, u32 color, int coverage = 255) {  // [x0, x1)
    if (y < 0 || y >= kHeight) return;
    x0 = std::max(x0, 0);
    x1 = std::min(x1, kWidth);
    if (x0 >= x1) return;
    u32* row = g_pixels + static_cast<size_t>(y) * g_stride;
    if (((color >> 24) & 0xFF) == 255 && coverage == 255) {
        for (int x = x0; x < x1; ++x) row[x] = color;
    } else {
        for (int x = x0; x < x1; ++x) blend(&row[x], color, coverage);
    }
}

void build_background() {
    g_background.assign(static_cast<size_t>(kWidth) * kHeight, 0);
    for (int y = 0; y < kHeight; ++y) {
        for (int x = 0; x < kWidth; ++x) {
            // Dark navy with a soft blue glow from the top centre, like the HOME menu mockups.
            const float dx = (x - kWidth / 2) / (kWidth * 0.62f);
            const float dy = y / (kHeight * 0.95f);
            float t = std::sqrt(dx * dx + dy * dy);
            t = t > 1.0f ? 1.0f : t;
            const float r = 34 + (11 - 34) * t, g = 48 + (15 - 48) * t, b = 107 + (29 - 107) * t;
            g_background[static_cast<size_t>(y) * kWidth + x] =
                rgba(static_cast<u32>(r), static_cast<u32>(g), static_cast<u32>(b));
        }
    }
}

// Decodes one UTF-8 code point; advances `p`. Invalid bytes become U+FFFD.
uint32_t next_codepoint(const char*& p) {
    const unsigned char c = static_cast<unsigned char>(*p);
    if (c < 0x80) { ++p; return c; }
    int extra = c >= 0xF0 ? 3 : (c >= 0xE0 ? 2 : (c >= 0xC0 ? 1 : -1));
    if (extra < 0) { ++p; return 0xFFFD; }
    uint32_t cp = c & (0x3F >> extra);
    ++p;
    for (int i = 0; i < extra; ++i) {
        if ((static_cast<unsigned char>(*p) & 0xC0) != 0x80) return 0xFFFD;
        cp = (cp << 6) | (static_cast<unsigned char>(*p) & 0x3F);
        ++p;
    }
    return cp;
}

const Glyph& glyph(uint32_t cp, int size) {
    const uint64_t key = (static_cast<uint64_t>(cp) << 16) | static_cast<uint64_t>(size);
    auto found = g_glyphs.find(key);
    if (found != g_glyphs.end()) return found->second;
    Glyph item;
    const float scale = stbtt_ScaleForPixelHeight(&g_font, static_cast<float>(size));
    int advance = 0, bearing = 0;
    stbtt_GetCodepointHMetrics(&g_font, static_cast<int>(cp), &advance, &bearing);
    item.advance = advance * scale;
    int w = 0, h = 0, xoff = 0, yoff = 0;
    unsigned char* bitmap = stbtt_GetCodepointBitmap(&g_font, scale, scale, static_cast<int>(cp), &w, &h, &xoff, &yoff);
    if (bitmap) {
        item.w = w;
        item.h = h;
        item.xoff = xoff;
        item.yoff = yoff;
        item.alpha.assign(bitmap, bitmap + static_cast<size_t>(w) * h);
        stbtt_FreeBitmap(bitmap, nullptr);
    }
    if (g_glyphs.size() > 3000) g_glyphs.clear();
    return g_glyphs.emplace(key, std::move(item)).first->second;
}

int ascent(int size) {
    int a = 0, d = 0, gap = 0;
    stbtt_GetFontVMetrics(&g_font, &a, &d, &gap);
    return static_cast<int>(a * stbtt_ScaleForPixelHeight(&g_font, static_cast<float>(size)));
}

}  // namespace

bool init() {
    if (R_FAILED(plInitialize(PlServiceType_User))) return false;
    g_pl_ready = true;
    PlFontData data;
    if (R_FAILED(plGetSharedFontByType(&data, PlSharedFontType_Standard)) || !data.address || data.size == 0) {
        shutdown();
        return false;
    }
    const unsigned char* bytes = static_cast<const unsigned char*>(data.address);
    if (!stbtt_InitFont(&g_font, bytes, stbtt_GetFontOffsetForIndex(bytes, 0))) {
        shutdown();
        return false;
    }
    g_font_ready = true;
    NWindow* window = nwindowGetDefault();
    if (!window || R_FAILED(framebufferCreate(&g_fb, window, kWidth, kHeight, PIXEL_FORMAT_RGBA_8888, 2))) {
        shutdown();
        return false;
    }
    g_fb_ready = true;
    if (R_FAILED(framebufferMakeLinear(&g_fb))) {
        shutdown();
        return false;
    }
    build_background();
    return true;
}

void shutdown() {
    if (g_fb_ready) {
        framebufferClose(&g_fb);
        g_fb_ready = false;
    }
    if (g_pl_ready) {
        plExit();
        g_pl_ready = false;
    }
    g_font_ready = false;
    g_pixels = nullptr;
    g_glyphs.clear();
    std::vector<u32>().swap(g_background);
}

void begin() {
    u32 stride_bytes = 0;
    g_pixels = static_cast<u32*>(framebufferBegin(&g_fb, &stride_bytes));
    g_stride = stride_bytes / 4;
    for (int y = 0; y < kHeight; ++y) {
        std::memcpy(g_pixels + static_cast<size_t>(y) * g_stride, g_background.data() + static_cast<size_t>(y) * kWidth,
                    kWidth * sizeof(u32));
    }
}

void end() {
    framebufferEnd(&g_fb);
    g_pixels = nullptr;
}

void fill(int x, int y, int w, int h, u32 color) {
    for (int row = y; row < y + h; ++row) hspan(row, x, x + w, color);
}

void rrect(int x, int y, int w, int h, int radius, u32 color) {
    radius = std::min(radius, std::min(w, h) / 2);
    for (int row = 0; row < h; ++row) {
        int inset = 0;
        const int from_edge = std::min(row, h - 1 - row);
        if (from_edge < radius) {
            const float dy = static_cast<float>(radius - from_edge) - 0.5f;
            const float dx = static_cast<float>(radius) - std::sqrt(std::max(0.0f, static_cast<float>(radius * radius) - dy * dy));
            inset = static_cast<int>(std::ceil(dx - 0.5f));
        }
        hspan(y + row, x + inset, x + w - inset, color);
    }
}

void rrect_gradient(int x, int y, int w, int h, int radius, u32 top, u32 bottom) {
    radius = std::min(radius, std::min(w, h) / 2);
    for (int row = 0; row < h; ++row) {
        int inset = 0;
        const int from_edge = std::min(row, h - 1 - row);
        if (from_edge < radius) {
            const float dy = static_cast<float>(radius - from_edge) - 0.5f;
            const float dx = static_cast<float>(radius) - std::sqrt(std::max(0.0f, static_cast<float>(radius * radius) - dy * dy));
            inset = static_cast<int>(std::ceil(dx - 0.5f));
        }
        const float t = h > 1 ? static_cast<float>(row) / static_cast<float>(h - 1) : 0.0f;
        const auto mix = [&](int shift) {
            const float a = static_cast<float>((top >> shift) & 0xFF), b = static_cast<float>((bottom >> shift) & 0xFF);
            return static_cast<u32>(a + (b - a) * t);
        };
        hspan(y + row, x + inset, x + w - inset, rgba(mix(0), mix(8), mix(16), 255));
    }
}

void circle(int cx, int cy, int radius, u32 color) {
    for (int dy = -radius; dy <= radius; ++dy) {
        const float half = std::sqrt(std::max(0.0f, static_cast<float>(radius * radius) - static_cast<float>(dy * dy) + 0.25f));
        hspan(cy + dy, cx - static_cast<int>(half + 0.5f), cx + static_cast<int>(half + 0.5f) + 1, color);
    }
}

void ring(int cx, int cy, int radius, int thickness, u32 color) {
    const float outer = static_cast<float>(radius), inner = static_cast<float>(radius - thickness);
    for (int dy = -radius; dy <= radius; ++dy) {
        for (int dx = -radius; dx <= radius; ++dx) {
            const float d = std::sqrt(static_cast<float>(dx * dx + dy * dy));
            if (d <= outer && d >= inner) hspan(cy + dy, cx + dx, cx + dx + 1, color);
        }
    }
}

void triangle(int x0, int y0, int x1, int y1, int x2, int y2, u32 color) {
    const int min_y = std::min(y0, std::min(y1, y2)), max_y = std::max(y0, std::max(y1, y2));
    for (int y = min_y; y <= max_y; ++y) {
        float lo = 1e9f, hi = -1e9f;
        const int xs[3] = {x0, x1, x2}, ys[3] = {y0, y1, y2};
        for (int e = 0; e < 3; ++e) {
            const int ax = xs[e], ay = ys[e], bx = xs[(e + 1) % 3], by = ys[(e + 1) % 3];
            if ((y < std::min(ay, by)) || (y > std::max(ay, by)) || ay == by) continue;
            const float x = ax + (bx - ax) * static_cast<float>(y - ay) / static_cast<float>(by - ay);
            lo = std::min(lo, x);
            hi = std::max(hi, x);
        }
        if (lo <= hi) hspan(y, static_cast<int>(std::floor(lo)), static_cast<int>(std::ceil(hi)) + 1, color);
    }
}

void line(int x0, int y0, int x1, int y1, int thickness, u32 color) {
    const float dx = static_cast<float>(x1 - x0), dy = static_cast<float>(y1 - y0);
    const float length = std::max(1.0f, std::sqrt(dx * dx + dy * dy));
    const int steps = static_cast<int>(length);
    const int r = std::max(1, thickness / 2);
    for (int i = 0; i <= steps; ++i) {
        const float t = static_cast<float>(i) / static_cast<float>(steps);
        circle(static_cast<int>(x0 + dx * t), static_cast<int>(y0 + dy * t), r, color);
    }
}

int text_width(const std::string& utf8, int size) {
    if (!g_font_ready) return 0;
    float width = 0;
    for (const char* p = utf8.c_str(); *p;) width += glyph(next_codepoint(p), size).advance;
    return static_cast<int>(width + 0.5f);
}

void text(int x, int y, const std::string& utf8, int size, u32 color) {
    if (!g_font_ready || !g_pixels) return;
    float pen = static_cast<float>(x);
    const int baseline = y + ascent(size);
    for (const char* p = utf8.c_str(); *p;) {
        const Glyph& item = glyph(next_codepoint(p), size);
        const int gx = static_cast<int>(pen) + item.xoff, gy = baseline + item.yoff;
        for (int row = 0; row < item.h; ++row) {
            const int py = gy + row;
            if (py < 0 || py >= kHeight) continue;
            u32* dst = g_pixels + static_cast<size_t>(py) * g_stride;
            for (int col = 0; col < item.w; ++col) {
                const int px = gx + col;
                if (px < 0 || px >= kWidth) continue;
                const int coverage = item.alpha[static_cast<size_t>(row) * item.w + col];
                if (coverage) blend(&dst[px], color, coverage);
            }
        }
        pen += item.advance;
    }
}

void text_center(int cx, int y, const std::string& utf8, int size, u32 color) {
    text(cx - text_width(utf8, size) / 2, y, utf8, size, color);
}

void text_right(int right, int y, const std::string& utf8, int size, u32 color) {
    text(right - text_width(utf8, size), y, utf8, size, color);
}

std::string ellipsize(const std::string& utf8, int size, int max_width) {
    if (text_width(utf8, size) <= max_width) return utf8;
    std::string cut = utf8;
    while (!cut.empty() && text_width(cut + "...", size) > max_width) {
        // drop one whole UTF-8 sequence
        size_t n = cut.size() - 1;
        while (n > 0 && (static_cast<unsigned char>(cut[n]) & 0xC0) == 0x80) --n;
        cut.resize(n);
    }
    return cut + "...";
}

}  // namespace gfx
