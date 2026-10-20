// Menu of the single PokeNX NRO: shown before the game starts and after it ends.
// The normal menu (menu.cpp) is drawn by gfx.cpp on a libnx framebuffer with the system font. If the framebuffer or the
// font cannot start, run_menu falls back to the text console menu (menu_text.cpp), which is plain ASCII and always works,
// so a problem is still explained instead of showing a black screen.
#pragma once

#include <string>

enum class MenuResult { Play, Quit };

struct MenuInfo {
    std::string message;          // shown under the menu (why the last run ended, memory warning, ...)
    bool play_allowed = true;     // false: starting the game is not possible in this process
    std::string blocked_reason;   // shown when Play is chosen while not allowed
};

// Runs the menu until the player chooses Play or Quit. Initialises and releases its own display.
MenuResult run_menu(const MenuInfo& info);

// The text console version (fallback). Same contract as run_menu.
MenuResult run_text_menu(const MenuInfo& info);
