#!/usr/bin/env bash
# ==============================================================================
# ADBHUB-LINK — Cliente Todo-en-Uno para Linux (Lubuntu / Ubuntu / Debian)
# Autodetecta dependencias (adb, python3, python3-tk, permisos USB).
# Si faltan, las instala automáticamente; si ya están, abre la app al instante.
# ==============================================================================
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$HOME/adbhub"

# 1. Comprobar si faltan dependencias básicas o módulo tkinter
NEED_INSTALL=false
if ! command -v adb >/dev/null 2>&1; then
    NEED_INSTALL=true
fi
if ! command -v python3 >/dev/null 2>&1; then
    NEED_INSTALL=true
elif ! python3 -c "import tkinter" >/dev/null 2>&1; then
    NEED_INSTALL=true
fi

# 2. Si falta algo, instalar dependencias y configurar permisos USB automáticamente
if [ "$NEED_INSTALL" = true ]; then
    echo "================================================================================"
    echo " ADBHUB-LINK: Configurando entorno y dependencias por primera vez..."
    echo "================================================================================"
    
    # Solicitar sudo solo si es necesario instalar paquetes
    if command -v apt-get >/dev/null 2>&1; then
        sudo apt-get update -qq || true
        sudo apt-get install -y -qq adb android-sdk-platform-tools-common python3 python3-tk 2>/dev/null || \
        sudo apt-get install -y -qq adb python3 python3-tk 2>/dev/null || true
    fi

    # Configurar permisos USB del usuario
    sudo usermod -aG plugdev "$USER" 2>/dev/null || true

    # Regla udev para permitir acceso total a dispositivos Android por USB sin root
    if [ ! -f /etc/udev/rules.d/51-android.rules ]; then
        echo 'SUBSYSTEM=="usb", ATTR{idVendor}=="*", MODE="0666", GROUP="plugdev"' | sudo tee /etc/udev/rules.d/51-android.rules >/dev/null 2>&1 || true
        sudo udevadm control --reload-rules 2>/dev/null || true
        sudo udevadm trigger 2>/dev/null || true
    fi

    # Configurar firewall ufw si está activo
    if command -v ufw >/dev/null 2>&1; then
        sudo ufw allow 5555/tcp >/dev/null 2>&1 || true
    fi

    # Instalar copia en ~/adbhub y crear acceso directo en el Escritorio
    mkdir -p "$APP_DIR"
    if [ -f "$DIR/adbhub-client.py" ]; then
        cp -f "$DIR/adbhub-client.py" "$APP_DIR/adbhub-client.py"
    fi
    cp -f "$0" "$APP_DIR/ADBHUB-LINK.sh" 2>/dev/null || true
    chmod +x "$APP_DIR/adbhub-client.py" "$APP_DIR/ADBHUB-LINK.sh" 2>/dev/null || true

    for DESK in "$HOME/Desktop" "$HOME/Escritorio"; do
        if [ -d "$DESK" ]; then
            cat <<EOF > "$DESK/ADBHUB-Cliente.desktop"
[Desktop Entry]
Type=Application
Name=ADBHUB-LINK Cliente
Comment=Estación del Cliente para Soporte Remoto ADB
Exec=bash $APP_DIR/ADBHUB-LINK.sh
Icon=phone
Terminal=false
Categories=Utility;Network;
EOF
            chmod +x "$DESK/ADBHUB-Cliente.desktop" 2>/dev/null || true
        fi
    done
    echo "[OK] Dependencias y permisos listos."
fi

# 3. Lanzar la aplicación
CLIENT_PY=""
if [ -f "$DIR/adbhub-client.py" ]; then
    CLIENT_PY="$DIR/adbhub-client.py"
elif [ -f "$APP_DIR/adbhub-client.py" ]; then
    CLIENT_PY="$APP_DIR/adbhub-client.py"
fi

if [ -n "$CLIENT_PY" ]; then
    exec python3 "$CLIENT_PY" "$@"
else
    echo "Error: No se encontró adbhub-client.py en $DIR ni en $APP_DIR"
    exit 1
fi

