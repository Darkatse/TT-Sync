#!/bin/sh

set -eu

REPO="${TT_SYNC_REPO:-Darkatse/TT-Sync}"
INSTALL_DIR="${TT_SYNC_INSTALL_DIR:-}"
VERSION=""
USE_NIGHTLY=0
BINARY_NAME="tt-sync"
RELEASE_BASE=""
RELEASE_LABEL=""

usage() {
  cat <<'EOF'
Install TT-Sync from GitHub Releases.

Usage:
  install.sh [--nightly] [--version <version>] [--dir <path>] [--repo <owner/name>]

Examples:
  install.sh
  install.sh --nightly
  install.sh --version 0.1.0
  install.sh --dir "$HOME/.local/bin"
EOF
}

say() {
  printf '%s\n' "$*"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

normalize_version() {
  case "$1" in
    "" ) printf '%s' "" ;;
    nightly | v* ) printf '%s' "$1" ;;
    [0-9]* ) printf 'v%s' "$1" ;;
    * ) printf '%s' "$1" ;;
  esac
}

url_exists() {
  curl -fsSIL "$1" >/dev/null 2>&1
}

compute_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    die "need sha256sum or shasum to verify the download"
  fi
}

resolve_asset_name() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux) platform="linux" ;;
    Darwin) platform="macos" ;;
    *)
      die "unsupported operating system: $os"
      ;;
  esac

  case "$arch" in
    x86_64 | amd64) suffix="x64" ;;
    arm64 | aarch64) suffix="arm64" ;;
    *)
      die "unsupported architecture: $arch"
      ;;
  esac

  printf '%s-%s-%s' "$BINARY_NAME" "$platform" "$suffix"
}

resolve_install_dir() {
  if [ -n "$INSTALL_DIR" ]; then
    printf '%s' "$INSTALL_DIR"
    return
  fi

  if [ "$(id -u)" -eq 0 ]; then
    printf '%s' "/usr/local/bin"
    return
  fi

  if [ -d "/usr/local/bin" ] && [ -w "/usr/local/bin" ]; then
    printf '%s' "/usr/local/bin"
    return
  fi

  [ -n "${HOME:-}" ] || die "HOME is not set and --dir was not provided"
  printf '%s' "$HOME/.local/bin"
}

resolve_release() {
  asset_name="$1"
  release_root="https://github.com/$REPO/releases"

  if [ -n "$VERSION" ]; then
    RELEASE_LABEL="release $VERSION"
    RELEASE_BASE="$release_root/download/$VERSION"
    return
  fi

  if [ "$USE_NIGHTLY" -eq 1 ]; then
    RELEASE_LABEL="nightly"
    RELEASE_BASE="$release_root/download/nightly"
    return
  fi

  latest_asset_url="$release_root/latest/download/$asset_name"
  latest_checksums_url="$release_root/latest/download/SHA256SUMS.txt"
  if url_exists "$latest_asset_url" && url_exists "$latest_checksums_url"; then
    RELEASE_LABEL="latest stable release"
    RELEASE_BASE="$release_root/latest/download"
    return
  fi

  RELEASE_LABEL="nightly"
  RELEASE_BASE="$release_root/download/nightly"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --nightly)
      USE_NIGHTLY=1
      shift
      ;;
    --version)
      [ "$#" -ge 2 ] || die "--version requires a value"
      VERSION="$2"
      shift 2
      ;;
    --dir)
      [ "$#" -ge 2 ] || die "--dir requires a value"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --repo)
      [ "$#" -ge 2 ] || die "--repo requires a value"
      REPO="$2"
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[ -z "$VERSION" ] || VERSION="$(normalize_version "$VERSION")"
[ -z "$VERSION" ] || [ "$USE_NIGHTLY" -eq 0 ] || die "--nightly and --version cannot be used together"

need_cmd curl
need_cmd uname
need_cmd mktemp
need_cmd grep
need_cmd awk
need_cmd chmod
need_cmd cp
need_cmd mkdir
need_cmd id

ASSET_NAME="$(resolve_asset_name)"
INSTALL_DIR="$(resolve_install_dir)"
# Avoid command substitution here: POSIX sh runs $(...) in a subshell,
# so RELEASE_BASE / RELEASE_LABEL assignments would not persist.
resolve_release "$ASSET_NAME"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT HUP INT TERM

ASSET_PATH="$TMP_DIR/$ASSET_NAME"
CHECKSUMS_PATH="$TMP_DIR/SHA256SUMS.txt"
DEST_PATH="$INSTALL_DIR/$BINARY_NAME"

say "Installing TT-Sync from $RELEASE_LABEL"
say "Downloading $ASSET_NAME"
curl -fsSL "$RELEASE_BASE/$ASSET_NAME" -o "$ASSET_PATH" || die "failed to download $ASSET_NAME"
curl -fsSL "$RELEASE_BASE/SHA256SUMS.txt" -o "$CHECKSUMS_PATH" || die "failed to download SHA256SUMS.txt"

EXPECTED_HASH="$(awk -v file="$ASSET_NAME" '$2 == file { print $1 }' "$CHECKSUMS_PATH")"
[ -n "$EXPECTED_HASH" ] || die "checksum entry for $ASSET_NAME not found"

ACTUAL_HASH="$(compute_sha256 "$ASSET_PATH")"
[ "$ACTUAL_HASH" = "$EXPECTED_HASH" ] || die "checksum mismatch for $ASSET_NAME"

mkdir -p "$INSTALL_DIR"
cp "$ASSET_PATH" "$DEST_PATH"
chmod 755 "$DEST_PATH"

say "Installed to $DEST_PATH"
case ":${PATH:-}:" in
  *":$INSTALL_DIR:"*)
    say "The install directory is already on PATH."
    ;;
  *)
    say "Add this directory to PATH if needed: $INSTALL_DIR"
    ;;
esac

say "Try '$BINARY_NAME' for the TUI, or '$BINARY_NAME onboard' for the guided first-time setup."
