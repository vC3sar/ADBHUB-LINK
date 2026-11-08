//! # `gui.rs` — Native Enterprise Desktop GUI for ADBHUB-LINK Desktop (Technician Station)
//!
//! Fully responsive, scrollable, multi-panel desktop interface using `SidePanel::left`,
//! `TopBottomPanel::top`, `TopBottomPanel::bottom`, and `CentralPanel` with vertical
//! `ScrollArea` and adaptive 2/4-column grids so it fits any laptop or desktop screen
//! without overflowing or clipping.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use adbhub_core::{AdbDeviceInfo, AdbDeviceState, AdbEngine, AnyDeskCode};
use adbhub_desktop::DesktopController;
use eframe::egui::{
    self, Align, CentralPanel, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Frame,
    Layout, Margin, Panel, Pos2, RichText, ScrollArea, Sense, Stroke, Vec2,
};
use tokio::runtime::Runtime;

// ============================================================================
// ENTERPRISE COLOR PALETTE
// ============================================================================
const BG_CANVAS: Color32 = Color32::from_rgb(9, 13, 22); // #090D16
const BG_SIDEBAR: Color32 = Color32::from_rgb(12, 17, 29); // #0C111D
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
const STATUS_RED: Color32 = Color32::from_rgb(239, 68, 68); // #EF4444
const STATUS_RED_BG: Color32 = Color32::from_rgb(69, 15, 20);

pub const APK_MEDIAFIRE_URL: &str =
    "https://www.mediafire.com/file/5vh10fh39e0vcwi/adbhub-client.apk/file";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TechNavTab {
    Dashboard,
    TerminalPro,
    ServerDevices,
    DiagnosticsGuide,
    ApkQr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TechConnectionState {
    Disconnected,
    Connecting,
    Connected { device: AdbDeviceInfo },
    Error(String),
}

#[derive(Debug, Clone, Default)]
pub struct ExtendedTelemetry {
    pub cpu_abi: String,
    pub soc_platform: String,
    pub sdk_int: String,
    pub build_id: String,
}

pub enum GuiAsyncEvent {
    Log(String, String),
    ConnectionSuccess {
        device: AdbDeviceInfo,
        telemetry: ExtendedTelemetry,
        all_devices: Vec<AdbDeviceInfo>,
    },
    ConnectionFailed {
        error: String,
        target_endpoint: String,
    },
    ServerDevicesUpdated {
        devices: Vec<AdbDeviceInfo>,
    },
    CommandResult {
        output: String,
    },
    CommandError {
        error: String,
    },
    RebootResult {
        message: String,
        is_error: bool,
    },
}

pub struct DesktopGuiApp {
    _rt: Arc<Runtime>,
    _controller: Arc<DesktopController>,
    active_nav: TechNavTab,
    input_code: String,
    recent_endpoints: Vec<String>,
    state: TechConnectionState,
    telemetry: ExtendedTelemetry,
    server_devices: Vec<AdbDeviceInfo>,
    last_server_poll: Instant,
    server_poll_in_flight: bool,
    adb: AdbEngine,
    command_input: String,
    terminal_logs: Vec<(String, String)>,
    last_action_message: Option<(Instant, String)>,
    event_tx: Sender<GuiAsyncEvent>,
    event_rx: Receiver<GuiAsyncEvent>,
    is_command_running: bool,
    qr_texture: Option<egui::TextureHandle>,
}

impl DesktopGuiApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rt: Arc<Runtime>,
        controller: Arc<DesktopController>,
    ) -> Self {
        Self::configure_fonts_and_theme(&cc.egui_ctx);
        let qr_tex = Self::generate_qr_texture(&cc.egui_ctx, APK_MEDIAFIRE_URL);

        let (tx, rx) = channel();

        let mut app = Self {
            _rt: rt,
            _controller: controller,
            active_nav: TechNavTab::Dashboard,
            input_code: "192.168.1.83:5555".to_string(),
            recent_endpoints: vec![
                "192.168.1.83:5555".to_string(),
                "127.0.0.1:5555".to_string(),
            ],
            state: TechConnectionState::Disconnected,
            telemetry: ExtendedTelemetry::default(),
            server_devices: Vec::new(),
            last_server_poll: Instant::now(),
            server_poll_in_flight: false,
            adb: AdbEngine::new(),
            command_input: "getprop ro.product.model".to_string(),
            terminal_logs: vec![
                (
                    "SISTEMA".to_string(),
                    "ADBHUB-LINK Estación del Técnico inicializada correctamente.".to_string(),
                ),
                (
                    "MOTOR".to_string(),
                    "Servidor ADB local activo en 127.0.0.1:5037 (Compatible con UnlockTool, Chimera, SamFirm)."
                        .to_string(),
                ),
            ],
            last_action_message: None,
            event_tx: tx,
            event_rx: rx,
            is_command_running: false,
            qr_texture: qr_tex,
        };

        app.trigger_server_devices_poll();
        app
    }

    fn generate_qr_texture(ctx: &egui::Context, url: &str) -> Option<egui::TextureHandle> {
        use qrcode::{Color, EcLevel, QrCode};
        let qr = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::M)
            .or_else(|_| QrCode::new(url.as_bytes()))
            .ok()?;
        let qr_width = qr.width();
        let border = 3;
        let total_width = qr_width + border * 2;
        let mut pixels = vec![Color32::WHITE; total_width * total_width];

        for y in 0..qr_width {
            for x in 0..qr_width {
                if qr[(x, y)] == Color::Dark {
                    let px = x + border;
                    let py = y + border;
                    pixels[py * total_width + px] = Color32::BLACK;
                }
            }
        }

        let color_image = egui::ColorImage::new([total_width, total_width], pixels);
        Some(ctx.load_texture("qr_mediafire_apk", color_image, egui::TextureOptions::NEAREST))
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

    fn extract_telemetry(props: &HashMap<String, String>) -> ExtendedTelemetry {
        ExtendedTelemetry {
            cpu_abi: props
                .get("ro.product.cpu.abi")
                .cloned()
                .unwrap_or_else(|| "arm64-v8a".to_string()),
            soc_platform: props
                .get("ro.board.platform")
                .or_else(|| props.get("ro.hardware"))
                .cloned()
                .unwrap_or_else(|| "SoC Android".to_string()),
            sdk_int: props
                .get("ro.build.version.sdk")
                .cloned()
                .unwrap_or_else(|| "N/D".to_string()),
            build_id: props
                .get("ro.build.display.id")
                .or_else(|| props.get("ro.build.id"))
                .cloned()
                .unwrap_or_else(|| "N/D".to_string()),
        }
    }

    fn trigger_server_devices_poll(&mut self) {
        if self.server_poll_in_flight {
            return;
        }
        self.server_poll_in_flight = true;
        self.last_server_poll = Instant::now();
        let tx = self.event_tx.clone();
        let adb = self.adb;

        std::thread::spawn(move || {
            let devices = adb.list_devices().unwrap_or_default();
            let _ = tx.send(GuiAsyncEvent::ServerDevicesUpdated { devices });
        });
    }

    fn connect_to_code(&mut self) {
        if matches!(self.state, TechConnectionState::Connecting) {
            return;
        }

        let code_str = self.input_code.trim();
        if code_str.is_empty() {
            self.state = TechConnectionState::Error(
                "Ingrese una dirección IP:5555 o el código de la estación cliente.".to_string(),
            );
            return;
        }

        let mut candidate_endpoints: Vec<String> = Vec::new();

        if code_str.contains('.') {
            let ep = if code_str.contains(':') {
                code_str.to_string()
            } else {
                format!("{code_str}:5555")
            };
            candidate_endpoints.push(ep);
        } else {
            match AnyDeskCode::parse_flexible(code_str) {
                Ok(parsed) => {
                    let resolved = parsed.resolve_endpoints();
                    if !resolved.is_empty() {
                        candidate_endpoints.extend(resolved);
                    } else {
                        for r in &self.recent_endpoints {
                            if !r.starts_with("127.") && !candidate_endpoints.contains(r) {
                                candidate_endpoints.push(r.clone());
                            }
                        }
                        if candidate_endpoints.is_empty() {
                            candidate_endpoints.push("192.168.1.83:5555".to_string());
                        }
                    }
                }
                Err(_) => {
                    let ep = if code_str.contains(':') {
                        code_str.to_string()
                    } else {
                        format!("{code_str}:5555")
                    };
                    candidate_endpoints.push(ep);
                }
            }
        }

        let primary_label = candidate_endpoints.join(" / ");
        for ep in candidate_endpoints.iter().rev() {
            if !self.recent_endpoints.contains(ep) {
                self.recent_endpoints.insert(0, ep.clone());
            }
        }
        self.recent_endpoints.truncate(4);

        self.state = TechConnectionState::Connecting;
        self.terminal_logs.push((
            "CONEXION".to_string(),
            format!("Resolviendo ruta remota hacia [{primary_label}]..."),
        ));

        let tx = self.event_tx.clone();
        let adb = self.adb;

        std::thread::spawn(move || {
            let mut last_err = String::new();
            let mut last_ep = String::new();

            for endpoint_clone in candidate_endpoints {
                last_ep = endpoint_clone.clone();
                let connect_out = match adb.connect_tcp_device(&endpoint_clone) {
                    Ok(out) => out,
                    Err(e) => format!("Error ejecutando adb connect: {e}"),
                };

                let _ = tx.send(GuiAsyncEvent::Log(
                    "ADB".to_string(),
                    format!("adb connect {endpoint_clone} -> {connect_out}"),
                ));

                std::thread::sleep(std::time::Duration::from_millis(450));

                let all_devices = adb.list_devices().unwrap_or_default();
                let matched = all_devices
                    .iter()
                    .find(|d| d.serial == endpoint_clone)
                    .or_else(|| {
                        if connect_out.contains("connected to")
                            || connect_out.contains("already connected")
                        {
                            all_devices.iter().find(|d| d.state == AdbDeviceState::Device)
                        } else {
                            None
                        }
                    })
                    .cloned();

                if let Some(dev) = matched {
                    let props = adb.get_device_props(&dev.serial).unwrap_or_default();
                    let telemetry = Self::extract_telemetry(&props);
                    let _ = tx.send(GuiAsyncEvent::ConnectionSuccess {
                        device: dev,
                        telemetry,
                        all_devices,
                    });
                    return;
                } else if connect_out.contains("connected to")
                    || connect_out.contains("already connected")
                {
                    let dev = AdbDeviceInfo {
                        serial: endpoint_clone.clone(),
                        state: AdbDeviceState::Device,
                        product: Some("Dispositivo Remoto".to_string()),
                        model: Some(format!("Equipo Remoto ({endpoint_clone})")),
                        device: Some("android".to_string()),
                        transport_id: None,
                        android_version: None,
                        brand: Some("Android".to_string()),
                        battery_percent: None,
                        security_patch: None,
                    };
                    let props = adb.get_device_props(&dev.serial).unwrap_or_default();
                    let telemetry = Self::extract_telemetry(&props);
                    let _ = tx.send(GuiAsyncEvent::ConnectionSuccess {
                        device: dev,
                        telemetry,
                        all_devices,
                    });
                    return;
                }

                last_err = connect_out;
            }

            let _ = tx.send(GuiAsyncEvent::ConnectionFailed {
                error: last_err,
                target_endpoint: last_ep,
            });
        });
    }

    fn execute_shell_command(&mut self, custom_cmd: Option<&str>) {
        if self.is_command_running {
            return;
        }

        if let Some(c) = custom_cmd {
            self.command_input = c.to_string();
        }

        let cmd = self.command_input.trim().to_string();
        if cmd.is_empty() {
            return;
        }

        let target_serial = match &self.state {
            TechConnectionState::Connected { device } => Some(device.serial.clone()),
            _ => self.server_devices.first().map(|d| d.serial.clone()),
        };

        self.is_command_running = true;
        self.terminal_logs.push((
            "SHELL".to_string(),
            format!(
                "adb {}shell {}",
                target_serial
                    .as_ref()
                    .map(|s| format!("-s {s} "))
                    .unwrap_or_default(),
                cmd
            ),
        ));

        let tx = self.event_tx.clone();
        let adb = self.adb;

        std::thread::spawn(move || {
            let res = adb.execute_shell(target_serial.as_deref(), &cmd);
            match res {
                Ok(output) => {
                    let _ = tx.send(GuiAsyncEvent::CommandResult { output });
                }
                Err(e) => {
                    let _ = tx.send(GuiAsyncEvent::CommandError {
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    fn reboot_target(&mut self, target: &str) {
        let target_serial = match &self.state {
            TechConnectionState::Connected { device } => Some(device.serial.clone()),
            _ => self.server_devices.first().map(|d| d.serial.clone()),
        };

        let target_str = target.to_string();
        let target_name = match target {
            "" => "Sistema Normal",
            "recovery" => "Modo Recovery",
            "bootloader" => "Fastboot / Bootloader",
            "edl" => "Modo Qualcomm EDL 9008",
            other => other,
        }
        .to_string();

        let tx = self.event_tx.clone();
        let adb = self.adb;

        std::thread::spawn(move || {
            let res = adb.reboot(target_serial.as_deref(), &target_str);
            match res {
                Ok(_) => {
                    let _ = tx.send(GuiAsyncEvent::RebootResult {
                        message: format!("Orden de reinicio hacia '{target_name}' enviada."),
                        is_error: false,
                    });
                }
                Err(e) => {
                    let _ = tx.send(GuiAsyncEvent::RebootResult {
                        message: format!("Error al reiniciar hacia '{target_name}': {e}"),
                        is_error: true,
                    });
                }
            }
        });
    }

    // ========================================================================
    // VECTOR DRAWING & UI HELPERS
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

    fn spec_card(ui: &mut egui::Ui, title: &str, value: &str, subtitle: &str, accent: Color32) {
        Frame::new()
            .fill(BG_RECESSED)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(title).size(9.0).strong().color(TEXT_MUTED));
                ui.label(
                    RichText::new(value)
                        .size(12.0)
                        .monospace()
                        .strong()
                        .color(accent),
                );
                ui.label(RichText::new(subtitle).size(9.5).color(TEXT_SECONDARY));
            });
    }

    fn render_sidebar_contents(&mut self, ui: &mut egui::Ui) {
        ScrollArea::vertical()
            .id_salt("tech_sidebar_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    Self::draw_brand_mark(ui);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("ADBHUB-LINK")
                                .size(14.5)
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.label(
                            RichText::new("ESTACIÓN DEL TÉCNICO")
                                .size(9.5)
                                .strong()
                                .color(BORDER_ACCENT),
                        );
                    });
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                ui.label(
                    RichText::new("MÓDULOS DE TRABAJO")
                        .size(9.5)
                        .strong()
                        .color(TEXT_MUTED),
                );
                ui.add_space(4.0);

                Self::sidebar_nav_btn(
                    ui,
                    &mut self.active_nav,
                    TechNavTab::Dashboard,
                    "01  Panel de Control",
                    "Ficha técnica y reinicio",
                );
                ui.add_space(4.0);
                Self::sidebar_nav_btn(
                    ui,
                    &mut self.active_nav,
                    TechNavTab::TerminalPro,
                    "02  Consola ADB",
                    "Shell y diagnósticos 1-clic",
                );
                ui.add_space(4.0);
                Self::sidebar_nav_btn(
                    ui,
                    &mut self.active_nav,
                    TechNavTab::ServerDevices,
                    &format!("03  Equipos Activos ({})", self.server_devices.len()),
                    "Puertos en este servidor",
                );
                ui.add_space(4.0);
                Self::sidebar_nav_btn(
                    ui,
                    &mut self.active_nav,
                    TechNavTab::DiagnosticsGuide,
                    "04  Guía & Laboratorio",
                    "UnlockTool / Chimera / SamFirm",
                );
                ui.add_space(4.0);
                Self::sidebar_nav_btn(
                    ui,
                    &mut self.active_nav,
                    TechNavTab::ApkQr,
                    "05  Compartir APK (QR)",
                    "MediaFire • Enlace al Cliente",
                );

                ui.add_space(8.0);
                Frame::new()
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0, Color32::from_rgb(14, 165, 233)))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if let Some(tex) = &self.qr_texture {
                                ui.image((tex.id(), Vec2::new(38.0, 38.0)));
                            }
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("QR APK CLIENTE")
                                        .size(9.5)
                                        .strong()
                                        .color(Color32::WHITE),
                                );
                                if ui
                                    .button(
                                        RichText::new("Ver QR / Compartir")
                                            .size(9.0)
                                            .color(BORDER_ACCENT),
                                    )
                                    .clicked()
                                {
                                    self.active_nav = TechNavTab::ApkQr;
                                }
                            });
                        });
                    });

                ui.add_space(12.0);
                Frame::new()
                    .fill(BG_RECESSED)
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (col, txt) = match &self.state {
                                TechConnectionState::Connected { .. } => {
                                    (STATUS_GREEN, "ENLACE ACTIVO")
                                }
                                TechConnectionState::Connecting => (STATUS_AMBER, "CONECTANDO..."),
                                _ => (TEXT_SECONDARY, "SERVIDOR LISTO"),
                            };
                            Self::draw_status_dot(ui, col);
                            ui.label(RichText::new(txt).size(10.0).strong().color(col));
                        });
                        ui.label(
                            RichText::new("127.0.0.1:5037")
                                .size(9.5)
                                .monospace()
                                .color(TEXT_MUTED),
                        );
                    });
            });
    }

    fn sidebar_nav_btn(
        ui: &mut egui::Ui,
        current: &mut TechNavTab,
        target: TechNavTab,
        title: &str,
        subtitle: &str,
    ) {
        let active = *current == target;
        let fill = if active {
            Color32::from_rgb(20, 42, 74)
        } else {
            BG_CARD
        };
        let border = if active { ACCENT_BLUE } else { BORDER_SUBTLE };

        let resp = Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0, border))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(8, 7))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(title)
                            .size(11.5)
                            .strong()
                            .color(if active { Color32::WHITE } else { TEXT_PRIMARY }),
                    );
                    ui.label(RichText::new(subtitle).size(9.5).color(TEXT_MUTED));
                });
            })
            .response
            .interact(Sense::click());

        if resp.clicked() {
            *current = target;
        }
    }

    fn render_connection_header(
        &mut self,
        ui: &mut egui::Ui,
        do_connect: &mut bool,
        do_cancel: &mut bool,
        do_disconnect: &mut Option<String>,
    ) {
        let is_connecting = matches!(self.state, TechConnectionState::Connecting);
        let is_connected = matches!(self.state, TechConnectionState::Connected { .. });

        // Contenedor principal con estilo Hero / Command Center
        let card_border = if is_connected {
            STATUS_GREEN
        } else if is_connecting {
            STATUS_AMBER
        } else {
            BORDER_SUBTLE
        };

        Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, card_border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // Fila Superior: Título descriptivo + Badge de estado
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("VINCULACIÓN REMOTA CON EL CLIENTE")
                            .size(10.5)
                            .strong()
                            .color(TEXT_MUTED),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let TechConnectionState::Connected { device } = &self.state {
                            Self::pill_badge(
                                ui,
                                &format!("CONECTADO: {}", device.serial),
                                STATUS_GREEN,
                                STATUS_GREEN_BG,
                            );
                        } else if is_connecting {
                            Self::pill_badge(
                                ui,
                                "RESOLVIENDO TÚNEL...",
                                STATUS_AMBER,
                                Color32::from_rgb(60, 40, 10),
                            );
                        } else {
                            Self::pill_badge(
                                ui,
                                "LISTO PARA CONEXIÓN",
                                ACCENT_BLUE,
                                Color32::from_rgb(10, 30, 55),
                            );
                        }
                    });
                });

                ui.add_space(8.0);

                // Caja de Entrada Hero (Input Box Prominente con Recessed Glow)
                let input_frame_bg = BG_RECESSED;
                let input_stroke_color = if is_connecting {
                    STATUS_AMBER
                } else if is_connected {
                    STATUS_GREEN
                } else {
                    Color32::from_rgb(38, 56, 84)
                };

                Frame::new()
                    .fill(input_frame_bg)
                    .stroke(Stroke::new(1.5, input_stroke_color))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        ui.horizontal(|ui| {
                            // Icono visual de enlace o llave
                            ui.label(
                                RichText::new("ID / IP")
                                    .size(11.0)
                                    .strong()
                                    .color(BORDER_ACCENT),
                            );

                            ui.add_space(6.0);

                            // Campo de texto grande y de alto contraste
                            let avail = ui.available_width();
                            let reserve_w = if is_connecting {
                                170.0
                            } else if is_connected {
                                260.0
                            } else {
                                230.0
                            };
                            let input_w = (avail - reserve_w).max(180.0);

                            let text_edit = egui::TextEdit::singleline(&mut self.input_code)
                                .hint_text("Pegue aquí el Código (ej. 8924-BE29-19BC-30DB) o IP:5555")
                                .font(egui::TextStyle::Monospace)
                                .text_color(TEXT_PRIMARY);

                            let edit_response = ui.add_sized([input_w, 32.0], text_edit);

                            // Botón de pegar desde el portapapeles si está vacío o para comodidad
                            let paste_btn = ui.add(
                                egui::Button::new(
                                    RichText::new("Pegar")
                                        .size(11.0)
                                        .strong()
                                        .color(BORDER_ACCENT),
                                )
                                .fill(Color32::from_rgb(20, 32, 50))
                                .stroke(Stroke::new(1.0, Color32::from_rgb(40, 60, 90)))
                                .min_size(Vec2::new(56.0, 32.0)),
                            );

                            if paste_btn.clicked() {
                                let clip = ui.input(|i| {
                                    i.events.iter().find_map(|e| {
                                        if let egui::Event::Paste(s) = e {
                                            Some(s.clone())
                                        } else {
                                            None
                                        }
                                    })
                                });
                                if let Some(text) = clip {
                                    let trimmed = text.trim().to_string();
                                    if !trimmed.is_empty() {
                                        self.input_code = trimmed;
                                    }
                                }
                            }

                            // Botón de Limpiar si hay texto
                            if !self.input_code.is_empty() {
                                let clear_btn = ui.add(
                                    egui::Button::new(
                                        RichText::new("x")
                                            .size(11.5)
                                            .strong()
                                            .color(TEXT_MUTED),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .min_size(Vec2::new(24.0, 32.0)),
                                );
                                if clear_btn.clicked() {
                                    self.input_code.clear();
                                }
                            }

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if is_connecting {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("CANCELAR")
                                                    .size(11.5)
                                                    .strong()
                                                    .color(Color32::WHITE),
                                            )
                                            .fill(STATUS_RED_BG)
                                            .stroke(Stroke::new(1.0, STATUS_RED))
                                            .min_size(Vec2::new(95.0, 32.0)),
                                        )
                                        .clicked()
                                    {
                                        *do_cancel = true;
                                    }
                                    ui.add(egui::Spinner::new().size(18.0));
                                } else {
                                    let connect_btn = ui.add(
                                        egui::Button::new(
                                            RichText::new("CONECTAR AHORA")
                                                .size(12.0)
                                                .strong()
                                                .color(Color32::WHITE),
                                        )
                                        .fill(ACCENT_BLUE_DARK)
                                        .stroke(Stroke::new(1.0, ACCENT_BLUE))
                                        .min_size(Vec2::new(135.0, 32.0)),
                                    );

                                    let enter = edit_response.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                    if enter || connect_btn.clicked() {
                                        *do_connect = true;
                                    }

                                    if let TechConnectionState::Connected { device } = &self.state {
                                        let ser = device.serial.clone();
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("DESCONECTAR")
                                                        .size(11.0)
                                                        .strong()
                                                        .color(Color32::WHITE),
                                                )
                                                .fill(STATUS_RED_BG)
                                                .stroke(Stroke::new(1.0, STATUS_RED))
                                                .min_size(Vec2::new(105.0, 32.0)),
                                            )
                                            .clicked()
                                        {
                                            *do_disconnect = Some(ser);
                                        }
                                    }
                                }
                            });
                        });
                    });

                ui.add_space(6.0);

                // Fila Inferior: Chips de Historial / Recientes y Guía Rápida
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Historial Reciente:")
                            .size(10.0)
                            .strong()
                            .color(TEXT_MUTED),
                    );

                    let eps = self.recent_endpoints.clone();
                    if eps.is_empty() {
                        ui.label(
                            RichText::new("Sin conexiones previas")
                                .size(10.0)
                                .italics()
                                .color(TEXT_MUTED),
                        );
                    } else {
                        for ep in eps {
                            let is_active = self.input_code.trim() == ep.trim();
                            let (chip_bg, chip_stroke, chip_fg) = if is_active {
                                (Color32::from_rgb(20, 45, 75), ACCENT_BLUE, Color32::WHITE)
                            } else {
                                (Color32::from_rgb(18, 25, 40), BORDER_SUBTLE, TEXT_SECONDARY)
                            };

                            let btn = egui::Button::new(
                                RichText::new(&ep)
                                    .monospace()
                                    .size(10.5)
                                    .color(chip_fg),
                            )
                            .fill(chip_bg)
                            .stroke(Stroke::new(1.0, chip_stroke))
                            .corner_radius(CornerRadius::same(5));

                            if ui.add(btn).clicked() {
                                self.input_code = ep;
                            }
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("Acepta códigos AnyDesk (16 car.) o IP:5555 • Presione Enter para conectar")
                                .size(9.5)
                                .color(TEXT_MUTED),
                        );
                    });
                });

                if let TechConnectionState::Error(err) = &self.state {
                    ui.add_space(6.0);
                    Frame::new()
                        .fill(STATUS_RED_BG)
                        .stroke(Stroke::new(1.0, STATUS_RED))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(10, 7))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("!")
                                        .size(13.0)
                                        .strong()
                                        .color(STATUS_RED),
                                );
                                ui.label(RichText::new(err).size(11.0).color(Color32::WHITE));
                            });
                        });
                }
            });
    }

    fn render_dashboard_tab(
        &mut self,
        ui: &mut egui::Ui,
        do_reboot: &mut Option<&'static str>,
        do_reconnect_serial: &mut Option<String>,
    ) {
        let active_device = match &self.state {
            TechConnectionState::Connected { device } => Some(device.clone()),
            _ => self.server_devices.first().cloned(),
        };
        let wide = ui.available_width() >= 540.0;

        if let Some(device) = active_device {
            let current_serial = device.serial.clone();

            // CARD 1: HARDWARE TELEMETRY SHEET
            Frame::new()
                .fill(BG_CARD)
                .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.vertical(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(device.display_name())
                                        .size(15.5)
                                        .strong()
                                        .color(Color32::WHITE),
                                );
                                Self::pill_badge(
                                    ui,
                                    "MONTADO EN ADB SERVER LOCAL",
                                    STATUS_GREEN,
                                    STATUS_GREEN_BG,
                                );
                            });
                            ui.label(
                                RichText::new(format!(
                                    "ID: {}  •  Listo para UnlockTool, Chimera, SamFirm y Terminal",
                                    device.serial
                                ))
                                .size(11.0)
                                .color(TEXT_SECONDARY),
                            );
                        });

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let cmd_str = format!("adb -s {current_serial} shell");
                            if ui
                                .small_button(RichText::new("Copiar Shell").size(11.0))
                                .clicked()
                            {
                                ui.ctx().copy_text(cmd_str);
                                self.last_action_message = Some((
                                    Instant::now(),
                                    "Comando copiado al portapapeles.".to_string(),
                                ));
                            }
                            if ui
                                .small_button(RichText::new("Refrescar").size(11.0))
                                .clicked()
                            {
                                *do_reconnect_serial = Some(current_serial.clone());
                            }
                        });
                    });

                    ui.add_space(8.0);

                    let bat_str = device
                        .battery_percent
                        .map(|b| format!("{b}%"))
                        .unwrap_or_else(|| "Activa".to_string());
                    let soc_str = if self.telemetry.soc_platform.is_empty() {
                        "ARM64".to_string()
                    } else {
                        self.telemetry.soc_platform.clone()
                    };
                    let build_str = if self.telemetry.build_id.is_empty() {
                        "Release".to_string()
                    } else {
                        self.telemetry.build_id.clone()
                    };

                    if wide {
                        ui.columns(4, |cols| {
                            Self::spec_card(
                                &mut cols[0],
                                "MARCA / MODELO",
                                &device.display_name(),
                                device.product.as_deref().unwrap_or("Android"),
                                Color32::WHITE,
                            );
                            Self::spec_card(
                                &mut cols[1],
                                "SERIAL / ENDPOINT",
                                &device.serial,
                                if device.is_network_device() {
                                    "Túnel TCP/IP"
                                } else {
                                    "Cable USB"
                                },
                                BORDER_ACCENT,
                            );
                            Self::spec_card(
                                &mut cols[2],
                                "VERSIÓN ANDROID",
                                device.android_version.as_deref().unwrap_or("Android OS"),
                                &format!("SDK: {}", self.telemetry.sdk_int),
                                Color32::WHITE,
                            );
                            Self::spec_card(
                                &mut cols[3],
                                "NIVEL BATERÍA",
                                &bat_str,
                                "Energía USB",
                                STATUS_GREEN,
                            );
                        });
                        ui.add_space(6.0);
                        ui.columns(4, |cols| {
                            Self::spec_card(
                                &mut cols[0],
                                "PLATAFORMA / SOC",
                                &soc_str,
                                &format!("ABI: {}", self.telemetry.cpu_abi),
                                Color32::WHITE,
                            );
                            Self::spec_card(
                                &mut cols[1],
                                "PARCHE SEGURIDAD",
                                device.security_patch.as_deref().unwrap_or("Estándar"),
                                "Seguridad OS",
                                Color32::WHITE,
                            );
                            Self::spec_card(
                                &mut cols[2],
                                "COMPILACIÓN",
                                &build_str,
                                "Firmware",
                                Color32::WHITE,
                            );
                            Self::spec_card(
                                &mut cols[3],
                                "ESTADO PUENTE",
                                "AUTORIZADO",
                                "Puerto 5037 activo",
                                STATUS_GREEN,
                            );
                        });
                    } else {
                        ui.columns(2, |cols| {
                            Self::spec_card(
                                &mut cols[0],
                                "MARCA / MODELO",
                                &device.display_name(),
                                device.product.as_deref().unwrap_or("Android"),
                                Color32::WHITE,
                            );
                            Self::spec_card(
                                &mut cols[1],
                                "SERIAL / ENDPOINT",
                                &device.serial,
                                "Activo",
                                BORDER_ACCENT,
                            );
                        });
                        ui.add_space(4.0);
                        ui.columns(2, |cols| {
                            Self::spec_card(
                                &mut cols[0],
                                "VERSIÓN ANDROID",
                                device.android_version.as_deref().unwrap_or("Android OS"),
                                &format!("SDK: {}", self.telemetry.sdk_int),
                                Color32::WHITE,
                            );
                            Self::spec_card(
                                &mut cols[1],
                                "NIVEL BATERÍA",
                                &bat_str,
                                "Energía USB",
                                STATUS_GREEN,
                            );
                        });
                    }
                });

            ui.add_space(8.0);

            // CARD 2: REBOOT & MODE CONTROL MATRIX
            Frame::new()
                .fill(BG_CARD)
                .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("CONTROL DE MODOS DE ARRANQUE E INGENIERÍA")
                                .size(10.5)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                        if let Some((t, msg)) = &self.last_action_message {
                            if t.elapsed() < std::time::Duration::from_secs(5) {
                                ui.label(
                                    RichText::new(msg).size(10.5).strong().color(STATUS_GREEN),
                                );
                            }
                        }
                    });
                    ui.add_space(6.0);

                    if wide {
                        ui.columns(4, |cols| {
                            if Self::mode_action_card(
                                &mut cols[0],
                                "Reinicio Normal",
                                "adb reboot",
                                ACCENT_BLUE,
                            ) {
                                *do_reboot = Some("");
                            }
                            if Self::mode_action_card(
                                &mut cols[1],
                                "Modo Recovery",
                                "adb reboot recovery",
                                STATUS_AMBER,
                            ) {
                                *do_reboot = Some("recovery");
                            }
                            if Self::mode_action_card(
                                &mut cols[2],
                                "Fastboot / Bootloader",
                                "adb reboot bootloader",
                                BORDER_ACCENT,
                            ) {
                                *do_reboot = Some("bootloader");
                            }
                            if Self::mode_action_card(
                                &mut cols[3],
                                "Qualcomm EDL 9008",
                                "adb reboot edl",
                                STATUS_RED,
                            ) {
                                *do_reboot = Some("edl");
                            }
                        });
                    } else {
                        ui.columns(2, |cols| {
                            if Self::mode_action_card(
                                &mut cols[0],
                                "Reinicio Normal",
                                "adb reboot",
                                ACCENT_BLUE,
                            ) {
                                *do_reboot = Some("");
                            }
                            if Self::mode_action_card(
                                &mut cols[1],
                                "Modo Recovery",
                                "adb reboot recovery",
                                STATUS_AMBER,
                            ) {
                                *do_reboot = Some("recovery");
                            }
                        });
                        ui.add_space(4.0);
                        ui.columns(2, |cols| {
                            if Self::mode_action_card(
                                &mut cols[0],
                                "Fastboot / Bootloader",
                                "adb reboot bootloader",
                                BORDER_ACCENT,
                            ) {
                                *do_reboot = Some("bootloader");
                            }
                            if Self::mode_action_card(
                                &mut cols[1],
                                "Qualcomm EDL 9008",
                                "adb reboot edl",
                                STATUS_RED,
                            ) {
                                *do_reboot = Some("edl");
                            }
                        });
                    }
                });

            ui.add_space(8.0);
        } else {
            Frame::new()
                .fill(BG_CARD)
                .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(16, 18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new("Ningún Equipo Remoto Seleccionado Actualmente")
                            .size(14.0)
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.label(
                        RichText::new(
                            "Ingrese la dirección IP:5555 o el código de la estación del cliente arriba y pulse 'CONECTAR EQUIPO'.",
                        )
                        .size(11.5)
                        .color(TEXT_SECONDARY),
                    );
                });
            ui.add_space(8.0);
        }

        // COMPACT LIVE TERMINAL IN DASHBOARD
        self.render_terminal_widget(ui, 150.0);
    }

    fn mode_action_card(ui: &mut egui::Ui, title: &str, cmd_sub: &str, accent: Color32) -> bool {
        let resp = Frame::new()
            .fill(BG_RECESSED)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(11.5).strong().color(accent));
                    ui.label(
                        RichText::new(cmd_sub)
                            .size(9.5)
                            .monospace()
                            .color(TEXT_MUTED),
                    );
                });
            })
            .response
            .interact(Sense::click());

        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp.clicked()
    }

    fn render_terminal_pro_tab(&mut self, ui: &mut egui::Ui) {
        let mut quick_cmd: Option<&'static str> = None;

        Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("COMANDOS RÁPIDOS DE LABORATORIO GSM (1 CLIC)")
                        .size(10.5)
                        .strong()
                        .color(TEXT_SECONDARY),
                );
                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    for (label, cmd) in [
                        (
                            "Identidad Completa",
                            "getprop | grep -E 'model|brand|version|serial'",
                        ),
                        ("Estado Batería", "dumpsys battery"),
                        ("Espacio Particiones", "df -h /data /system"),
                        ("Cuentas Sincronizadas", "dumpsys account"),
                        ("Estado FRP / Lock", "getprop ro.frp.pst"),
                        ("Configuración Red IP", "ip addr show wlan0"),
                        ("Paquetes 3ros", "pm list packages -3"),
                        ("Abrir Ajustes", "am start -a android.settings.SETTINGS"),
                    ] {
                        if ui
                            .button(RichText::new(label).size(11.0))
                            .clicked()
                        {
                            quick_cmd = Some(cmd);
                        }
                    }
                });
            });

        if let Some(c) = quick_cmd {
            self.execute_shell_command(Some(c));
        }

        ui.add_space(8.0);
        self.render_terminal_widget(ui, 260.0);
    }

    fn render_terminal_widget(&mut self, ui: &mut egui::Ui, log_height: f32) {
        let mut run_cmd: Option<String> = None;

        Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("CONSOLA TERMINAL ADB INTERACTIVA")
                            .size(10.5)
                            .strong()
                            .color(TEXT_SECONDARY),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Limpiar").clicked() {
                            self.terminal_logs.clear();
                        }
                        if ui.small_button("Copiar Log").clicked() {
                            let text = self
                                .terminal_logs
                                .iter()
                                .map(|(t, m)| format!("[{t}] {m}"))
                                .collect::<Vec<_>>()
                                .join("\n");
                            ui.ctx().copy_text(text);
                        }
                    });
                });

                ui.add_space(6.0);

                Frame::new()
                    .fill(BG_RECESSED)
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ScrollArea::vertical()
                            .id_salt("tech_terminal_scroll")
                            .max_height(log_height)
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                for (tag, line) in &self.terminal_logs {
                                    ui.horizontal_wrapped(|ui| {
                                        let col = match tag.as_str() {
                                            "ERROR" => STATUS_RED,
                                            "CONEXION" | "REBOOT" => STATUS_GREEN,
                                            "SHELL" | "ADB" => BORDER_ACCENT,
                                            "SALIDA" => Color32::WHITE,
                                            _ => TEXT_MUTED,
                                        };
                                        ui.label(
                                            RichText::new(format!("[{tag}]"))
                                                .size(10.5)
                                                .monospace()
                                                .strong()
                                                .color(col),
                                        );
                                        ui.label(
                                            RichText::new(line)
                                                .size(11.0)
                                                .monospace()
                                                .color(TEXT_PRIMARY),
                                        );
                                    });
                                }
                            });
                    });

                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("adb >")
                            .size(11.0)
                            .monospace()
                            .strong()
                            .color(BORDER_ACCENT),
                    );

                    let w = (ui.available_width() - 95.0).max(100.0);
                    let edit = ui.add_sized(
                        [w, 26.0],
                        egui::TextEdit::singleline(&mut self.command_input)
                            .hint_text("Comando shell...")
                            .font(egui::TextStyle::Monospace),
                    );

                    let enter =
                        edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let btn = ui
                        .add_enabled(
                            !self.is_command_running,
                            egui::Button::new(
                                RichText::new("Ejecutar")
                                    .size(11.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(ACCENT_BLUE_DARK)
                            .stroke(Stroke::new(1.0, ACCENT_BLUE))
                            .min_size(Vec2::new(82.0, 26.0)),
                        )
                        .clicked();

                    if (enter || btn) && !self.command_input.trim().is_empty() {
                        run_cmd = Some(self.command_input.clone());
                    }
                });
            });

        if let Some(c) = run_cmd {
            self.execute_shell_command(Some(&c));
        }
    }

    fn render_server_devices_tab(
        &mut self,
        ui: &mut egui::Ui,
        do_disconnect: &mut Option<String>,
    ) {
        Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("EQUIPOS MONTADOS EN EL SERVIDOR ADB DE ESTA PC")
                                .size(11.5)
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.label(
                            RichText::new(
                                "Lista en tiempo real de los teléfonos conectados al servidor ADB local.",
                            )
                            .size(11.0)
                            .color(TEXT_SECONDARY),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Actualizar Lista").clicked() {
                            self.trigger_server_devices_poll();
                        }
                    });
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                if self.server_devices.is_empty() {
                    Frame::new()
                        .fill(BG_RECESSED)
                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(14, 20))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("No hay dispositivos activos en adb devices")
                                        .size(13.0)
                                        .strong()
                                        .color(TEXT_SECONDARY),
                                );
                            });
                        });
                } else {
                    let list = self.server_devices.clone();
                    for dev in list {
                        Frame::new()
                            .fill(BG_RECESSED)
                            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal_wrapped(|ui| {
                                    ui.vertical(|ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(
                                                RichText::new(dev.display_name())
                                                    .size(13.5)
                                                    .strong()
                                                    .color(Color32::WHITE),
                                            );
                                            if dev.is_network_device() {
                                                Self::pill_badge(
                                                    ui,
                                                    "PUENTE REMOTO TCP/IP",
                                                    BORDER_ACCENT,
                                                    Color32::from_rgb(12, 45, 72),
                                                );
                                            } else {
                                                Self::pill_badge(
                                                    ui,
                                                    "USB FÍSICO LOCAL",
                                                    STATUS_GREEN,
                                                    STATUS_GREEN_BG,
                                                );
                                            }
                                        });
                                        ui.label(
                                            RichText::new(format!(
                                                "ID: {}  •  Estado: {}  •  Android: {}",
                                                dev.serial,
                                                dev.state,
                                                dev.android_version.as_deref().unwrap_or("N/D")
                                            ))
                                            .size(11.0)
                                            .monospace()
                                            .color(TEXT_SECONDARY),
                                        );
                                    });

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if dev.is_network_device() {
                                            if ui
                                                .small_button(
                                                    RichText::new("Desconectar").color(STATUS_RED),
                                                )
                                                .clicked()
                                            {
                                                *do_disconnect = Some(dev.serial.clone());
                                            }
                                        }
                                        if ui
                                            .small_button(
                                                RichText::new("Seleccionar")
                                                    .strong()
                                                    .color(BORDER_ACCENT),
                                            )
                                            .clicked()
                                        {
                                            self.state = TechConnectionState::Connected {
                                                device: dev.clone(),
                                            };
                                            self.active_nav = TechNavTab::Dashboard;
                                        }
                                    });
                                });
                            });
                        ui.add_space(6.0);
                    }
                }
            });
    }

    fn render_diagnostics_guide_tab(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("TUTORIAL: FLUJO DE CONEXIÓN CON EL CLIENTE REMOTO")
                        .size(12.0)
                        .strong()
                        .color(BORDER_ACCENT),
                );
                ui.label(
                    RichText::new("Guía de 3 pasos para vincular clientes en cualquier parte del mundo sin configuración de puertos.")
                        .size(10.5)
                        .color(TEXT_MUTED),
                );
                ui.add_space(8.0);

                let steps = [
                    (
                        "1",
                        "Solicite el Código de 16 caracteres al Cliente",
                        "Indique al cliente que abra 'adbhub-agent.exe' (Windows), 'ADBHUB-LINK.sh' (Linux) o 'adbhub-client.apk' (Android OTG) y pulse 'COPIAR CÓDIGO DE ESTACIÓN'.",
                        Color32::from_rgb(14, 165, 233),
                    ),
                    (
                        "2",
                        "Pegue el Código en la Barra Superior y presione Conectar",
                        "Use el botón 'Pegar' o 'Ctrl+V'. La aplicación decodificará automáticamente el túnel de internet ('bore.pub:<puerto>') o la IP local y establecerá el enlace ADB sin consolas.",
                        Color32::from_rgb(245, 158, 11),
                    ),
                    (
                        "3",
                        "El Cliente Autoriza la Huella RSA en la Pantalla del Móvil",
                        "Al conectar, saltará en el celular del cliente la ventana: '¿Permitir depuración por USB?'. El cliente debe marcar 'Permitir siempre' y tocar Aceptar. En ese instante el equipo quedará en estado 'device' listo para UnlockTool, Chimera, SamFirm o Consola ADB.",
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

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    RichText::new("INTEGRACIÓN CON SOFTWARE GSM DE TERCEROS")
                        .size(11.5)
                        .strong()
                        .color(BORDER_ACCENT),
                );
                ui.add_space(6.0);

                for (title, body) in [
                    (
                        "¿Cómo ven el teléfono remoto programas como UnlockTool, Chimera o SamFirm?",
                        "Cuando pulsa 'CONECTAR AHORA', ADBHUB-LINK monta el dispositivo remoto dentro del demonio local de Windows (127.0.0.1:5037). Cualquier herramienta GSM que consulte el servidor ADB detectará el teléfono automáticamente.",
                    ),
                    (
                        "Compatibilidad Celular a Celular (USB-C OTG sin PC)",
                        "Si el cliente no tiene PC, puede instalar 'adbhub-client.apk' en otro celular Android y conectarlo al móvil a reparar mediante un cable USB-C a USB-C. El APK expone el túnel y usted lo controla desde aquí igual que si estuviera en una computadora.",
                    ),
                ] {
                    Frame::new()
                        .fill(BG_RECESSED)
                        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new(title)
                                    .size(11.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                            ui.add_space(3.0);
                            ui.label(RichText::new(body).size(10.5).color(TEXT_SECONDARY));
                        });
                    ui.add_space(6.0);
                }
            });
    }

    fn render_apk_qr_tab(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("DESCARGA DE APK CLIENTE — CÓDIGO QR Y ENLACE DIRECTO")
                        .size(13.0)
                        .strong()
                        .color(BORDER_ACCENT),
                );
                ui.label(
                    RichText::new(
                        "Comparta este código QR o enlace con el cliente remoto para que instale 'adbhub-client.apk' en su teléfono Android OTG.",
                    )
                    .size(10.5)
                    .color(TEXT_MUTED),
                );

                ui.add_space(14.0);

                ui.horizontal_top(|ui| {
                    // Columna Izquierda: QR Grande de alto contraste
                    Frame::new()
                        .fill(Color32::WHITE)
                        .stroke(Stroke::new(2.0, BORDER_ACCENT))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::same(14))
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                if let Some(tex) = &self.qr_texture {
                                    ui.image((tex.id(), Vec2::new(210.0, 210.0)));
                                } else {
                                    ui.label(
                                        RichText::new("Generando código QR...")
                                            .size(11.0)
                                            .color(Color32::BLACK),
                                    );
                                }
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new("Escanear con Cámara o App QR")
                                        .size(10.5)
                                        .strong()
                                        .color(Color32::from_rgb(15, 23, 42)),
                                );
                            });
                        });

                    ui.add_space(16.0);

                    // Columna Derecha: Enlace MediaFire, botones y pasos
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("ENLACE DIRECTO DE DESCARGA (MEDIAFIRE):")
                                .size(10.0)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                        ui.add_space(4.0);

                        Frame::new()
                            .fill(BG_RECESSED)
                            .stroke(Stroke::new(1.0, Color32::from_rgb(38, 56, 84)))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(APK_MEDIAFIRE_URL)
                                            .size(10.5)
                                            .color(Color32::from_rgb(56, 189, 248))
                                            .monospace(),
                                    );
                                });
                            });

                        ui.add_space(8.0);

                        ui.horizontal(|ui| {
                            if ui
                                .button(
                                    RichText::new("Copiar Enlace al Portapapeles")
                                        .size(11.0)
                                        .strong()
                                        .color(Color32::WHITE),
                                )
                                .clicked()
                            {
                                ui.ctx().copy_text(APK_MEDIAFIRE_URL.to_string());
                                self.terminal_logs.push((
                                    "COMPARTIR".to_string(),
                                    "Enlace de descarga de MediaFire copiado al portapapeles."
                                        .to_string(),
                                ));
                                self.last_action_message = Some((
                                    Instant::now(),
                                    "Enlace MediaFire copiado al portapapeles con éxito.".to_string(),
                                ));
                            }

                            if ui
                                .button(
                                    RichText::new("Abrir en Navegador")
                                        .size(11.0)
                                        .color(BORDER_ACCENT),
                                )
                                .clicked()
                            {
                                #[cfg(target_os = "windows")]
                                {
                                    let _ = std::process::Command::new("cmd")
                                        .args(["/c", "start", "", APK_MEDIAFIRE_URL])
                                        .spawn();
                                }
                                #[cfg(not(target_os = "windows"))]
                                {
                                    let _ = std::process::Command::new("xdg-open")
                                        .arg(APK_MEDIAFIRE_URL)
                                        .spawn();
                                }
                            }
                        });

                        ui.add_space(14.0);

                        // Tarjeta de Instrucciones para el Cliente
                        Frame::new()
                            .fill(BG_RECESSED)
                            .stroke(Stroke::new(1.0, BORDER_SUBTLE))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::same(12))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    RichText::new("INSTRUCCIONES RÁPIDAS PARA EL CLIENTE:")
                                        .size(10.0)
                                        .strong()
                                        .color(Color32::WHITE),
                                );
                                ui.add_space(6.0);

                                for (num, text) in [
                                    (
                                        "1",
                                        "Apuntar la cámara del teléfono al código QR de la izquierda o abrir el enlace enviado.",
                                    ),
                                    (
                                        "2",
                                        "Tocar el botón azul 'Download' en MediaFire para descargar 'adbhub-client.apk'.",
                                    ),
                                    (
                                        "3",
                                        "Instalar la app (si Android avisa sobre origen desconocido, seleccionar 'Permitir desde esta fuente').",
                                    ),
                                    (
                                        "4",
                                        "Abrir ADBHUB-LINK, conectar el cable USB-C a USB-C hacia el teléfono a reparar y pulsar 'COPIAR CÓDIGO DE ESTACIÓN'.",
                                    ),
                                ] {
                                    ui.horizontal_top(|ui| {
                                        ui.label(
                                            RichText::new(format!("[{num}]"))
                                                .size(10.0)
                                                .strong()
                                                .color(BORDER_ACCENT),
                                        );
                                        ui.label(
                                            RichText::new(text)
                                                .size(10.0)
                                                .color(TEXT_SECONDARY),
                                        );
                                    });
                                    ui.add_space(3.0);
                                }
                            });
                    });
                });
            });
    }
}

impl eframe::App for DesktopGuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                GuiAsyncEvent::Log(tag, msg) => {
                    self.terminal_logs.push((tag, msg));
                }
                GuiAsyncEvent::ConnectionSuccess {
                    device,
                    telemetry,
                    all_devices,
                } => {
                    let display_name = device.display_name();
                    let serial = device.serial.clone();
                    self.telemetry = telemetry;
                    self.server_devices = all_devices;
                    self.state = TechConnectionState::Connected { device };
                    self.terminal_logs.push((
                        "CONEXION".to_string(),
                        format!("Enlace establecido con éxito: {display_name} [{serial}]"),
                    ));
                }
                GuiAsyncEvent::ConnectionFailed {
                    error,
                    target_endpoint,
                } => {
                    let msg = if error.contains("10013") {
                        format!(
                            "Bloqueo de VPN detectado hacia [{target_endpoint}] (Error 10013): Su VPN (ej. ProtonVPN) está bloqueando el acceso a la red local LAN. Active 'Permitir conexiones LAN' en su VPN o use el Código con Relay Global (bore.pub) del cliente."
                        )
                    } else {
                        format!(
                            "No se pudo conectar a [{target_endpoint}]. Verifique que el cliente tenga el puente activo. ({error})"
                        )
                    };
                    self.state = TechConnectionState::Error(msg);
                    self.terminal_logs.push((
                        "ERROR".to_string(),
                        format!("Fallo de conexión hacia [{target_endpoint}]: {error}"),
                    ));
                }
                GuiAsyncEvent::ServerDevicesUpdated { devices } => {
                    self.server_poll_in_flight = false;
                    self.server_devices = devices;
                }
                GuiAsyncEvent::CommandResult { output } => {
                    self.is_command_running = false;
                    let trimmed = output.trim().to_string();
                    if trimmed.is_empty() {
                        self.terminal_logs.push((
                            "SALIDA".to_string(),
                            "[Comando completado sin salida]".to_string(),
                        ));
                    } else {
                        self.terminal_logs.push(("SALIDA".to_string(), trimmed));
                    }
                }
                GuiAsyncEvent::CommandError { error } => {
                    self.is_command_running = false;
                    self.terminal_logs.push((
                        "ERROR".to_string(),
                        format!("Error ejecutando comando: {error}"),
                    ));
                }
                GuiAsyncEvent::RebootResult { message, is_error } => {
                    self.last_action_message = Some((Instant::now(), message.clone()));
                    let tag = if is_error { "ERROR" } else { "REBOOT" };
                    self.terminal_logs.push((tag.to_string(), message));
                }
            }
        }

        if matches!(self.state, TechConnectionState::Connecting) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(1500));
        }

        if !self.server_poll_in_flight
            && self.last_server_poll.elapsed() > std::time::Duration::from_secs(4)
        {
            self.trigger_server_devices_poll();
        }

        let mut do_connect = false;
        let mut do_cancel = false;
        let mut do_disconnect = None;
        let mut do_reconnect_serial = None;
        let mut do_reboot = None;

        // 1. LEFT RESIZABLE & SCROLLABLE SIDEBAR PANEL
        Panel::left("tech_left_sidebar")
            .resizable(true)
            .default_size(205.0)
            .size_range(175.0..=270.0)
            .frame(
                Frame::new()
                    .fill(BG_SIDEBAR)
                    .inner_margin(Margin::symmetric(12, 12))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ui, |ui| {
                self.render_sidebar_contents(ui);
            });

        // 2. BOTTOM STATUS FOOTER
        Panel::bottom("tech_bottom_footer")
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(BG_SIDEBAR)
                    .inner_margin(Margin::symmetric(14, 6))
                    .stroke(Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(
                            "ADBHUB-LINK Desktop v2.0 • Laboratorio GSM & Puente ADB • 127.0.0.1:5037",
                        )
                        .size(10.5)
                        .color(TEXT_MUTED),
                    );
                });
            });

        // 3. CENTRAL SCROLLABLE WORKSPACE
        CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(BG_CANVAS)
                    .inner_margin(Margin::symmetric(14, 12)),
            )
            .show(ui, |ui| {
                self.render_connection_header(
                    ui,
                    &mut do_connect,
                    &mut do_cancel,
                    &mut do_disconnect,
                );

                ui.add_space(8.0);

                ScrollArea::vertical()
                    .id_salt("tech_main_workspace_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.active_nav {
                        TechNavTab::Dashboard => {
                            self.render_dashboard_tab(
                                ui,
                                &mut do_reboot,
                                &mut do_reconnect_serial,
                            );
                        }
                        TechNavTab::TerminalPro => {
                            self.render_terminal_pro_tab(ui);
                        }
                        TechNavTab::ServerDevices => {
                            self.render_server_devices_tab(ui, &mut do_disconnect);
                        }
                        TechNavTab::DiagnosticsGuide => {
                            self.render_diagnostics_guide_tab(ui);
                        }
                        TechNavTab::ApkQr => {
                            self.render_apk_qr_tab(ui);
                        }
                    });
            });

        // Deferred Actions
        if do_connect {
            self.connect_to_code();
        }
        if do_cancel {
            self.state = TechConnectionState::Disconnected;
            self.terminal_logs.push((
                "CONEXION".to_string(),
                "Solicitud de conexión cancelada por el técnico.".to_string(),
            ));
        }
        if let Some(serial_disc) = do_disconnect {
            let adb = self.adb;
            let serial_clone = serial_disc.clone();
            let tx = self.event_tx.clone();
            std::thread::spawn(move || {
                let _ = adb.disconnect_tcp_device(&serial_clone);
                let devices = adb.list_devices().unwrap_or_default();
                let _ = tx.send(GuiAsyncEvent::ServerDevicesUpdated { devices });
            });
            self.state = TechConnectionState::Disconnected;
            self.terminal_logs.push((
                "SESION".to_string(),
                format!("Sesión remota con [{serial_disc}] desconectada."),
            ));
        }
        if let Some(recon_serial) = do_reconnect_serial {
            let tx = self.event_tx.clone();
            let adb = self.adb;
            let recon_clone = recon_serial.clone();
            self.terminal_logs.push((
                "BRIDGE".to_string(),
                format!("Refrescando enlace TCP con {recon_serial}..."),
            ));
            std::thread::spawn(move || {
                let res = adb.connect_tcp_device(&recon_clone);
                let out_msg = match res {
                    Ok(o) => o,
                    Err(e) => format!("Error: {e}"),
                };
                let _ = tx.send(GuiAsyncEvent::Log(
                    "BRIDGE".to_string(),
                    format!("Reconexión: {out_msg}"),
                ));
            });
        }
        if let Some(mode) = do_reboot {
            self.reboot_target(mode);
        }
    }
}

pub fn run_desktop_native_gui(
    rt: Arc<Runtime>,
    controller: Arc<DesktopController>,
) -> Result<(), eframe::Error> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 720.0])
            .with_min_inner_size([720.0, 480.0])
            .with_maximized(true)
            .with_title("ADBHUB-LINK — Estación del Técnico"),
        ..Default::default()
    };

    eframe::run_native(
        "ADBHUB-LINK Desktop",
        native_options,
        Box::new(move |cc| Ok(Box::new(DesktopGuiApp::new(cc, rt, controller)))),
    )
}
