// Creates (and removes) the PokeNX icon on the console's HOME menu from inside the NRO: a small "forwarder" title whose
// program starts sdmc:/switch/pokenx/pokenx.nro. The three NCA files are built in memory (home_nca.h), written to the SD
// card's content storage through ncm and registered with ns. Needs Atmosphère with signature patches ("sigpatches"),
// like any forwarder. Not verified on a console.
#pragma once

#include <string>

namespace home_install {

constexpr unsigned long long kTitleId = 0x01007E3A9F2C4000ULL;  // must match app/forwarder/forwarder.json

struct Outcome {
    bool ok = false;
    unsigned int code = 0;  // the console's result code of the step that failed
    std::string step;       // what was being done when it failed
    std::string detail;     // a plain-language hint
};

// Called before each step with a short description, its 1-based number and the total.
using Progress = void (*)(const char* step, int index, int total);

// Whether the application record of the shortcut exists.
bool is_installed();

Outcome install(Progress progress);
Outcome uninstall();

// "2345-0123"-style text of a result code.
std::string code_text(unsigned int code);

}  // namespace home_install
