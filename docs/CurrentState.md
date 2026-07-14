# TT-Sync: Current State (2026-07-14)

This document is a snapshot of what is **implemented** and what is **still pending** in the TT-Sync workspace.

## Implemented

### `ttsync-contract` (protocol/domain types)

- Strong wire types: `SyncPath`, `DeviceId`, `PlanId`, `SessionToken`, `PeerGrant`, `Permissions`.
- Dataset scope wire contract: `DatasetSelection`, `DATASET_POLICY_VERSION`, `dataset_scope_v1`.
- Per-operation overwrite contract: `OverwritePolicy::{Exact, PreferNewer}` and `overwrite_policy_v1`.
- v2 pairing URI: `PairUri` (`tauritavern://tt-sync/pair?...&spki=...`) with render + parse.
- v2 session headers constants: `TT-Device-Id`, `TT-Timestamp-Ms`, `TT-Nonce`, `TT-Signature`.
- Canonical request format: `CanonicalRequest` (string form + parse).
- v2 sync requests: `PullPlanRequest`, `PushPlanRequest`, `CommitResponse`. Plan requests accept an optional `overwrite_policy` field that defaults to `Exact`.

### `ttsync-core` (use-cases)

- `compute_plan()` algorithm (incremental + mirror) with unit tests.
- `PreferNewer` skips a changed same-path entry only when the target mtime is strictly newer; Mirror deletion of target-only paths is unchanged.
- `DatasetPolicy` resolves stable dataset ids and public profile ids into scan roots, files, path predicates, exclusions, runtime eligibility, and scope-aware delete boundaries.
- `compute_plan_for_policy()` validates source/target manifests against the selected dataset before diffing.
- `validate_plan_scope()` verifies remote plans before a client applies transfers or deletes.
- Shared bundle framing helpers and capability constants (`bundle_v1`, `zstd_v1`).
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
- Manifest scanning: `scan_manifest(mounts, policy)` produces `ManifestV2` using canonical **wire paths** and the selected DatasetPolicy.
- TauriTavern Agent run history scanning only includes terminal runs (`completed`, `partial_success`, `cancelled`, `failed`); active run directories and run-index files stay out of manifests.
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
  - `GET  /v2/plans/{plan_id}/bundle` (download bundle for pull plans)
  - `PUT  /v2/plans/{plan_id}/bundle` (upload bundle for push plans)
  - `POST /v2/plans/{plan_id}/commit` (applies mirror deletes on server for push plans)
- Server-side plan state is stored in-memory with a 30-minute TTL.
- `/v2/status` advertises `dataset_scope_v1`, `overwrite_policy_v1`, `dataset_policy_version`, supported leaf dataset ids, supported profile ids, and default dataset ids.
- Pull/push plan requests carry `DatasetSelection` and the logical initiator's overwrite policy; server scanning, manifest validation, transfer plans, and mirror deletes are scoped to the request rather than server configuration.

#### Client (TLS pinning)

- `SyncClient::new(base_url, Some(spki_sha256))` configures reqwest with a custom rustls verifier that:
  - Ignores hostname / issuer / expiry.
  - Accepts the server only if SPKI hash matches the pinned value.
  - Still verifies TLS handshake signatures (delegated to `WebPkiServerVerifier`).

### `ttsync-client` (shared client engine)

- `ClientSyncEngine` implements reusable pull and direct-push orchestration for native clients:
  - status feature check + `dataset_scope_v1` enforcement
  - conditional `overwrite_policy_v1` enforcement when `PreferNewer` is selected
  - Ed25519 session open
  - permission checks for read/write/mirror delete
  - local scan via `ClientWorkspace`
  - pull/push plan request
  - DatasetPolicy plan-scope validation before applying changes
  - bundle/zstd transfer when supported, with per-file fallback
  - mirror delete handling and push commit
- `ClientWorkspace` has a blanket implementation for any `ManifestStore`, and lets native adapters report whether a failed write/delete may have changed local state.
- `SyncObserver` reports progress without depending on Tauri events, CLI progress bars, or any UI runtime.
- End-to-end client test spins up the real HTTPS server and exercises bundle+zstd pull and push.

### `tt-sync` (presentation layer) — **newly implemented**

#### Architecture

- `config.rs`: Configuration management with `config.toml` (workspace path, layout mode, public URL, listen address) and `identity.json` (device UUID, Ed25519 key pair, device name). State directory resolved via `--state-dir` flag → `TT_SYNC_STATE_DIR` env → platform-local-data. Config path defaults next to the executable, with a CLI-only `--config-file` override.
- `output.rs`: Zero-dependency ANSI styling with `Style` struct that respects `--no-color` flag and `NO_COLOR` env var.
- `main.rs`: Global flags (`--no-color`, `--quiet`, `--state-dir`, `--config-file`), styled clap help, conditional tracing initialization (only for `serve` or when `RUST_LOG` is set), colored error display. TUI entrypoints intentionally ignore `--config-file`.

### Docker deployment assets

- `Dockerfile`: multi-stage Linux image build with a `scratch` runtime and baked-in `--state-dir /state --config-file /state/config.toml` entrypoint.
- `docker-compose.yaml`: default local deployment shape with bind-mounted state and workspace paths.
- `.env.example`: compose variables for container name, port, state path, and workspace mount.
- `config.toml.example`: headless-friendly config template for container and CLI deployments.

#### Subcommands

| Command | Status | Notes |
|---------|--------|-------|
| `tt-sync init` | ✅ | Creates state dir, config file, identity.json, TLS cert+key. Validates workspace path exists. Prints derived mount points. Rejects re-init for the selected config path. |
| `tt-sync serve` | ✅ | Loads config from the default path or CLI `--config-file`, derives mount points, starts TLS HTTPS server via `ttsync-http::spawn_server()`, prints server banner, Ctrl+C graceful shutdown. |
| `tt-sync background-serve` | ✅ (internal) | Windows-only hidden background entrypoint for Task Scheduler. Reuses the same server runtime and hides the console window after successful startup. |
| `tt-sync pair open` | ✅ | Generates one-time token + pair URI via `ttsync-core::pairing`, and persists the token into a file-based token store so a running server can consume it. Supports `--json`, `--ro`, `--mirror`, `--expires`. |
| `tt-sync peers list` | ✅ | Reads `JsonPeerStore`, displays formatted table (`comfy-table`). Supports `--json`. |
| `tt-sync peers revoke` | ✅ | Matches by device ID, prefix, or name (case-insensitive). |
| `tt-sync doctor` | ✅ | Validates state dir, the selected config path, mount derivation, identity, TLS cert, peers.json. Styled ✓/!/✗ indicators. |
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
  - Service mode: user-scope service manager (Linux `systemd --user`, macOS `LaunchAgent`, Windows `Task Scheduler` beta) or in-process foreground server, then a final summary screen.
- Main menu screens implemented:
  - **Pairing**: generates QR/link, live peer list, per-device permission confirmation after pairing.
  - **Peers**: list paired devices, edit permissions, revoke peers.
  - **Serve**: start/stop the server in-process; on Linux, install/enable/start/stop a `systemd --user` service; on macOS, register/bootstrap/bootout a `LaunchAgent`; on Windows, register/start/stop/query a per-user `Task Scheduler` task that launches the hidden `background-serve` entrypoint (**beta**).

## Pending / Next Milestones

- `tt-sync`:
  - TUI onboarding flow: finish the final summary/exit polishing.
- Cross-platform service management:
  - Strategy is documented in `docs/ServiceManagement.md`.
  - Current scope is explicitly **user-scope only**: Linux `systemd --user`, macOS `LaunchAgent`, Windows Task Scheduler.
  - Linux `systemd --user`, macOS `LaunchAgent`, and Windows `Task Scheduler` (**beta**) are implemented.
- TauriTavern integration:
  - Wire TauriTavern to `ttsync-client`: implement its `ClientWorkspace`, map `SyncObserver` to Tauri events, refresh AppState caches after successful local changes, and persist TT-Sync pairing/config state.
- Tests:
  - Add TauriTavern adapter integration tests and focused failure-path tests for partial local apply reporting.

## Verification

From `TT-Sync/`:

```bash
cargo test
```

Run `cargo test` from the workspace root; unit tests cover dataset policy, plan diffing, bundle framing, HTTP body limits, TLS helpers, pairing/session primitives, and shared client pull/push orchestration.
