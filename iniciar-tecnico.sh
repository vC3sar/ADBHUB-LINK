#!/usr/bin/env bash
# ADBHUB-LINK — Iniciar Estacion del Tecnico en Linux / Lubuntu
set -e
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

echo "================================================================================"
echo " Iniciando ADBHUB-LINK Tecnico (Modo Remoto ADB Estilo AnyDesk) en Linux"
echo "================================================================================"

if command -v ./adbhub-desktop >/dev/null 2>&1; then
    chmod +x ./adbhub-desktop
    ./adbhub-desktop
elif command -v target/release/adbhub-desktop >/dev/null 2>&1; then
    chmod +x target/release/adbhub-desktop
    target/release/adbhub-desktop
else
    cargo run --release -p adbhub-desktop
fi

