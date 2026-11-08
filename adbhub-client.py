#!/usr/bin/env python3
"""
ADBHUB-LINK — Estación del Cliente para Linux (Lubuntu / Ubuntu / Debian)
Arquitectura Directa USB/ADB:
- Detecta exclusivamente dispositivos Android conectados por cable USB físico local.
- Expone el puente TCP seguro en 0.0.0.0:5555 hacia el dispositivo USB local (127.0.0.1:15555).
- Interfaz gráfica profesional Dark-Slate responsiva con Scroll (Tkinter) sin consolas emergentes,
  con modo silencioso en segundo plano (--daemon) para servidores/VPS o ejecución por SSH.
"""

import os
import sys
import json
import time
import socket
import random
import threading
import subprocess

LOCAL_FORWARD_PORT = 15555
PUBLIC_PORT = 5555

# Evitar que el demonio ADB local escanee el puerto 5555 como si fuera emulator-5554
os.environ["ADB_LOCAL_TRANSPORT_MAX_PORT"] = "5553"

_last_forwarded_serial = None
_forward_lock = threading.Lock()
_activity_log = []
_tunnel_port = 0
_station_code = ""
_code_salt = 0x24


def log_event(tag, msg):
    ts = time.strftime("%H:%M:%S")
    entry = f"[{ts}] [{tag}] {msg}"
    _activity_log.insert(0, (tag, entry))
    if len(_activity_log) > 80:
        _activity_log.pop()
    print(entry, flush=True)


def get_local_ip():
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.connect(("8.8.8.8", 80))
        ip = s.getsockname()[0]
        s.close()
        return ip
    except Exception:
        return "127.0.0.1"


def encode_station(local_ip, cloud_tunnel_port=0, salt_byte=0x24):
    """Codifica la IP LAN y el puerto de Relay Global (bore.pub) en un código XXXX-XXXX-XXXX-XXXX."""
    try:
        octets = [int(x) & 0xFF for x in local_ip.split(".")]
        if len(octets) != 4:
            octets = [127, 0, 0, 1]
    except Exception:
        octets = [127, 0, 0, 1]

    salt = salt_byte if (salt_byte & 0xFF) != 0 else 0x42
    port = int(cloud_tunnel_port) & 0xFFFF
    raw = [
        0xAD ^ salt,
        salt,
        octets[0] ^ salt ^ 0x5A,
        octets[1] ^ salt ^ 0xA5,
        octets[2] ^ salt ^ 0x3C,
        octets[3] ^ salt ^ 0xC3,
        ((port >> 8) & 0xFF) ^ salt ^ 0x7E,
        (port & 0xFF) ^ salt ^ 0xE7,
    ]
    hex_chars = "0123456789ABCDEF"
    s = "".join(hex_chars[(b >> 4) & 0xF] + hex_chars[b & 0xF] for b in raw)
    return f"{s[0:4]}-{s[4:8]}-{s[8:12]}-{s[12:16]}"


def start_bore_tunnel(local_ip, local_port=PUBLIC_PORT):
    """Conecta con bore.pub:7835 para exponer un túnel TCP público compatible con VPN e Internet."""
    global _tunnel_port, _station_code
    while True:
        ctrl = None
        try:
            ctrl = socket.create_connection(("bore.pub", 7835), timeout=8.0)
            ctrl.sendall(b'{"Hello":0}\x00')
            buf = b""
            while b"\x00" not in buf:
                chunk = ctrl.recv(1024)
                if not chunk:
                    raise ConnectionError("EOF en saludo bore.pub")
                buf += chunk
            msg_bytes, rest = buf.split(b"\x00", 1)
            hello = json.loads(msg_bytes.decode("utf-8", errors="ignore"))
            if "Hello" in hello and int(hello["Hello"]) > 0:
                _tunnel_port = int(hello["Hello"])
                _station_code = encode_station(local_ip, _tunnel_port, _code_salt)
                log_event(
                    "TUNEL",
                    f"Relay Global VPN/Internet activo en bore.pub:{_tunnel_port} -> Código Estación: {_station_code}",
                )

                ctrl.settimeout(None)
                buf = rest
                while True:
                    while b"\x00" in buf:
                        frame_bytes, buf = buf.split(b"\x00", 1)
                        if not frame_bytes:
                            continue
                        frame = json.loads(frame_bytes.decode("utf-8", errors="ignore"))
                        if "Connection" in frame:
                            uuid_str = frame["Connection"]
                            threading.Thread(
                                target=_handle_bore_connection,
                                args=(uuid_str, local_port),
                                daemon=True,
                            ).start()
                    chunk = ctrl.recv(2048)
                    if not chunk:
                        break
                    buf += chunk
        except Exception as e:
            log_event("AVISO", f"Reconectando túnel global bore.pub ({e})...")
        finally:
            if ctrl:
                try:
                    ctrl.close()
                except Exception:
                    pass
        time.sleep(5.0)


def _handle_bore_connection(uuid_str, _local_port):
    remote = None
    local = None
    try:
        remote = socket.create_connection(("bore.pub", 7835), timeout=6.0)
        accept_payload = json.dumps({"Accept": uuid_str}).encode("utf-8") + b"\x00"
        remote.sendall(accept_payload)
        remote.settimeout(None)

        if not is_forward_active():
            if not setup_adb_usb_forward():
                remote.close()
                return

        local = socket.create_connection(("127.0.0.1", LOCAL_FORWARD_PORT), timeout=4.0)
        local.settimeout(None)
        log_event("TUNEL", "Técnico remoto conectado vía Relay Global (bore.pub)")

        t1 = threading.Thread(target=pipe_sockets, args=(remote, local), daemon=True)
        t2 = threading.Thread(target=pipe_sockets, args=(local, remote), daemon=True)
        t1.start()
        t2.start()
    except Exception:
        for s in (remote, local):
            if s:
                try:
                    s.close()
                except Exception:
                    pass


def get_connected_usb_device():
    """Consulta únicamente dispositivos físicos conectados por USB (excluye IP:puerto)."""
    try:
        out = subprocess.check_output(
            ["adb", "devices", "-l"], text=True, stderr=subprocess.DEVNULL, timeout=4
        )
        for line in out.strip().splitlines():
            line = line.strip()
            if not line or line.startswith("List of") or line.startswith("*"):
                continue
            parts = line.split()
            if len(parts) >= 2:
                serial = parts[0]
                if ":" in serial or serial.startswith("emulator-"):
                    continue
                state = parts[1]
                model = "Dispositivo Android"
                product = "USB"
                for p in parts[2:]:
                    if p.startswith("model:"):
                        model = p.replace("model:", "").replace("_", " ")
                    elif p.startswith("product:"):
                        product = p.replace("product:", "")
                return serial, state, model, product
    except Exception:
        pass
    return None, None, None, None


def get_device_extra_props(serial):
    """Obtiene versión de Android y nivel de batería si el dispositivo está autorizado."""
    android_ver = "N/D"
    battery = "N/D"
    patch = "N/D"
    try:
        v = subprocess.check_output(
            ["adb", "-s", serial, "shell", "getprop", "ro.build.version.release"],
            text=True,
            stderr=subprocess.DEVNULL,
            timeout=2,
        ).strip()
        if v:
            android_ver = f"Android {v}"
    except Exception:
        pass
    try:
        p = subprocess.check_output(
            ["adb", "-s", serial, "shell", "getprop", "ro.build.version.security_patch"],
            text=True,
            stderr=subprocess.DEVNULL,
            timeout=2,
        ).strip()
        if p:
            patch = p
    except Exception:
        pass
    try:
        b_out = subprocess.check_output(
            ["adb", "-s", serial, "shell", "dumpsys", "battery"],
            text=True,
            stderr=subprocess.DEVNULL,
            timeout=2,
        )
        for line in b_out.splitlines():
            if line.strip().startswith("level:"):
                battery = line.split(":", 1)[1].strip() + "%"
                break
    except Exception:
        pass
    return android_ver, battery, patch


def is_forward_active():
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.settimeout(0.3)
            return s.connect_ex(("127.0.0.1", LOCAL_FORWARD_PORT)) == 0
    except Exception:
        return False


def setup_adb_usb_forward(force=False):
    global _last_forwarded_serial
    with _forward_lock:
        serial, state, model, _ = get_connected_usb_device()
        if not serial or state != "device":
            _last_forwarded_serial = None
            return False

        if is_forward_active() and serial == _last_forwarded_serial and not force:
            return True

        log_event("ADB", f"Configurando puente USB para {model} ({serial})...")

        try:
            subprocess.run(
                ["adb", "-s", serial, "tcpip", "5555"],
                capture_output=True,
                timeout=6,
            )
            time.sleep(1.5)
            subprocess.run(
                ["adb", "-s", serial, "wait-for-device"],
                capture_output=True,
                timeout=6,
            )
        except Exception as e:
            log_event("AVISO", f"Espera de reinicio TCP en móvil: {e}")

        try:
            subprocess.run(
                ["adb", "-s", serial, "forward", "--remove", f"tcp:{LOCAL_FORWARD_PORT}"],
                capture_output=True,
            )
            res = subprocess.run(
                [
                    "adb",
                    "-s",
                    serial,
                    "forward",
                    f"tcp:{LOCAL_FORWARD_PORT}",
                    "tcp:5555",
                ],
                capture_output=True,
                text=True,
                timeout=6,
            )
            if res.returncode != 0:
                log_event("ERROR", f"Fallo en adb forward: {res.stderr.strip()}")
                return False
        except Exception as e:
            log_event("ERROR", f"Excepción en adb forward: {e}")
            return False

        time.sleep(0.4)
        if is_forward_active():
            _last_forwarded_serial = serial
            log_event(
                "PUENTE",
                f"Túnel USB activo en 127.0.0.1:{LOCAL_FORWARD_PORT} -> 0.0.0.0:{PUBLIC_PORT}",
            )
            return True
        else:
            log_event("AVISO", f"El puerto interno {LOCAL_FORWARD_PORT} aún no responde.")
            return False


def pipe_sockets(src, dst):
    try:
        while True:
            data = src.recv(8192)
            if not data:
                break
            dst.sendall(data)
    except Exception:
        pass
    finally:
        for s in (src, dst):
            try:
                s.close()
            except Exception:
                pass


def start_tcp_bridge(bind_ip, listen_port=PUBLIC_PORT):
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        server.bind((bind_ip, listen_port))
        server.listen(12)
        log_event("RED", f"Servidor Puente TCP escuchando en {bind_ip}:{listen_port}")
    except Exception as e:
        log_event("ERROR", f"No se pudo abrir el puerto {listen_port}: {e}")
        return

    while True:
        client_sock = None
        try:
            client_sock, addr = server.accept()
            log_event("CONEXION", f"Técnico conectado desde {addr[0]}:{addr[1]}")

            if not is_forward_active():
                if not setup_adb_usb_forward():
                    log_event("ERROR", "Sin teléfono USB autorizado para puentear.")
                    client_sock.close()
                    continue

            target_sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            target_sock.settimeout(5.0)
            try:
                target_sock.connect(("127.0.0.1", LOCAL_FORWARD_PORT))
                target_sock.settimeout(None)
            except Exception:
                if setup_adb_usb_forward(force=True):
                    try:
                        target_sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                        target_sock.connect(("127.0.0.1", LOCAL_FORWARD_PORT))
                    except Exception:
                        client_sock.close()
                        continue
                else:
                    client_sock.close()
                    continue

            t1 = threading.Thread(
                target=pipe_sockets, args=(client_sock, target_sock), daemon=True
            )
            t2 = threading.Thread(
                target=pipe_sockets, args=(target_sock, client_sock), daemon=True
            )
            t1.start()
            t2.start()
        except Exception:
            if client_sock:
                try:
                    client_sock.close()
                except Exception:
                    pass
            time.sleep(0.2)


def run_headless_loop(code, local_ip):
    log_event(
        "INICIO",
        f"Estación Linux activa en {local_ip}:{PUBLIC_PORT} (ID: {code})",
    )
    last_serial = None
    last_state = None
    while True:
        serial, state, model, _ = get_connected_usb_device()
        if serial:
            if serial != last_serial or state != last_state:
                log_event("USB", f"Dispositivo detectado: {model} ({serial}) [{state}]")
                if state == "device" and not is_forward_active():
                    setup_adb_usb_forward()
                last_serial = serial
                last_state = state
            elif state == "device" and not is_forward_active():
                setup_adb_usb_forward()
        else:
            if last_serial is not None:
                log_event("USB", "Teléfono USB desconectado.")
            last_serial = None
            last_state = None
        time.sleep(2.0)


def start_gui(code, local_ip):
    if "--daemon" in sys.argv or "--headless" in sys.argv:
        run_headless_loop(code, local_ip)
        return

    if not os.environ.get("DISPLAY"):
        # Intentar usar DISPLAY=:0 si existe sesión X11 local en Lubuntu
        if os.path.exists("/tmp/.X11-unix/X0"):
            os.environ["DISPLAY"] = ":0"
        else:
            run_headless_loop(code, local_ip)
            return

    try:
        import tkinter as tk
        from tkinter import ttk
        import tkinter.font as tkfont

        root = tk.Tk()
        root.title("ADBHUB-LINK — Estación del Cliente (Linux)")
        root.geometry("980x640")
        root.minsize(680, 440)
        root.configure(bg="#090d16")

        # Iniciar maximizado (sin pantalla completa invasiva, conservando barra de título estándar)
        try:
            root.state("zoomed")
        except Exception:
            try:
                root.attributes("-zoomed", True)
            except Exception:
                pass

        # Detección inteligente de la mejor tipografía instalada
        available_fonts = set(tkfont.families(root))
        prop_font = "DejaVu Sans"
        for candidate in ["Inter", "Ubuntu", "Segoe UI", "Roboto", "DejaVu Sans", "Helvetica", "Arial"]:
            if candidate in available_fonts:
                prop_font = candidate
                break

        mono_font = "DejaVu Sans Mono"
        for candidate in ["Cascadia Code", "JetBrains Mono", "Ubuntu Mono", "DejaVu Sans Mono", "Consolas", "Courier"]:
            if candidate in available_fonts:
                mono_font = candidate
                break

        # Tipografía responsiva autoadaptativa - Escala Centralizada Design System
        f_title = tkfont.Font(family=prop_font, size=16, weight="bold")
        f_hero_name = tkfont.Font(family=prop_font, size=14, weight="bold")
        f_header = tkfont.Font(family=prop_font, size=12, weight="bold")
        f_badge = tkfont.Font(family=prop_font, size=10, weight="bold")
        f_body = tkfont.Font(family=prop_font, size=11)
        f_body_bold = tkfont.Font(family=prop_font, size=11, weight="bold")
        f_caption = tkfont.Font(family=prop_font, size=10)
        f_caption_bold = tkfont.Font(family=prop_font, size=10, weight="bold")
        f_code_hero = tkfont.Font(family=mono_font, size=18, weight="bold")
        f_code_sub = tkfont.Font(family=mono_font, size=11, weight="bold")
        f_mono_specs = tkfont.Font(family=mono_font, size=11)

        _last_scale = [1.0]

        def on_window_resize(event):
            if event.widget != root:
                return
            w = max(640, event.width)
            scale = round(max(0.95, min(1.30, w / 960.0)), 2)
            if abs(scale - _last_scale[0]) < 0.05:
                return
            _last_scale[0] = scale
            f_title.configure(size=max(14, int(round(16 * scale))))
            f_hero_name.configure(size=max(13, int(round(14 * scale))))
            f_header.configure(size=max(11, int(round(12 * scale))))
            f_badge.configure(size=max(10, int(round(10 * scale))))
            f_body.configure(size=max(10, int(round(11 * scale))))
            f_body_bold.configure(size=max(10, int(round(11 * scale))))
            f_caption.configure(size=max(9, int(round(10 * scale))))
            f_caption_bold.configure(size=max(9, int(round(10 * scale))))
            f_code_hero.configure(size=max(16, int(round(18 * scale))))
            f_code_sub.configure(size=max(10, int(round(11 * scale))))
            f_mono_specs.configure(size=max(10, int(round(11 * scale))))

        root.bind("<Configure>", on_window_resize)

        # Paleta Enterprise Dark Slate
        BG_CANVAS = "#090d16"
        BG_HEADER = "#0d1320"
        BG_CARD = "#111827"
        BG_RECESSED = "#070a11"
        BORDER_SUBTLE = "#1e293b"
        ACCENT_BLUE = "#0ea5e9"
        ACCENT_BLUE_DARK = "#0369a1"
        STATUS_GREEN = "#10b981"
        STATUS_AMBER = "#f59e0b"
        TEXT_PRIMARY = "#f1f5f9"
        TEXT_SECONDARY = "#94a3b8"
        TEXT_MUTED = "#64748b"

        hostname = socket.gethostname()

        # 1. TOP HEADER BAR
        top_bar = tk.Frame(
            root, bg=BG_HEADER, padx=16, pady=10, highlightthickness=1, highlightbackground=BORDER_SUBTLE
        )
        top_bar.pack(fill="x", side="top")

        title_frame = tk.Frame(top_bar, bg=BG_HEADER)
        title_frame.pack(side="left")

        tk.Label(
            title_frame,
            text="ADBHUB-LINK",
            font=f_title,
            fg="#ffffff",
            bg=BG_HEADER,
        ).pack(side="left")

        tk.Label(
            title_frame,
            text="  ESTACIÓN DEL CLIENTE LINUX  ",
            font=f_badge,
            fg="#38bdf8",
            bg="#0c2d48",
            padx=6,
            pady=2,
        ).pack(side="left", padx=8)

        status_badge_var = tk.StringVar(value="ESPERANDO CABLE USB")
        status_lbl = tk.Label(
            top_bar,
            textvariable=status_badge_var,
            font=f_header,
            fg=TEXT_SECONDARY,
            bg=BG_HEADER,
        )
        status_lbl.pack(side="right", padx=8)

        # 2. BOTTOM FOOTER
        footer = tk.Frame(
            root, bg=BG_HEADER, padx=16, pady=6, highlightthickness=1, highlightbackground=BORDER_SUBTLE
        )
        footer.pack(fill="x", side="bottom")

        tk.Label(
            footer,
            text=f"ADBHUB-LINK v2.0 • Host: {hostname} • Puerto Puente: {local_ip}:5555",
            font=f_body,
            fg=TEXT_MUTED,
            bg=BG_HEADER,
        ).pack(side="left")

        toast_var = tk.StringVar(value="Sistema activo y escuchando en el puerto 5555.")
        tk.Label(
            footer,
            textvariable=toast_var,
            font=f_body_bold,
            fg="#38bdf8",
            bg=BG_HEADER,
        ).pack(side="right")

        # 3. SCROLLABLE MAIN CONTAINER
        container = tk.Frame(root, bg=BG_CANVAS)
        container.pack(fill="both", expand=True)

        canvas = tk.Canvas(container, bg=BG_CANVAS, highlightthickness=0)
        scrollbar = ttk.Scrollbar(container, orient="vertical", command=canvas.yview)
        scrollable_frame = tk.Frame(canvas, bg=BG_CANVAS, padx=14, pady=12)

        scrollable_frame.bind(
            "<Configure>",
            lambda e: canvas.configure(scrollregion=canvas.bbox("all")),
        )
        canvas_window = canvas.create_window((0, 0), window=scrollable_frame, anchor="nw")
        canvas.bind(
            "<Configure>",
            lambda e: canvas.itemconfig(canvas_window, width=e.width),
        )
        canvas.configure(yscrollcommand=scrollbar.set)

        canvas.pack(side="left", fill="both", expand=True)
        scrollbar.pack(side="right", fill="y")

        # Responsive 2-column grid inside scrollable_frame
        scrollable_frame.columnconfigure(0, weight=2, minsize=260)
        scrollable_frame.columnconfigure(1, weight=3, minsize=340)

        # LEFT COLUMN: CREDENTIALS & CHECKLIST
        left_col = tk.Frame(scrollable_frame, bg=BG_CANVAS)
        left_col.grid(row=0, column=0, sticky="nsew", padx=(0, 8), pady=4)

        cred_card = tk.Frame(
            left_col, bg=BG_CARD, padx=14, pady=14, highlightthickness=1, highlightbackground=BORDER_SUBTLE
        )
        cred_card.pack(fill="x", pady=(0, 10))

        tk.Label(
            cred_card,
            text="CREDENCIALES DE ESTA ESTACIÓN",
            font=f_header,
            fg=TEXT_SECONDARY,
            bg=BG_CARD,
        ).pack(anchor="w")

        tk.Label(
            cred_card,
            text="Comparta esta dirección IP o código con el técnico.",
            font=f_body,
            fg=TEXT_MUTED,
            bg=BG_CARD,
        ).pack(anchor="w", pady=(2, 8))

        ip_box = tk.Frame(
            cred_card, bg=BG_RECESSED, padx=12, pady=10, highlightthickness=1, highlightbackground="#263854"
        )
        ip_box.pack(fill="x")

        code_var = tk.StringVar(value=f"{_station_code}")
        tk.Label(
            ip_box,
            text="CÓDIGO DE ESTACIÓN (COMPATIBLE CON VPN / INTERNET)",
            font=f_caption_bold,
            fg=TEXT_MUTED,
            bg=BG_RECESSED,
        ).pack()

        tk.Label(
            ip_box,
            textvariable=code_var,
            font=f_code_hero,
            fg="#38bdf8",
            bg=BG_RECESSED,
        ).pack(pady=3)

        endpoint_str = f"{local_ip}:5555"
        relay_var = tk.StringVar(value=f"LAN: {endpoint_str}  •  Relay: Conectando...")
        tk.Label(
            ip_box,
            textvariable=relay_var,
            font=f_code_sub,
            fg=STATUS_GREEN,
            bg=BG_RECESSED,
        ).pack()

        def copy_station_code():
            root.clipboard_clear()
            root.clipboard_append(_station_code)
            toast_var.set(f"Código {_station_code} copiado al portapapeles.")

        def copy_endpoint():
            root.clipboard_clear()
            root.clipboard_append(endpoint_str)
            toast_var.set(f"Dirección {endpoint_str} copiada al portapapeles.")

        tk.Button(
            cred_card,
            text="COPIAR CÓDIGO DE ESTACIÓN",
            command=copy_station_code,
            bg=ACCENT_BLUE_DARK,
            fg="white",
            activebackground=ACCENT_BLUE,
            activeforeground="white",
            font=f_header,
            relief="flat",
            pady=6,
            cursor="hand2",
        ).pack(fill="x", pady=(8, 4))

        tk.Button(
            cred_card,
            text="COPIAR IP LAN DIRECTA",
            command=copy_endpoint,
            bg="#1e293b",
            fg=TEXT_PRIMARY,
            activebackground=ACCENT_BLUE_DARK,
            activeforeground="white",
            font=f_body_bold,
            relief="flat",
            pady=4,
            cursor="hand2",
        ).pack(fill="x")

        # Checklist Card
        chk_card = tk.Frame(
            left_col, bg=BG_CARD, padx=14, pady=12, highlightthickness=1, highlightbackground=BORDER_SUBTLE
        )
        chk_card.pack(fill="x")

        tk.Label(
            chk_card,
            text="LISTA DE VERIFICACIÓN USB",
            font=f_body_bold,
            fg=TEXT_SECONDARY,
            bg=BG_CARD,
        ).pack(anchor="w", pady=(0, 6))

        step1_var = tk.StringVar(value="[1] Cable USB Físico: Buscando...")
        step2_var = tk.StringVar(value="[2] Depuración USB: Pendiente")
        step3_var = tk.StringVar(value="[3] Túnel TCP 5555: En espera")

        for svar in (step1_var, step2_var, step3_var):
            tk.Label(
                chk_card,
                textvariable=svar,
                font=f_code_sub,
                fg=TEXT_PRIMARY,
                bg=BG_RECESSED,
                padx=8,
                pady=6,
                anchor="w",
            ).pack(fill="x", pady=3)

        # RIGHT COLUMN: DEVICE DETAILS & GUIDE
        right_col = tk.Frame(scrollable_frame, bg=BG_CANVAS)
        right_col.grid(row=0, column=1, sticky="nsew", padx=(8, 0), pady=4)

        dev_card = tk.Frame(
            right_col, bg=BG_CARD, padx=16, pady=14, highlightthickness=1, highlightbackground=BORDER_SUBTLE
        )
        dev_card.pack(fill="x", pady=(0, 10))

        tk.Label(
            dev_card,
            text="DISPOSITIVO USB CONECTADO A ESTA ESTACIÓN",
            font=f_body_bold,
            fg=TEXT_SECONDARY,
            bg=BG_CARD,
        ).pack(anchor="w")

        dev_name_var = tk.StringVar(value="Escaneando puertos USB locales...")
        dev_name_lbl = tk.Label(
            dev_card,
            textvariable=dev_name_var,
            font=f_hero_name,
            fg="#ffffff",
            bg=BG_CARD,
        )
        dev_name_lbl.pack(anchor="w", pady=(6, 2))

        dev_specs_var = tk.StringVar(value="Serial: N/D  •  Android: N/D  •  Batería: N/D")
        tk.Label(
            dev_card,
            textvariable=dev_specs_var,
            font=f_mono_specs,
            fg=TEXT_SECONDARY,
            bg=BG_CARD,
        ).pack(anchor="w", pady=(0, 8))

        def manual_bridge():
            toast_var.set("Reconfigurando puente TCP 5555...")
            threading.Thread(target=lambda: setup_adb_usb_forward(force=True), daemon=True).start()

        tk.Button(
            dev_card,
            text="RECONECTAR PUENTE USB -> TCP 5555",
            command=manual_bridge,
            bg="#1e293b",
            fg="#38bdf8",
            activebackground=ACCENT_BLUE_DARK,
            activeforeground="white",
            font=f_body_bold,
            relief="flat",
            padx=10,
            pady=5,
            cursor="hand2",
        ).pack(anchor="w")

        # Guide Card
        guide_card = tk.Frame(
            right_col, bg=BG_CARD, padx=16, pady=14, highlightthickness=1, highlightbackground=BORDER_SUBTLE
        )
        guide_card.pack(fill="both", expand=True)

        tk.Label(
            guide_card,
            text="TUTORIAL: CONEXIÓN REMOTA CON EL TÉCNICO",
            font=f_header,
            fg="#38bdf8",
            bg=BG_CARD,
        ).pack(anchor="w", pady=(0, 6))

        guide_text = (
            "1. Conecte el teléfono Android por cable USB a esta computadora.\n"
            "2. En el teléfono: Ajustes > Acerca del teléfono > Toque 7 veces 'Número de compilación'.\n"
            "3. En Opciones para desarrolladores: active 'Depuración por USB'.\n"
            "4. En esta pantalla, pulse 'COPIAR CÓDIGO DE ESTACIÓN' y envíeselo al técnico.\n"
            "5. Cuando el técnico se conecte, en la pantalla del celular saldrá un aviso: marque 'Permitir siempre' y toque ACEPTAR."
        )
        tk.Label(
            guide_card,
            text=guide_text,
            font=f_body,
            fg=TEXT_PRIMARY,
            bg=BG_CARD,
            justify="left",
        ).pack(anchor="w")

        def poll_ui():
            code_var.set(_station_code)
            if _tunnel_port > 0:
                relay_var.set(f"LAN: {endpoint_str}  •  VPN/Relay: bore.pub:{_tunnel_port}")
            else:
                relay_var.set(f"LAN: {endpoint_str}  •  VPN/Relay: Conectando...")

            serial, state, model, _ = get_connected_usb_device()
            fwd_ok = is_forward_active()
            if serial:
                step1_var.set(f"[OK] Cable USB Físico: {serial}")
                step2_var.set("[OK] Depuración USB: Activa")
                if state == "device":
                    if not fwd_ok:
                        threading.Thread(target=setup_adb_usb_forward, daemon=True).start()
                    ver, bat, patch = get_device_extra_props(serial)
                    dev_name_var.set(f"{model}  [AUTORIZADO]")
                    dev_name_lbl.config(fg=STATUS_GREEN)
                    dev_specs_var.set(
                        f"Serial: {serial}  •  {ver}  •  Batería: {bat}  •  Parche: {patch}"
                    )
                    status_badge_var.set("EQUIPO USB LISTO Y PUENTEADO")
                    status_lbl.config(fg=STATUS_GREEN)
                    step3_var.set(f"[OK] Túnel TCP 5555: Activo ({local_ip}:5555)")
                else:
                    dev_name_var.set(f"{model}  [ACEPTAR PERMISO EN PANTALLA]")
                    dev_name_lbl.config(fg=STATUS_AMBER)
                    dev_specs_var.set(
                        f"Serial: {serial}  •  Estado: {state.upper()} (Autorizar RSA en el móvil)"
                    )
                    status_badge_var.set("AUTORIZACIÓN PENDIENTE")
                    status_lbl.config(fg=STATUS_AMBER)
                    step3_var.set("[..] Túnel TCP 5555: Esperando huella RSA")
            else:
                dev_name_var.set("Ningún teléfono detectado por cable USB")
                dev_name_lbl.config(fg=TEXT_SECONDARY)
                dev_specs_var.set("Conecte un dispositivo Android con Depuración USB activa.")
                status_badge_var.set("ESPERANDO CABLE USB")
                status_lbl.config(fg=TEXT_SECONDARY)
                step1_var.set("[1] Cable USB Físico: No detectado")
                step2_var.set("[2] Depuración USB: Pendiente")
                step3_var.set("[3] Túnel TCP 5555: En espera")

            root.after(2000, poll_ui)

        poll_ui()
        root.mainloop()
    except Exception as e:
        log_event("GUI", f"Pasando a modo servicio en segundo plano ({e})")
        run_headless_loop(_station_code, local_ip)


if __name__ == "__main__":
    local_ip = get_local_ip()
    _station_code = encode_station(local_ip, 0, _code_salt)

    t = threading.Thread(target=start_tcp_bridge, args=(local_ip, PUBLIC_PORT), daemon=True)
    t.start()

    tb = threading.Thread(target=start_bore_tunnel, args=(local_ip, PUBLIC_PORT), daemon=True)
    tb.start()

    start_gui(_station_code, local_ip)
