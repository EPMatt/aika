"""Production Linux startup/restart check in disposable XDG directories.

Requires Python 3 and a desktop session. This is not a CRUD or upgrade test.
"""

import json
import os
from pathlib import Path
import signal
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time

if sys.platform != "linux" or len(sys.argv) < 2:
    raise SystemExit(
        "Usage: python3 scripts/linux-release-smoke.py "
        "<binary-or-AppImage> [runtime arguments]"
    )

binary = str(Path(sys.argv[1]).resolve())
results = []
with tempfile.TemporaryDirectory(prefix="aika-release-") as temporary:
    directory = Path(temporary)
    env = dict(
        os.environ,
        XDG_DATA_HOME=str(directory / "data"),
        XDG_CONFIG_HOME=str(directory / "config"),
        XDG_CACHE_HOME=str(directory / "cache"),
        AIKA_TEST_DATA_DIR=str(directory / "must-not-be-used"),
    )
    installed = directory / Path(binary).name
    for attempt in range(2):
        # Portable installation and same-version file replacement, never a downgrade.
        shutil.copy2(binary, installed)
        installed.chmod(0o755)
        with open(directory / "launch.log", "w+") as log:
            app = subprocess.Popen(
                [str(installed), *sys.argv[2:]],
                env=env,
                stdout=log,
                stderr=log,
                start_new_session=True,
            )
            try:
                database = (
                    directory / "data/dev.projectcal.desktop/calendar.sqlite3"
                )
                for _ in range(150):
                    if app.poll() is not None:
                        log.seek(0)
                        raise RuntimeError("Production app exited: " + log.read())
                    if database.exists():
                        break
                    time.sleep(0.1)
                assert database.exists(), "No production SQLite startup evidence"
                time.sleep(2)
                assert app.poll() is None, "Production app exited after startup"
                with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as db:
                    record = {
                        "launch": attempt + 1,
                        "schema": db.execute("PRAGMA user_version").fetchone()[0],
                        "events": db.execute("SELECT count(*) FROM events").fetchone()[0],
                        "calendars": db.execute("SELECT * FROM calendars").fetchall(),
                        "preferences": db.execute(
                            "SELECT payload FROM preferences"
                        ).fetchone()[0],
                    }
                assert record["schema"] == 1 and record["events"] == 0
                assert record["calendars"] == [("local-default", "Personal")]
                with socket.socket() as connection:
                    connection.settimeout(0.5)
                    assert connection.connect_ex(("127.0.0.1", 4445)) != 0, (
                        "Unexpected listener on test-driver port"
                    )
                assert not (directory / "must-not-be-used").exists(), (
                    "Production used test directory override"
                )
                results.append(record)
            finally:
                if app.poll() is None:
                    os.killpg(app.pid, signal.SIGTERM)
                    app.wait(timeout=10)
    assert results[0]["preferences"] == results[1]["preferences"]

print(json.dumps({
    "binary": binary,
    "isolatedProductionStartupAndRestart": results,
    "sameVersionPortableReplacement": binary.endswith(".AppImage"),
    "driverPortClosed": True,
    "testDirectoryIgnored": True,
}, indent=2))
