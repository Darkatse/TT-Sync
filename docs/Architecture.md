# Architecture

TT-Sync serves TauriTavern and SillyTavern data over the v2 sync protocol. Native clients can reuse its libraries for pull and direct upload. Each operation has one source and one target.

## Crate boundaries

| Crate | Owns | Internal dependencies |
|---|---|---|
| [ttsync-contract](../crates/ttsync-contract/src/lib.rs) | Wire types, identifiers, path validation, protocol versions | None |
| [ttsync-core](../crates/ttsync-core/src/lib.rs) | Dataset policy, diff planning, pairing/session rules, storage ports | contract |
| [ttsync-fs](../crates/ttsync-fs/src/lib.rs) | Layout mapping, scanning, file writes/deletes, peer persistence | core, contract |
| [ttsync-http](../crates/ttsync-http/src/lib.rs) | HTTP routes and client, TLS, transport state | core, contract |
| [ttsync-client](../crates/ttsync-client/src/lib.rs) | Pull/push orchestration, client workspace and progress ports | http, core, contract |
| [tt-sync](../crates/ttsync-cli/src/main.rs) | CLI/TUI, configuration, service management, server composition | fs, http, core, contract |

Protocol types and sync rules stay independent of concrete storage and HTTP implementations. The shared client engine uses `SyncClient` directly. Native hosts supply `ClientWorkspace` and `SyncObserver`, and own their UI events, job scheduling, and cache refresh.

## Sync semantics

The initiator chooses the direction, `DatasetSelection`, `SyncMode`, and `OverwritePolicy` for each operation. Pull downloads from the server; direct push uploads to it. Both use connections initiated by the client. The planner detects changes by file size and modification time.

- `Incremental` updates selected files and retains target-only files.
- `Mirror` also deletes eligible target-only files within the selected dataset policy.
- `Exact`, the default overwrite policy, takes changed files from the source. `PreferNewer` preserves a same-path target whose modification time is strictly newer; target-only Mirror deletion still follows the selected policy.

[Dataset policy](../crates/ttsync-core/src/dataset/mod.rs) owns dataset ids, profiles, exclusions, runtime eligibility, and deletion boundaries. Manifests and plans must agree with the requested selection. Scope changes belong here so scanning, transfer, and deletion use the same rules.

Wire paths use a shared namespace such as `default-user/...` and `extensions/third-party/...`. [Layout mapping](../crates/ttsync-fs/src/layout.rs) translates these paths to local directories; platform paths stay outside the protocol.

TauriTavern stores appearance, active presets/prompts, and layout separately from core settings:

| Dataset | Files under `default-user/` |
|---|---|
| `settings.core` | `settings.json`, `tauritavern-settings.json`, `image-metadata.json` |
| `settings.appearance` | `settings/appearance.json`, `settings/dynamic-theme.json` |
| `settings.presets` | `settings/presets.json` |
| `settings.layout` | `settings/layout.json` |

All settings sections are included in both `tauritavern.default` and `tauritavern.full`. Named theme and preset libraries retain their existing datasets. TauriTavern owns settings migration and JSON field partitioning; TT-Sync transfers these files using the ordinary dataset policy. Both applications must support the partitioned settings format and use TT-Sync 2.5 or later.

## Transfer flow

The [client engine](../crates/ttsync-client/src/engine.rs) follows this sequence:

1. Read server capabilities and check the requested dataset and overwrite features.
2. Open an authenticated session and check the peer's permissions.
3. Scan the local workspace, request a server plan, and validate its scope.
4. Transfer planned files using negotiated bundle/zstd support or per-file endpoints.
5. Apply Mirror deletions after transfer. For uploads, the server applies deletions at commit.

The v2 contract requires explicit dataset selection and a matching policy version. `PreferNewer` requires the peer's `overwrite_policy_v1` capability. Wire definitions live in `ttsync-contract`; routes live in [server.rs](../crates/ttsync-http/src/server.rs).

[File writes](../crates/ttsync-fs/src/writer.rs) use a temporary file, replace the destination, and preserve the source modification time. Manifests describe files; deletion also prunes fileless directories up to the owning dataset boundary.

An operation can fail after earlier files have changed. [ClientWorkspace](../crates/ttsync-client/src/workspace.rs) reports write/delete side effects, and the engine carries local changes in its success and failure results. Hosts use that information when refreshing application state.

## Trust and authorization

Pairing registers a device's Ed25519 public key with the permissions carried by a one-time token. The [pair URI](../crates/ttsync-contract/src/pair.rs) supplies the HTTPS origin, token, expiry, and SPKI pin.

Transport trust comes from the pinned TLS public key. The pinned verifier checks the SPKI hash and TLS handshake signatures; it does not use certificate issuer, hostname, or expiry as trust criteria. The server supports TLS 1.3 with HTTP/1.1 and optional HTTP/2.

After pairing, the device signs a canonical session-open request with a timestamp and nonce. [Session management](../crates/ttsync-core/src/session.rs) verifies it and issues a short-lived bearer token. Peer grants authorize operations; plans bind file access to the peer and selected paths.

The CLI owns the choice of certificate files and advertised public pin. An explicit public pin takes precedence for pairing, while the HTTPS listener uses its configured certificate and key. Configuration details are in [CLI.md](./CLI.md#tls-certificates-and-public-pins).
