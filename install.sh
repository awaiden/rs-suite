#!/usr/bin/env sh
# Install rs-suite binaries (rsdu, rsfind, rsgrep, rstop, rsnet, rssock, rsfetch, rslog, rssvc) from GitHub Releases or source.
# Run with --help for full usage, flags, and environment variables.
set -eu

REPO="awaiden/rs-suite"
DEFAULT_DIR="$HOME/.local/bin"
DIR="${INSTALL_DIR:-$DEFAULT_DIR}"
DRY_RUN=0
FROM_SOURCE=0
VERIFY_CHECKSUM=1
EXPLICIT_TAG=""
TOOLS=""

usage() {
  cat <<EOF
rs-suite installer — install rsdu, rsfind, rsgrep, rstop, rsnet, rssock, rsfetch, rslog, and rssvc

USAGE:
  install.sh [OPTIONS] [TOOL...]

TOOLS:
  rsdu     Fast disk usage analyzer (Linux x86_64/aarch64, macOS arm64)
  rsfind   Fast parallel file finder & fuzzy TUI (Linux x86_64/aarch64, macOS arm64)
  rsgrep   Fast parallel content search & split-pane TUI (Linux x86_64/aarch64, macOS arm64)
  rstop    Interactive system & process monitor (Linux x86_64/aarch64)
  rsnet    Interactive network bandwidth & connection monitor (Linux x86_64/aarch64)
  rssock   Interactive socket & connection explorer (Linux x86_64/aarch64)
  rsfetch  Fast native Linux system information fetcher (Linux x86_64/aarch64)
  rslog    Interactive Linux log & journald viewer (Linux x86_64/aarch64)
  rssvc    Interactive systemd service manager & inspector (Linux x86_64/aarch64)

  If no tools are specified, all tools supported on this OS are installed.

OPTIONS:
  -d, --dir <PATH>      Install directory (default: ~/.local/bin or \$INSTALL_DIR)
  -v, --version <TAG>   Pin a specific release tag (e.g. v0.1.0 or rsdu-v0.2.0)
      --from-source     Build and install from source using cargo
      --no-verify       Skip SHA-256 checksum verification
      --dry-run         Print planned actions without downloading or writing files
  -h, --help            Show this help message

ENVIRONMENT:
  INSTALL_DIR           Target install directory (overridden by --dir)
  RSDU_VERSION          Pin release tag for rsdu (overridden by --version)
  RSFIND_VERSION        Pin release tag for rsfind (overridden by --version)
  RSGREP_VERSION        Pin release tag for rsgrep (overridden by --version)
  RSTOP_VERSION         Pin release tag for rstop (overridden by --version)
  RSNET_VERSION         Pin release tag for rsnet (overridden by --version)
  RSSOCK_VERSION        Pin release tag for rssock (overridden by --version)
  RSFETCH_VERSION       Pin release tag for rsfetch (overridden by --version)
  RSLOG_VERSION         Pin release tag for rslog (overridden by --version)
  RSSVC_VERSION         Pin release tag for rssvc (overridden by --version)

EXAMPLES:
  # Install all supported tools to ~/.local/bin
  curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | sh

  # Install only rsdu and rsnet to /usr/local/bin
  curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | sh -s -- --dir /usr/local/bin rsdu rsnet

  # Build from source via cargo
  ./install.sh --from-source
EOF
}

log() {
  printf '%s\n' "$*"
}

err() {
  printf 'error: %s\n' "$*" >&2
}

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    err "required command '$1' not found"
    exit 1
  fi
}

fetch() {
  url="$1"
  out="${2:-}"
  if command -v curl >/dev/null 2>&1; then
    if [ -n "$out" ]; then
      curl -fsSL "$url" -o "$out"
    else
      curl -fsSL "$url"
    fi
  elif command -v wget >/dev/null 2>&1; then
    if [ -n "$out" ]; then
      wget -qO "$out" "$url"
    else
      wget -qO- "$url"
    fi
  else
    err "neither curl nor wget is installed"
    exit 1
  fi
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    -d|--dir)
      [ "$#" -ge 2 ] || { err "--dir requires an argument"; exit 1; }
      DIR="$2"
      shift 2
      ;;
    -v|--version)
      [ "$#" -ge 2 ] || { err "--version requires an argument"; exit 1; }
      EXPLICIT_TAG="$2"
      shift 2
      ;;
    --from-source)
      FROM_SOURCE=1
      shift
      ;;
    --no-verify)
      VERIFY_CHECKSUM=0
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --)
      shift
      while [ "$#" -gt 0 ]; do
        TOOLS="$TOOLS $1"
        shift
      done
      ;;
    -*)
      err "unknown option: $1 (try --help)"
      exit 1
      ;;
    *)
      TOOLS="$TOOLS $1"
      shift
      ;;
  esac
done

OS="$(uname -s)"
ARCH="$(uname -m)"

if [ -z "$TOOLS" ]; then
  if [ "$OS" = "Linux" ]; then
    TOOLS="rsdu rsfind rsgrep rstop rsnet rssock rsfetch rslog rssvc"
  else
    TOOLS="rsdu rsfind rsgrep"
  fi
fi

# Validate tool names upfront.
for tool in $TOOLS; do
  case "$tool" in
    rsdu|rsfind|rsgrep|rstop|rsnet|rssock|rsfetch|rslog|rssvc) ;;
    *)
      err "unknown tool '$tool' (expected: rsdu, rsfind, rsgrep, rstop, rsnet, rssock, rsfetch, rslog, or rssvc)"
      exit 1
      ;;
  esac
done

resolve_target() {
  tool="$1"
  case "$tool" in
    rsdu|rsfind|rsgrep)
      case "$OS-$ARCH" in
        Linux-x86_64|Linux-amd64)   echo "x86_64-unknown-linux-gnu" ;;
        Linux-aarch64|Linux-arm64)  echo "aarch64-unknown-linux-gnu" ;;
        Darwin-arm64)               echo "aarch64-apple-darwin" ;;
        *) echo "unsupported" ;;
      esac
      ;;
    rstop|rsnet|rssock|rsfetch|rslog|rssvc)
      if [ "$OS" != "Linux" ]; then
        echo "unsupported"
        return
      fi
      case "$ARCH" in
        x86_64|amd64)   echo "x86_64-unknown-linux-musl" ;;
        aarch64|arm64)  echo "aarch64-unknown-linux-gnu" ;;
        *) echo "unsupported" ;;
      esac
      ;;
  esac
}

pinned_tag_for() {
  if [ -n "$EXPLICIT_TAG" ]; then
    echo "$EXPLICIT_TAG"
    return
  fi
  case "$1" in
    rsdu)    echo "${RSDU_VERSION:-}" ;;
    rsfind)  echo "${RSFIND_VERSION:-}" ;;
    rsgrep)  echo "${RSGREP_VERSION:-}" ;;
    rstop)   echo "${RSTOP_VERSION:-}" ;;
    rsnet)   echo "${RSNET_VERSION:-}" ;;
    rssock)  echo "${RSSOCK_VERSION:-}" ;;
    rsfetch) echo "${RSFETCH_VERSION:-}" ;;
    rslog)   echo "${RSLOG_VERSION:-}" ;;
    rssvc)   echo "${RSSVC_VERSION:-}" ;;
  esac
}

install_bin() {
  src="$1"
  dest="$2"
  parent="$DIR"
  while [ ! -e "$parent" ]; do
    next="$(dirname "$parent")"
    [ "$next" = "$parent" ] && break
    parent="$next"
  done

  if [ -d "$parent" ] && [ -w "$parent" ]; then
    mkdir -p "$DIR"
    install -m 755 "$src" "$dest"
  elif command -v sudo >/dev/null 2>&1; then
    sudo mkdir -p "$DIR"
    sudo install -m 755 "$src" "$dest"
  else
    err "cannot write to $DIR and sudo is not available (set --dir ~/.local/bin)"
    exit 1
  fi
}

if [ "$FROM_SOURCE" -eq 1 ]; then
  need_cmd cargo
  for tool in $TOOLS; do
    target="$(resolve_target "$tool")"
    if [ "$target" = "unsupported" ]; then
      log "Skipping $tool (not supported on $OS $ARCH)"
      continue
    fi
    if [ "$DRY_RUN" -eq 1 ]; then
      log "[dry-run] cargo install --git https://github.com/$REPO $tool"
    else
      log "Building $tool from source..."
      cargo install --git "https://github.com/$REPO" "$tool" --locked
    fi
  done
  exit 0
fi

need_cmd tar
need_cmd install
if [ "$VERIFY_CHECKSUM" -eq 1 ]; then
  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    err "neither sha256sum nor shasum is installed (pass --no-verify to skip)"
    exit 1
  fi
fi

RELEASES_JSON=""
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

INSTALLED=""
for tool in $TOOLS; do
  target="$(resolve_target "$tool")"
  if [ "$target" = "unsupported" ]; then
    log "Skipping $tool (not supported on $OS $ARCH)"
    continue
  fi

  tag="$(pinned_tag_for "$tool")"
  if [ -z "$tag" ]; then
    if [ -z "$RELEASES_JSON" ]; then
      RELEASES_JSON="$(fetch "https://api.github.com/repos/$REPO/releases?per_page=100")"
    fi
    tag="$(
      printf '%s\n' "$RELEASES_JSON" \
        | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' \
        | grep -E "^(${tool}-v|v[0-9])" \
        | head -n 1 || true
    )"
    if [ -z "$tag" ]; then
      err "failed to determine latest release tag for $tool (try --from-source or --version)"
      exit 1
    fi
  fi

  name="${tool}-${tag}-${target}"
  archive="${name}.tar.gz"
  base="https://github.com/$REPO/releases/download/$tag"

  if [ "$DRY_RUN" -eq 1 ]; then
    log "[dry-run] would download $base/$archive -> $DIR/$tool"
    continue
  fi

  log "Downloading $name..."
  fetch "$base/$archive" "$TMP_DIR/$archive"

  if [ "$VERIFY_CHECKSUM" -eq 1 ]; then
    fetch "$base/$archive.sha256" "$TMP_DIR/$archive.sha256"
    if command -v sha256sum >/dev/null 2>&1; then
      (cd "$TMP_DIR" && sha256sum -c "$archive.sha256")
    else
      (cd "$TMP_DIR" && shasum -a 256 -c "$archive.sha256")
    fi
  fi

  tar -C "$TMP_DIR" -xzf "$TMP_DIR/$archive"
  if [ -f "$TMP_DIR/$name/$tool" ]; then
    bin_path="$TMP_DIR/$name/$tool"
  else
    bin_path="$TMP_DIR/$tool"
  fi

  install_bin "$bin_path" "$DIR/$tool"
  log "Installed $tool ($tag) -> $DIR/$tool"
  INSTALLED="$INSTALLED $tool"
done

if [ -n "$INSTALLED" ]; then
  case ":$PATH:" in
    *":$DIR:"*) ;;
    *) log "Note: $DIR is not on your PATH. Add it with: export PATH=\"$DIR:\$PATH\"" ;;
  esac
  case "$INSTALLED" in
    *rsnet*)
      log "Tip: for live AF_PACKET capture in rsnet without sudo, run:"
      log "  sudo setcap cap_net_raw,cap_net_admin=eip $DIR/rsnet"
      ;;
  esac
fi
