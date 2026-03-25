# TT-Sync Docker Guide

This repository now ships a complete Docker baseline for headless VPS and NAS deployment:

- `Dockerfile`
- `docker-compose.yaml`
- `.dockerignore`
- `.env.example`
- `config.toml.example`

The design is intentionally simple:

- the container runs exactly one foreground process: `tt-sync serve`
- `/state` stores TT-Sync state and secrets
- `/data` is the mounted sync workspace
- the image entrypoint always injects `--state-dir /state --config-file /state/config.toml`

That keeps the container story explicit and avoids special filesystem tricks.

## 1. Runtime Layout

Inside the container:

```text
/app/
└── tt-sync

/state/
├── config.toml
├── identity.json
├── peers.json
├── tls/
│   ├── key.pem
│   └── cert.pem
└── pairing-tokens/

/data/
```

Two hard rules:

- keep `/state` outside the synced tree
- treat `/data` as the layout anchor you would normally pass to `tt-sync init --path`

## 2. What The Image Does

The production image uses:

- multi-stage build
- Alpine builder
- stripped release binary
- `scratch` runtime
- `STOPSIGNAL SIGINT` so `docker stop` matches TT-Sync's graceful shutdown path

The image entrypoint is:

```text
/app/tt-sync --state-dir /state --config-file /state/config.toml
```

So:

- `docker compose up` starts `serve`
- `docker compose run --rm tt-sync doctor` runs `doctor`
- `docker compose run --rm tt-sync pair open --rw` runs `pair open --rw`

without repeating the state/config flags every time.

## 3. Quick Start With Compose

### Step 1: Prepare local files

```bash
cp .env.example .env
mkdir -p ./.tt-sync/state
cp config.toml.example ./.tt-sync/state/config.toml
```

### Step 2: Edit `.env`

At minimum, set:

- `TT_SYNC_WORKSPACE` to the host path you want to sync
- `TT_SYNC_PORT` if `8443` is not what you want

Default example:

```dotenv
TT_SYNC_CONTAINER_NAME=tt-sync
TT_SYNC_PORT=8443
TT_SYNC_STATE_DIR=./.tt-sync/state
TT_SYNC_WORKSPACE=/srv/tauritavern/data
```

### Step 3: Edit `config.toml`

Start from [`config.toml.example`](../config.toml.example).

Typical container values:

```toml
workspace_path = "/data"
layout = "tauritavern"
public_url = "https://sync.example.com:8443"
listen = "0.0.0.0:8443"

[ui]
language = "en"
```

Notes:

- `workspace_path` is the container path, not the host path
- `public_url` must be the external URL your clients will actually use
- `layout` must match the mounted dataset shape

### Step 4: Build and start

```bash
docker compose up -d --build
```

On first start, `serve` will create:

- `identity.json`
- `tls/key.pem`
- `tls/cert.pem`

inside `/state` automatically.

### Step 5: Check the instance

```bash
docker compose run --rm tt-sync doctor
```

### Step 6: Open pairing

```bash
docker compose run --rm tt-sync pair open --rw
```

The pairing token is written into the shared state directory, so the running server container can consume it normally.

## 4. Optional `init` Workflow

If you prefer TT-Sync to generate the config and validate the workspace path up front, use `init` instead of manually creating `config.toml`.

Important:

- do **not** pre-create `/state/config.toml` for this flow
- `init` will refuse to overwrite an existing config file

Example:

```bash
cp .env.example .env
mkdir -p ./.tt-sync/state
docker compose build
docker compose run --rm tt-sync \
  init \
  --path /data \
  --layout tauritavern \
  --public-url https://sync.example.com:8443
docker compose up -d
```

This path is a little more guided. The manual-config path is a little more transparent. Both are valid.

## 5. Direct `docker run`

If you do not want Compose:

```bash
docker build -t tt-sync:local .
docker run -d \
  --name tt-sync \
  --restart unless-stopped \
  -p 8443:8443 \
  -v "$PWD/.tt-sync/state:/state" \
  -v "/srv/tauritavern/data:/data" \
  tt-sync:local
```

Operational commands:

```bash
docker run --rm \
  -v "$PWD/.tt-sync/state:/state" \
  -v "/srv/tauritavern/data:/data" \
  tt-sync:local doctor

docker run --rm \
  -v "$PWD/.tt-sync/state:/state" \
  -v "/srv/tauritavern/data:/data" \
  tt-sync:local pair open --rw
```

## 6. Layout Mapping

Choose the host mount and config layout together:

| Layout | Host path to mount | Container path | `workspace_path` |
|--------|--------------------|----------------|------------------|
| `tauritavern` | TauriTavern `data/` | `/data` | `/data` |
| `sillytavern` | SillyTavern repo root | `/data` | `/data` |
| `sillytavern-docker` | Docker root containing `data/` and `extensions/` | `/data` | `/data` |

For `sillytavern` and `sillytavern-docker`, `/data` is just the container-side mount point name. What matters is that the mounted host path matches the selected layout.

Practical rule:

- if TT-Sync needs to see both `data/` and `extensions/`, mount their common parent

## 7. Why The Compose File Uses Bind Mounts

The default Compose file uses a bind-mounted state directory instead of a named volume:

- Docker users can inspect and edit `config.toml` directly
- `config.toml.example` becomes immediately useful
- `identity.json`, `peers.json`, and TLS files stay visible on disk

If you prefer named volumes later, that is easy to switch. For the first-run experience, bind mounts are more transparent.

## 8. User And Permissions

The shipped image deliberately does **not** force a non-root runtime user.

Reason:

- first-run bind mount compatibility on VPS and NAS matters more than ideology
- root avoids the most common “why can't the container write `/state`?” failure on day one

If you control host ownership and want to lock it down further, add a `user:` override in `docker-compose.yaml`:

```yaml
services:
  tt-sync:
    user: "1000:1000"
```

Once you do that, make sure both the state path and workspace path are writable by that UID/GID.

## 9. Reverse Proxy Boundary

Today TT-Sync self-terminates TLS and clients pin the server SPKI during pairing.

That means the safe default remains:

- expose TT-Sync's HTTPS port directly
- set `public_url` to the actual TT-Sync HTTPS endpoint

Do not make reverse-proxy TLS offload the default story yet. A different certificate identity breaks the current trust model.

## 10. Files To Touch For Docker Users

In practice, most Docker deployments only need these files:

- [`Dockerfile`](../Dockerfile)
- [`docker-compose.yaml`](../docker-compose.yaml)
- [`config.toml.example`](../config.toml.example)
- [`.env.example`](../.env.example)

Everything else is supporting documentation.
