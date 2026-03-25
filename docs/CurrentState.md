# TT-Sync: Current State (2026-03-24)

This document is a snapshot of what is **implemented** and what is **still pending** in the TT-Sync workspace.

## Implemented

### `ttsync-contract` (protocol/domain types)

- Strong wire types: `SyncPath`, `DeviceId`, `PlanId`, `SessionToken`, `PeerGrant`, `Permissions`.
- v2 pairing URI: `PairUri` (`tauritavern://tt-sync/pair?...&spki=...`) with render + parse.
- v2 session headers constants: `TT-Device-Id`, `TT-Timestamp-Ms`, `TT-Nonce`, `TT-Signature`.
- Canonical request format: `CanonicalRequest` (string form + parse).
- v2 sync requests: `PullPlanRequest`, `PushPlanRequest`, `CommitResponse`.

### `ttsync-core` (use-cases)

- `compute_plan()` algorithm (incremental + mirror) with unit tests.
- Pairing use-case:
  - `create_pairing_session(public_url, spki, config) -> (PairingSession, PairUri)`
  - `complete_pairing(session, req, server_id, server_name) -> (PeerGrant, PairCompleteResponse)`
- Session use-case:
  - `SessionManager::open_session(...)` verifies Ed25519 signature over canonical request + timestamp window + nonce replay prevention, then issues a short-lived session token.
  - `SessionManager::validate_session(...)` validates bearer token and returns device id.
- Ports:
  - `ManifestStore` (scan/read/write/delete)
  - `PeerStore` (get/save/remove/list)

### `ttsync-fs` (filesystem adapters)

- Wire↔local mapping: `LayoutMode` + `WorkspaceMounts` + `resolve_to_local()`.
- Manifest scanning: `scan_manifest(mounts)` produces `ManifestV2` using the canonical **wire paths** and the v2 dataset allowlist.
- `FsManifestStore` implements `ttsync-core::ports::ManifestStore`.
- Atomic writes + mtime preservation: `writer::write_file_atomic(...)` and `writer::delete_file(...)`.
- `JsonPeerStore` persists peers in `<state-dir>/peers.json` and implements `ttsync-core::ports::PeerStore`.

### `ttsync-http` (HTTP + TLS adapters)

#### TLS

- `SelfManagedTls::load_or_create(state_dir)`:
  - Uses `<state-dir>/tls/key.pem` + `<state-dir>/tls/cert.pem` (created if missing).
  - Computes `spki_sha256` (base64url(SHA256(SPKI DER))).
  - Builds a `rustls::ServerConfig` (TLS 1.3, ALPN `h2` + `http/1.1`).

#### Server

- `spawn_server(addr, tls_provider, state)` runs an HTTPS server (axum + axum-server) and returns a `ServerHandle` with graceful shutdown.
- Implemented endpoints (v2):
  - `GET /v2/status`
  - `POST /v2/pair/complete?token=...` (consumes one-time token, saves peer grant)
  - `POST /v2/session/open` (Ed25519-signed canonical request via headers)
  - `POST /v2/sync/pull-plan` (requires `Authorization: Bearer <session_token>`)
  - `POST /v2/sync/push-plan` (requires `Authorization: Bearer <session_token>`)
  - `GET  /v2/plans/{plan_id}/files/{path_b64}` (download for pull plans)
  - `PUT  /v2/plans/{plan_id}/files/{path_b64}` (upload for push plans)
  - `POST /v2/plans/{plan_id}/commit` (applies mirror deletes on server for push plans)
- Server-side plan state is stored in-memory with a 30-minute TTL.

#### Client (TLS pinning)

- `SyncClient::new(base_url, Some(spki_sha256))` configures reqwest with a custom rustls verifier that:
  - Ignores hostname / issuer / expiry.
  - Accepts the server only if SPKI hash matches the pinned value.
  - Still verifies TLS handshake signatures (delegated to `WebPkiServerVerifier`).

### `ttsync-cli` (presentation layer) — **newly implemented**

#### Architecture

- `config.rs`: Configuration management with `config.toml` (workspace path, layout mode, public URL, listen address) and `identity.json` (device UUID, Ed25519 key pair, device name). State directory resolved via `--state-dir` flag → `TT_SYNC_STATE_DIR` env → platform-local-data.
- `output.rs`: Zero-dependency ANSI styling with `Style` struct that respects `--no-color` flag and `NO_COLOR` env var.
- `main.rs`: Global flags (`--no-color`, `--quiet`, `--state-dir`), styled clap help, conditional tracing initialization (only for `serve` or when `RUST_LOG` is set), colored error display.

#### Subcommands

| Command | Status | Notes |
|---------|--------|-------|
| `tt-sync init` | ✅ | Creates state dir, config.toml, identity.json, TLS cert+key. Validates workspace path exists. Prints derived mount points. Rejects re-init. |
| `tt-sync serve` | ✅ | Loads config, derives mount points, starts TLS HTTPS server via `ttsync-http::spawn_server()`, prints server banner, Ctrl+C graceful shutdown. |
| `tt-sync pair open` | ✅ | Generates one-time token + pair URI via `ttsync-core::pairing`, and persists the token into a file-based token store so a running server can consume it. Supports `--json`, `--ro`, `--mirror`, `--expires`. |
| `tt-sync peers list` | ✅ | Reads `JsonPeerStore`, displays formatted table (`comfy-table`). Supports `--json`. |
| `tt-sync peers revoke` | ✅ | Matches by device ID, prefix, or name (case-insensitive). |
| `tt-sync doctor` | ✅ | Validates state dir, config.toml, mount derivation, identity, TLS cert, peers.json. Styled ✓/!/✗ indicators. |
| `tt-sync cert show` | ✅ | Displays SPKI SHA-256 fingerprint, file paths, mode. |
| `tt-sync cert rotate-leaf` | ✅ | Re-signs cert with existing key (via `rcgen`), confirms SPKI pin unchanged. |

#### Output Modes

- **Default**: Styled ANSI output with colored labels, success/warning/error indicators.
- **`--quiet`**: Machine-readable bare output (e.g., just the pair URI string).
- **`--json`**: JSON output for `pair open` and `peers list`.
- **`--no-color`**: Disables ANSI escape codes; also auto-detected via `NO_COLOR` env / `TERM=dumb`.

#### TUI (ratatui)

- `tt-sync` (no args) enters a full-screen TUI main menu.
- `tt-sync onboard` runs a guided flow (implemented so far):
  - Language → listen port → public URL → layout mode → workspace detection/confirm → pair-now decision.
  - Pairing screen (optional): QR/link + live peers list + per-device permission confirmation, then “pair more / next step”.
  - Service mode: user-scope service manager (Linux `systemd --user`, macOS `LaunchAgent`) or in-process foreground server, then a final summary screen.
- Main menu screens implemented:
  - **Pairing**: generates QR/link, live peer list, per-device permission confirmation after pairing.
  - **Peers**: list paired devices, edit permissions, revoke peers.
  - **Serve**: start/stop the server in-process; on Linux, install/enable/start/stop a `systemd --user` service; on macOS, register/bootstrap/bootout a `LaunchAgent`.

## Pending / Next Milestones

- `ttsync-cli`:
  - TUI onboarding flow: finish the final summary/exit polishing and add Windows Task Scheduler support for user-scope auto-start.
- Cross-platform service management:
  - Strategy is documented in `docs/ServiceManagement.md`.
  - Current scope is explicitly **user-scope only**: Linux `systemd --user`, macOS `LaunchAgent`, Windows Task Scheduler.
  - macOS `LaunchAgent` is implemented; Windows Task Scheduler remains pending.
- TauriTavern integration:
  - Client-side manifest scan + plan apply orchestration (pull/push loops) and progress events.
- Tests:
  - Add integration tests that spin up the HTTPS server and exercise session + plan + file transfer end-to-end.

## Verification

From `TT-Sync/`:

```bash
cargo test
```

All unit tests pass as of this snapshot (13/13).
