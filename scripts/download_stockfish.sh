#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="$ROOT_DIR/stockfish"
mkdir -p "$TARGET_DIR"

OS="$(uname -s)"
ARCH="$(uname -m)"

if [[ "$OS" == "Linux" && "$ARCH" == "x86_64" ]]; then
  URL="https://github.com/official-stockfish/Stockfish/releases/download/sf_17.1/stockfish-ubuntu-x86-64-avx2.tar"
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  curl -L "$URL" -o "$TMP/stockfish.tar"
  tar -xf "$TMP/stockfish.tar" -C "$TMP"
  BIN="$(find "$TMP" -type f -name 'stockfish-ubuntu*' | head -n1)"
  cp "$BIN" "$TARGET_DIR/stockfish"
  chmod +x "$TARGET_DIR/stockfish"
  echo "Installed $TARGET_DIR/stockfish"
else
  echo "Unsupported platform for automatic download: OS=$OS ARCH=$ARCH"
  echo "Please download Stockfish manually and place executable at $TARGET_DIR/stockfish (or stockfish.exe on Windows)."
  exit 1
fi
