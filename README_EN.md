<p align="center">
  <img src="https://img.shields.io/badge/Rust-🦀-orange?style=for-the-badge" alt="Built with Rust"/>
  <img src="https://img.shields.io/badge/TLS_1.3-Secure-blue?style=for-the-badge" alt="TLS 1.3"/>
  <img src="https://img.shields.io/badge/Single_Binary-Portable-green?style=for-the-badge" alt="Portable"/>
</p>

<h1 align="center">TT-Sync</h1>

<p align="center">
  <strong>Remote Synchronization Server for TauriTavern</strong><br/>
  <em>I wanna give my characters a bigger home!</em>
</p>

<p align="center">
  <a href="./README.md">中文</a>
</p>

---

## Why TT-Sync?

Ever found yourself:
- Wanting to sync your TauriTavern data between home and a VPS, but LAN Sync only works locally?
- Wishing you could keep a NAS as the canonical copy and pull from anywhere?
- Hoping to sync with vanilla SillyTavern too?

**Say hello to TT-Sync** — a standalone CLI server that brings your TauriTavern data to the public internet, securely.

Built with **Rust** 🦀, TT-Sync is:
- **End-to-end encrypted** — TLS 1.3 + SPKI certificate pinning, no CA required
- **Single binary** — drop it on any VPS, NAS, or home server, no runtime dependencies
- **Ed25519 device identity** — every paired device is cryptographically verified
- **Bidirectionally compatible** — works with both TauriTavern and original SillyTavern

---

## Installation

### From Source (Recommended)

```bash
git clone https://github.com/Darkatse/TT-Sync.git
cd TT-Sync
cargo build --release

# Copy the binary to wherever you like!
cp target/release/tt-sync ~/.local/bin/
```

### Just the Binary

Download a pre-built binary from [Releases](https://github.com/Darkatse/TT-Sync/releases) and put it in your `$PATH`. Done.

---

## Quick Start

### 1. Initialize

```bash
tt-sync init \
  --data-root /path/to/sillytavern/data/ \
  --public-url https://my-vps.example.com:8443
```

This creates:
- `config.toml` — your server configuration
- `identity.json` — a unique device UUID + Ed25519 keypair
- `tls/key.pem` + `tls/cert.pem` — a self-signed TLS certificate

### 2. Start the Server

```bash
tt-sync serve
```

You'll see a banner like this:

```
  ▶ TT-Sync server running

  Listen       0.0.0.0:8443
  Public URL   https://my-vps.example.com:8443
  TLS          self-managed (SPKI pin)
  SPKI SHA-256 dGVzdC1zcGtp...

  Press Ctrl+C to stop.
```

### 3. Pair a Client

```bash
# Read-only pairing (default)
tt-sync pair open

# Read+write pairing, 1 hour expiry
tt-sync pair open --rw --expires 1h

# Machine-readable output
tt-sync pair open --json
```

Copy the `tauritavern://tt-sync/pair?...` URI into your TauriTavern client.

### 4. Manage Peers

```bash
# List all paired devices
tt-sync peers list

# Revoke a device (by ID prefix or name)
tt-sync peers revoke "My Phone"
```

---

## Features at a Glance

| Command | What it does |
|---------|--------------|
| `init` | Initialize server: config, identity, TLS cert |
| `serve` | Start the HTTPS sync server |
| `pair open` | Generate a one-time pairing token + URI |
| `peers list` | Show all paired devices in a table |
| `peers revoke` | Remove a paired device |
| `profile list` | Show what directories each scope profile includes |
| `doctor` | Validate config, TLS, data root, identity |
| `cert show` | Display SPKI SHA-256 fingerprint |
| `cert rotate-leaf` | Re-sign TLS cert with same key (preserves SPKI pin) |

### Global Flags

| Flag | Effect |
|------|--------|
| `--no-color` | Disable ANSI colored output |
| `--quiet` | Suppress non-essential output (great for scripts) |
| `--state-dir <path>` | Override default state directory |

---

## Scope Profiles

TT-Sync ships with two built-in sync scope profiles:

| Profile | Description |
|---------|-------------|
| `default` | Full TauriTavern user content — characters, chats, settings, themes, extensions, etc. |
| `compatible-minimal` | Exact equivalent of the v1 LAN Sync whitelist. Suitable for SillyTavern compatibility. |

Use `tt-sync profile list` to see exactly which directories and files each profile covers.

---

## Security Model

```
┌──────────────────────────────────────────────────────────┐
│  Layer 1: Transport Security                             │
│  TLS 1.3 (self-signed) + SPKI certificate pinning       │
│  → Every client pins the server's public key at pairing  │
├──────────────────────────────────────────────────────────┤
│  Layer 2: Peer Identity                                  │
│  Ed25519 keypair per device, canonical request signing   │
│  → Session tokens issued after signature verification    │
├──────────────────────────────────────────────────────────┤
│  Layer 3: Authorization                                  │
│  Per-peer ACL: read / write / mirror-delete              │
│  Scope profile restricts visible directories             │
└──────────────────────────────────────────────────────────┘
```

---

## Architecture

```
TT-Sync/crates/
├── ttsync-contract   # Protocol types & wire contracts (domain layer)
├── ttsync-core       # Use-case orchestration & trait definitions (application layer)
├── ttsync-fs         # File system adapter — scanning, atomic writes, peer store
├── ttsync-http       # HTTPS server (axum) & client (reqwest) with SPKI pinning
└── ttsync-cli        # CLI binary — the entry point you interact with
```

Built on Clean Architecture: dependencies flow inward. The CLI depends on HTTP and FS adapters, which depend on core, which depends on contract. Contract has zero knowledge of anything else.

---

## Packaging for Developers

### Build for Your Platform

```bash
cargo build --release
# Binary: target/release/tt-sync (or tt-sync.exe on Windows)
```

### Run Tests

```bash
cargo test
```

### GitHub Actions

Every push to `main` triggers automated builds via GitHub Actions for:

| Platform | Architecture | Artifact |
|----------|--------------|----------|
| Linux | x86_64 | `tt-sync-linux-x64` |
| Linux | ARM64 | `tt-sync-linux-arm64` |
| Windows | x86_64 | `tt-sync-windows-x64.exe` |
| macOS | x86_64 (Intel) | `tt-sync-macos-x64` |
| macOS | ARM64 (Apple Silicon) | `tt-sync-macos-arm64` |

Binaries are published as a nightly pre-release on the [Releases](https://github.com/Darkatse/TT-Sync/releases) page.

---

## Contributing

Found a bug? Want a feature? PRs welcome!

```bash
cargo test
cargo build
```

---

## License

MIT License — sync freely, just don't blame us if your waifu disappears.

---

<p align="center">
  <em>Made with ❤️ for the TauriTavern community.</em><br/>
  <strong>Happy syncing!</strong>
</p>