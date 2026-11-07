#!/usr/bin/env bash
# ADBHUB-LINK — Instalador Automático para Lubuntu / Ubuntu / Debian
set -e

SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REAL_USER="${SUDO_USER:-$USER}"
REAL_HOME="$(getent passwd "$REAL_USER" | cut -d: -f6)"
if [ -z "$REAL_HOME" ]; then
    REAL_HOME="$HOME"
fi
APP_DIR="$REAL_HOME/adbhub"

echo "================================================================================"
echo " ADBHUB-LINK — Instalando Estación del Cliente en Linux ($REAL_USER)"
echo "================================================================================"

# 1. Instalar adb, python3 y python3-tk si faltan
if ! command -v adb >/dev/null 2>&1 || ! command -v python3 >/dev/null 2>&1; then
    echo ">>> Instalando paquetes necesarios (adb, python3, python3-tk)..."
    sudo apt-get update && sudo apt-get install -y adb android-sdk-platform-tools-common python3 python3-tk || sudo apt-get install -y adb python3 python3-tk
fi

# 2. Configurar permisos de grupo plugdev y puerto 5555
sudo usermod -aG plugdev "$REAL_USER" 2>/dev/null || true
if command -v ufw >/dev/null 2>&1; then
    sudo ufw allow 5555/tcp >/dev/null 2>&1 || true
fi

# 3. Copiar archivos a ~/adbhub
mkdir -p "$APP_DIR"
if [ -f "$SRC_DIR/adbhub-client.py" ] && [ "$SRC_DIR" != "$APP_DIR" ]; then
    cp -f "$SRC_DIR/adbhub-client.py" "$APP_DIR/adbhub-client.py"
fi
if [ -f "$SRC_DIR/iniciar-cliente.sh" ] && [ "$SRC_DIR" != "$APP_DIR" ]; then
    cp -f "$SRC_DIR/iniciar-cliente.sh" "$APP_DIR/iniciar-cliente.sh"
fi

chmod +x "$APP_DIR/adbhub-client.py" "$APP_DIR/iniciar-cliente.sh" 2>/dev/null || true
cp -f "$APP_DIR/iniciar-cliente.sh" "$REAL_HOME/iniciar-cliente.sh" 2>/dev/null || true
chmod +x "$REAL_HOME/iniciar-cliente.sh" 2>/dev/null || true
chown -R "$REAL_USER:$REAL_USER" "$APP_DIR" "$REAL_HOME/iniciar-cliente.sh" 2>/dev/null || true

# 4. Crear accesos directos de Escritorio sin consola (Terminal=false)
for DESK in "$REAL_HOME/Desktop" "$REAL_HOME/Escritorio"; do
    if [ -d "$DESK" ]; then
        cat <<EOF > "$DESK/ADBHUB-Cliente.desktop"
[Desktop Entry]
Type=Application
Name=ADBHUB-LINK Cliente
Comment=Estación del Cliente para Soporte Remoto ADB
Exec=python3 $APP_DIR/adbhub-client.py
Icon=phone
Terminal=false
Categories=Utility;Network;
EOF
        chmod +x "$DESK/ADBHUB-Cliente.desktop" 2>/dev/null || true
        chown "$REAL_USER:$REAL_USER" "$DESK/ADBHUB-Cliente.desktop" 2>/dev/null || true
    fi
done

echo "================================================================================"
echo " [OK] Instalación completada en $APP_DIR"
echo "================================================================================"
