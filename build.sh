#!/bin/bash
# build.sh — Compila quick-actions y copia el .exe a C:\quick-actions
# Uso: ./build.sh [--no-check p/ saltar lints y tests]

set -e

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WINDOWS_TARGET="x86_64-pc-windows-gnu"
RELEASE_DIR="$PROJECT_DIR/target/$WINDOWS_TARGET/release"
DEST_DIR="/mnt/c/quick-actions"
# p/ CC/AR implicitamente con .cargo/config.toml
cd "$PROJECT_DIR"

SKIP_CHECKS=false
[[ "${1:-}" == "--no-check" ]] && SKIP_CHECKS=true

echo "=== Quick Actions Build ==="

if [[ "$SKIP_CHECKS" == false ]]; then
  echo ">> cargo fmt --all --check"
  cargo fmt --all --check
  echo ">> cargo test --lib (cross-platform)"
  cargo test --lib
  echo "OK"
fi

echo "Compilando para Windows ($WINDOWS_TARGET)..."

# Build release en Windows
cargo build --release --target "$WINDOWS_TARGET"

echo "Copiando a $DEST_DIR ..."
mkdir -p "$DEST_DIR"
cp "$RELEASE_DIR/quick-actions.exe" "$DEST_DIR/"
# Config: solo si no existe en destino — no pisar la personalizada del usuario.
if [ ! -f "$DEST_DIR/config.toml" ]; then
    cp "$PROJECT_DIR/config.toml" "$DEST_DIR/"
    echo "Config copiada (no existia)."
else
    echo "Config existente conservada en $DEST_DIR/config.toml."
fi

echo ""
echo "Build completado exitosamente."
echo "Archivos en: $DEST_DIR"
ls -lh "$DEST_DIR"
