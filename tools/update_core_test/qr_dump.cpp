// Prints the QR modules of app/engine/qr.h for a text and every mask, for tools/update_core_test/qr_compare.mjs.
#include <cstdio>
#include <string>

#include "../../app/engine/qr.h"

int main(int argc, char** argv) {
    if (argc < 2) return 2;
    const std::string text = argv[1];
    for (int mask = 0; mask < 8; ++mask) {
        const qr::Code code = qr::encode(text, mask);
        std::printf("mask %d size %d\n", mask, code.size);
        for (int y = 0; y < code.size; ++y) {
            for (int x = 0; x < code.size; ++x) std::putchar(code.dark(x, y) ? '1' : '0');
            std::putchar('\n');
        }
    }
    return 0;
}
