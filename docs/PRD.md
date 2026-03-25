# TT-Sync — Product Requirements Document

## 1. Problem Statement

TauriTavern's existing **LAN Sync** feature enables synchronization of user data between devices on the same local network. While widely appreciated, users have consistently expressed a need for **remote synchronization** — the ability to sync data with folders hosted on external infrastructure such as a VPS, NAS, or home server accessible over the public internet.

LAN Sync's current architecture (plain HTTP, HMAC shared-secret auth, "notify peer to pull" model) is fundamentally unsuitable for public-internet deployment:

- **No transport encryption** — data travels in cleartext.
- **No initiator-driven upload** — the "push = tell peer to pull" model assumes mutual reachability, which breaks when clients are behind NAT/CGNAT.
- **Hardcoded sync scope** — the set of synced directories is scattered across source code, making it impossible to extend or contract without code changes.
- **Coupled to Tauri runtime** — the sync engine directly holds `AppHandle` and emits Tauri-specific events, preventing reuse in a standalone context.

## 2. Product Vision

**TT-Sync** is a standalone, headless Rust command-line utility whose sole purpose is to serve as a remote synchronization endpoint for TauriTavern (and, optionally, SillyTavern). Users deploy it on infrastructure they control, pair it with their TauriTavern clients, and synchronize data over an encrypted channel.

### Core Principles

1. **CLI-first** — TT-Sync is a server-mode daemon managed entirely via the command line. No GUI.
2. **Deploy anywhere** — Must run on Linux (x86_64, aarch64), Windows, and macOS with zero runtime dependencies beyond the binary itself.
3. **Secure by default** — All communication over TLS 1.3. Device identity via Ed25519. No cleartext fallback.
4. **Protocol as contract** — The v2 protocol is a versioned, stable interface shared between TT-Sync and TauriTavern, not an implementation detail buried in either codebase.
5. **Minimal, composable** — Synchronization means "single-source-of-truth replication" (pull or push). Not conflict resolution, not CRDT, not real-time collaboration.

## 3. Target Users & Deployment Scenarios

| Persona | Scenario | Key Concern |
|---------|----------|-------------|
| Power user with VPS | Deploys TT-Sync on a cloud VPS. Syncs TauriTavern on phone ↔ VPS ↔ desktop. | Easy setup, reliable over unreliable mobile connections. |
| Self-hoster with NAS | Runs TT-Sync on Synology/TrueNAS alongside existing SillyTavern data. | Must coexist with ST's `data/` layout. |
| Home server user | Runs TT-Sync on a Raspberry Pi or home lab machine behind DDNS. | Works without a static IP or domain name. |
| SillyTavern user | Wants to sync upstream ST data to TauriTavern (or vice versa) on same machine or across network. | Bidirectional compatibility with ST's data layout. |

## 4. Core Capabilities (MVP)

### 4.1 Initialization & Configuration

- `tt-sync init` — interactive/non-interactive setup: choose workspace path + layout mode, public base URL, generate cryptographic identity (Ed25519 + TLS), write `config.toml`.
- Runtime state stored in a **state directory** separate from the synced data tree.
  - Default: platform-appropriate app data dir (e.g., `~/.local/state/tt-sync`, `%AppData%/TT-Sync`).
  - Overridable via `--state-dir`.
- The config file defaults next to the executable.
  - CLI deployments may override it via `--config-file <path>`.
  - TUI entrypoints continue using the default config path.

### 4.2 Device Pairing

- `tt-sync pair open` — generates a one-time pairing token with configurable expiry and permission set. Outputs a `tauritavern://` pair URI (and optionally a QR code).
- Pairing flow:
  1. Server generates one-time token + SPKI pin material.
  2. Client scans/enters pair URI and connects over TLS (pinned to server's SPKI).
  3. Client submits device identity (Ed25519 public key + metadata).
  4. Server consumes token, registers peer with granted permissions.
- **Holding the one-time token is sufficient authorization** — no interactive confirmation popup in headless mode.
- `tt-sync peers list` / `tt-sync peers revoke <peer>` — manage paired devices.

### 4.3 Session Management

- After pairing, clients authenticate via short-lived session tokens (`POST /v2/session/open`).
- Session open request is Ed25519-signed with a canonical request format including timestamp and nonce.
- Server validates signature, enforces time window (±90s), and tracks nonces to prevent replay.
- Session tokens are valid for 10–30 minutes; high-frequency file operations use the session token rather than per-request public-key signatures.

### 4.4 Synchronization

#### Pull (Remote → Local)
1. Client scans local manifest.
2. `POST /v2/sync/pull-plan` with sync mode + local manifest.
3. Server diffs against its own manifest, returns a plan (files to download, files to delete, total bytes).
4. Client downloads files via `GET /v2/plans/{plan_id}/files/{path_b64}`.
5. In mirror mode, client deletes local files not present on server.

#### Push (Local → Remote)
1. Client scans local manifest.
2. `POST /v2/sync/push-plan` with sync mode + local manifest.
3. Server diffs against its own manifest, returns a plan (files to upload, files to delete, total bytes).
4. Client uploads files via `PUT /v2/plans/{plan_id}/files/{path_b64}`.
5. `POST /v2/plans/{plan_id}/commit` — server applies deletions only after all uploads succeed.

#### Key Constraints
- **Each sync operation has exactly one source of truth.** There is no automatic conflict merging.
- **File access is plan-scoped** — the server only permits access to paths that appear in the active plan, providing a clear security boundary.
- Diff uses `mtime + size` as the default fast path. BLAKE3 content hashing is available as an optional verification mode.

### 4.5 Dataset Scope (v2)

TT-Sync v2 ships with a **single curated dataset** (fixed allowlist). It is not “sync every file under disk”.

- Included roots (directories + individual files) are defined in `ttsync-core::scope`.
- Explicit exclusion: `default-user/user/lan-sync` (sync-engine local state).
- `default-user/secrets.json` is **included** and is expected to sync across devices (protected by TLS + pairing).

**Extension point** (post-MVP): user-defined include/exclude overlay rules on top of the default dataset.

### 4.6 Layout Mode (filesystem mapping)

TT-Sync keeps the **wire namespace canonical** (TauriTavern-shaped paths like `default-user/...` and `extensions/third-party/...`), and makes the local mapping configurable via a layout mode:

- `tauritavern` — TauriTavern `data/` layout (global extensions live under `data/extensions/third-party`).
- `sillytavern` — SillyTavern repo layout (global extensions live under `public/scripts/extensions/third-party`).
- `sillytavern-docker` — SillyTavern docker volume layout (global extensions live under `./extensions`).

Users provide a single `workspace_path` (layout anchor). TT-Sync derives mount points (data root, default-user root, extensions root) deterministically from `(layout_mode, workspace_path)`.

### 4.7 CLI User Experience

- All commands support `--json`, `--quiet`, `--no-color` output modes.
- `tt-sync serve` prints startup banner with: listen address, public base URL, workspace path, layout mode, derived mount points, TLS mode, state directory.
- Progress reporting during sync operations: phase, file count, byte count, current path.
- `tt-sync doctor` — validates configuration, checks TLS cert, and validates workspace/mount derivation.
- `tt-sync cert show` / `tt-sync cert rotate-leaf` — TLS certificate management.

## 5. Explicit Non-Goals (MVP)

| Non-Goal | Rationale |
|----------|-----------|
| Automatic conflict resolution / CRDT | ST/TT data is file-based; single-source-of-truth replication is the correct semantic. |
| Browser/WebView direct connection | Would require users to install self-signed certs into system trust stores — unacceptable UX. |
| `provided-cert` / `behind-proxy` TLS modes | Module structure reserves interfaces; implementation deferred. |
| User-defined scope overlays | Interface reserved; implementation deferred. |
| HTTP/2 as protocol requirement | Must remain optional; Android ALPN constraints prevent it from being a hard dependency. |
| Real-time / continuous sync | Each sync is an explicit, user-initiated operation. |

## 6. Success Criteria

1. A user can `tt-sync init && tt-sync serve` on a fresh VPS and have a working sync endpoint in under 2 minutes.
2. A TauriTavern client can pair, pull, and push data over the public internet with end-to-end TLS encryption.
3. `tt-sync` can serve an existing SillyTavern `data/` directory without restructuring it.
4. The v2 protocol contract is a standalone crate usable by both TT-Sync and TauriTavern without coupling either to the other's runtime.
