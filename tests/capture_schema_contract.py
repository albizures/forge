"""Capture DDL only from an explicitly supplied SQLite snapshot (never row data).

Usage: python3 tests/capture_schema_contract.py SNAPSHOT OUTPUT.sql
Review and commit the output when introducing a supported schema contract.
"""
import sqlite3
import sys
from pathlib import Path

source, destination = map(Path, sys.argv[1:])
connection = sqlite3.connect(source.resolve().as_uri() + "?mode=ro", uri=True)
revision = connection.execute("PRAGMA user_version").fetchone()[0]
statements = connection.execute(
    "SELECT sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' "
    "ORDER BY CASE WHEN type='table' THEN 0 ELSE 1 END, name"
).fetchall()
content = f"-- Frozen schema {revision} DDL. No user data or seed values.\n"
content += "\n".join(statement.rstrip(";") + ";" for (statement,) in statements) + "\n"
destination.write_text(content)
