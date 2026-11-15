#!/bin/sh
# Install the latest rsdu release binary.
#   INSTALL_DIR   target directory (default: ~/.local/bin)
#   RSDU_VERSION  release tag to install (default: latest)
set -eu

repo="awaiden/rs-suite"
dir="${INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)              target=x86_64-unknown-linux-gnu ;;
  Linux-aarch64|Linux-arm64) target=aarch64-unknown-linux-gnu ;;
  Darwin-arm64)              target=aarch64-apple-darwin ;;
  *) echo "No prebuilt binary for $(uname -s) $(uname -m). Try: cargo install --git https://github.com/$repo rsdu" >&2; exit 1 ;;
esac

if [ -n "${RSDU_VERSION:-}" ]; then
  tag="$RSDU_VERSION"
else
  tag="$(
    curl -fsSL "https://api.github.com/repos/$repo/releases?per_page=100" \
      | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' \
      | grep -E '^(rsdu-v|v[0-9])' \
      | head -n 1 || true
  )"
  if [ -z "$tag" ]; then
    echo "error: failed to determine latest rsdu release tag from GitHub" >&2
    exit 1
  fi
fi

base="https://github.com/$repo/releases/download/$tag"
name="rsdu-$tag-$target"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading $name..."
curl -fsSL "$base/$name.tar.gz" -o "$tmp/$name.tar.gz"
curl -fsSL "$base/$name.tar.gz.sha256" -o "$tmp/$name.tar.gz.sha256"

echo "Verifying checksum..."
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$tmp" && sha256sum -c "$name.tar.gz.sha256")
elif command -v shasum >/dev/null 2>&1; then
  (cd "$tmp" && shasum -a 256 -c "$name.tar.gz.sha256")
else
  echo "error: cannot verify checksum (neither sha256sum nor shasum is installed)" >&2
  exit 1
fi

tar xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$dir"
install -m 755 "$tmp/$name/rsdu" "$dir/rsdu"
echo "Installed rsdu $tag to $dir/rsdu"
case ":$PATH:" in *":$dir:"*) ;; *) echo "Note: $dir is not on your PATH." ;; esac
