"""Bounded Linux host-mechanism probes, not Forge's production supervisor."""
import fcntl
import multiprocessing
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest


def contender(directory, channel, start):
    fd = os.open(Path(directory) / "owner.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        channel.send("ready")
        if not start.wait(5):
            raise TimeoutError("Start barrier timed out")
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            channel.send("lost-before-open")
            return
        with sqlite3.connect(Path(directory) / "runtime.sqlite") as db:
            db.execute("CREATE TABLE IF NOT EXISTS admissions (owner INTEGER)")
            db.execute("INSERT INTO admissions VALUES (?)", (os.getpid(),))
        channel.send("owns-and-opened")
        if not channel.poll(5):
            raise TimeoutError("Release barrier timed out")
        assert channel.recv() == "release"
    finally:
        os.close(fd)
        channel.close()


class OwnershipProbe(unittest.TestCase):
    def test_race_fences_writable_open_and_reuses_persistent_inode(self):
        with tempfile.TemporaryDirectory(prefix="forge-t01-lock-") as directory:
            lock = Path(directory) / "owner.lock"
            lock.write_text("stale pid 999999 is not authority\n")
            inode = lock.stat().st_ino
            ctx = multiprocessing.get_context("spawn")
            start = ctx.Event()
            channels = [ctx.Pipe() for _ in range(2)]
            processes = [ctx.Process(target=contender, args=(directory, child, start))
                         for _, child in channels]
            try:
                for process in processes:
                    process.start()
                for parent, child in channels:
                    child.close()
                    self.assertTrue(parent.poll(5), "missing ready signal")
                    self.assertEqual(parent.recv(), "ready")
                start.set()
                results = []
                for parent, _ in channels:
                    self.assertTrue(parent.poll(5), "missing admission result")
                    results.append(parent.recv())
                self.assertCountEqual(results, ["lost-before-open", "owns-and-opened"])
                with sqlite3.connect(Path(directory) / "runtime.sqlite") as db:
                    self.assertEqual(db.execute("SELECT COUNT(*) FROM admissions").fetchone()[0], 1)
                channels[results.index("owns-and-opened")][0].send("release")
                for process in processes:
                    process.join(5)
                    self.assertEqual(process.exitcode, 0)
                fd = os.open(lock, os.O_RDWR | os.O_NOFOLLOW)
                try:
                    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    self.assertEqual(os.fstat(fd).st_ino, inode)
                finally:
                    os.close(fd)
            finally:
                for process in processes:
                    if process.is_alive():
                        process.kill()
                    process.join(5)
                for parent, _ in channels:
                    parent.close()

    def test_owner_lock_release_does_not_prove_child_exit(self):
        with tempfile.TemporaryDirectory(prefix="forge-t01-child-") as directory:
            lock = Path(directory) / "owner.lock"
            fd = os.open(lock, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            # The child intentionally does NOT inherit the ownership descriptor.
            # It stays alive after the runner's ownership descriptor is closed.
            child = subprocess.Popen([sys.executable, "-c", "import sys; print('ready', flush=True); sys.stdin.read()"],
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            try:
                # communicate with a bounded timeout below; readiness is not a sleep.
                import select
                self.assertTrue(select.select([child.stdout], [], [], 5)[0])
                self.assertEqual(child.stdout.readline().strip(), "ready")
                os.close(fd)
                fd = None
                replacement_fd = os.open(lock, os.O_RDWR | os.O_NOFOLLOW)
                try:
                    fcntl.flock(replacement_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    self.assertIsNone(child.poll(), "child must still be alive")
                    # Acquiring this lock cannot authorize replacement. Forge must
                    # retain its process/effect blockers independently of the lock.
                finally:
                    os.close(replacement_fd)
                child.communicate(timeout=5)
                self.assertEqual(child.returncode, 0)
            finally:
                if fd is not None:
                    os.close(fd)
                if child.poll() is None:
                    child.kill()
                child.communicate(timeout=5)

    def test_external_supervisor_can_stop_stalled_runner_and_group(self):
        # Simulates the OS supervisor receiving fake-clock expiry, independently
        # of the runner's blocked event loop. No claim about escaped descendants.
        import select
        import signal
        child_code = "import time; print('child-ready', flush=True); time.sleep(60)"
        runner_code = ("import subprocess,sys; "
                       f"subprocess.Popen([sys.executable, '-c', {child_code!r}]); "
                       "print('runner-ready', flush=True); "
                       "exec('while True: pass')")
        runner = subprocess.Popen([sys.executable, "-c", runner_code],
                                  start_new_session=True, stdout=subprocess.PIPE)
        try:
            # Read unbuffered bytes so select cannot miss data already buffered
            # by TextIOWrapper while the child is starting.
            ready = b""
            while ready.count(b"\n") < 2:
                self.assertTrue(select.select([runner.stdout], [], [], 5)[0])
                chunk = os.read(runner.stdout.fileno(), 4096)
                self.assertTrue(chunk, "group exited before readiness")
                ready += chunk
            self.assertIn(b"runner-ready", ready)
            self.assertIn(b"child-ready", ready)
            os.killpg(runner.pid, signal.SIGTERM)
            runner.wait(timeout=5)
            self.assertEqual(runner.returncode, -signal.SIGTERM)
            self.assertTrue(select.select([runner.stdout], [], [], 5)[0])
            self.assertEqual(os.read(runner.stdout.fileno(), 4096), b"")
        finally:
            try:
                os.killpg(runner.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            runner.wait(timeout=5)
            runner.stdout.close()

    def test_symlink_lock_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix="forge-t01-symlink-") as directory:
            target = Path(directory) / "target"
            target.write_text("do not mutate")
            lock = Path(directory) / "owner.lock"
            lock.symlink_to(target)
            with self.assertRaises(OSError):
                os.open(lock, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
            self.assertEqual(target.read_text(), "do not mutate")


if __name__ == "__main__":
    unittest.main(verbosity=2)
