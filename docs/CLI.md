# Running TT-Sync

Use `tt-sync onboard` for initial setup and `tt-sync` for the terminal interface. For headless operation, run `tt-sync serve` under a process manager. The TUI can manage a user service through systemd, LaunchAgent, or Windows Task Scheduler.

## Configuration and state

[config.toml.example](../config.toml.example) describes every setting in Chinese and English. The program reads `config.toml` next to its executable by default. CLI commands accept `--config-file`; TUI entrypoints use the default location.

Runtime state holds the device identity, self-managed TLS files, peer grants, and pairing tokens. Its location comes from `--state-dir`, then `TT_SYNC_STATE_DIR`, then the platform's local data directory. Persist it outside the sync workspace to retain device identity and pairings.

For a custom installation, use the same paths across commands:

```bash
tt-sync --state-dir /srv/tt-sync --config-file /srv/tt-sync/config.toml serve
tt-sync --state-dir /srv/tt-sync --config-file /srv/tt-sync/config.toml pair open --json
```

## Command entry points

| Task | Command |
|---|---|
| Create a configuration from the command line | `tt-sync init --help` |
| Generate a pairing link | `tt-sync pair open` |
| Inspect or revoke paired devices | `tt-sync peers --help` |
| Check configuration and local files | `tt-sync doctor` |
| Inspect local TLS and public pairing fingerprints | `tt-sync cert show` |

Use `<command> --help` for flags and defaults. `pair open --json` and `peers list --json` provide structured output; `--quiet pair open` prints only the URI. A pairing link grants its configured permissions to the device that consumes it.

## TLS certificates and public pins

The configuration example covers self-managed certificates, external PEM files, and a public endpoint pin. When both external files and `public_spki_sha256` are set, the explicit pin wins for pairing. `cert show` displays the local TLS pin and the effective pairing pin separately.

External certificate files are read-only inputs. Restart the server after replacing them or changing TLS configuration. `cert rotate-leaf` renews only the self-managed certificate with its existing key.

To compute a public key fingerprint from a certificate you control:

```bash
set -o pipefail
openssl x509 -in fullchain.pem -pubkey -noout |
  openssl pkey -pubin -outform DER |
  openssl dgst -sha256 -binary |
  openssl base64 -A |
  tr '+/' '-_' | tr -d '='
```

This produces the unpadded base64url SHA-256 hash of DER SPKI. Renewing a certificate with the same key preserves its pin; changing the key requires updating client pins, for example through pairing again. For a TLS-terminating proxy or CDN, use the certificate clients receive at that endpoint. See [Docker deployment](./Docker.md) for the container layout.
