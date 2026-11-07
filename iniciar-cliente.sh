#!/usr/bin/env bash
# ADBHUB-LINK — Lanzador Silencioso del Cliente Linux
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$HOME/adbhub"

if [ -f "$DIR/adbhub-client.py" ]; then
    exec python3 "$DIR/adbhub-client.py" "$@"
elif [ -f "$APP_DIR/adbhub-client.py" ]; then
    exec python3 "$APP_DIR/adbhub-client.py" "$@"
else
    echo "Error: No se encontró adbhub-client.py en $DIR ni en $APP_DIR"
    exit 1
fi
