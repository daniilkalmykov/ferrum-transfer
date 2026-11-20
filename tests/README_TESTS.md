# Ferrum Transfer — Integration and Failure Tests

This test suite exercises a **real, running TCP server** through its public binary protocol. It does not import any Rust server code and requires no additional Python packages.

## How to Run

Place the `tests/` directory in the root of your Rust project, then run the following commands from the project root:

```bash
cargo build
python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer'
```

If your executable has a different name, update the path passed to `--command`. It is best to build the binary **separately** with `cargo build` rather than using `cargo run`: the failure tests need to send signals directly to the server process under test. On Windows, the path will be `target/debug/ferrum-transfer.exe`, and the SIGINT/SIGKILL tests are skipped.

You can also run individual test groups:

```bash
python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer' --group smoke
python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer' --group failure
python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer' --group recovery
```

To preserve logs and temporary storage when tests fail, use:

```bash
python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer' --keep-artifacts
```

Each scenario reports `PASS`, `FAIL`, or `SKIP`, followed by a summary. The exit code is 0 if there are no failures and 1 if any tests fail.

## Test Coverage

- **Smoke (15):** PING over a single connection, 100 PING requests in one TCP packet, a fragmented frame, invalid opcodes and UTF-8, an oversized frame, path traversal protection, a nonexistent file, UPLOAD/DOWNLOAD with SHA-256 verification for sizes 0, 1, 65535, 65536, 65537, and 1 MiB, and the sequence `UPLOAD + raw bytes + SHA256 + PING` sent in a single `sendall()` call.
- **Failure (9):** Incorrect SHA-256, prevention of overwriting an existing completed file, concurrent uploads using the same filename, interrupted upload, upload timeout, idle connection timeout, Slowloris, exceeding MAX_CONNECTIONS, and transfer queuing.
- **Recovery (2, macOS/Linux):** SIGINT during UPLOAD followed by shutdown and cleanup; SIGKILL during UPLOAD followed by a fresh start and cleanup of `.part` files without damaging an already stored file.

## Safety and Limitations

- The server runs on `127.0.0.1` using an available local port; **STORAGE_PATH and LOG_PATH point to a separate temporary directory**, so existing user data is not affected.
- For restarts, tests reuse the same temporary storage directory but use a new port each time.
- Do not run these tests against someone else's process or a production process: the script starts and terminates **its own** child process.
- MAX_CONNECTIONS and timeout isolation are configured through environment variables defined by `Config` in the supplied Rust `src`.
- These scenarios validate observable behavior. They do not replace `cargo test`, `cargo clippy`, sanitizers, property-based/fuzz testing, or load tests on a real network.
- This environment does not have `cargo`/`rustc`, so the actual Rust test scenarios **were not executed here**. The Python script was checked for syntax errors.

## Project Completion Criteria

1. `cargo test` and `cargo clippy --all-targets -- -D warnings` complete without errors.
2. All required smoke/failure/recovery tests report PASS (on macOS/Linux).
3. All identified defects are either fixed or documented as known, accepted project limitations.
4. The protocol and startup procedure are documented in the README, followed by the `v1.0.0` release tag.
