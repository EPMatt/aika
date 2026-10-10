"""Inspect both executables inside a tagged release package on its native OS.

Windows installs NSIS into a disposable directory (run only on disposable CI).
Linux extracts deb/RPM/AppImage; macOS mounts the DMG read-only.
"""

import argparse
import configparser
import hashlib
import json
from pathlib import Path
import plistlib
import shlex
import stat
import struct
import subprocess
import sys
import tempfile


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def run(*args, cwd=None):
    return subprocess.check_output(list(map(str, args)), cwd=cwd).decode().strip()


def inspect(directory, target, version):
    extension = ".exe" if sys.platform == "win32" else ""
    for name in ["cal-desktop", "aika-cli"]:
        binary = directory / (name + extension)
        require(binary.is_file(), f"Missing bundled executable: {binary}")
        data = binary.read_bytes()
        if sys.platform == "win32":
            require(data[:2] == b"MZ", f"Expected PE executable: {binary}")
            offset = struct.unpack_from("<I", data, 60)[0]
            require(data[offset:offset + 4] == b"PE\0\0" and
                    struct.unpack_from("<H", data, offset + 4)[0] == 0x8664,
                    f"Expected x64 PE executable: {binary}")
        else:
            require(stat.S_IMODE(binary.stat().st_mode) & 0o111 == 0o111,
                    f"Missing executable permissions: {binary}")
            if sys.platform == "linux":
                require(data[:6] == b"\x7fELF\x02\x01" and
                        struct.unpack_from("<H", data, 18)[0] == 62,
                        f"Expected x64 ELF executable: {binary}")
            else:
                architecture = "arm64" if target.startswith("aarch64") else "x86_64"
                require(architecture in run("file", binary), f"Wrong Mach-O architecture: {binary}")
    require(run(directory / ("aika-cli" + extension), "--version") == f"aika-cli {version}",
            "Bundled CLI version differs from the release")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate", type=Path)
    args = parser.parse_args()
    candidate = args.candidate.resolve()
    manifests = list(candidate.glob("*.manifest.json"))
    require(len(manifests) == 1, "Expected one release manifest")
    manifest = json.loads(manifests[0].read_text())
    target, version = manifest["target"], manifest["version"]
    require(manifest["cli"] == {"bundled": True, "version": version}, "Expected bundled CLI")
    require((sys.platform == "linux" and target == "x86_64-unknown-linux-gnu") or
            (sys.platform == "win32" and target == "x86_64-pc-windows-msvc") or
            (sys.platform == "darwin" and target in ["aarch64-apple-darwin", "x86_64-apple-darwin"]),
            "Inspection requires the native release target")
    for artifact in manifest["artifacts"]:
        package = candidate / artifact["file"]
        data = package.read_bytes()
        require(len(data) == artifact["bytes"] and hashlib.sha256(data).hexdigest() == artifact["sha256"],
                f"Package checksum mismatch: {package}")
        with tempfile.TemporaryDirectory(prefix="aika-package-") as temporary:
            root = Path(temporary)
            if package.suffix == ".deb":
                subprocess.run(["dpkg-deb", "--extract", str(package), str(root)], check=True)
                require(run("dpkg-deb", "--field", package, "Version") == version, "Wrong deb version")
                inspect(root / "usr/bin", target, version)
            elif package.suffix == ".rpm":
                subprocess.run(["bsdtar", "-xf", str(package), "-C", str(root)], check=True)
                metadata = run("rpm", "-qp", "--queryformat", "%{NAME} %{VERSION} %{ARCH}", package)
                require(metadata == f"aika {version} x86_64", "Unexpected RPM metadata")
                dependencies = run("rpm", "-qp", "--requires", package).splitlines()
                for dependency in ["libgtk-3.so.0()(64bit)", "libwebkit2gtk-4.1.so.0()(64bit)"]:
                    require(dependency in dependencies, f"Missing RPM dependency: {dependency}")
                desktop = configparser.ConfigParser(interpolation=None)
                desktop.read(root / "usr/share/applications/aika.desktop")
                require(desktop.has_section("Desktop Entry"), "Missing RPM launcher")
                entry = desktop["Desktop Entry"]
                require(entry.get("Type") == "Application" and entry.get("Name") == "aika" and
                        shlex.split(entry.get("Exec", ""))[:1] == ["cal-desktop"] and
                        entry.get("Icon") == "cal-desktop", "Invalid RPM launcher")
                require(any((root / "usr/share/icons/hicolor").glob("*/apps/cal-desktop.png")), "Missing RPM icons")
                inspect(root / "usr/bin", target, version)
            elif package.suffix == ".AppImage":
                package.chmod(0o755)
                run(package, "--appimage-extract", cwd=root)
                inspect(root / "squashfs-root/usr/bin", target, version)
            elif package.suffix == ".dmg":
                mount = root / "mounted"
                mount.mkdir()
                run("hdiutil", "attach", "-readonly", "-nobrowse", "-mountpoint", mount, package)
                try:
                    app = mount / "aika.app"
                    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
                    require(info["CFBundleShortVersionString"] == version, "Wrong macOS app version")
                    run("codesign", "--verify", "--deep", "--strict", app)
                    inspect(app / "Contents/MacOS", target, version)
                finally:
                    run("hdiutil", "detach", mount)
            elif package.suffix == ".exe":
                install = root / "installed"
                # NSIS requires /D to be the final argument, without embedded quotes.
                subprocess.run([str(package), "/S", f"/D={install}"], check=True, timeout=180)
                try:
                    inspect(install, target, version)
                finally:
                    uninstall = install / "uninstall.exe"
                    if uninstall.exists():
                        subprocess.run([str(uninstall), "/S", f"_?={install}"], check=True, timeout=180)
            else:
                raise SystemExit(f"Unexpected release package: {package}")
        print(f"Verified desktop and CLI: {package.name}")


if __name__ == "__main__":
    main()
