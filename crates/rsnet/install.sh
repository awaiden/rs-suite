#!/usr/bin/env sh
# Install the latest rsnet release binary.
#   RSNET_INSTALL_DIR  target directory (default: ~/.local/bin)
#   RSNET_VERSION      release tag to install (default: latest)
set -eu

REPO="awaiden/rs-suite"
BIN_NAME="rsnet"
INSTALL_DIR="${RSNET_INSTALL_DIR:-${INSTALL_DIR:-$HOME/.local/bin}}"

OS="$(uname -s)"
if [ "$OS" != "Linux" ]; then
  echo "error: $BIN_NAME only supports Linux (detected: $OS)" >&2
  exit 1
fi

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64 | amd64)
    TARGET="x86_64-unknown-linux-musl"
    ;;
  aarch64 | arm64)
    TARGET="aarch64-unknown-linux-gnu"
    ;;
  *)
    echo "error: unsupported architecture: $ARCH" >&2
    exit 1
    ;;
esac

if [ -n "${RSNET_VERSION:-}" ]; then
  TAG="$RSNET_VERSION"
else
  TAG="$(
    curl -fsSL "https://api.github.com/repos/$REPO/releases?per_page=100" \
      | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' \
      | grep -E '^(rsnet-v|v[0-9])' \
      | head -n 1 || true
  )"
  if [ -z "$TAG" ]; then
    echo "error: failed to determine latest rsnet release tag from GitHub" >&2
    exit 1
  fi
fi

NAME="${BIN_NAME}-${TAG}-${TARGET}"
ARCHIVE="${NAME}.tar.gz"
BASE_URL="https://github.com/$REPO/releases/download/$TAG"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

echo "Downloading $BIN_NAME $TAG ($TARGET)..."
curl -fsSL "$BASE_URL/$ARCHIVE" -o "$TMP_DIR/$ARCHIVE"

if command -v sha256sum >/dev/null 2>&1; then
  curl -fsSL "$BASE_URL/$ARCHIVE.sha256" -o "$TMP_DIR/$ARCHIVE.sha256"
  (cd "$TMP_DIR" && sha256sum -c "$ARCHIVE.sha256")
elif command -v shasum >/dev/null 2>&1; then
  curl -fsSL "$BASE_URL/$ARCHIVE.sha256" -o "$TMP_DIR/$ARCHIVE.sha256"
  (cd "$TMP_DIR" && shasum -a 256 -c "$ARCHIVE.sha256")
else
  echo "error: cannot verify checksum (neither sha256sum nor shasum is installed)" >&2
  exit 1
fi

tar -C "$TMP_DIR" -xzf "$TMP_DIR/$ARCHIVE"

if [ -f "$TMP_DIR/$NAME/$BIN_NAME" ]; then
  BIN_PATH="$TMP_DIR/$NAME/$BIN_NAME"
else
  BIN_PATH="$TMP_DIR/$BIN_NAME"
fi

mkdir -p "$INSTALL_DIR"
install -m 755 "$BIN_PATH" "$INSTALL_DIR/$BIN_NAME"
echo "Installed $BIN_NAME $TAG to $INSTALL_DIR/$BIN_NAME"
case ":$PATH:" in *":$INSTALL_DIR:"*) ;; *) echo "Note: $INSTALL_DIR is not on your PATH." ;; esac
