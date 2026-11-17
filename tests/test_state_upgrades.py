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
        for schema in (7, 8, 9, 10, 11, 12, 13):
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
                    self.command("forge", "record", "start", identity, "--workspace", "personal")
                    self.command("forge", "record", "done", identity, "--workspace", "personal", "--resolution", "Ordinary historical closure")
                    before = self.command("forge", "record", "show", identity, "--workspace", "personal", "--json").stdout
                    self.assertNotIn("forced_completion", json.loads(before))
                    history = self.command("forge", "record", "updates", identity, "--workspace", "personal", "--json").stdout
                    comments = self.command("forge", "record", "comments", identity, "--workspace", "personal", "--json").stdout
                    # Running managed daemons must block execution even if the DB is otherwise eligible.
                    self.command("forgeup", "upgrade", "prepare", "--from", "old", "--to", "new", "--confirm-agents-stopped", success=False)
                    self.command("forgeup", "daemon", "stop")
                    source = home / "data/state" / selectors["old"] / "forge.db"
                    # Construct historical fixtures only in this private test store.
                    # Remove each additive table so the marker matches its full contract.
                    with sqlite3.connect(source) as connection:
                        for introduced, table in (
                            (12, "builds_on_dependencies"),
                            (11, "forced_completions"),
                            (10, "task_unchanged_results"),
                            (9, "ordering_only_dependencies"),
                            (8, "task_stacks"),
                        ):
                            if schema < introduced:
                                connection.execute(f"DROP TABLE {table}")
                        connection.execute(f"PRAGMA user_version={schema}")
                        if schema == 11:
                            # Frozen legacy ambiguity, not a manual operator repair.
                            connection.execute("INSERT INTO projects VALUES('legacy',?,'git@example:legacy','personal','legacy')", (str(self.workspace / 'legacy'),))
                            connection.executescript("""
                                INSERT INTO records(id,record_type,body,status,workspace_id) VALUES
                                (100,'spec','Legacy Spec','open','personal'),
                                (101,'task','Blocked','open','personal'),
                                (102,'task','Prerequisite','open','personal');
                                INSERT INTO record_projects VALUES(100,'legacy'),(101,'legacy'),(102,'legacy');
                                INSERT INTO record_relationships(from_id,to_id,parent_id,child_id,kind) VALUES
                                (101,100,100,101,'parent-child'),(102,100,100,102,'parent-child');
                                INSERT INTO record_relationships(from_id,to_id,kind) VALUES(101,102,'depends-on');
                            """)
                        if schema in (12, 13):
                            # Valid incomplete Open planning state: canonical lineage
                            # exists before either Task has execution configuration.
                            connection.execute("INSERT INTO projects VALUES('planned',?,'git@example:planned','personal','planned')", (str(self.workspace / 'planned'),))
                            connection.executescript("""
                                INSERT INTO records(id,record_type,body,status,workspace_id) VALUES
                                (201,'task','Planned successor','open','personal'),
                                (202,'task','Unconfigured root','open','personal');
                                INSERT INTO record_projects VALUES(201,'planned'),(202,'planned');
                                INSERT INTO record_relationships(from_id,to_id,kind) VALUES(201,202,'depends-on');
                                INSERT INTO builds_on_dependencies(from_id,to_id) VALUES(201,202);
                            """)
                    # A stopped dispatcher can retain an unexpired scan lease.
                    # Synchronize to its recorded expiry rather than resetting evidence.
                    with sqlite3.connect(source) as connection:
                        expiry = connection.execute("SELECT COALESCE(MAX(scan_lease_until_millis),0) FROM agent_dispatch_configuration").fetchone()[0]
                    threading.Event().wait(max(0, expiry / 1000 - time.time() + 0.01))
                    source_bytes = source.read_bytes()
                    plan = json.loads(self.command("forgeup", "upgrade", "plan", "--from", "old", "--to", "new").stdout)
                    self.assertEqual(plan["blockers"], [])
                    self.assertEqual(plan["migrations"], [[version, version + 1] for version in range(schema, 13)])
                    decisions = []
                    if schema == 11:
                        failed = self.command("forgeup", "upgrade", "prepare", "--from", "old", "--to", "new", "--confirm-agents-stopped", success=False)
                        self.assertIn("101 -> 102", failed.stderr)
                        self.assertEqual(source.read_bytes(), source_bytes)
                        self.command("forgeup", "upgrade", "activate", "--to", "new", "--confirm-agents-stopped", success=False)
                        self.command("forgeup", "use", "new", success=False)
                        self.command("forgeup", "upgrade", "recover", "--to", "new")
                        decision_file = self.workspace / "decisions.json"
                        decision_file.write_text(json.dumps([dict(from_id=101, to_id=102)]))
                        decisions = ["--waits-for-decisions", str(decision_file)]
                    receipt = json.loads(self.command("forgeup", "upgrade", "prepare", "--from", "old", "--to", "new", "--confirm-agents-stopped", *decisions).stdout)
                    if schema == 11:
                        self.assertEqual(receipt["waits_for_decisions"], [dict(from_id=101, to_id=102)])
                        retry = json.loads(self.command("forgeup", "upgrade", "prepare", "--from", "old", "--to", "new", "--confirm-agents-stopped", *decisions).stdout)
                        self.assertEqual(receipt, retry)
                    destination = home / "data/state" / selectors["new"] / "forge.db"
                    with sqlite3.connect(destination) as connection:
                        self.assertEqual(connection.execute("PRAGMA user_version").fetchone()[0], 13)
                        self.assertEqual(connection.execute("SELECT COUNT(*) FROM forced_completions").fetchone()[0], 0)
                        if schema in (12, 13):
                            self.assertEqual(connection.execute("SELECT from_id,to_id FROM builds_on_dependencies").fetchall(), [(201, 202)])
                            self.assertEqual(connection.execute("SELECT COUNT(*) FROM task_stacks").fetchone()[0], 0)
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
