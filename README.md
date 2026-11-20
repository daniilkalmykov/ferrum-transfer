# Ferrum Transfer

**An asynchronous, concurrent TCP file-transfer server written in Rust.**

Ferrum Transfer is a learning-focused backend project built with **Tokio** and a custom binary protocol. It supports uploading and downloading files, verifies their integrity using **SHA-256**, limits resource usage, and handles interrupted transfers and server restarts.

The project is intentionally built on raw TCP rather than HTTP to explore async I/O, framing, backpressure, concurrency, cancellation, and failure recovery in Rust.

> **Project status:** Functional prototype / educational project. The included black-box suite has passed **26/26 integration, failure, and recovery tests** on macOS. This is **not** a hardened public file-hosting service: the protocol currently has no authentication, authorization, or TLS.

## Features

- **Asynchronous TCP server** using Tokio; multiple requests can be processed sequentially over a persistent connection.
- **Custom length-prefixed binary protocol** supporting `PING`, `UPLOAD`, and `DOWNLOAD`.
- **Streaming transfers** in 64 KiB chunks, without loading whole files into RAM.
- **SHA-256 integrity checks** for uploads and downloads.
- **Atomic, no-overwrite uploads:** temporary `.part` files are published using a hard link only after the checksum is validated.
- **Interrupted-transfer cleanup** with upload-session cleanup on failure/cancellation and stale `.part` removal at startup.
- **Concurrency controls** with separate limits for open connections and active transfers, plus a bounded wait for transfer permits.
- **Timeout protection** for idle connections, incomplete request frames (Slowloris-style clients), transfer progress, transfer queueing, and handler execution.
- **Graceful `Ctrl+C` shutdown** with a configurable grace period and cancellation of remaining tasks.
- **Structured logging** using `tracing`, including console output and daily rotating JSON log files.
- **Black-box E2E test suite** covering normal operations, concurrent transfers, protocol errors, timeouts, shutdown, and crash recovery.

## Tech stack

| Component | Technology |
| --- | --- |
| Language | Rust (edition 2024) |
| Async runtime and TCP | Tokio |
| Task tracking and cancellation | `tokio-util` (`TaskTracker`, `CancellationToken`) |
| Buffer management | `bytes` (`BytesMut`) |
| Hashing | `sha2` (SHA-256) |
| Async interfaces | `async-trait` |
| Logging | `tracing`, `tracing-subscriber`, `tracing-appender` |
| Upload IDs | `uuid` (v4) |
| Integration tests | Python 3 standard library |

## Getting started

**Requirements:** a Rust toolchain supporting edition 2024, Cargo, and (to run the E2E tests) Python 3.

```bash
# Clone your own repository, then enter the project directory.
cd ferrum-transfer

# Optional: create the default data and log directories.
mkdir -p storage logs

# Build and start the server.
cargo run --release
```

By default, the server listens on **`127.0.0.1:8080`**, saves completed files under **`./storage`**, and writes logs under **`./logs`**. Stop the server with **`Ctrl+C`**.

### Try a PING request

This project uses a **binary TCP protocol**, so an ordinary HTTP client or browser cannot communicate with it. With the server running in another terminal, use Python's standard library to send a PING frame:

```bash
python3 - <<'PY'
import socket
import struct

with socket.create_connection(("127.0.0.1", 8080)) as sock:
    # [u32 payload length = 0][u8 opcode = 0x00 / PING]
    sock.sendall(struct.pack(">IB", 0, 0x00))

    with sock.makefile("rb") as reader:
        header = reader.read(3)
        payload_length, status = struct.unpack(">HB", header)
        payload = reader.read(payload_length)

    print(f"status={status}, payload={payload!r}")
    # Expected: status=0, payload=b''
PY
```

## Configuration

Configuration is read from **environment variables** when the server starts. Time values are in **seconds**.

| Environment variable | Default | Purpose |
| --- | --- | --- |
| `SERVER_HOST` | `127.0.0.1` | Address to bind |
| `SERVER_PORT` | `8080` | Listening TCP port |
| `STORAGE_PATH` | `./storage` | Final file storage directory |
| `LOG_PATH` | `./logs` | JSON log directory |
| `MAX_CONNECTIONS` | `12` | Maximum simultaneously handled TCP connections |
| `MAX_CONCURRENT_TRANSFERS` | `4` | Maximum concurrent upload/download handlers |
| `QUEUE_TIMEOUT` | `5` | Max time waiting for an available transfer slot |
| `TRANSFER_TIMEOUT` | `5` | Per-operation transfer I/O timeout (see note) |
| `CONNECTION_TIMEOUT` | `5` | Idle TCP read timeout; also used for short control-response writes |
| `FRAME_DEADLINE` | `10` | Absolute time limit for receiving a request frame |
| `HANDLER_DEADLINE` | `120` | Maximum handler execution time |
| `GRACE_PERIOD` | `30` | Shutdown grace period before aborting remaining tasks |

Example:

```bash
SERVER_HOST=127.0.0.1 \
SERVER_PORT=9000 \
STORAGE_PATH=./storage \
LOG_PATH=./logs \
MAX_CONNECTIONS=12 \
MAX_CONCURRENT_TRANSFERS=4 \
QUEUE_TIMEOUT=5 \
TRASNFER_TIMEOUT=5 \
CONNECTION_TIMEOUT=5 \
FRAME_DEADLINE=10 \
HANDLER_DEADLINE=120 \
GRACE_PERIOD=30 \
cargo run --release
```

> **Configuration note:** The current source code reads **`TRASNFER_TIMEOUT`** (misspelled), **not `TRANSFER_TIMEOUT`**. Use the exact name shown above until it is corrected in `src/infrastructure/config.rs`.
>
> Several integer settings are range-checked; out-of-range values fall back to their defaults. See `config.rs` for exact validation rules.

## Wire protocol

Ferrum Transfer uses one persistent **TCP byte stream**. All integers in the protocol are **big-endian** (network byte order). TCP packet boundaries do **not** correspond to message boundaries: the server reassembles incomplete frames and can decode multiple frames from one receive buffer.

### Request frame

```text
+------------------------+-------------+----------------------------+
| Payload length (u32 BE)| Opcode (u8) | Payload (N bytes)          |
+------------------------+-------------+----------------------------+
         4 bytes              1 byte      N = payload length
```

The `payload length` does **not** include the opcode or the four-byte length prefix. Maximum control-frame payload: **1,024 bytes**. File content is transferred separately as raw bytes, not as individual request frames.

| Opcode | Value | Payload |
| --- | --- | --- |
| `PING` | `0x00` | Empty |
| `UPLOAD` | `0x01` | `[u16 filename length][UTF-8 filename][u64 file size]` |
| `DOWNLOAD` | `0x02` | `[u16 filename length][UTF-8 filename]` |

File names must be a **single path component**; path traversal and nested paths are rejected. The server rejects uploads larger than **20 GiB**.

### Response frame

```text
+------------------------+-------------+----------------------------+
| Payload length (u16 BE)| Status (u8) | Payload (N bytes)          |
+------------------------+-------------+----------------------------+
         2 bytes              1 byte      N = payload length
```

| Status | Value | Meaning |
| --- | --- | --- |
| `OK` | `0x00` | Operation succeeded |
| `READY` | `0x01` | Server is ready to transfer file bytes |
| `ERROR` | `0x02` | Operation failed |

An `ERROR` response payload is `[u16 error code BE][UTF-8 error message]`. Error code ranges registered in the executable are: `100–199` (protocol), `200–299` (storage), `300–399` (server), `400–499` (handler), `500–599` (executor), `600–699` (connection), and `900–999` (fallback). Not every number in these ranges is currently used.

### PING

```text
Client -> Server: [u32 0][0x00]
Server -> Client: [u16 0][OK]
```

### UPLOAD

```text
Client -> Server: UPLOAD request frame (filename + declared file size)
Server -> Client: READY response (empty payload)
Client -> Server: exactly <file size> raw bytes
Client -> Server: 32 raw bytes containing SHA-256(file contents)
Server -> Client: OK response (empty payload), or ERROR
```

The server writes incoming data to a unique temporary `.part` file. After checking the hash, it publishes the file without overwriting an existing destination. A hash mismatch aborts the upload and removes the temporary file.

### DOWNLOAD

```text
Client -> Server: DOWNLOAD request frame (filename)
Server -> Client: READY response with [u64 file size BE]
Server -> Client: exactly <file size> raw bytes
Server -> Client: OK response containing 32 raw SHA-256 bytes
```

The final download `OK` frame contains the checksum so the client can verify the received bytes.

**Important:** Upload/download data and checksums are **raw stream bytes**, whereas the command/response messages use the framed format. A client must read the announced file size exactly before parsing the next response frame.

## Architecture

```text
src/
├── main.rs                         # Dependency wiring and server startup
├── lib.rs                          # Public exports
├── errors/                         # Typed errors, macros, error-code registry
└── infrastructure/
    ├── config.rs                   # Environment-based configuration
    ├── logging.rs                  # Tracing setup and rotating JSON logs
    ├── network/
    │   ├── server.rs               # Listener, admission limits, shutdown
    │   ├── connection.rs           # TCP I/O, framing, deadlines, dispatch
    │   ├── context.rs              # Shared handler context
    │   ├── permission_guard.rs     # Timed transfer-semaphore acquisition
    │   ├── protocol/               # Opcodes, frames, status, encoding
    │   └── handlers/
    │       ├── ping.rs
    │       ├── upload.rs
    │       ├── download.rs
    │       ├── registry.rs          # Opcode -> handler mapping
    │       └── execution/          # Executors and executor registry
    └── storage/
        ├── file_storage.rs         # File-backed Storage implementation
        └── upload_session.rs      # Temporary file lifecycle and cleanup

tests/
└── test_ferrum_e2e.py              # Black-box server test suite
```

**Request processing flow:**

```text
TCP listener
   -> connection permit
   -> connection task + BytesMut / frame decoder
   -> opcode-based handler registry
   -> executor (immediate for PING, permit + deadline for transfers)
   -> PING / UPLOAD / DOWNLOAD handler
   -> Storage abstraction (file-backed implementation)
   -> protocol response
```

Key design decisions:

- **Separate semaphores:** one limits admitted connections; another limits active file transfers. Transfer requests wait for a bounded interval before receiving a busy error.
- **Executor abstraction:** `InstantExecutor` runs PING requests; `DeadlinePermissionExecutor` applies a transfer permit and overall handler deadline to UPLOAD/DOWNLOAD.
- **`Storage` trait:** handlers depend on a storage interface rather than directly managing filesystem paths.
- **RAII upload sessions:** abandoned temporary files are cleaned up on ordinary failure and in `Drop` where possible. At startup, the server scans `storage/temp` and removes leftover regular `.part` files.
- **Atomic publication:** an upload becomes visible at the final path through filesystem hard-link creation after checksum verification, preventing replacement of an existing file in the normal single-instance setup.
- **Task tracking:** `TaskTracker` and `CancellationToken` coordinate shutdown; permits are released when tasks finish or are dropped.

## Testing

Build the server first, then run the **black-box end-to-end suite** from the repository root:

```bash
cargo build
python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer'
```

The test harness runs the actual server executable using a **temporary storage directory and randomly selected localhost port**. It does not use mocked handlers or the project's internal Rust APIs.

Run specific groups:

```bash
python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer' --group smoke
python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer' --group failure
python3 tests/test_ferrum_e2e.py --command './target/debug/ferrum-transfer' --group recovery
```

To preserve logs and temporary files for debugging, add `--keep-artifacts`.

| Group | Cases | Highlights |
| --- | ---: | --- |
| Smoke/integration | 15 | Persistent/pipelined/fragmented PING, protocol errors, path validation, file round trips (0 B to 1 MiB), coalesced commands |
| Failure handling | 9 | SHA-256 mismatch, duplicate and concurrent uploads, disconnects, transfer timeout, idle timeout, Slowloris, connection limit, transfer queue limit |
| Recovery | 2 | `SIGINT` during upload, `SIGKILL` followed by startup cleanup |
| **Total** | **26** | **26 passed, 0 failed, 0 skipped in the reported macOS run** |

Additional Rust checks:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The `SIGINT`/`SIGKILL` recovery tests require a POSIX platform (for example, macOS or Linux); the test harness skips them on Windows.

## Operational notes and limitations

- **Do not expose this server directly to the public internet.** It currently does not implement authentication, per-user access control, or transport encryption. The default bind address is loopback-only for this reason.
- This is a **single-server, local-filesystem design**, not a distributed storage service. Concurrent server processes sharing a storage directory are not a supported deployment model.
- The startup cleanup assumes exclusive ownership of the configured temporary directory: it deletes regular `*.part` files in `STORAGE_PATH/temp`.
- The absolute handler deadline can terminate a long transfer even if individual chunks are making progress. Configure it with the expected workloads in mind.
- Graceful shutdown is wired to **`Ctrl+C` / SIGINT**. Other termination signals do not currently run the same explicit graceful-shutdown path.
- An atomic hard-link publication prevents accidental overwrite, but it is **not** a guarantee of data durability across sudden power loss (`fsync`/directory synchronization are not part of this implementation).
- There is no bundled general-purpose upload/download CLI; the protocol is documented above and exercised by the included Python E2E client/tests.

## Motivation

Ferrum Transfer was built as a hands-on Rust systems-programming project, focusing on correctness at the boundaries between **TCP streams, async tasks, filesystem state, and cancellation**. Its goal is to make the failure cases visible and testable rather than to be a production-ready alternative to SFTP or object storage.
