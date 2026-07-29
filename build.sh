#!/bin/bash
# build.sh — Compila quick-actions y copia el .exe a C:\quick-actions

set -e

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WINDOWS_TARGET="x86_64-pc-windows-gnu"
RELEASE_DIR="$PROJECT_DIR/target/$WINDOWS_TARGET/release"
DEST_DIR="/mnt/c/quick-actions"

echo "=== Quick Actions Build ==="
echo "Compilando para Windows ($WINDOWS_TARGET)..."

cd "$PROJECT_DIR"
cargo build --release --target "$WINDOWS_TARGET"

echo "Copiando a $DEST_DIR ..."
mkdir -p "$DEST_DIR"
cp "$RELEASE_DIR/quick-actions.exe" "$DEST_DIR/"
cp "$PROJECT_DIR/config.toml" "$DEST_DIR/"

echo ""
echo "Build completado exitosamente."
echo "Archivos en: $DEST_DIR"
ls -lh "$DEST_DIR"
