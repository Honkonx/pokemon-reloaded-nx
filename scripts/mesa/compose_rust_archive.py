"""Prepare a local GNU ar archive with engine-owned Rust failure handlers.

GNU objcopy cannot rewrite this complete Mesa archive on Windows because some
members have directory components. Rewrite Rust objects for private libc
imports, weaken only std/alloc failure handlers, preserve the C/C++ payloads,
then let ar rebuild the symbol index.
"""
import argparse
from pathlib import Path
import subprocess
import tempfile

LINUX_IMPORTS = (
    "__errno_location", "clock_gettime", "getrandom", "syscall", "getauxval",
    "open64", "fstat64", "stat64", "lseek64", "read", "write", "writev", "close",
    "posix_memalign", "mmap64", "munmap",
)


def compose(source, output, objcopy, ar, symbols):
    source, output = Path(source).resolve(), Path(output).resolve()
    if source == output:
        raise ValueError("the original SDK archive must remain unchanged")
    changed = 0
    rust_members = 0
    with tempfile.TemporaryDirectory(prefix="mesa-rust-", dir=output.parent) as temporary:
        temporary = Path(temporary).resolve()
        if temporary.parent != output.parent:
            raise ValueError("unexpected temporary directory")
        staged = temporary / "composed.a"
        with source.open("rb") as src, staged.open("wb") as dst:
            if src.read(8) != b"!<arch>\n":
                raise ValueError("only ordinary GNU archives are supported")
            dst.write(b"!<arch>\n")
            long_names = b""
            while header := src.read(60):
                if len(header) != 60 or header[58:] != b"`\n":
                    raise ValueError("invalid archive member header")
                size = int(header[48:58])
                if size < 0:
                    raise ValueError("negative member size")
                data = src.read(size)
                if len(data) != size or (size & 1 and len(src.read(1)) != 1):
                    raise ValueError("truncated archive member")
                name = header[:16].decode("ascii").strip()
                # Discard stale offsets. ar -s recreates the symbol table below.
                if name in ("/", "/SYM64/"):
                    continue
                if name == "//":
                    long_names = data
                elif name.startswith("/"):
                    offset = int(name[1:])
                    end = long_names.find(b"/\n", offset)
                    if offset < 0 or end < offset:
                        raise ValueError("invalid GNU long filename")
                    name = long_names[offset:end].decode("utf-8")
                elif name.startswith("#1/"):
                    raise ValueError("BSD extended member names are unsupported")
                else:
                    name = name.removesuffix("/")
                basename = name.rsplit("/", 1)[-1]
                if basename.endswith(".rcgu.o"):
                    original = temporary / "input.o"
                    modified = temporary / "output.o"
                    original.write_bytes(data)
                    weak = [f"--weaken-symbol={s}" for s in symbols] if basename.startswith(("std-", "alloc-")) else []
                    renamed = [f"--redefine-sym={s}=pokenx_mesa_linux_{s}" for s in LINUX_IMPORTS]
                    subprocess.run([objcopy, *weak, *renamed,
                                    str(original), str(modified)], check=True)
                    data = modified.read_bytes()
                    if weak:
                        changed += 1
                    rust_members += 1
                header = header[:48] + f"{len(data):<10}".encode("ascii") + b"`\n"
                dst.write(header)
                dst.write(data)
                if len(data) & 1:
                    dst.write(b"\n")
        if not changed:
            raise ValueError("no Rust std/alloc objects found; review archive layout")
        subprocess.run([ar, "s", str(staged)], check=True)
        staged.replace(output)
    print(f"Prepared {output.name}: {changed} std/alloc members, {rust_members} Rust members with private libc imports; SDK unchanged")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source")
    parser.add_argument("output")
    parser.add_argument("--objcopy", required=True)
    parser.add_argument("--ar", required=True)
    parser.add_argument("--symbol", action="append", required=True)
    args = parser.parse_args()
    compose(args.source, args.output, args.objcopy, args.ar, args.symbol)
