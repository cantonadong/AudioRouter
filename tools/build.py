"""Build AudioRouter and replace the single user testing executable."""
from pathlib import Path
import os
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
TOOLCHAIN = "stable-x86_64-pc-windows-gnu"


def run(args, **kwargs):
    subprocess.run(args, cwd=ROOT, check=True, **kwargs)


def main():
    os.chdir(ROOT)
    run([sys.executable, "tools/make_resources.py"])
    rustup = shutil.which("rustup")
    if not rustup:
        raise RuntimeError("rustup not found; add the installed Rust cargo/bin to PATH")
    sysroot = Path(subprocess.check_output([rustup, "run", TOOLCHAIN, "rustc", "--print", "sysroot"], text=True).strip())
    candidates = [sysroot / "lib/rustlib/x86_64-pc-windows-gnu/bin/self-contained/x86_64-w64-mingw32-gcc.exe",
                  sysroot / "lib/rustlib/x86_64-pc-windows-gnu/bin/gcc.exe", sysroot / "bin/gcc.exe"]
    gcc = next((file for file in candidates if file.is_file()), None)
    if gcc is None:
        raise RuntimeError("GNU linker missing. Install rust-mingw for the Rust GNU toolchain.")
    env = os.environ.copy()
    env["PATH"] = str(gcc.parent) + os.pathsep + env["PATH"]
    env["CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER"] = str(gcc)
    env["RUSTFLAGS"] = env.get("RUSTFLAGS", "") + " -C link-self-contained=yes"
    run([rustup, "run", TOOLCHAIN, "cargo", "build", "--release", "--offline", "--target", "x86_64-pc-windows-gnu"], env=env)
    # User preference: stop existing instances before overwriting the fixed EXE.
    run(["powershell.exe", "-NoProfile", "-Command", "try { Get-Process -Name AudioRouter,WinSpeaker -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction Stop; exit 0 } catch { Write-Error $_; exit 1 }"])
    source = ROOT / "target/x86_64-pc-windows-gnu/release/AudioRouter.exe"
    destination = ROOT / "AudioRouter.exe"
    shutil.copy2(source, destination)
    legacy = ROOT / "WinSpeaker.exe"
    if legacy.is_file():
        legacy.unlink()
    run([str(destination), "--compact-config"])
    run(["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(ROOT / "tools/sign.ps1")])
    print(f"Build complete: {destination} ({destination.stat().st_size:,} bytes)")
    print("Native audio build. No tests were run.")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, subprocess.CalledProcessError) as error:
        print(f"Build failed: {error}", file=sys.stderr)
        sys.exit(1)
