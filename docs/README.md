# Maintainer guide

These documents describe the module boundaries and operating model. Configuration fields are documented in [config.toml.example](../config.toml.example), command options in `--help`, and protocol fields in the Rust types. Keep shared rules here and implementation details beside the code that owns them.

- [Architecture](./Architecture.md): crate ownership, sync semantics, transfer flow, and trust.
- [Running TT-Sync](./CLI.md): configuration/state locations, command entry points, and TLS.
- [Docker deployment](./Docker.md): the supported container layout and startup flow.

## Development

From the workspace root:

```bash
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
```

Run focused tests in the affected crate while working. The workspace tests include real HTTPS pairing and transfer; changes to shared sync semantics should retain that coverage. Dependency versions and feature flags are defined in [Cargo.toml](../Cargo.toml) and the crate manifests.

## Release

Update the workspace version and internal dependency versions together in `Cargo.toml`, refresh `Cargo.lock` with `cargo update --workspace`, and run:

```bash
python3 scripts/ci/cargo_release.py validate
```

Pushing a version change to `main` triggers [Publish Crates](../.github/workflows/crates-publish.yml). It runs tests, publishes packages in dependency order, and creates the release tag. The tag triggers [binary releases](../.github/workflows/build.yml) and [container releases](../.github/workflows/docker-publish.yml). The workflows define credentials, artifact names, and retry behavior.
