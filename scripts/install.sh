#!/usr/bin/env sh
# One-line installer for flipfile (macOS & Linux)
# Usage: curl -fsSL https://raw.githubusercontent.com/wcksnrdn/flipfile/main/scripts/install.sh | sh

set -eu

REPO="wcksnrdn/flipfile"
OS="$(uname -s)"
ARCH="$(uname -m)"

printf "=> Detecting latest flipfile release from GitHub (%s)...\n" "$REPO"
TAG="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')"
VERSION="${TAG#v}"

if [ -z "$VERSION" ]; then
  printf "Error: Could not determine latest release version.\n" >&2
  exit 1
fi

printf "=> Latest version: %s (%s / %s)\n" "$TAG" "$OS" "$ARCH"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64|aarch64) DMG_ARCH="aarch64" ;;
      x86_64)        DMG_ARCH="x64" ;;
      *) printf "Unsupported macOS architecture: %s\n" "$ARCH" >&2; exit 1 ;;
    esac
    URL="https://github.com/${REPO}/releases/download/${TAG}/flipfile_${VERSION}_${DMG_ARCH}.dmg"
    DMG_PATH="${TMP_DIR}/flipfile.dmg"
    printf "=> Downloading %s...\n" "$URL"
    curl -fL --progress-bar "$URL" -o "$DMG_PATH"

    MOUNT_POINT="$(hdiutil attach "$DMG_PATH" -nobrowse -noautoopen | awk '/\/Volumes\// {for (i=3; i<=NF; i++) printf "%s%s", $i, (i<NF ? OFS : ORS)}' | head -n 1)"
    printf "=> Installing flipfile.app to /Applications...\n"
    rm -rf "/Applications/flipfile.app"
    cp -R "${MOUNT_POINT}/flipfile.app" "/Applications/flipfile.app"
    hdiutil detach "$MOUNT_POINT" -quiet || true
    xattr -cr "/Applications/flipfile.app" 2>/dev/null || true
    printf "✓ flipfile %s installed to /Applications/flipfile.app!\n" "$TAG"
    ;;

  Linux)
    if command -v dpkg >/dev/null 2>&1; then
      URL="https://github.com/${REPO}/releases/download/${TAG}/flipfile_${VERSION}_amd64.deb"
      DEB_PATH="${TMP_DIR}/flipfile.deb"
      printf "=> Downloading %s...\n" "$URL"
      curl -fL --progress-bar "$URL" -o "$DEB_PATH"
      sudo apt-get install -y "$DEB_PATH"
      printf "✓ flipfile %s installed!\n" "$TAG"
    else
      URL="https://github.com/${REPO}/releases/download/${TAG}/flipfile_${VERSION}_amd64.AppImage"
      DEST="${HOME}/.local/bin/flipfile"
      mkdir -p "${HOME}/.local/bin"
      printf "=> Downloading %s...\n" "$URL"
      curl -fL --progress-bar "$URL" -o "$DEST"
      chmod +x "$DEST"
      printf "✓ flipfile %s installed to %s!\n" "$TAG" "$DEST"
    fi
    ;;

  *)
    printf "Unsupported OS: %s. For Windows, use scripts/install.ps1.\n" "$OS" >&2
    exit 1
    ;;
esac
