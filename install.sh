#!/bin/sh
set -e

REPO="datibbaw/fintrack"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64)  TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *) echo "Unsupported macOS architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
      *) echo "Unsupported Linux architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS" >&2
    exit 1
    ;;
esac

# Resolve latest release tag if not specified
if [ -z "$VERSION" ]; then
  VERSION="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
  if [ -z "$VERSION" ]; then
    echo "Failed to determine latest release version." >&2
    exit 1
  fi
fi

ARTIFACT="fintrack-$TARGET"
URL="https://github.com/$REPO/releases/download/$VERSION/$ARTIFACT.tar.gz"

echo "Installing fintrack $VERSION for $TARGET..."

# Download and extract to a temp directory
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL "$URL" | tar xz -C "$TMP"

# Remove macOS quarantine flag so Gatekeeper doesn't block the binary
if [ "$OS" = "Darwin" ]; then
  xattr -d com.apple.quarantine "$TMP/$ARTIFACT" 2>/dev/null || true
fi

# Install
mkdir -p "$INSTALL_DIR"
mv "$TMP/$ARTIFACT" "$INSTALL_DIR/fintrack"
chmod +x "$INSTALL_DIR/fintrack"

echo "Installed to $INSTALL_DIR/fintrack"

# Warn if the install dir isn't in PATH
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "Note: $INSTALL_DIR is not in your PATH. Add it to your shell profile." ;;
esac
