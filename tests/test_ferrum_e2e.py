#!/usr/bin/env python3
"""Ferrum Transfer black-box integration, failure and recovery tests.

Runs the ACTUAL server executable with a private storage directory on localhost.
No non-standard Python packages. Does not use the project's internal Rust APIs.

Example (from the Rust project root):
    cargo build
    python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer'

Run individual groups:
    python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer' --group smoke
    python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer' --group failure
    python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer' --group recovery

On Windows, SIGINT and SIGKILL subprocess tests are skipped; normal tests still run.
"""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import shlex
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time
import uuid

PING, UPLOAD, DOWNLOAD = 0, 1, 2
OK, READY, ERROR = 0, 1, 2
HOST = "127.0.0.1"
READ_TIMEOUT = 8.0
TRANSFER_TIMEOUT = 2
CONNECTION_TIMEOUT = 3
FRAME_DEADLINE = 6
MAX_CONNECTIONS = 4


class TestFailure(AssertionError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise TestFailure(message)


def recv_exact(conn: socket.socket, length: int) -> bytes:
    require(length >= 0, "negative receive length")
    buf = bytearray()
    while len(buf) < length:
        chunk = conn.recv(length - len(buf))
        if not chunk:
            raise TestFailure(f"premature EOF: wanted {length} bytes, received {len(buf)}")
        buf.extend(chunk)
    return bytes(buf)


def recv_response(conn: socket.socket) -> tuple[int, bytes]:
    header = recv_exact(conn, 3)
    payload_len, status = struct.unpack(">HB", header)
    require(status in (OK, READY, ERROR), f"invalid response status {status}")
    payload = recv_exact(conn, payload_len)
    if status == ERROR:
        require(len(payload) >= 2, "error response lacks u16 error code")
    return status, payload


def frame(opcode: int, payload: bytes = b"") -> bytes:
    return struct.pack(">IB", len(payload), opcode) + payload


def filename_payload(name: str) -> bytes:
    encoded = name.encode("utf-8")
    require(0 < len(encoded) <= 65535, "invalid test filename")
    return struct.pack(">H", len(encoded)) + encoded


def upload_frame(name: str, size: int) -> bytes:
    return frame(UPLOAD, filename_payload(name) + struct.pack(">Q", size))


def download_frame(name: str) -> bytes:
    return frame(DOWNLOAD, filename_payload(name))


def sha(data: bytes) -> bytes:
    return hashlib.sha256(data).digest()


def pick_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind((HOST, 0))
        return int(s.getsockname()[1])


def wait_until(predicate, *, timeout: float = 3.0, interval: float = .03) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(interval)
    return bool(predicate())


class Server:
    def __init__(self, command: list[str], cwd: Path, root: Path):
        self.command = command
        self.cwd = cwd
        self.root = root
        self.storage = root / "storage"
        self.log_dir = root / "logs"
        self.storage.mkdir(parents=True, exist_ok=True)
        self.log_dir.mkdir(parents=True, exist_ok=True)
        self.proc: subprocess.Popen | None = None
        self.log_handle = None
        self.port = 0
        self.round = 0

    def start(self, overrides: dict[str, str] | None = None) -> None:
        require(self.proc is None, "server already started")
        self.round += 1
        self.port = pick_port()
        env = os.environ.copy()
        env.update({
            "SERVER_HOST": HOST,
            "SERVER_PORT": str(self.port),
            "STORAGE_PATH": str(self.storage),
            "LOG_PATH": str(self.log_dir),
            "MAX_CONNECTIONS": str(MAX_CONNECTIONS),
            "MAX_CONCURRENT_TRANSFERS": "2",
            "QUEUE_TIMEOUT": "2",
            "TRANSFER_TIMEOUT": str(TRANSFER_TIMEOUT),
            "CONNECTION_TIMEOUT": str(CONNECTION_TIMEOUT),
            "FRAME_DEADLINE": str(FRAME_DEADLINE),
            "HANDLER_DEADLINE": "60",
            "GRACE_PERIOD": "20",
        })
        if overrides:
            env.update(overrides)
        self.log_handle = (self.root / f"server-{self.round:02d}.log").open("wb")
        try:
            kwargs = {"start_new_session": True} if os.name == "posix" else {}
            self.proc = subprocess.Popen(
                self.command, cwd=self.cwd, env=env,
                stdout=self.log_handle, stderr=subprocess.STDOUT, **kwargs
            )
        except Exception:
            self.log_handle.close()
            self.log_handle = None
            raise
        try:
            self.wait_ready()
        except BaseException:
            self.stop()
            raise

    def wait_ready(self) -> None:
        deadline = time.monotonic() + 25.0
        while time.monotonic() < deadline:
            if self.proc is None or self.proc.poll() is not None:
                raise TestFailure(f"server exited during start; logs:\n{self.log_tail()}")
            try:
                with socket.create_connection((HOST, self.port), timeout=.25):
                    # Probe uses one connection slot briefly; it closes immediately.
                    return
            except OSError:
                time.sleep(.12)
        raise TestFailure(f"server did not listen within 25s; logs:\n{self.log_tail()}")

    def open(self, timeout: float = READ_TIMEOUT) -> socket.socket:
        require(self.proc is not None and self.proc.poll() is None, "server not running")
        conn = socket.create_connection((HOST, self.port), timeout=timeout)
        conn.settimeout(timeout)
        return conn

    def stop(self, *, force: bool = False, graceful: bool = False) -> int | None:
        proc = self.proc
        if proc is None:
            return None
        try:
            if proc.poll() is None:
                if os.name == "posix":
                    sig = signal.SIGKILL if force else (signal.SIGINT if graceful else signal.SIGTERM)
                    os.killpg(proc.pid, sig)
                else:
                    if force:
                        proc.kill()
                    elif graceful:
                        # No portable Ctrl+C signal delivery to a new Windows console group.
                        raise TestFailure("graceful SIGINT test requires POSIX")
                    else:
                        proc.terminate()
            proc.wait(timeout=12 if graceful else 4)
        except subprocess.TimeoutExpired:
            if os.name == "posix":
                os.killpg(proc.pid, signal.SIGKILL)
            else:
                proc.kill()
            proc.wait(timeout=4)
            if graceful:
                raise TestFailure("graceful shutdown exceeded 12 seconds")
        finally:
            self.proc = None
            if self.log_handle is not None:
                self.log_handle.close()
                self.log_handle = None
        return proc.returncode

    def log_tail(self, chars: int = 3500) -> str:
        path = self.root / f"server-{self.round:02d}.log"
        if not path.exists():
            return "(no log file)"
        return path.read_text(errors="replace")[-chars:]

    def part_files(self) -> list[Path]:
        temp = self.storage / "temp"
        return list(temp.glob("*.part")) if temp.exists() else []


class Suite:
    def __init__(self, server: Server):
        self.s = server
        self.passed = 0
        self.failed = 0
        self.skipped = 0
        self.failures: list[str] = []

    def run(self, name: str, method, *, group: str, selected: str) -> None:
        if selected != "all" and selected != group:
            return
        try:
            method()
        except NotImplementedError as exc:
            self.skipped += 1
            print(f"SKIP [{group}] {name}: {exc}", flush=True)
        except Exception as exc:
            self.failed += 1
            detail = f"{name}: {type(exc).__name__}: {exc}"
            self.failures.append(detail)
            print(f"FAIL [{group}] {detail}", flush=True)
        else:
            self.passed += 1
            print(f"PASS [{group}] {name}", flush=True)

    def unique(self, prefix: str) -> str:
        return f"{prefix}-{uuid.uuid4().hex[:12]}.bin"

    def assert_ping(self, conn: socket.socket) -> None:
        conn.sendall(frame(PING))
        got = recv_response(conn)
        require(got == (OK, b""), f"PING: expected OK/empty, got {got}")

    def begin_upload(self, conn: socket.socket, name: str, size: int) -> None:
        conn.sendall(upload_frame(name, size))
        status, payload = recv_response(conn)
        require((status, payload) == (READY, b""), f"UPLOAD READY: got {(status, payload)!r}")

    def finish_upload(self, conn: socket.socket, data: bytes, digest: bytes | None = None) -> tuple[int, bytes]:
        conn.sendall(data + (digest if digest is not None else sha(data)))
        return recv_response(conn)

    def assert_no_part(self, name: str, timeout: float = 3.5) -> None:
        # Part paths include timestamp and UUID after the user-supplied filename.
        require(wait_until(lambda: not any(p.name.startswith(name + "(") for p in self.s.part_files()), timeout=timeout),
                f"temporary .part for {name} survived: {self.s.part_files()}")

    def test_ping_reuse(self) -> None:
        with self.s.open() as conn:
            for _ in range(4):
                self.assert_ping(conn)

    def test_ping_pipeline(self) -> None:
        with self.s.open() as conn:
            conn.sendall(frame(PING) * 100)
            for i in range(100):
                require(recv_response(conn) == (OK, b""), f"pipelined PING #{i} failed")

    def test_fragmented_frame(self) -> None:
        with self.s.open() as conn:
            for byte in frame(PING):
                conn.sendall(bytes([byte]))
                time.sleep(.015)
            require(recv_response(conn) == (OK, b""), "fragmented PING failed")
            self.assert_ping(conn)

    def test_invalid_opcode(self) -> None:
        with self.s.open() as conn:
            conn.sendall(frame(255))
            require(recv_response(conn)[0] == ERROR, "unknown opcode not rejected")
            require(conn.recv(1) == b"", "malformed opcode connection not closed")

    def test_oversized_frame(self) -> None:
        with self.s.open() as conn:
            conn.sendall(struct.pack(">I", 1025))  # MAX_FRAME_SIZE=1024; no body required
            require(recv_response(conn)[0] == ERROR, "oversized frame not rejected")
            require(conn.recv(1) == b"", "oversized frame connection not closed")

    def test_invalid_utf8(self) -> None:
        with self.s.open() as conn:
            conn.sendall(frame(DOWNLOAD, b"\x00\x01\xff"))
            require(recv_response(conn)[0] == ERROR, "invalid UTF-8 not rejected")

    def test_invalid_path_then_ping(self) -> None:
        with self.s.open() as conn:
            conn.sendall(upload_frame("../escape.txt", 10))
            require(recv_response(conn)[0] == ERROR, "path traversal not rejected")
            self.assert_ping(conn)
        require(not (self.s.root / "escape.txt").exists(), "path traversal created external file")

    def test_missing_download_then_ping(self) -> None:
        with self.s.open() as conn:
            conn.sendall(download_frame(self.unique("missing")))
            require(recv_response(conn)[0] == ERROR, "missing download not rejected")
            self.assert_ping(conn)

    def roundtrip(self, size: int) -> None:
        name = self.unique(f"roundtrip{size}")
        data = os.urandom(size)
        with self.s.open() as conn:
            self.begin_upload(conn, name, size)
            for offset in range(0, size, 5003):
                conn.sendall(data[offset:offset + 5003])
            conn.sendall(sha(data))
            require(recv_response(conn) == (OK, b""), f"UPLOAD {size} final status not OK")
            self.assert_ping(conn)
        require((self.s.storage / name).read_bytes() == data, "uploaded bytes changed on disk")
        with self.s.open() as conn:
            conn.sendall(download_frame(name))
            status, payload = recv_response(conn)
            require(status == READY and len(payload) == 8, f"DOWNLOAD READY malformed: {status} {payload!r}")
            got_size = struct.unpack(">Q", payload)[0]
            require(got_size == size, f"DOWNLOAD expected {size}, got {got_size}")
            downloaded = recv_exact(conn, size)
            final_status, received_hash = recv_response(conn)
            require(final_status == OK and received_hash == sha(data), "DOWNLOAD hash/status incorrect")
            require(downloaded == data, "DOWNLOAD bytes changed")
            self.assert_ping(conn)
        self.assert_no_part(name)

    def test_coalesced_upload_and_ping(self) -> None:
        name = self.unique("coalesced")
        data = os.urandom(9_001)
        with self.s.open() as conn:
            # Deliberately send meta + file + SHA + next frame in ONE write.
            conn.sendall(upload_frame(name, len(data)) + data + sha(data) + frame(PING))
            require(recv_response(conn) == (READY, b""), "coalesced upload did not return READY")
            require(recv_response(conn) == (OK, b""), "coalesced upload final OK missing")
            require(recv_response(conn) == (OK, b""), "PING after coalesced upload failed")
        require((self.s.storage / name).read_bytes() == data, "coalesced uploaded file corrupted")

    def test_bad_sha_and_reuse(self) -> None:
        name = self.unique("badsha")
        data = os.urandom(9001)
        bad = b"\x00" * 32
        if sha(data) == bad:
            bad = b"\xff" * 32
        with self.s.open() as conn:
            self.begin_upload(conn, name, len(data))
            require(self.finish_upload(conn, data, bad)[0] == ERROR, "mismatched SHA accepted")
            self.assert_ping(conn)
        require(not (self.s.storage / name).exists(), "invalid file was published")
        self.assert_no_part(name)

    def test_duplicate_not_overwritten(self) -> None:
        name = self.unique("duplicate")
        initial = b"original" * 500
        with self.s.open() as conn:
            self.begin_upload(conn, name, len(initial))
            require(self.finish_upload(conn, initial) == (OK, b""), "first upload failed")
            conn.sendall(upload_frame(name, 4))
            require(recv_response(conn)[0] == ERROR, "duplicate upload unexpectedly allowed")
            self.assert_ping(conn)
        require((self.s.storage / name).read_bytes() == initial, "duplicate overwrote original file")
        self.assert_no_part(name)

    def test_parallel_same_name(self) -> None:
        name = self.unique("race")
        first = os.urandom(130_000)
        second = os.urandom(130_000)
        with self.s.open() as a, self.s.open() as b:
            self.begin_upload(a, name, len(first))
            self.begin_upload(b, name, len(second))
            got_a = self.finish_upload(a, first)[0]
            got_b = self.finish_upload(b, second)[0]
            require((got_a, got_b) == (OK, ERROR), f"atomic publish expected OK/ERROR, got {got_a}/{got_b}")
        require((self.s.storage / name).read_bytes() == first, "parallel loser corrupted winner's file")
        self.assert_no_part(name)

    def test_disconnect_during_upload(self) -> None:
        name = self.unique("eof")
        conn = self.s.open()
        try:
            self.begin_upload(conn, name, 1_000_000)
            conn.sendall(os.urandom(1900))
        finally:
            conn.close()
        require(not (self.s.storage / name).exists(), "partial file published after disconnect")
        self.assert_no_part(name)

    def test_upload_timeout(self) -> None:
        name = self.unique("stall")
        with self.s.open(timeout=TRANSFER_TIMEOUT + 5) as conn:
            self.begin_upload(conn, name, 10_000)
            status, _ = recv_response(conn)
            require(status == ERROR, "stalled upload did not receive fatal ERROR")
            require(conn.recv(1) == b"", "stalled upload connection remains open")
        require(not (self.s.storage / name).exists(), "stalled upload published file")
        self.assert_no_part(name)

    def test_idle_timeout(self) -> None:
        with self.s.open(timeout=CONNECTION_TIMEOUT + 4) as conn:
            require(conn.recv(1) == b"", "idle connection not closed")

    def test_slowloris(self) -> None:
        with self.s.open(timeout=FRAME_DEADLINE + 4) as conn:
            # 1000-byte frame payload length; never complete; keep feeding each second.
            conn.sendall(struct.pack(">IB", 1000, PING))
            for _ in range(FRAME_DEADLINE - 1):
                time.sleep(1.0)
                try:
                    conn.sendall(b"\x00")
                except (BrokenPipeError, ConnectionResetError):
                    break
            status, _ = recv_response(conn)
            require(status == ERROR, "Slowloris did not trigger SlowConnection response")
            require(conn.recv(1) == b"", "Slowloris connection not closed")

    def test_connection_limit_and_recovery(self) -> None:
        sockets: list[socket.socket] = []
        try:
            for _ in range(MAX_CONNECTIONS):
                sockets.append(self.s.open(timeout=2))
            time.sleep(.15)
            with self.s.open(timeout=1.4) as extra:
                try:
                    extra.sendall(frame(PING))
                    got = extra.recv(1)
                    require(got == b"", "connection over max limit received data")
                except (ConnectionResetError, BrokenPipeError):
                    pass  # TCP RST is also a valid rejection.
        finally:
            for conn in sockets:
                conn.close()
        def can_ping():
            try:
                with self.s.open(timeout=.5) as conn:
                    self.assert_ping(conn)
                    return True
            except (OSError, TestFailure):
                return False
        require(wait_until(can_ping, timeout=3), "connection permits were not released")

    def test_transfer_queue_timeout(self) -> None:
        # A different server config ensures queued transfer times out BEFORE active transfers.
        self.s.stop()
        self.s.start({"TRANSFER_TIMEOUT": "7", "QUEUE_TIMEOUT": "2"})
        first = second = third = None
        try:
            first = self.s.open(timeout=6)
            second = self.s.open(timeout=6)
            third = self.s.open(timeout=6)
            self.begin_upload(first, self.unique("held-a"), 400_000)
            self.begin_upload(second, self.unique("held-b"), 400_000)
            third.sendall(upload_frame(self.unique("queued"), 1024))
            status, _ = recv_response(third)
            require(status == ERROR, "third transfer should be rejected after queue_timeout")
        finally:
            for conn in (first, second, third):
                if conn is not None:
                    conn.close()
            self.s.stop()
            self.s.start()
        require(wait_until(lambda: not self.s.part_files(), timeout=3), "stalled transfer parts survived shutdown/restart")

    def test_graceful_shutdown_active_upload(self) -> None:
        if os.name != "posix":
            raise NotImplementedError("SIGINT test requires macOS/Linux")
        name = self.unique("graceful")
        conn = self.s.open(timeout=3)
        try:
            self.begin_upload(conn, name, 200_000)
            conn.sendall(b"a" * 1234)
            rc = self.s.stop(graceful=True)
            require(rc == 0, f"graceful server exit code={rc}")
        finally:
            conn.close()
            if self.s.proc is not None:
                self.s.stop()
            self.s.start()
        require(not (self.s.storage / name).exists(), "graceful shutdown published incomplete upload")
        self.assert_no_part(name)
        with self.s.open() as conn:
            self.assert_ping(conn)

    def test_crash_and_recovery(self) -> None:
        if os.name != "posix":
            raise NotImplementedError("SIGKILL recovery test requires macOS/Linux")
        name = self.unique("crash")
        # Keep a committed file; it must not disappear on restart.
        committed = self.unique("committed")
        (self.s.storage / committed).write_bytes(b"safe-committed-data")
        conn = self.s.open()
        try:
            self.begin_upload(conn, name, 500_000)
            require(any(p.name.startswith(name + "(") for p in self.s.part_files()), ".part file missing before crash")
            conn.sendall(b"z" * 8192)
            rc = self.s.stop(force=True)
            require(rc is not None and rc != 0, "SIGKILL unexpectedly exited cleanly")
        finally:
            conn.close()
            if self.s.proc is not None:
                self.s.stop()
        # Stale .part may still be present immediately after SIGKILL; startup must clean it.
        self.s.start()
        require(not (self.s.storage / name).exists(), "incomplete upload was published on restart")
        self.assert_no_part(name)
        require((self.s.storage / committed).read_bytes() == b"safe-committed-data", "restart damaged committed file")
        with self.s.open() as live:
            self.assert_ping(live)


def run_suite(suite: Suite, selected: str) -> None:
    cases = [
        ("persistent PING", suite.test_ping_reuse, "smoke"),
        ("100 pipelined PING frames", suite.test_ping_pipeline, "smoke"),
        ("fragmented PING frame", suite.test_fragmented_frame, "smoke"),
        ("unknown opcode -> ERROR + EOF", suite.test_invalid_opcode, "smoke"),
        ("oversized frame -> ERROR + EOF", suite.test_oversized_frame, "smoke"),
        ("invalid UTF-8 frame", suite.test_invalid_utf8, "smoke"),
        ("path traversal rejection + socket reuse", suite.test_invalid_path_then_ping, "smoke"),
        ("missing DOWNLOAD + socket reuse", suite.test_missing_download_then_ping, "smoke"),
        ("UPLOAD/DOWNLOAD roundtrip 0 B", lambda: suite.roundtrip(0), "smoke"),
        ("UPLOAD/DOWNLOAD roundtrip 1 B", lambda: suite.roundtrip(1), "smoke"),
        ("UPLOAD/DOWNLOAD roundtrip 65535 B", lambda: suite.roundtrip(65535), "smoke"),
        ("UPLOAD/DOWNLOAD roundtrip 65536 B", lambda: suite.roundtrip(65536), "smoke"),
        ("UPLOAD/DOWNLOAD roundtrip 65537 B", lambda: suite.roundtrip(65537), "smoke"),
        ("UPLOAD/DOWNLOAD roundtrip 1 MiB", lambda: suite.roundtrip(1024 * 1024), "smoke"),
        ("coalesced UPLOAD + SHA256 + PING", suite.test_coalesced_upload_and_ping, "smoke"),
        ("bad SHA256: rollback + socket reuse", suite.test_bad_sha_and_reuse, "failure"),
        ("duplicate upload cannot overwrite", suite.test_duplicate_not_overwritten, "failure"),
        ("parallel same-name uploads are atomic", suite.test_parallel_same_name, "failure"),
        ("disconnect during UPLOAD cleans temp", suite.test_disconnect_during_upload, "failure"),
        ("stalled UPLOAD timeout + cleanup", suite.test_upload_timeout, "failure"),
        ("idle connection timeout", suite.test_idle_timeout, "failure"),
        ("Slowloris trickle triggers deadline", suite.test_slowloris, "failure"),
        ("MAX_CONNECTIONS and permit recovery", suite.test_connection_limit_and_recovery, "failure"),
        ("transfer semaphore queue timeout", suite.test_transfer_queue_timeout, "failure"),
        ("SIGINT during UPLOAD + cleanup", suite.test_graceful_shutdown_active_upload, "recovery"),
        ("SIGKILL during UPLOAD + startup recovery", suite.test_crash_and_recovery, "recovery"),
    ]
    for name, test, group in cases:
        suite.run(name, test, group=group, selected=selected)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--command", required=True,
                        help="compiled server binary path; e.g. './target/debug/ferrum-transfer'")
    parser.add_argument("--cwd", default=".", help="server working directory / Rust project root")
    parser.add_argument("--group", choices=("all", "smoke", "failure", "recovery"), default="all")
    parser.add_argument("--keep-artifacts", action="store_true", help="keep storage and logs after tests")
    args = parser.parse_args()

    cwd = Path(args.cwd).expanduser().resolve()
    require(cwd.is_dir(), f"--cwd does not exist: {cwd}")
    command = shlex.split(args.command)
    require(bool(command), "--command is empty")
    # Relative executable paths are evaluated against cwd, not the test script folder.
    def execute(root: Path) -> int:
        s = Server(command, cwd, root)
        suite = Suite(s)
        try:
            s.start()
            print(f"Ferrum E2E: {len(command)}-part command, {HOST}:{s.port}, isolated storage", flush=True)
            run_suite(suite, args.group)
        except Exception as exc:
            suite.failed += 1
            suite.failures.append(f"harness: {type(exc).__name__}: {exc}")
            print(f"FAIL [harness] {type(exc).__name__}: {exc}", flush=True)
        finally:
            s.stop()

        print(f"\nSUMMARY: {suite.passed} PASS, {suite.failed} FAIL, {suite.skipped} SKIP", flush=True)
        if suite.failures:
            print("\nFailures:")
            for item in suite.failures:
                print("  -", item)
            print("\nLast server log excerpt:\n" + s.log_tail())
            print(f"\nRun artifacts: {root}")
        return 1 if suite.failed else 0

    if args.keep_artifacts:
        root = Path(tempfile.mkdtemp(prefix="ferrum-e2e-"))
        print(f"Artifacts: {root}")
        return execute(root)
    with tempfile.TemporaryDirectory(prefix="ferrum-e2e-") as tmp:
        return execute(Path(tmp))


if __name__ == "__main__":
    sys.exit(main())
