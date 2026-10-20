// FPS meter overlay: the numbers, the pixel font and the rectangles to paint. Free of GL and libnx so tools/update_core_test
// can check it on a PC; main.cpp paints the rectangles with scissored clears right before eglSwapBuffers (it needs no
// program, texture or buffer state, so it cannot disturb the game's own drawing).
#pragma once

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <string>
#include <vector>

namespace fps_overlay {

struct Rect {
    int x, y, w, h;  // pixels from the top-left of the window
};

// 5x7 glyphs, one string of 7 rows of 5 characters ('#' = lit). Only what the meter prints.
inline const char* glyph(char c) {
    switch (c) {
        case '0': return "01110" "10001" "10011" "10101" "11001" "10001" "01110";
        case '1': return "00100" "01100" "00100" "00100" "00100" "00100" "01110";
        case '2': return "01110" "10001" "00001" "00010" "00100" "01000" "11111";
        case '3': return "11110" "00001" "00001" "01110" "00001" "00001" "11110";
        case '4': return "00010" "00110" "01010" "10010" "11111" "00010" "00010";
        case '5': return "11111" "10000" "11110" "00001" "00001" "10001" "01110";
        case '6': return "00110" "01000" "10000" "11110" "10001" "10001" "01110";
        case '7': return "11111" "00001" "00010" "00100" "01000" "01000" "01000";
        case '8': return "01110" "10001" "10001" "01110" "10001" "10001" "01110";
        case '9': return "01110" "10001" "10001" "01111" "00001" "00010" "01100";
        case 'F': return "11111" "10000" "10000" "11110" "10000" "10000" "10000";
        case 'P': return "11110" "10001" "10001" "11110" "10000" "10000" "10000";
        case 'S': return "01111" "10000" "10000" "01110" "00001" "00001" "11110";
        case 'M': return "10001" "11011" "10101" "10101" "10001" "10001" "10001";
        default: return nullptr;  // space and anything unknown
    }
}

constexpr int kGlyphW = 5, kGlyphH = 7, kAdvance = 6;

// Width and height of `text` (lines separated by '\n') in pixels at `scale`.
inline void measure(const std::string& text, int scale, int& width, int& height) {
    int line_chars = 0, max_chars = 0, lines = 1;
    for (char c : text) {
        if (c == '\n') {
            ++lines;
            line_chars = 0;
        } else {
            max_chars = std::max(max_chars, ++line_chars);
        }
    }
    width = max_chars ? (max_chars * kAdvance - 1) * scale : 0;
    height = (lines * (kGlyphH + 2) - 2) * scale;
}

// The lit pixels of `text` merged into as few rectangles as possible (vertical runs first), at (x, y) and `scale`.
inline std::vector<Rect> build_rects(const std::string& text, int x, int y, int scale) {
    std::vector<Rect> rects;
    int pen_x = x, pen_y = y;
    for (char c : text) {
        if (c == '\n') {
            pen_x = x;
            pen_y += (kGlyphH + 2) * scale;
            continue;
        }
        if (const char* bitmap = glyph(c)) {
            for (int col = 0; col < kGlyphW; ++col) {
                int row = 0;
                while (row < kGlyphH) {
                    if (bitmap[row * kGlyphW + col] != '1') {
                        ++row;
                        continue;
                    }
                    int end = row;
                    while (end < kGlyphH && bitmap[end * kGlyphW + col] == '1') ++end;
                    rects.push_back({pen_x + col * scale, pen_y + row * scale, scale, (end - row) * scale});
                    row = end;
                }
            }
        }
        pen_x += kAdvance * scale;
    }
    // merge neighbouring columns that share the same vertical run
    std::vector<Rect> merged;
    for (const Rect& rect : rects) {
        bool joined = false;
        for (Rect& other : merged) {
            if (other.y == rect.y && other.h == rect.h && other.x + other.w == rect.x) {
                other.w += rect.w;
                joined = true;
                break;
            }
        }
        if (!joined) merged.push_back(rect);
    }
    return merged;
}

// Frames per second and the longest gap between two presented frames, over windows of about half a second of play.
//
// The game pauses itself (GML sleep(), 200-2000 ms in dialogs and scenes) and presents no frame meanwhile. `paused_total_ns`
// is the engine's running total of those pauses: the time is taken out of the gaps and out of the window, so a pause is not
// shown as a drop to 1-5 FPS. A window closes after half a second of non-paused time; a third line "P <ms>" says how much
// pause the closed window contained.
class Meter {
public:
    // Call once per presented frame with a monotonic time in nanoseconds and the running total of the game's own pauses.
    // Returns true when the numbers changed.
    bool on_frame(uint64_t now_ns, uint64_t paused_total_ns = 0) {
        bool changed = false;
        const uint64_t paused_since_last = paused_total_ns > last_paused_ ? paused_total_ns - last_paused_ : 0;
        last_paused_ = paused_total_ns;
        if (last_ != 0) {
            uint64_t gap = now_ns - last_;
            gap = gap > paused_since_last ? gap - paused_since_last : 0;
            if (gap > peak_ns_) peak_ns_ = gap;
        }
        last_ = now_ns;
        if (window_start_ == 0) {
            window_start_ = now_ns;  // the first frame only starts the clock
            window_paused_ = paused_total_ns;
            return false;
        }
        ++frames_;
        const uint64_t span = now_ns - window_start_;
        const uint64_t paused = paused_total_ns > window_paused_ ? paused_total_ns - window_paused_ : 0;
        const uint64_t active = span > paused ? span - paused : 0;
        if (active >= 500'000'000ULL) {
            fps_ = static_cast<int>((static_cast<uint64_t>(frames_) * 1'000'000'000ULL + active / 2) / active);
            peak_ms_ = static_cast<int>((peak_ns_ + 500'000ULL) / 1'000'000ULL);
            pause_ms_ = static_cast<int>(paused / 1'000'000ULL);
            frames_ = 0;
            peak_ns_ = 0;
            window_start_ = now_ns;
            window_paused_ = paused_total_ns;
            changed = true;
        }
        return changed;
    }
    int fps() const { return fps_; }
    int peak_ms() const { return peak_ms_; }
    int pause_ms() const { return pause_ms_; }
    std::string text() const {
        char buffer[64];
        if (pause_ms_ > 0)
            std::snprintf(buffer, sizeof(buffer), "%d FPS\n%d MS\nP %d", fps_, peak_ms_, pause_ms_);
        else
            std::snprintf(buffer, sizeof(buffer), "%d FPS\n%d MS", fps_, peak_ms_);
        return buffer;
    }
    // 0 = fine (green), 1 = below the 60 target (yellow), 2 = poor (red).
    int quality() const { return fps_ >= 55 ? 0 : (fps_ >= 40 ? 1 : 2); }

private:
    uint64_t last_ = 0, window_start_ = 0, peak_ns_ = 0, last_paused_ = 0, window_paused_ = 0;
    int frames_ = 0, fps_ = 0, peak_ms_ = 0, pause_ms_ = 0;
};

}  // namespace fps_overlay
