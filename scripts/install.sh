#!/usr/bin/env bash
#
# Sebenza installer — downloads the latest GitHub Release build of
# `sebenza-server` + `sebenza-cli` and puts them on your PATH.
#
#   curl -fsSL https://raw.githubusercontent.com/groundtruthsystems/sebenza/main/scripts/install.sh | bash
#
# Options (flag or environment variable):
#
#   --version <tag>   SEBENZA_VERSION      release tag to install (default: latest)
#   --dir <path>      SEBENZA_INSTALL_DIR  install directory (default: ~/.local/bin)
#   --repo <o/r>      SEBENZA_REPO         source repo (default: groundtruthsystems/sebenza)
#   --uninstall                            remove installed binaries and exit
#   --help
#
# GITHUB_TOKEN is used for the API call when set (private repos / rate limits).

set -euo pipefail

REPO="${SEBENZA_REPO:-groundtruthsystems/sebenza}"
VERSION="${SEBENZA_VERSION:-latest}"
INSTALL_DIR="${SEBENZA_INSTALL_DIR:-$HOME/.local/bin}"
BINARIES=(sebenza-server sebenza-cli)
UNINSTALL=0

# --- output -----------------------------------------------------------------

if [ -t 2 ]; then
  BOLD=$(printf '\033[1m'); RED=$(printf '\033[31m')
  YELLOW=$(printf '\033[33m'); GREEN=$(printf '\033[32m'); RESET=$(printf '\033[0m')
else
  BOLD=""; RED=""; YELLOW=""; GREEN=""; RESET=""
fi

info() { printf '%s==>%s %s\n' "$BOLD" "$RESET" "$*" >&2; }
warn() { printf '%swarning:%s %s\n' "$YELLOW" "$RESET" "$*" >&2; }
ok()   { printf '%s==>%s %s\n' "$GREEN" "$RESET" "$*" >&2; }
die()  { printf '%serror:%s %s\n' "$RED" "$RESET" "$*" >&2; exit 1; }

# Print the header comment block. When piped (curl | bash) $0 is not readable,
# so fall back to a one-liner.
usage() {
  if [ -r "$0" ]; then
    awk 'NR > 2 { if ($0 !~ /^#/) exit; sub(/^# ?/, ""); print }' "$0"
  else
    echo "usage: install.sh [--version <tag>] [--dir <path>] [--repo <owner/name>] [--uninstall]"
  fi
  exit 0
}

# --- args -------------------------------------------------------------------

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="${2:-}"; [ -n "$VERSION" ] || die "--version needs a value"; shift 2 ;;
    --version=*) VERSION="${1#*=}"; shift ;;
    --dir) INSTALL_DIR="${2:-}"; [ -n "$INSTALL_DIR" ] || die "--dir needs a value"; shift 2 ;;
    --dir=*) INSTALL_DIR="${1#*=}"; shift ;;
    --repo) REPO="${2:-}"; [ -n "$REPO" ] || die "--repo needs a value"; shift 2 ;;
    --repo=*) REPO="${1#*=}"; shift ;;
    --uninstall) UNINSTALL=1; shift ;;
    -h|--help) usage ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
done

# --- uninstall --------------------------------------------------------------

if [ "$UNINSTALL" -eq 1 ]; then
  removed=0
  for bin in "${BINARIES[@]}"; do
    path="$INSTALL_DIR/$bin"
    if [ -e "$path" ]; then
      rm -f "$path" || die "could not remove $path"
      info "removed $path"
      removed=1
    fi
  done
  [ "$removed" -eq 1 ] || warn "nothing to remove in $INSTALL_DIR"
  exit 0
fi

# --- platform ---------------------------------------------------------------

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Linux)
    case "$arch" in
      x86_64|amd64) TARGET="x86_64-unknown-linux-gnu" ;;
      aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
      *) die "unsupported Linux architecture: $arch" ;;
    esac
    ;;
  Darwin)
    case "$arch" in
      arm64) TARGET="aarch64-apple-darwin" ;;
      x86_64)
        # Releases ship Apple Silicon only. On an Intel Mac, check whether this
        # is really an arm64 machine running the script under Rosetta.
        if [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = "1" ]; then
          TARGET="aarch64-apple-darwin"
        else
          die "Intel macOS is not published as a release build. Build from source: https://github.com/$REPO"
        fi
        ;;
      *) die "unsupported macOS architecture: $arch" ;;
    esac
    ;;
  *) die "unsupported operating system: $os (Linux and macOS only)" ;;
esac

# --- prerequisites ----------------------------------------------------------

if command -v curl >/dev/null 2>&1; then
  DOWNLOADER=curl
elif command -v wget >/dev/null 2>&1; then
  DOWNLOADER=wget
else
  die "need curl or wget on PATH"
fi

command -v tar >/dev/null 2>&1 || die "need tar on PATH"

if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  sha256() { echo ""; }
fi

# fetch <url> <dest>  — returns non-zero on HTTP error
fetch() {
  if [ "$DOWNLOADER" = curl ]; then
    curl -fsSL ${GITHUB_TOKEN:+-H "Authorization: Bearer $GITHUB_TOKEN"} -o "$2" "$1"
  else
    wget -q ${GITHUB_TOKEN:+--header="Authorization: Bearer $GITHUB_TOKEN"} -O "$2" "$1"
  fi
}

# --- resolve the release tag ------------------------------------------------

resolve_latest() {
  local api="https://api.github.com/repos/$REPO/releases/latest" body tag
  body="$(
    if [ "$DOWNLOADER" = curl ]; then
      curl -fsSL ${GITHUB_TOKEN:+-H "Authorization: Bearer $GITHUB_TOKEN"} "$api"
    else
      wget -qO- ${GITHUB_TOKEN:+--header="Authorization: Bearer $GITHUB_TOKEN"} "$api"
    fi
  )" || body=""

  tag="$(printf '%s' "$body" | tr ',' '\n' | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"

  # Fall back to the /releases/latest redirect when the API is unavailable
  # (rate limited, no network to api.github.com, …).
  if [ -z "$tag" ] && [ "$DOWNLOADER" = curl ]; then
    local url
    url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" 2>/dev/null || true)"
    case "$url" in */tag/*) tag="${url##*/tag/}" ;; esac
  fi

  printf '%s' "$tag"
}

if [ "$VERSION" = latest ]; then
  info "resolving latest release of $REPO"
  TAG="$(resolve_latest)"
  [ -n "$TAG" ] || die "could not determine the latest release for $REPO (set --version, or GITHUB_TOKEN if rate limited)"
else
  TAG="$VERSION"
fi

# Release assets are named with the bare version: sebenza-0.1.0-<target>.tar.gz
NUM_VERSION="${TAG#v}"
ARCHIVE="sebenza-${NUM_VERSION}-${TARGET}.tar.gz"
BASE_URL="https://github.com/$REPO/releases/download/$TAG"

# --- download ---------------------------------------------------------------

TMPDIR_="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_"' EXIT

info "downloading $ARCHIVE ($TAG)"
fetch "$BASE_URL/$ARCHIVE" "$TMPDIR_/$ARCHIVE" \
  || die "download failed: $BASE_URL/$ARCHIVE — is $TAG a published release with a $TARGET build?"

if fetch "$BASE_URL/$ARCHIVE.sha256" "$TMPDIR_/$ARCHIVE.sha256" 2>/dev/null; then
  expected="$(cut -d' ' -f1 < "$TMPDIR_/$ARCHIVE.sha256")"
  actual="$(sha256 "$TMPDIR_/$ARCHIVE")"
  if [ -z "$actual" ]; then
    warn "no sha256sum/shasum available — skipping checksum verification"
  elif [ "$expected" != "$actual" ]; then
    die "checksum mismatch for $ARCHIVE (expected $expected, got $actual)"
  else
    info "checksum verified"
  fi
else
  warn "no .sha256 published for $ARCHIVE — skipping checksum verification"
fi

# --- install ----------------------------------------------------------------

tar -xzf "$TMPDIR_/$ARCHIVE" -C "$TMPDIR_" || die "could not extract $ARCHIVE"
SRC="$TMPDIR_/sebenza-${NUM_VERSION}-${TARGET}"
[ -d "$SRC" ] || die "unexpected archive layout: $SRC not found"

mkdir -p "$INSTALL_DIR" 2>/dev/null || true
[ -d "$INSTALL_DIR" ] || die "install directory does not exist and could not be created: $INSTALL_DIR"
# Absolute path, so the PATH hint below is copy-pasteable.
INSTALL_DIR="$(cd "$INSTALL_DIR" && pwd)"

SUDO=""
if [ ! -w "$INSTALL_DIR" ]; then
  if command -v sudo >/dev/null 2>&1; then
    warn "$INSTALL_DIR is not writable — using sudo"
    SUDO="sudo"
  else
    die "$INSTALL_DIR is not writable (set --dir to somewhere you own)"
  fi
fi

for bin in "${BINARIES[@]}"; do
  [ -f "$SRC/$bin" ] || die "$bin missing from the release archive"
  $SUDO install -m 0755 "$SRC/$bin" "$INSTALL_DIR/$bin" \
    || die "could not install $bin into $INSTALL_DIR"
  info "installed $INSTALL_DIR/$bin"
done

# macOS quarantines files downloaded via a browser; harmless here but strip it
# anyway so Gatekeeper does not block the unsigned binaries.
if [ "$os" = Darwin ] && command -v xattr >/dev/null 2>&1; then
  for bin in "${BINARIES[@]}"; do
    $SUDO xattr -d com.apple.quarantine "$INSTALL_DIR/$bin" 2>/dev/null || true
  done
fi

ok "sebenza $TAG installed ($TARGET)"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    warn "$INSTALL_DIR is not on your PATH — add it to your shell profile:"
    printf '\n    export PATH="%s:$PATH"\n\n' "$INSTALL_DIR" >&2
    ;;
esac

if [ -x "$INSTALL_DIR/sebenza-cli" ] && [ -z "$SUDO" ]; then
  "$INSTALL_DIR/sebenza-cli" --version 2>/dev/null || true
fi
