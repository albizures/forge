"""Real CLI/daemon upgrade boundary; run after building forgeup/forge/forge-server.

All subprocesses use a private FORGEUP_HOME. No installed registry is touched.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get("FORGE_TEST_BIN_DIR", ROOT / "target/release"))


class StateUpgradeTests(unittest.TestCase):
    def command(self, program, *arguments, success=True):
        result = subprocess.run(
            [str(BIN / program), *arguments], env=self.environment,
            cwd=self.workspace, text=True, capture_output=True, timeout=30,
            check=False,
        )
        if success:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def test_preserves_real_records_across_schema_and_same_schema_upgrades(self):
        for schema in (7, 8):
            with self.subTest(schema=schema), tempfile.TemporaryDirectory(prefix="forge-upgrade-e2e-") as temporary:
                self.workspace = Path(temporary).resolve()
                home = self.workspace / "forgeup"
                self.environment = {
                    key: value for key, value in os.environ.items()
                    if not key.startswith(("FORGE_", "FORGEUP_"))
                }
                self.environment["FORGEUP_HOME"] = str(home)
                try:
                    selectors = {}
                    for label in ("old", "new"):
                        selectors[label] = self.command(
                            "forgeup", "register", "--source-label", label,
                            "--cli", str(BIN / "forge"), "--daemon", str(BIN / "forge-server"),
                            "--local-build-alias", label,
                        ).stdout.strip()
                    self.command("forgeup", "use", "old")
                    self.command("forgeup", "daemon", "start")
                    self.command("forge", "workspace", "add", "personal", "--root", str(self.workspace))
                    record = json.loads(self.command("forge", "record", "open", "--workspace", "personal", "--no-projects", "--body", "Preserve my record", "--json").stdout)
                    identity = str(record["id"])
                    self.command("forge", "record", "comment", identity, "--workspace", "personal", "--message", "Preserve my comment")
                    before = self.command("forge", "record", "show", identity, "--workspace", "personal", "--json").stdout
                    history = self.command("forge", "record", "updates", identity, "--workspace", "personal", "--json").stdout
                    comments = self.command("forge", "record", "comments", identity, "--workspace", "personal", "--json").stdout
                    # Running managed daemons must block execution even if the DB is otherwise eligible.
                    self.command("forgeup", "upgrade", "prepare", "--from", "old", "--to", "new", "--confirm-agents-stopped", success=False)
                    self.command("forgeup", "daemon", "stop")
                    source = home / "data/state" / selectors["old"] / "forge.db"
                    if schema == 7:
                        with sqlite3.connect(source) as connection:
                            connection.executescript("DROP TABLE task_stacks; PRAGMA user_version=7;")
                    # A stopped dispatcher can retain an unexpired scan lease.
                    # Synchronize to its recorded expiry rather than resetting evidence.
                    with sqlite3.connect(source) as connection:
                        expiry = connection.execute("SELECT COALESCE(MAX(scan_lease_until_millis),0) FROM agent_dispatch_configuration").fetchone()[0]
                    threading.Event().wait(max(0, expiry / 1000 - time.time() + 0.01))
                    source_bytes = source.read_bytes()
                    plan = json.loads(self.command("forgeup", "upgrade", "plan", "--from", "old", "--to", "new").stdout)
                    self.assertEqual(plan["blockers"], [])
                    self.assertEqual(plan["migrations"], [[7, 8]] if schema == 7 else [])
                    self.command("forgeup", "upgrade", "prepare", "--from", "old", "--to", "new", "--confirm-agents-stopped")
                    self.assertEqual(source.read_bytes(), source_bytes)
                    self.command("forgeup", "use", "new", success=False)
                    self.command("forgeup", "upgrade", "activate", "--to", "new", "--confirm-agents-stopped")
                    # Ordinary Forge commands start only the now-activated target.
                    self.assertEqual(self.command("forge", "record", "show", identity, "--workspace", "personal", "--json").stdout, before)
                    self.assertEqual(self.command("forge", "record", "updates", identity, "--workspace", "personal", "--json").stdout, history)
                    self.assertEqual(self.command("forge", "record", "comments", identity, "--workspace", "personal", "--json").stdout, comments)
                    self.command("forgeup", "use", "old", success=False)
                    self.assertEqual(source.read_bytes(), source_bytes)
                    self.command("forgeup", "daemon", "stop")
                    self.command("forgeup", "upgrade", "cleanup", "--to", "new")
                    self.command("forgeup", "use", "old", success=False)
                    self.command("forgeup", "use", "old", "--acknowledge-divergence")
                finally:
                    self.command("forgeup", "daemon", "stop")


if __name__ == "__main__":
    unittest.main()
