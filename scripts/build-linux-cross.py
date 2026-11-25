"""Build the Linux x86_64 AudioCraft from Windows (or any computer), without a Linux machine.

    python scripts/build-linux-cross.py

Needs: Rust with the x86_64-unknown-linux-gnu target (rustup target add x86_64-unknown-linux-gnu),
Zig (python -m pip install --user ziglang) and cargo-zigbuild (cargo install cargo-zigbuild).

How it works: Zig links against glibc 2.28, so the program runs on Linux distributions from
2018 on (Ubuntu 20.04+, Debian 10+, Fedora 29+). The window system (X11 / Wayland / OpenGL) is
loaded when the program starts. Only the ALSA sound library is linked directly. For that link
step this script builds a stand-in libasound.so that has the same function names and the same
library name (libasound.so.2). It is never shipped: on Linux the real ALSA library (installed on
every desktop distribution) is loaded instead.

The result goes to dist/linux/: the program, its icon and desktop entry, README, licence and a
.tar.gz with all of them.
"""
import glob
import os
import re
import shutil
import struct
import subprocess
import sys
import tarfile

APP = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = "x86_64-unknown-linux-gnu"
GLIBC = "2.28"
WORK = os.path.join(APP, "target-linux")
STUB = os.path.join(WORK, "alsa-stub")
DIST = os.path.join(APP, "dist", "linux")
NAME = "audiocraft-linux-x86_64"


def run(cmd, **kw):
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, check=True, **kw)


def alsa_functions():
    """Every C function the Rust ALSA bindings may call."""
    home = os.environ.get("CARGO_HOME") or os.path.join(os.path.expanduser("~"), ".cargo")
    found = sorted(glob.glob(os.path.join(home, "registry", "src", "*", "alsa-sys-*", "src", "generated.rs")))
    if not found:
        sys.exit("alsa-sys sources not found: run `cargo fetch` first")
    names = set()
    for path in found:
        src = open(path, encoding="utf-8").read()
        names.update(re.findall(r"pub fn (snd_\w+)\s*\(", src))
    return sorted(names)


def build_stub():
    os.makedirs(STUB, exist_ok=True)
    c = os.path.join(STUB, "asound_stub.c")
    with open(c, "w", encoding="utf-8") as f:
        f.write("/* link-time stand-in for libasound.so.2; never shipped */\n")
        for n in alsa_functions():
            f.write(f"void {n}(void) {{}}\n")
    run([sys.executable, "-m", "ziglang", "cc", "-target", f"x86_64-linux-gnu.{GLIBC}",
         "-shared", "-fPIC", "-Wl,-soname,libasound.so.2",
         "-o", os.path.join(STUB, "libasound.so"), c])


def needed_libraries(path):
    """The shared libraries an ELF program asks for (DT_NEEDED), read without any Linux tool."""
    data = open(path, "rb").read()
    if data[:4] != b"\x7fELF" or data[4] != 2:
        raise SystemExit(f"{path} is not a 64-bit Linux program")
    shoff, = struct.unpack_from("<Q", data, 0x28)
    shentsize, shnum = struct.unpack_from("<HH", data, 0x3A)
    sections = [struct.unpack_from("<IIQQQQIIQQ", data, shoff + i * shentsize) for i in range(shnum)]
    out = []
    for (_, typ, _, _, off, size, link, _, _, entsize) in sections:
        if typ != 6:  # SHT_DYNAMIC
            continue
        stroff = sections[link][4]
        for i in range(0, size, entsize or 16):
            tag, val = struct.unpack_from("<qQ", data, off + i)
            if tag == 0:
                break
            if tag == 1:  # DT_NEEDED
                end = data.index(b"\0", stroff + val)
                out.append(data[stroff + val:end].decode())
    return out


def main():
    build_stub()
    env = dict(os.environ)
    # cargo-zigbuild looks for `zig` on PATH or `python3 -m ziglang`; point it at the pip copy
    if "CARGO_ZIGBUILD_ZIG_PATH" not in env and not shutil.which("zig"):
        import ziglang
        exe = "zig.exe" if os.name == "nt" else "zig"
        env["CARGO_ZIGBUILD_ZIG_PATH"] = os.path.join(os.path.dirname(ziglang.__file__), exe)
    # alsa-sys declares `links = "alsa"`: these settings stand in for its pkg-config search
    cfg = [
        "--config", f'target.{TARGET}.alsa.rustc-link-lib=["asound"]',
        "--config", f'target.{TARGET}.alsa.rustc-link-search=["{STUB.replace(os.sep, "/")}"]',
    ]
    run(["cargo", "zigbuild", "--release", "--locked", "--target", f"{TARGET}.{GLIBC}",
         "--target-dir", WORK] + cfg, cwd=APP, env=env)
    out = os.path.join(WORK, TARGET, "release")
    exe = os.path.join(out, "audiocraft")
    libs = needed_libraries(exe)
    print("needs:", ", ".join(libs))
    allowed = {"libasound.so.2", "libc.so.6", "libm.so.6", "libdl.so.2", "libpthread.so.0",
               "libgcc_s.so.1", "librt.so.1", "ld-linux-x86-64.so.2"}
    extra = [l for l in libs if l not in allowed]
    if extra:
        print("warning: also needs", ", ".join(extra))

    os.makedirs(DIST, exist_ok=True)
    shutil.copyfile(exe, os.path.join(DIST, NAME))
    files = [NAME]
    for name, src in [("README.md", os.path.join(APP, "README.md")),
                      ("LICENSE", os.path.join(APP, "LICENSE")),
                      ("COMMANDS.md", os.path.join(APP, "docs", "COMMANDS.md")),
                      ("audiocraft.png", os.path.join(out, "audiocraft.png")),
                      ("audiocraft.desktop", os.path.join(out, "audiocraft.desktop"))]:
        if os.path.exists(src):
            shutil.copyfile(src, os.path.join(DIST, name))
            files.append(name)
    tgz = os.path.join(DIST, NAME + ".tar.gz")
    with tarfile.open(tgz, "w:gz") as t:
        for name in files:
            info = t.gettarinfo(os.path.join(DIST, name), arcname=f"{NAME}/{name}")
            # Windows has no exec bit: set it in the archive so the program runs after unpacking
            info.mode = 0o755 if name == NAME else 0o644
            with open(os.path.join(DIST, name), "rb") as f:
                t.addfile(info, f)
    print(f"\nLinux build ready: {os.path.join(DIST, NAME)}")
    print(f"Archive: {tgz}")
    print("On Linux: tar xzf audiocraft-linux-x86_64.tar.gz && ./audiocraft-linux-x86_64/audiocraft-linux-x86_64")


if __name__ == "__main__":
    main()
