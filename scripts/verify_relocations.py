#!/usr/bin/env python3
"""Fails when a dynamic relocation of a Switch ELF points outside a writable PT_LOAD segment.

Why: libnx's crt0 (nx/source/runtime/dynamic.c, __nx_dynamic) applies every relocation (.rela.dyn and the packed
.relr.dyn) by writing to base+offset. hbloader (nx-hbloader/source/main.c, ~lines 429-444) maps the NRO's text segment
R+X and its rodata segment R only *before* the program starts. A relocation inside either of them is a write to
read-only memory: the process dies in crt0, before userAppInit, with no log, no menu and a black screen.
The stock link (-z text) refuses such a binary; the project once bypassed that with -z notext and shipped one.
The linker script app/engine/switch-pokenx.ld moves .rodata to the writable RELRO area so this cannot happen.

Usage: verify_relocations.py <file.elf>   (exit 0 = every relocation is in a writable segment)
"""
import collections
import struct
import sys


def main(path: str) -> int:
    f = open(path, 'rb').read()
    if f[:4] != b'\x7fELF' or f[4] != 2:
        print('not an ELF64 file:', path)
        return 2
    shoff = struct.unpack_from('<Q', f, 0x28)[0]
    shentsize, shnum, shstrndx = struct.unpack_from('<HHH', f, 0x3A)
    secs = [struct.unpack_from('<IIQQQQ', f, shoff + i * shentsize) for i in range(shnum)]
    stro = secs[shstrndx][4]

    def name_of(n):
        return f[stro + n:f.index(b'\0', stro + n)].decode()

    sec = {name_of(s[0]): s for s in secs}
    phoff = struct.unpack_from('<Q', f, 0x20)[0]
    phentsize, phnum = struct.unpack_from('<HH', f, 0x36)
    segs = []
    for i in range(phnum):
        p_type, flags, _off, vaddr, _pa, _filesz, memsz, _align = struct.unpack_from('<IIQQQQQQ', f, phoff + i * phentsize)
        if p_type == 1:
            segs.append((flags, vaddr, vaddr + memsz))

    targets = []
    if '.relr.dyn' in sec:
        s = sec['.relr.dyn']
        entries = struct.unpack_from('<%dQ' % (s[5] // 8), f, s[4])
        cursor = None
        for entry in entries:
            if entry & 1 == 0:
                targets.append(entry)
                cursor = entry + 8
            else:
                bitmap = entry >> 1
                address = cursor
                while bitmap:
                    if bitmap & 1:
                        targets.append(address)
                    bitmap >>= 1
                    address += 8
                cursor += 63 * 8
    if '.rela.dyn' in sec:
        s = sec['.rela.dyn']
        for k in range(s[5] // 24):
            offset, _info, _addend = struct.unpack_from('<QQq', f, s[4] + k * 24)
            targets.append(offset)

    bad = collections.Counter()
    total = 0
    for address in targets:
        total += 1
        placed = False
        for flags, lo, hi in segs:
            if lo <= address < hi:
                placed = True
                if not flags & 2:  # PF_W
                    section = next((n for n, s in sec.items() if s[3] and s[3] <= address < s[3] + s[5]), '?')
                    bad[('R' + ('X' if flags & 1 else '-'), section)] += 1
                break
        if not placed:
            bad[('none', '?')] += 1
    print('%s: %d relocated words checked' % (path, total))
    if bad:
        print('ERROR: relocations outside writable segments (the NRO would crash in crt0 before any log):')
        for (perm, section), count in sorted(bad.items()):
            print('  %s segment, section %s: %d' % (perm, section, count))
        return 1
    print('OK: every dynamic relocation is in a writable segment')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1]))
