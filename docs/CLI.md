# TT-Sync CLI Reference

This document is for **developers, automation, and headless deployment**.

For normal end-user setup and day-to-day operation, prefer the TUI:
- `tt-sync onboard` for first-time setup
- `tt-sync` for the main menu

## Invocation Model

- `tt-sync`
  - Opens the full-screen TUI when stdout is attached to a terminal.
  - Prints help text instead when stdout is not a terminal.
- `tt-sync onboard`
  - Jumps directly into the guided onboarding TUI.
- `tt-sync <subcommand>`
  - Uses the lower-level CLI surface documented below.

## Config And State

Current behavior is split into two locations:

- `config.toml`
  - By default, stored next to the `tt-sync` executable.
  - CLI subcommands may override it with `--config-file <path>`.
  - TUI entrypoints ignore `--config-file` and continue using the default path.
  - Contains workspace path, layout mode, listen address, public URL, UI language, and optional TLS settings.
- state directory
  - Defaults to the platform-local data directory plus `tt-sync`.
  - Override with global `--state-dir <path>` or `TT_SYNC_STATE_DIR`.
  - Holds `identity.json`, `tls/`, `peers.json`, and other runtime state.

That split matters in packaging and service management: moving only the state directory does **not** move the default config path unless you also pass `--config-file`.

For the recommended container layout built around `--config-file`, see [Docker Guide](./Docker.md).

## TLS Certificates And Public Pins

With no TLS settings, TT-Sync uses its existing self-managed certificate in the state directory.
To serve with external PEM files, add this table to `config.toml`:

```toml
[tls]
cert_file = "/certs/fullchain.pem"
key_file = "/certs/privkey.pem"
```

Both paths are required. Relative paths resolve from the config file's directory.
The certificate file contains the leaf first, followed by any intermediates; the key must be unencrypted.
TT-Sync validates the certificate and key, reads them without modifying them, and fails if they cannot be loaded or do not match.
Renew the files externally and restart the service to load the update.
Restart the running server after changing TLS configuration as well; pairing commands read the current config file on each invocation.

When a reverse proxy terminates public TLS, configure the public endpoint's SPKI pin at the **top level**, before any TOML table:

```toml
public_url = "https://sync.example.com"
public_spki_sha256 = "REPLACE_WITH_PUBLIC_ENDPOINT_SPKI"
```

**An explicit `public_spki_sha256` always wins over the local certificate's computed pin**, including when `[tls]` is also configured.
Without it, pairing uses the local certificate's pin. The override only selects the public pin; the configured certificate and key must still be valid for the local HTTPS listener.
Pair URIs, CLI/TUI pairing, server status, and startup output all use this same public pin.
`cert show` displays both the local TLS pin and the effective pairing pin. TUI onboarding preserves these settings when editing other fields.

The format is unpadded base64url of `SHA256(DER SubjectPublicKeyInfo)`, not a whole-certificate fingerprint or colon-separated hex.
For a public endpoint certificate you control, compute it from its PEM file:

```bash
set -o pipefail
openssl x509 -in fullchain.pem -pubkey -noout |
  openssl pkey -pubin -outform DER |
  openssl dgst -sha256 -binary |
  openssl base64 -A |
  tr '+/' '-_' | tr -d '='
```

The v2 pair URI and client pinning contract are unchanged. Renewing a certificate with the same public key preserves existing pairings; replacing the key requires updating client pins, for example by pairing again.
See [reverse proxy deployment](./Docker.md#9-external-certificates-and-reverse-proxies) for the HTTPS upstream and Cloudflare boundary.

## Global Flags

| Flag | Effect |
|------|--------|
| `--no-color` | Disable ANSI color output |
| `--quiet` | Suppress non-essential output |
| `--state-dir <path>` | Override the runtime state directory |
| `--config-file <path>` | Override the config file path for CLI subcommands; ignored by TUI entrypoints |

## Commands

### `init`

Initialize a TT-Sync instance without using the TUI.

```bash
tt-sync init \
  --path /srv/tauritavern/data \
  --layout tauri-tavern \
  --public-url https://sync.example.com:8443
```

Arguments:
- `--path`: workspace path used as the layout anchor
- `--layout`: `tauri-tavern`, `silly-tavern`, or `silly-tavern-docker`
- `--public-url`: base URL embedded into pair URIs
- `--listen`: optional listen address, default `0.0.0.0:8443`

What it creates:
- the config file at the default path or at `--config-file <path>`
- `identity.json` in the state directory
- `tls/key.pem` and `tls/cert.pem` in the state directory

Use this for:
- scripted provisioning
- CI fixtures
- headless deployment where the TUI is not practical

### `serve`

Start the HTTPS synchronization server in the foreground.

```bash
tt-sync serve
```

Use this when:
- running under a process manager
- testing locally
- bringing up the server manually on a headless box

Stop it with `Ctrl+C`.

### `pair open`

Generate a one-time pairing token and the corresponding TauriTavern pair URI.

```bash
tt-sync pair open
tt-sync pair open --rw
tt-sync pair open --ro
tt-sync pair open --mirror
tt-sync --quiet pair open
tt-sync pair open --expires 1h --json
```

Permission model:
- default: read + write
- `--rw`: explicit read + write; equivalent to the default
- `--ro`: read-only
- `--mirror`: allow mirror-mode deletes; requires write access

Token lifetime:
- `--expires <duration>` accepts values such as `30s`, `5m`, or `1h`

Output modes:
- default: human-readable summary
- `--quiet`: print only the pair URI
- `--json`: print machine-readable JSON

Direct link output:

```bash
tt-sync --quiet pair open
```

### `peers list`

List paired devices.

```bash
tt-sync peers list
tt-sync peers list --json
```

### `peers revoke`

Revoke a paired device by full device ID, ID prefix, or exact device name.

```bash
tt-sync peers revoke "My Phone"
tt-sync peers revoke 123e4567
```

### `doctor`

Validate the local TT-Sync setup.

```bash
tt-sync doctor
```

Checks include:
- state directory presence
- the selected config file
- layout/mount derivation
- identity loading
- TLS certificate loading
- `peers.json` readability

### `cert show`

Show the selected TLS certificate file locations, the local certificate's SPKI fingerprint, and the effective public pairing fingerprint.

```bash
tt-sync cert show
```

### `cert rotate-leaf`

Re-sign the leaf certificate while preserving the same private key and SPKI pin.

```bash
tt-sync cert rotate-leaf
```

Paired clients remain valid because the SPKI hash stays unchanged. This command only manages the built-in self-signed certificate; it rejects `[tls]` configurations. Renew external certificates through their issuer and restart TT-Sync instead.

### `background-serve`

Internal Windows-only command used by the Task Scheduler integration.

It is intentionally hidden from normal help output and should not be treated as a public user command surface.

## Automation Notes

For scripts:
- prefer `--quiet` when you only need a single value such as a pair URI
- prefer `--json` where available for stable parsing
- pass `--state-dir` explicitly if the process manager or container runtime changes the runtime environment

Example:

```bash
tt-sync --state-dir /var/lib/tt-sync pair open --json
```

Container-oriented example:

```bash
tt-sync --state-dir /state --config-file /state/config.toml doctor
```

## Container Notes

For Docker, prefer the lower-level CLI surface over the TUI:

- run `init` once against mounted volumes
- run `serve` as the single foreground container process
- run `pair open`, `peers list`, and `doctor` with `docker exec` or `docker compose run`

Recommended container pattern:

- keep runtime state under `/state`
- keep synced data under `/data` or another mounted workspace path
- pass `--config-file /state/config.toml` explicitly to CLI subcommands
- start from [`config.toml.example`](../config.toml.example) for headless provisioning
- use the shipped [`docker-compose.yaml`](../docker-compose.yaml) if you want the default entrypoint and volume layout

See [Docker Guide](./Docker.md) for the full layout and command examples.

## Service Management Notes

Service management is currently **TUI-first**:
- Linux user service: `systemd --user`
- macOS user service: `LaunchAgent`
- Windows user service: `Task Scheduler` beta

There is no separate public CLI subcommand yet for installing or controlling those services. The TUI calls the platform-specific integrations for you.
