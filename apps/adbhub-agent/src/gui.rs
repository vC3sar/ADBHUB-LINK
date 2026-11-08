//! # `gui.rs` — Native Enterprise Desktop GUI for ADBHUB-LINK Agent (Client Station)
//!
//! Fully responsive, scrollable, multi-panel desktop interface using `TopBottomPanel`,
//! resizable `SidePanel`, `CentralPanel`, and adaptive 2/4-column grids so it fits any
//! screen resolution (from 1366x768 laptops to 4K monitors) without clipping or overflow.

use std::net::UdpSocket;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use adbhub_core::{AdbDeviceInfo, AdbDeviceState, AdbEngine, AnyDeskCode};
use adbhub_usb_discovery::{DiscoveryFilter, UsbDiscoveryEngine};
use eframe::egui::{
    self, Align, CentralPanel, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Frame,
    Layout, Margin, Panel, Pos2, RichText, ScrollArea, Sense, Stroke, Vec2,
};

// ============================================================================
// ENTERPRISE COLOR PALETTE
// ============================================================================
const BG_CANVAS: Color32 = Color32::from_rgb(9, 13, 22); // #090D16
const BG_HEADER: Color32 = Color32::from_rgb(13, 19, 32); // #0D1320
const BG_CARD: Color32 = Color32::from_rgb(17, 24, 39); // #111827
const BG_RECESSED: Color32 = Color32::from_rgb(7, 10, 17); // #070A11
const BORDER_SUBTLE: Color32 = Color32::from_rgb(30, 41, 59); // #1E293B
const BORDER_ACCENT: Color32 = Color32::from_rgb(56, 189, 248); // #38BDF8

const TEXT_PRIMARY: Color32 = Color32::from_rgb(241, 245, 249); // #F1F5F9
const TEXT_SECONDARY: Color32 = Color32::from_rgb(148, 163, 184); // #94A3B8
const TEXT_MUTED: Color32 = Color32::from_rgb(100, 116, 139); // #64748B

const ACCENT_BLUE: Color32 = Color32::from_rgb(14, 165, 233); // #0EA5E9
const ACCENT_BLUE_DARK: Color32 = Color32::from_rgb(3, 105, 161); // #0369A1
const STATUS_GREEN: Color32 = Color32::from_rgb(16, 185, 129); // #10B981
const STATUS_GREEN_BG: Color32 = Color32::from_rgb(6, 58, 43);
const STATUS_AMBER: Color32 = Color32::from_rgb(245, 158, 11); // #F59E0B
const STATUS_AMBER_BG: Color32 = Color32::from_rgb(69, 39, 7);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientTab {
    Tutorial,
    Devices,
    BrandGuide,
    ActivityLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhoneBrand {
    Samsung,
    Xiaomi,
    Motorola,
    TecnoInfinix,
    HuaweiHonor,
}

enum HardwareUpdate {
    ScanResult {
        adb_devices: Vec<AdbDeviceInfo>,
        usb_mobiles: Vec<String>,
    },
    BridgeEnabled {
        serial: String,
        success: bool,
    },
    TunnelPortAssigned {
        port: u16,
    },
}

pub struct AgentGuiApp {
    code: AnyDeskCode,
    code_salt: u8,
    tunnel_port: u16,
    adb: AdbEngine,
    local_ip: String,
    host_name: String,
    local_adb_devices: Vec<AdbDeviceInfo>,
    detected_usb_mobiles: Vec<String>,
    last_scan: Instant,
    scan_in_progress: bool,
    hw_tx: Sender<HardwareUpdate>,
    hw_rx: Receiver<HardwareUpdate>,
    active_tab: ClientTab,
    selected_brand: PhoneBrand,
    status_message: String,
    activity_log: Vec<(String, String)>,
}

impl AgentGuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::configure_fonts_and_theme(&cc.egui_ctx);

        let adb = AdbEngine::new();
        let (hw_tx, hw_rx) = mpsc::channel();

        let host_name = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "ESTACION-CLIENTE".to_string());
        let local_ip = Self::detect_local_ipv4();
        let code_salt: u8 = 0x24;
        let code = AnyDeskCode::encode_station(&local_ip, 0, code_salt);

        let tx_tunnel = hw_tx.clone();
        AdbEngine::spawn_cloud_tunnel(5555, move |port| {
            let _ = tx_tunnel.send(HardwareUpdate::TunnelPortAssigned { port });
        });

        let log = vec![
            (
                "INICIO".to_string(),
                format!("Estación inicializada en {host_name} (IP LAN: {local_ip})"),
            ),
            (
                "TUNEL".to_string(),
                "Conectando relay global VPN/Internet (bore.pub) para soporte por código..."
                    .to_string(),
            ),
            (
                "MOTOR".to_string(),
                "ADB Platform-Tools Oficial v36 activo (Modo silencioso sin consolas)".to_string(),
            ),
            (
                "FILTRO".to_string(),
                "Aislamiento de seguridad activo: solo se listan puertos USB físicos locales"
                    .to_string(),
            ),
        ];

        let mut app = Self {
            code,
            code_salt,
            tunnel_port: 0,
            adb,
            local_ip,
            host_name,
            local_adb_devices: Vec::new(),
            detected_usb_mobiles: Vec::new(),
            last_scan: Instant::now(),
            scan_in_progress: false,
            hw_tx,
            hw_rx,
            active_tab: ClientTab::Devices,
            selected_brand: PhoneBrand::Samsung,
            status_message: "Sistema listo. Esperando conexión o dispositivo USB.".to_string(),
            activity_log: log,
        };

        app.trigger_hardware_scan(true);
        app
    }

    fn configure_fonts_and_theme(ctx: &egui::Context) {
        let mut fonts = FontDefinitions::default();

        let prop_candidates = [
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "C:\\Windows\\Fonts\\segoeuib.ttf",
            "C:\\Windows\\Fonts\\calibri.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
            "/usr/share/fonts/truetype/inter/Inter-Regular.ttf",
            "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
        ];
        for path in prop_candidates {
            if let Ok(bytes) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("sys_prop".to_owned(), FontData::from_owned(bytes).into());
                if let Some(vec) = fonts.families.get_mut(&FontFamily::Proportional) {
                    vec.insert(0, "sys_prop".to_owned());
                }
                break;
            }
        }

        let mono_candidates = [
            "C:\\Windows\\Fonts\\CascadiaCode.ttf",
            "C:\\Windows\\Fonts\\CascadiaMono.ttf",
            "C:\\Windows\\Fonts\\consola.ttf",
            "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf",
            "/usr/share/fonts/truetype/cascadia-code/CascadiaCode.ttf",
            "/usr/share/fonts/truetype/ubuntu/UbuntuMono-R.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/freefont/FreeMono.ttf",
        ];
        for path in mono_candidates {
            if let Ok(bytes) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("sys_mono".to_owned(), FontData::from_owned(bytes).into());
                if let Some(vec) = fonts.families.get_mut(&FontFamily::Monospace) {
                    vec.insert(0, "sys_mono".to_owned());
                }
                break;
            }
        }

        ctx.set_fonts(fonts);
        ctx.set_theme(egui::Theme::Dark);

        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(10.0, 6.0);
        style.spacing.window_margin = Margin::same(0);
        style.spacing.scroll = egui::style::ScrollStyle::solid();
        style.spacing.scroll.bar_width = 7.0;

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG_CANVAS;
        visuals.window_fill = BG_CANVAS;
        visuals.extreme_bg_color = BG_RECESSED;
        visuals.faint_bg_color = BG_CARD;
        visuals.widgets.noninteractive.bg_fill = BG_CARD;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
        visuals.widgets.noninteractive.corner_radius = CornerRadius::same(6);

        visuals.widgets.inactive.bg_fill = Color32::from_rgb(24, 34, 52);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(45, 60, 85));
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
        visuals.widgets.inactive.corner_radius = CornerRadius::same(6);

        visuals.widgets.hovered.bg_fill = Color32::from_rgb(32, 46, 70);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_BLUE);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.widgets.hovered.corner_radius = CornerRadius::same(6);

        visuals.widgets.active.bg_fill = ACCENT_BLUE_DARK;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, BORDER_ACCENT);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.widgets.active.corner_radius = CornerRadius::same(6);

        visuals.selection.bg_fill = ACCENT_BLUE_DARK;
        visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);

        style.visuals = visuals;
        ctx.set_style_of(egui::Theme::Dark, style.clone());
        ctx.set_style_of(egui::Theme::Light, style);
    }

    fn detect_local_ipv4() -> String {
        if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
            if socket.connect("192.168.1.1:80").is_ok() {
                if let Ok(addr) = socket.local_addr() {
                    let ip = addr.ip().to_string();
                    if ip.starts_with("192.168.") || ip.starts_with("172.") {
                        return ip;
                    }
                }
            }
        }
        if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
            if socket.connect("8.8.8.8:80").is_ok() {
                if let Ok(addr) = socket.local_addr() {
                    return addr.ip().to_string();
                }
            }
        }
        "127.0.0.1".to_string()
    }

    fn scan_physical_usb_mobiles() -> Vec<String> {
        let mut mobiles = Vec::new();
        let engine = UsbDiscoveryEngine::with_windows_native(DiscoveryFilter {
            mobile_repair_devices_only: true,
        });

        if let Ok(devices) = engine.discover_all() {
            for dev in devices {
                mobiles.push(format!(
                    "{} • Modo: {:?} • Bus: {}",
                    dev.product_name,
                    dev.primary_mode(),
                    dev.bus_id
                ));
            }
        }
        mobiles
    }

    fn trigger_hardware_scan(&mut self, disconnect_network_first: bool) {
        if self.scan_in_progress {
            return;
        }
        self.scan_in_progress = true;
        self.last_scan = Instant::now();
        let tx = self.hw_tx.clone();
        let adb = self.adb;

        std::thread::spawn(move || {
            if disconnect_network_first {
                let _ = adb.disconnect_all_network_devices();
            }
            let adb_devices = adb.list_local_usb_devices().unwrap_or_default();
            let usb_mobiles = Self::scan_physical_usb_mobiles();
            let _ = tx.send(HardwareUpdate::ScanResult {
                adb_devices,
                usb_mobiles,
            });
        });
    }

    fn enable_bridge_for_device(&mut self, serial: String) {
        let tx = self.hw_tx.clone();
        let adb = self.adb;
        self.status_message = format!("Activando puente TCP:5555 para {serial}...");
        std::thread::spawn(move || {
            let ok = adb.enable_tcpip_forward(Some(&serial), 5555).is_ok();
            let _ = tx.send(HardwareUpdate::BridgeEnabled {
                serial,
                success: ok,
            });
        });
    }

    fn poll_hardware_updates(&mut self) {
        while let Ok(msg) = self.hw_rx.try_recv() {
            match msg {
                HardwareUpdate::ScanResult {
                    adb_devices,
                    usb_mobiles,
                } => {
                    self.scan_in_progress = false;
                    if adb_devices != self.local_adb_devices {
                        for d in &adb_devices {
                            if !self.local_adb_devices.iter().any(|c| c.serial == d.serial) {
                                self.activity_log.insert(
                                    0,
                                    (
                                        "HOTPLUG".to_string(),
                                        format!(
                                            "Teléfono detectado por USB: {} [Serial: {} | Estado: {}]",
                                            d.display_name(),
                                            d.serial,
                                            d.state
                                        ),
                                    ),
                                );
                            }
                        }
                        self.local_adb_devices = adb_devices;
                    }
                    self.detected_usb_mobiles = usb_mobiles;
                }
                HardwareUpdate::BridgeEnabled { serial, success } => {
                    if success {
                        self.status_message = format!(
                            "Puente TCP:5555 habilitado en {serial}. Listo para conexión remota."
                        );
                        self.activity_log.insert(
                            0,
                            (
                                "PUENTE".to_string(),
                                format!(
                                    "Puerto 5555 abierto para {serial} en {}:5555",
                                    self.local_ip
                                ),
                            ),
                        );
                    } else {
                        self.status_message = format!(
                            "No se pudo activar el puerto 5555 en {serial}. Verifique autorización ADB."
                        );
                    }
                }
                HardwareUpdate::TunnelPortAssigned { port } => {
                    self.tunnel_port = port;
                    self.code = AnyDeskCode::encode_station(&self.local_ip, port, self.code_salt);
                    self.status_message = format!(
                        "Túnel global VPN/Internet activo (bore.pub:{port}). Código listo."
                    );
                    self.activity_log.insert(
                        0,
                        (
                            "TUNEL".to_string(),
                            format!(
                                "Relay global VPN/Internet activo en bore.pub:{port} -> ID: {}",
                                self.code.as_str()
                            ),
                        ),
                    );
                }
            }
        }
    }

    // ========================================================================
    // CUSTOM VECTOR DRAWING HELPERS
    // ========================================================================
    fn draw_status_dot(ui: &mut egui::Ui, color: Color32) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(14.0, 14.0), Sense::hover());
        let center = rect.center();
        let painter = ui.painter();
        painter.circle_filled(
            center,
            6.0,
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 45),
        );
        painter.circle_filled(center, 3.5, color);
    }

    fn draw_brand_mark(ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(6), ACCENT_BLUE_DARK);
        painter.rect_stroke(
            rect,
            CornerRadius::same(6),
            Stroke::new(1.0, BORDER_ACCENT),
            egui::StrokeKind::Inside,
        );
        let c = rect.center();
        let p_left = Pos2::new(c.x - 6.0, c.y);
        let p_right = Pos2::new(c.x + 6.0, c.y);
        let p_top = Pos2::new(c.x + 1.0, c.y - 5.0);
        let p_bot = Pos2::new(c.x + 1.0, c.y + 5.0);
        let stroke = Stroke::new(1.8, Color32::WHITE);
        painter.line_segment([p_left, p_right], stroke);
        painter.line_segment([p_left, p_top], stroke);
        painter.line_segment([p_left, p_bot], stroke);
        painter.circle_filled(p_left, 2.5, BORDER_ACCENT);
        painter.circle_filled(p_right, 2.5, Color32::WHITE);
        painter.circle_filled(p_top, 2.0, Color32::WHITE);
        painter.circle_filled(p_bot, 2.0, Color32::WHITE);
    }

    fn draw_phone_vector(ui: &mut egui::Ui, accent: Color32) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(32.0, 42.0), Sense::hover());
        let painter = ui.painter();
        let phone = rect.shrink2(Vec2::new(5.0, 2.0));
        painter.rect_filled(phone, CornerRadius::same(5), BG_RECESSED);
        painter.rect_stroke(
            phone,
            CornerRadius::same(5),
            Stroke::new(1.5, accent),
            egui::StrokeKind::Inside,
        );
        let spk_y = phone.min.y + 4.0;
        painter.line_segment(
            [
                Pos2::new(phone.center().x - 4.0, spk_y),
                Pos2::new(phone.center().x + 4.0, spk_y),
            ],
            Stroke::new(1.2, TEXT_SECONDARY),
        );
        let bot_y = phone.max.y - 4.0;
        painter.circle_filled(Pos2::new(phone.center().x, bot_y), 1.8, accent);
    }

    fn pill_badge(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) {
        Frame::new()
            .fill(bg)
            .stroke(Stroke::new(1.0, fg))
            .corner_radius(CornerRadius::same(4))
            .inner_margin(Margin::symmetric(7, 2))
            .show(ui, |ui| {
                ui.label(RichText::new(text).size(10.0).strong().color(fg));
            });
    }

    fn render_left_credentials_panel(&mut self, ui: &mut egui::Ui) {
        ScrollArea::vertical()
            .id_salt("left_credentials_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // CARD 1: STATION ID (ANYDESK STYLE)
                Frame::new()
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("CREDENCIALES DE ESTA ESTACIÓN")
                                .size(10.5)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                        ui.label(
                            RichText::new(
                                "Comparta este código o IP con su técnico para autorizar el soporte.",
                            )
                            .size(11.0)
                            .color(TEXT_MUTED),
                        );
                        ui.add_space(6.0);

                        // Recessed Code Display Box
                        Frame::new()
                            .fill(BG_RECESSED)
                            .stroke(Stroke::new(1.0, Color32::from_rgb(38, 56, 84)))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::symmetric(10, 10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new("CÓDIGO DE EMPAREJAMIENTO")
                                            .size(9.5)
                                            .strong()
                                            .color(TEXT_MUTED),
                                    );
                                    ui.add_space(2.0);
                                    ui.label(
                                        RichText::new(self.code.as_str())
                                            .size(18.5)
                                            .strong()
                                            .monospace()
                                            .color(BORDER_ACCENT),
                                    );
                                });
                            });

                        ui.add_space(6.0);

                        let copy_btn = egui::Button::new(
                            RichText::new("COPIAR CÓDIGO DE ESTACIÓN")
                                .size(11.5)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(ACCENT_BLUE_DARK)
                        .stroke(Stroke::new(1.0, ACCENT_BLUE))
                        .min_size(Vec2::new(ui.available_width(), 30.0));

                        if ui.add(copy_btn).clicked() {
                            ui.ctx().copy_text(self.code.as_str().to_string());
                            self.status_message =
                                "Código de estación copiado al portapapeles.".to_string();
                        }

                        ui.add_space(4.0);
                        ui.columns(2, |cols| {
                            if cols[0]
                                .add_sized(
                                    [cols[0].available_width(), 25.0],
                                    egui::Button::new(RichText::new("Nuevo ID").size(11.0)),
                                )
                                .clicked()
                            {
                                self.code_salt = self.code_salt.wrapping_add(17).max(1);
                                self.code = AnyDeskCode::encode_station(
                                    &self.local_ip,
                                    self.tunnel_port,
                                    self.code_salt,
                                );
                                self.activity_log.insert(
                                    0,
                                    (
                                        "SEGURIDAD".to_string(),
                                        format!("Nuevo ID generado: {}", self.code.as_str()),
                                    ),
                                );
                            }
                            if cols[1]
                                .add_sized(
                                    [cols[1].available_width(), 25.0],
                                    egui::Button::new(RichText::new("9 Dígitos").size(11.0)),
                                )
                                .clicked()
                            {
                                self.code = AnyDeskCode::encode_numeric_station(
                                    &self.local_ip,
                                    self.tunnel_port,
                                );
                            }
                        });

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);

                        let endpoint = format!("{}:5555", self.local_ip);
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("ENDPOINT DIRECTO LAN")
                                        .size(9.5)
                                        .strong()
                                        .color(TEXT_MUTED),
                                );
                                ui.label(
                                    RichText::new(&endpoint)
                                        .size(12.5)
                                        .monospace()
                                        .strong()
                                        .color(STATUS_GREEN),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("Copiar IP").clicked() {
                                    ui.ctx().copy_text(endpoint);
                                    self.status_message =
                                        "Dirección IP:5555 copiada al portapapeles.".to_string();
                                }
                            });
                        });

                        if self.tunnel_port > 0 {
                            ui.add_space(4.0);
                            let cloud_ep = format!("bore.pub:{}", self.tunnel_port);
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new("RELAY GLOBAL VPN / INTERNET")
                                            .size(9.5)
                                            .strong()
                                            .color(TEXT_MUTED),
                                    );
                                    ui.label(
                                        RichText::new(&cloud_ep)
                                            .size(12.0)
                                            .monospace()
                                            .strong()
                                            .color(BORDER_ACCENT),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.small_button("Copiar Relay").clicked() {
                                        ui.ctx().copy_text(cloud_ep);
                                        self.status_message =
                                            "Endpoint Relay Global copiado al portapapeles."
                                                .to_string();
                                    }
                                });
                            });
                        }
                    });

                ui.add_space(8.0);

                // CARD 2: LIVE 3-STEP CONNECTION CHECKLIST
                let has_usb_cable =
                    !self.local_adb_devices.is_empty() || !self.detected_usb_mobiles.is_empty();
                let has_adb_mode = !self.local_adb_devices.is_empty();
                let has_rsa_auth = self
                    .local_adb_devices
                    .iter()
                    .any(|d| d.state == AdbDeviceState::Device);

                Frame::new()
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("LISTA DE VERIFICACIÓN USB")
                                .size(10.5)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                        ui.add_space(6.0);

                        Self::render_step_row(
                            ui,
                            "1",
                            "Cable USB Físico",
                            if has_usb_cable {
                                "Detectado en puerto local"
                            } else {
                                "Conecte el móvil por USB"
                            },
                            has_usb_cable,
                        );
                        ui.add_space(4.0);
                        Self::render_step_row(
                            ui,
                            "2",
                            "Depuración USB (ADB)",
                            if has_adb_mode {
                                "Interfaz ADB activa"
                            } else {
                                "Activar en Ajustes"
                            },
                            has_adb_mode,
                        );
                        ui.add_space(4.0);
                        Self::render_step_row(
                            ui,
                            "3",
                            "Autorización RSA",
                            if has_rsa_auth {
                                "Autorizado y listo"
                            } else {
                                "Aceptar en pantalla"
                            },
                            has_rsa_auth,
                        );
                    });
            });
    }

    fn render_step_row(ui: &mut egui::Ui, num: &str, title: &str, subtitle: &str, done: bool) {
        let (fg, bg, badge_txt) = if done {
            (STATUS_GREEN, STATUS_GREEN_BG, "OK")
        } else {
            (TEXT_SECONDARY, BG_RECESSED, num)
        };

        Frame::new()
            .fill(BG_RECESSED)
            .stroke(Stroke::new(
                1.0,
                if done {
                    Color32::from_rgb(16, 100, 75)
                } else {
                    BORDER_SUBTLE
                },
            ))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    Frame::new()
                        .fill(bg)
                        .stroke(Stroke::new(1.0, fg))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(Margin::symmetric(5, 2))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(badge_txt)
                                    .size(10.0)
                                    .monospace()
                                    .strong()
                                    .color(fg),
                            );
                        });
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(title)
                                .size(11.5)
                                .strong()
                                .color(if done { TEXT_PRIMARY } else { TEXT_SECONDARY }),
                        );
                        ui.label(RichText::new(subtitle).size(10.0).color(TEXT_MUTED));
                    });
                });
            });
    }

    fn render_right_workspace(&mut self, ui: &mut egui::Ui) {
        // Navigation Tab Bar (Responsive Wrap)
        Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    Self::tab_button(
                        ui,
                        &mut self.active_tab,
                        ClientTab::Tutorial,
                        "Como Conectarse (Guia)",
                    );
                    let dev_count = self.local_adb_devices.len() + self.detected_usb_mobiles.len();
                    Self::tab_button(
                        ui,
                        &mut self.active_tab,
                        ClientTab::Devices,
                        &format!("Dispositivos USB ({dev_count})"),
                    );
                    Self::tab_button(
                        ui,
                        &mut self.active_tab,
                        ClientTab::BrandGuide,
                        "Guía Paso a Paso por Marca",
                    );
                    Self::tab_button(
                        ui,
                        &mut self.active_tab,
                        ClientTab::ActivityLog,
                        &format!("Bitácora ({})", self.activity_log.len()),
                    );
                });
            });

        ui.add_space(8.0);

        ScrollArea::vertical()
            .id_salt("right_workspace_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| match self.active_tab {
                ClientTab::Tutorial => self.render_tutorial_tab(ui),
                ClientTab::Devices => self.render_devices_tab(ui),
                ClientTab::BrandGuide => self.render_brand_guide_tab(ui),
                ClientTab::ActivityLog => self.render_activity_log_tab(ui),
            });
    }

    fn tab_button(ui: &mut egui::Ui, current: &mut ClientTab, target: ClientTab, label: &str) {
        let active = *current == target;
        let btn = egui::Button::new(
            RichText::new(label)
                .size(11.5)
                .strong()
                .color(if active { Color32::WHITE } else { TEXT_SECONDARY }),
        )
        .fill(if active {
            ACCENT_BLUE_DARK
        } else {
            Color32::TRANSPARENT
        })
        .stroke(if active {
            Stroke::new(1.0, ACCENT_BLUE)
        } else {
            Stroke::NONE
        });

        if ui.add(btn).clicked() {
            *current = target;
        }
    }

    fn render_tutorial_tab(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("GUÍA RÁPIDA: CÓMO CONECTARSE CON SU TÉCNICO")
                        .size(12.0)
                        .strong()
                        .color(BORDER_ACCENT),
                );
                ui.label(
                    RichText::new("Siga estos 3 sencillos pasos para permitir el soporte remoto en su dispositivo.")
                        .size(10.5)
                        .color(TEXT_MUTED),
                );
                ui.add_space(8.0);

                let steps = [
                    (
                        "1",
                        "Conecte el Teléfono por Cable USB a esta PC",
                        "Use un cable de datos en buen estado. Desbloquee la pantalla del móvil. Si aparece un aviso de 'Modo USB', elija 'Transferir archivos (MTP)'.",
                        Color32::from_rgb(14, 165, 233),
                    ),
                    (
                        "2",
                        "Comparta su Código de Estación con el Técnico",
                        "En el panel izquierdo verá su 'Código de Emparejamiento' de 16 caracteres. Pulse 'COPIAR CÓDIGO DE ESTACIÓN' y envíeselo a su técnico por WhatsApp o chat.",
                        Color32::from_rgb(245, 158, 11),
                    ),
                    (
                        "3",
                        "Acepte la Huella de Seguridad en su Celular",
                        "Cuando el técnico inicie la conexión, saldrá una ventana en el teléfono: '¿Permitir depuración por USB?'. Marque la casilla 'Permitir siempre' y toque ACEPTAR.",
                        Color32::from_rgb(16, 185, 129),
                    ),
                ];

                for (num, title, desc, accent) in steps {
                    Frame::new()
                        .fill(BG_RECESSED)
                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                Frame::new()
                                    .fill(Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 35))
                                    .stroke(Stroke::new(1.0, accent))
                                    .corner_radius(CornerRadius::same(4))
                                    .inner_margin(Margin::symmetric(8, 4))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(num).size(13.0).strong().color(accent));
                                    });

                                ui.vertical(|ui| {
                                    ui.label(RichText::new(title).size(12.0).strong().color(Color32::WHITE));
                                    ui.add_space(2.0);
                                    ui.label(RichText::new(desc).size(10.5).color(TEXT_SECONDARY));
                                });
                            });
                        });
                    ui.add_space(6.0);
                }

                ui.add_space(4.0);
                Frame::new()
                    .fill(Color32::from_rgb(12, 22, 38))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(25, 50, 85)))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("¿Su teléfono no es detectado?")
                                .size(11.0)
                                .strong()
                                .color(BORDER_ACCENT),
                        );
                        ui.label(
                            RichText::new("Consulte la pestaña 'Guía Paso a Paso por Marca' en la barra superior para ver cómo activar 'Depuración por USB' en Samsung, Xiaomi, Motorola, Tecno o Huawei.")
                                .size(10.0)
                                .color(TEXT_MUTED),
                        );
                    });
            });
    }

    fn render_devices_tab(&mut self, ui: &mut egui::Ui) {
        let mut bridge_request: Option<String> = None;
        let wide = ui.available_width() >= 480.0;

        if !self.local_adb_devices.is_empty() {
            let devices = self.local_adb_devices.clone();
            for dev in devices {
                let is_auth = dev.state == AdbDeviceState::Device;
                let accent = if is_auth { STATUS_GREEN } else { STATUS_AMBER };

                Frame::new()
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            Self::draw_phone_vector(ui, accent);
                            ui.vertical(|ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new(dev.display_name())
                                            .size(15.0)
                                            .strong()
                                            .color(Color32::WHITE),
                                    );
                                    if is_auth {
                                        Self::pill_badge(
                                            ui,
                                            "AUTORIZADO Y LISTO",
                                            STATUS_GREEN,
                                            STATUS_GREEN_BG,
                                        );
                                    } else {
                                        Self::pill_badge(
                                            ui,
                                            "ACEPTAR PERMISO EN MÓVIL",
                                            STATUS_AMBER,
                                            STATUS_AMBER_BG,
                                        );
                                    }
                                });
                                ui.label(
                                    RichText::new(format!(
                                        "Cable USB Físico Local  •  ID: {}",
                                        dev.serial
                                    ))
                                    .size(11.0)
                                    .monospace()
                                    .color(TEXT_SECONDARY),
                                );
                            });

                            if is_auth {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("Activar Puente 5555")
                                                    .size(11.5)
                                                    .strong()
                                                    .color(Color32::WHITE),
                                            )
                                            .fill(ACCENT_BLUE_DARK)
                                            .stroke(Stroke::new(1.0, ACCENT_BLUE)),
                                        )
                                        .clicked()
                                    {
                                        bridge_request = Some(dev.serial.clone());
                                    }
                                });
                            }
                        });

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(6.0);

                        let bat_txt = dev
                            .battery_percent
                            .map(|b| format!("{b}%"))
                            .unwrap_or_else(|| "N/D".to_string());

                        if wide {
                            ui.columns(4, |cols| {
                                Self::spec_cell(&mut cols[0], "SERIAL FÍSICO", &dev.serial);
                                Self::spec_cell(
                                    &mut cols[1],
                                    "VERSIÓN ANDROID",
                                    dev.android_version.as_deref().unwrap_or("Consultando..."),
                                );
                                Self::spec_cell(
                                    &mut cols[2],
                                    "PARCHE SEGURIDAD",
                                    dev.security_patch.as_deref().unwrap_or("N/D"),
                                );
                                Self::spec_cell(&mut cols[3], "CARGA BATERÍA", &bat_txt);
                            });
                        } else {
                            ui.columns(2, |cols| {
                                Self::spec_cell(&mut cols[0], "SERIAL FÍSICO", &dev.serial);
                                Self::spec_cell(
                                    &mut cols[1],
                                    "VERSIÓN ANDROID",
                                    dev.android_version.as_deref().unwrap_or("Consultando..."),
                                );
                            });
                            ui.add_space(4.0);
                            ui.columns(2, |cols| {
                                Self::spec_cell(
                                    &mut cols[0],
                                    "PARCHE SEGURIDAD",
                                    dev.security_patch.as_deref().unwrap_or("N/D"),
                                );
                                Self::spec_cell(&mut cols[1], "CARGA BATERÍA", &bat_txt);
                            });
                        }

                        if !is_auth {
                            ui.add_space(8.0);
                            Frame::new()
                                .fill(STATUS_AMBER_BG)
                                .stroke(Stroke::new(1.0, STATUS_AMBER))
                                .corner_radius(CornerRadius::same(6))
                                .inner_margin(Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        RichText::new(
                                            "ACCIÓN REQUERIDA: Desbloquee la pantalla de su teléfono, marque 'Permitir siempre desde esta computadora' y toque Aceptar.",
                                        )
                                        .size(11.0)
                                        .strong()
                                        .color(STATUS_AMBER),
                                    );
                                });
                        }
                    });
                ui.add_space(8.0);
            }
        } else if !self.detected_usb_mobiles.is_empty() {
            for mob in &self.detected_usb_mobiles {
                Frame::new()
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0, STATUS_AMBER))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            Self::draw_phone_vector(ui, STATUS_AMBER);
                            ui.vertical(|ui| {
                                Self::pill_badge(
                                    ui,
                                    "USB DETECTADO SIN DEPURACIÓN ADB",
                                    STATUS_AMBER,
                                    STATUS_AMBER_BG,
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(mob)
                                        .size(13.0)
                                        .strong()
                                        .color(Color32::WHITE),
                                );
                                ui.label(
                                    RichText::new(
                                        "El cable USB está conectado, pero falta activar 'Depuración por USB' en los Ajustes del teléfono.",
                                    )
                                    .size(11.5)
                                    .color(TEXT_SECONDARY),
                                );
                            });
                        });
                        ui.add_space(6.0);
                        if ui
                            .button(
                                RichText::new("Ver Instrucciones Paso a Paso para mi Marca")
                                    .strong()
                                    .color(BORDER_ACCENT),
                            )
                            .clicked()
                        {
                            self.active_tab = ClientTab::BrandGuide;
                        }
                    });
                ui.add_space(8.0);
            }
        } else {
            Frame::new()
                .fill(BG_CARD)
                .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(20, 28))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        Self::draw_phone_vector(ui, TEXT_MUTED);
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Ningún Teléfono Detectado en Puertos USB Locales")
                                .size(14.5)
                                .strong()
                                .color(TEXT_PRIMARY),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Conecte su dispositivo Android directamente a la computadora usando un cable USB de transferencia de datos.",
                            )
                            .size(11.5)
                            .color(TEXT_SECONDARY),
                        );
                        ui.add_space(12.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Abrir Guía de Configuración USB por Marca")
                                        .size(12.0)
                                        .strong()
                                        .color(Color32::WHITE),
                                )
                                .fill(ACCENT_BLUE_DARK)
                                .stroke(Stroke::new(1.0, ACCENT_BLUE))
                                .min_size(Vec2::new(240.0, 32.0)),
                            )
                            .clicked()
                        {
                            self.active_tab = ClientTab::BrandGuide;
                        }
                    });
                });
        }

        if let Some(serial) = bridge_request {
            self.enable_bridge_for_device(serial);
        }
    }

    fn spec_cell(ui: &mut egui::Ui, label: &str, value: &str) {
        Frame::new()
            .fill(BG_RECESSED)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(5))
            .inner_margin(Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(label).size(9.0).strong().color(TEXT_MUTED));
                ui.label(
                    RichText::new(value)
                        .size(11.5)
                        .monospace()
                        .strong()
                        .color(TEXT_PRIMARY),
                );
            });
    }

    fn render_brand_guide_tab(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("SELECCIONE LA MARCA DE SU TELÉFONO")
                        .size(10.5)
                        .strong()
                        .color(TEXT_SECONDARY),
                );
                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    for (brand, name) in [
                        (PhoneBrand::Samsung, "Samsung Galaxy"),
                        (PhoneBrand::Xiaomi, "Xiaomi / Redmi / POCO"),
                        (PhoneBrand::Motorola, "Motorola / Google"),
                        (PhoneBrand::TecnoInfinix, "Tecno / Infinix"),
                        (PhoneBrand::HuaweiHonor, "Huawei / Honor"),
                    ] {
                        let active = self.selected_brand == brand;
                        let btn = egui::Button::new(
                            RichText::new(name)
                                .size(11.0)
                                .strong()
                                .color(if active { Color32::WHITE } else { TEXT_SECONDARY }),
                        )
                        .fill(if active {
                            ACCENT_BLUE_DARK
                        } else {
                            BG_RECESSED
                        })
                        .stroke(Stroke::new(
                            1.0,
                            if active { ACCENT_BLUE } else { BORDER_SUBTLE },
                        ));
                        if ui.add(btn).clicked() {
                            self.selected_brand = brand;
                        }
                    }
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                let (title, steps, note) = match self.selected_brand {
                    PhoneBrand::Samsung => (
                        "Configuración para Samsung Galaxy (One UI)",
                        vec![
                            "1. Abra Ajustes > Acerca del teléfono > Información de software.",
                            "2. Toque 7 veces seguidas sobre 'Número de compilación' e ingrese su PIN.",
                            "3. En One UI 6.0+: vaya a Ajustes > Seguridad y privacidad > Bloqueador automático y DESACTÍVELO.",
                            "4. Regrese al final de Ajustes > Opciones de desarrollador y active 'Depuración por USB'.",
                            "5. Conecte el cable USB y pulse 'Permitir' en la ventana emergente del teléfono.",
                        ],
                        "Nota: Si 'Depuración por USB' aparece en gris en Samsung modernos, desactive primero 'Bloqueador automático'.",
                    ),
                    PhoneBrand::Xiaomi => (
                        "Configuración para Xiaomi / Redmi / POCO (MIUI & HyperOS)",
                        vec![
                            "1. Abra Configuración > Acerca del teléfono.",
                            "2. Toque 7 veces seguidas en 'Versión de MIUI' o 'Versión de HyperOS'.",
                            "3. Vaya a Configuración > Ajustes adicionales > Opciones de desarrollador.",
                            "4. Active 'Depuración USB' y acepte la advertencia de 10 segundos.",
                            "5. Active también 'Depuración USB (Ajustes de seguridad)' para diagnóstico completo.",
                        ],
                        "En Xiaomi/Redmi se recomienda elegir 'Transferencia de archivos (MTP)' al conectar el cable.",
                    ),
                    PhoneBrand::Motorola => (
                        "Configuración para Motorola / Google Pixel / Android Puro",
                        vec![
                            "1. Abra Configuración > Acerca del teléfono.",
                            "2. Deslice hasta el final y toque 7 veces sobre 'Número de compilación'.",
                            "3. Retroceda a Configuración > Sistema > Opciones para desarrolladores.",
                            "4. Active el interruptor 'Depuración por USB'.",
                            "5. Al conectar el cable, marque 'Permitir siempre desde esta computadora' y toque Permitir.",
                        ],
                        "Use preferiblemente un puerto USB directo de la placa madre.",
                    ),
                    PhoneBrand::TecnoInfinix => (
                        "Configuración para Tecno / Infinix / Itel (HiOS & XOS)",
                        vec![
                            "1. Abra Ajustes > Mi Teléfono.",
                            "2. Toque 7 veces sobre 'Número de compilación' hasta activar el modo desarrollador.",
                            "3. Vaya a Ajustes > Sistema > Opciones para programadores.",
                            "4. Active 'Depuración por USB'.",
                            "5. Al conectar el cable USB, seleccione 'Transferencia de archivos' y acepte la huella RSA.",
                        ],
                        "En equipos MediaTek (Tecno/Infinix), mantener 'Transferencia de archivos' mantiene el puerto activo.",
                    ),
                    PhoneBrand::HuaweiHonor => (
                        "Configuración para Huawei / Honor (EMUI & MagicOS)",
                        vec![
                            "1. Abra Ajustes > Acerca del teléfono.",
                            "2. Toque 7 veces sobre 'Número de compilación'.",
                            "3. Vaya a Ajustes > Sistema y actualizaciones > Opciones de desarrollador.",
                            "4. Active 'Depuración por USB' y también 'Permitir depuración ADB en modo solo carga'.",
                            "5. Conecte el cable, elija 'Transferir archivos (MTP)' y autorice la computadora.",
                        ],
                        "Activar 'Permitir depuración ADB en modo solo carga' evita desconexiones automáticas.",
                    ),
                };

                ui.label(
                    RichText::new(title)
                        .size(13.5)
                        .strong()
                        .color(BORDER_ACCENT),
                );
                ui.add_space(6.0);

                for step in steps {
                    Frame::new()
                        .fill(BG_RECESSED)
                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                        .corner_radius(CornerRadius::same(5))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(step).size(11.5).color(TEXT_PRIMARY));
                        });
                    ui.add_space(4.0);
                }

                ui.add_space(4.0);
                Frame::new()
                    .fill(Color32::from_rgb(12, 38, 62))
                    .stroke(Stroke::new(1.0, ACCENT_BLUE))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(10, 7))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(note).size(11.0).color(TEXT_PRIMARY));
                    });
            });
    }

    fn render_activity_log_tab(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("BITÁCORA DE EVENTOS DE LA ESTACIÓN")
                            .size(10.5)
                            .strong()
                            .color(TEXT_SECONDARY),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Copiar Bitácora").clicked() {
                            let full = self
                                .activity_log
                                .iter()
                                .map(|(t, m)| format!("[{t}] {m}"))
                                .collect::<Vec<_>>()
                                .join("\n");
                            ui.ctx().copy_text(full);
                            self.status_message = "Bitácora copiada al portapapeles.".to_string();
                        }
                    });
                });
                ui.add_space(6.0);

                for (tag, msg) in &self.activity_log {
                    Frame::new()
                        .fill(BG_RECESSED)
                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                let col = match tag.as_str() {
                                    "HOTPLUG" | "PUENTE" => STATUS_GREEN,
                                    "SEGURIDAD" => STATUS_AMBER,
                                    _ => BORDER_ACCENT,
                                };
                                ui.label(
                                    RichText::new(format!("[{tag}]"))
                                        .size(10.5)
                                        .monospace()
                                        .strong()
                                        .color(col),
                                );
                                ui.label(
                                    RichText::new(msg)
                                        .size(11.0)
                                        .monospace()
                                        .color(TEXT_PRIMARY),
                                );
                            });
                        });
                    ui.add_space(4.0);
                }
            });
    }
}

impl eframe::App for AgentGuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_hardware_updates();

        let ctx = ui.ctx().clone();
        ctx.request_repaint_after(std::time::Duration::from_millis(1500));

        if !self.scan_in_progress && self.last_scan.elapsed() > std::time::Duration::from_secs(3) {
            self.trigger_hardware_scan(false);
        }

        // 1. TOP HEADER PANEL
        Panel::top("client_top_header")
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(BG_HEADER)
                    .inner_margin(Margin::symmetric(14, 10))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    Self::draw_brand_mark(ui);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("ADBHUB-LINK")
                                    .size(15.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                            Self::pill_badge(
                                ui,
                                "ESTACIÓN DEL CLIENTE",
                                BORDER_ACCENT,
                                Color32::from_rgb(12, 45, 72),
                            );
                        });
                        ui.label(
                            RichText::new(format!(
                                "Equipo: {}  •  IP LAN: {}",
                                self.host_name, self.local_ip
                            ))
                            .size(10.5)
                            .color(TEXT_SECONDARY),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .button(RichText::new("Actualizar USB").size(11.5).strong())
                            .clicked()
                        {
                            self.trigger_hardware_scan(false);
                            self.status_message = "Escaneando puertos USB físicos...".to_string();
                        }

                        if ui
                            .button(RichText::new("Aislar Red").size(11.5))
                            .on_hover_text("Desconecta endpoints externos y muestra solo USB físico")
                            .clicked()
                        {
                            self.trigger_hardware_scan(true);
                            self.status_message =
                                "Sesiones externas limpiadas. Solo USB físico activo.".to_string();
                        }

                        let authorized_count = self
                            .local_adb_devices
                            .iter()
                            .filter(|d| d.state == AdbDeviceState::Device)
                            .count();

                        let (status_txt, status_col) = if authorized_count > 0 {
                            (
                                format!("{authorized_count} USB LISTO(S)"),
                                STATUS_GREEN,
                            )
                        } else if !self.local_adb_devices.is_empty()
                            || !self.detected_usb_mobiles.is_empty()
                        {
                            ("AUTORIZACIÓN PENDIENTE".to_string(), STATUS_AMBER)
                        } else {
                            ("ESPERANDO CABLE USB".to_string(), TEXT_SECONDARY)
                        };

                        ui.label(
                            RichText::new(status_txt)
                                .size(11.0)
                                .strong()
                                .color(status_col),
                        );
                        Self::draw_status_dot(ui, status_col);
                    });
                });
            });

        // 2. BOTTOM STATUS BAR PANEL
        Panel::bottom("client_bottom_footer")
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(BG_HEADER)
                    .inner_margin(Margin::symmetric(14, 6))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("ADBHUB-LINK v2.0 • Motor USB/ADB Silencioso")
                            .size(10.5)
                            .color(TEXT_MUTED),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(&self.status_message)
                                .size(10.5)
                                .strong()
                                .color(BORDER_ACCENT),
                        );
                    });
                });
            });

        // 3. LEFT RESIZABLE & SCROLLABLE CREDENTIALS PANEL
        Panel::left("client_left_panel")
            .resizable(true)
            .default_size(275.0)
            .size_range(230.0..=360.0)
            .frame(
                Frame::new()
                    .fill(BG_CANVAS)
                    .inner_margin(Margin::symmetric(12, 10))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ui, |ui| {
                self.render_left_credentials_panel(ui);
            });

        // 4. CENTRAL SCROLLABLE WORKSPACE PANEL
        CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(BG_CANVAS)
                    .inner_margin(Margin::symmetric(12, 10)),
            )
            .show(ui, |ui| {
                self.render_right_workspace(ui);
            });
    }
}

pub fn run_agent_native_gui() -> Result<(), eframe::Error> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1040.0, 680.0])
            .with_min_inner_size([640.0, 420.0])
            .with_maximized(true)
            .with_title("ADBHUB-LINK — Estación del Cliente"),
        ..Default::default()
    };

    eframe::run_native(
        "ADBHUB-LINK Agent",
        native_options,
        Box::new(|cc| Ok(Box::new(AgentGuiApp::new(cc)))),
    )
}
