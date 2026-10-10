"""Inspect an alpha candidate's RPM and checksums without installing it.

Requires Python 3, rpm, and bsdtar (libarchive-tools on Ubuntu).
"""

import argparse
import configparser
import hashlib
import json
from pathlib import Path
import shlex
import stat
import struct
import subprocess
import tempfile


def require(condition, message):
    if not condition:
        raise SystemExit(message)


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("candidate", type=Path)
candidate = parser.parse_args().candidate.resolve()
manifest = json.loads((candidate / "manifest.json").read_text())
require(manifest["target"] == "x86_64-unknown-linux-gnu", "Expected Linux x86_64 candidate")
checksum_files = set()
for line in (candidate / "SHA256SUMS").read_text().splitlines():
    digest, filename = line.split("  ", 1)
    checksum_files.add(filename)
    require(
        hashlib.sha256((candidate / filename).read_bytes()).hexdigest() == digest,
        f"Checksum mismatch: {filename}",
    )
require(
    checksum_files == {item["file"] for item in manifest["artifacts"]}
    | {"manifest.json", "RELEASE-NOTES.md"},
    "Incomplete candidate checksums",
)

artifacts = [item for item in manifest["artifacts"] if item["file"].endswith(".rpm")]
require(len(artifacts) == 1, "Expected exactly one RPM in the manifest")
artifact = artifacts[0]
rpm = candidate / artifact["file"]
package_bytes = rpm.read_bytes()
require(len(package_bytes) == artifact["bytes"], "RPM size mismatch")
require(hashlib.sha256(package_bytes).hexdigest() == artifact["sha256"], "RPM hash mismatch")


def query(*args):
    return subprocess.check_output(["rpm", "-qp", *args, str(rpm)], text=True).strip()


metadata = query("--queryformat", "%{NAME}\n%{VERSION}\n%{RELEASE}\n%{ARCH}").splitlines()
require(
    metadata == ["aika", manifest["version"], "1", "x86_64"],
    f"Unexpected RPM metadata: {metadata}",
)
dependencies = query("--requires").splitlines()
for dependency in ["libgtk-3.so.0()(64bit)", "libwebkit2gtk-4.1.so.0()(64bit)"]:
    require(dependency in dependencies, f"Missing runtime dependency: {dependency}")

with tempfile.TemporaryDirectory(prefix="aika-rpm-") as temporary:
    root = Path(temporary)
    subprocess.run(["bsdtar", "-xf", str(rpm), "-C", str(root)], check=True)
    binary = root / "usr/bin/cal-desktop"
    require(
        binary.is_file() and stat.S_IMODE(binary.stat().st_mode) & 0o111 == 0o111,
        "Missing executable or execute permissions",
    )
    with binary.open("rb") as stream:
        header = stream.read(20)
    require(
        len(header) == 20
        and header[:6] == b"\x7fELF\x02\x01"
        and struct.unpack_from("<H", header, 18)[0] == 62,
        "Expected x86_64 ELF executable",
    )
    desktop = configparser.ConfigParser(interpolation=None)
    desktop.read(root / "usr/share/applications/aika.desktop")
    require(desktop.has_section("Desktop Entry"), "Missing desktop launcher")
    entry = desktop["Desktop Entry"]
    require(
        entry.get("Type") == "Application"
        and entry.get("Name") == "aika"
        and shlex.split(entry.get("Exec", ""))[:1] == ["cal-desktop"],
        "Invalid desktop launcher",
    )
    icon = entry.get("Icon", "")
    require(
        icon == "cal-desktop"
        and any((root / "usr/share/icons/hicolor").glob(f"*/apps/{icon}.png")),
        "Missing launcher icon",
    )

print(json.dumps({
    "rpm": str(rpm),
    "metadata": metadata,
    "dependencies": dependencies,
    "checksums": "passed",
    "executableLauncherAndIcons": "passed",
}, indent=2))
