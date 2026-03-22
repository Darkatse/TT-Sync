# TT-Sync — Technology Stack

## Language & Edition

| Item | Choice | Rationale |
|------|--------|-----------|
| Language | **Rust** | Memory safety, single-binary deployment, shared crate ecosystem with TauriTavern. |
| Edition | **2024** | Latest stable edition; aligns with TauriTavern's `src-tauri`. |
| MSRV | Defined by workspace, not pinned below latest stable. | Avoids unnecessary compatibility burden for a new project. |

## Async Runtime

| Item | Choice | Rationale |
|------|--------|-----------|
| Runtime | **tokio** (multi-thread) | Industry standard; required by axum, reqwest, rustls. TauriTavern already uses tokio. |
| Task spawning | `tokio::spawn`, `JoinSet` | Consistent with existing LAN Sync concurrency patterns. |

## Networking

### Server

| Item | Choice | Rationale |
|------|--------|-----------|
| HTTP framework | **axum** | Already used in TauriTavern's LAN Sync server. Modular, tower-based. |
| TLS termination | **axum-server** + **rustls** | Provides axum-native HTTPS with rustls. No OpenSSL dependency. |
| HTTP versions | HTTP/1.1 (baseline), HTTP/2 (opportunistic) | Android ALPN constraints require HTTP/1.1 as minimum. h2 is never a protocol prerequisite. |

### Client

| Item | Choice | Rationale |
|------|--------|-----------|
| HTTP client | **reqwest** + **rustls** | Already used in TauriTavern. Supports custom TLS verifiers for SPKI pinning. |
| SPKI pinning | Custom `rustls::client::danger::ServerCertVerifier` | Validates server's TLS public key hash against the pin from pair URI, bypassing hostname/issuer checks while preserving TLS handshake security. |

## Cryptography

| Purpose | Crate | Usage |
|---------|-------|-------|
| Device identity (long-term) | **ed25519-dalek** | Each device generates a persistent Ed25519 keypair. Used for session-open signatures. |
| Request signatures | **ed25519-dalek**, **sha2** | Ed25519 signature over canonical request (includes SHA-256 of request body). |
| TLS certificate generation | **rcgen** | Generates self-signed X.509 leaf cert with long-term TLS key. Self-managed mode only. |
| TLS runtime | **rustls** | Both server and client TLS. Pure Rust, no system OpenSSL dependency. |
| Content hashing (optional) | **blake3** | Optional file content verification mode. Not used in default `mtime+size` fast path. |
| Random generation | **rand** | Nonces, one-time tokens, device IDs. |
| Encoding | **base64** | URL-safe no-pad encoding for signatures, hashes, tokens. |
| Key derivation (future) | **hkdf** | Reserved for potential future session key derivation from x25519 exchange. |

## Serialization

| Format | Crate | Usage |
|--------|-------|-------|
| JSON | **serde** + **serde_json** | Wire protocol (request/response bodies), paired device storage. |
| TOML | **toml** | `config.toml` — human-editable configuration file. |

## CLI

| Purpose | Crate | Rationale |
|---------|-------|-----------|
| Argument parsing | **clap** (derive) | Industry standard. Supports subcommands, value validation, shell completions. |
| Progress bars | **indicatif** | Terminal progress bars and spinners for sync operations. |
| Table output | **comfy-table** | Formatted tables for `peers list`, etc. |
| Structured logging | **tracing** + **tracing-subscriber** | Consistent with TauriTavern's logging infrastructure. Supports JSON output. |

## File System

| Purpose | Crate | Rationale |
|---------|-------|-----------|
| mtime preservation | **filetime** | Set file modification time after download. Already used in TauriTavern LAN Sync. |
| Atomic writes | std (`write` → `rename`) | Temp file + atomic rename pattern. Same approach as existing `download_tmp_path`. |

## Build & Distribution

| Item | Choice |
|------|--------|
| Build system | Cargo workspace |
| CI | GitHub Actions (future) |
| Distribution | Single static binary per platform |
| Cross-compilation | `cross` or `cargo-zigbuild` for Linux aarch64 targets |

## Dependency Principles

1. **Minimize** — Every dependency must justify itself. Prefer well-maintained, audited crates.
2. **Pure Rust** — Avoid C dependencies (no OpenSSL, no system TLS). Enables easy cross-compilation.
3. **Shared with TauriTavern** — Where possible, use the same crate and version as TauriTavern to ease future integration (e.g., `axum`, `reqwest`, `serde`, `tokio`, `tracing`, `filetime`).
4. **Feature-gate optional capabilities** — BLAKE3 verify mode, QR code generation, etc. should be behind cargo features.
