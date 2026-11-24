package com.adbhub.link;

import android.animation.ObjectAnimator;
import android.animation.ValueAnimator;
import android.app.Activity;
import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.hardware.usb.UsbDevice;
import android.hardware.usb.UsbManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.HapticFeedbackConstants;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;

import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Date;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;

/**
 * Actividad Principal de ADBHUB-LINK Cliente Android USB-C OTG.
 * Diseño Enterprise Dark Slate / Material 3 Dark.
 * Incluye visual stepper OTG, código auto-ajustable sin corte de línea,
 * selector segmentado de formato, compartir nativo, feedback háptico y acordeones colapsables.
 */
public class MainActivity extends Activity implements OtgAdbBridge.BridgeLogger, BoreTunnelClient.TunnelListener {

    private static final String ACTION_USB_PERMISSION = "com.adbhub.link.USB_PERMISSION";

    // Paleta Enterprise Dark Slate / Material 3 Dark
    private static final int COLOR_BG_CANVAS      = Color.parseColor("#090D16");
    private static final int COLOR_BG_HEADER      = Color.parseColor("#0D1320");
    private static final int COLOR_BG_CARD        = Color.parseColor("#121927");
    private static final int COLOR_BG_RECESSED    = Color.parseColor("#080C14");
    private static final int COLOR_BORDER_SUBTLE  = Color.parseColor("#1E293B");
    private static final int COLOR_BORDER_ACCENT  = Color.parseColor("#38BDF8");
    private static final int COLOR_ACCENT_BLUE    = Color.parseColor("#0EA5E9");
    private static final int COLOR_ACCENT_DARK    = Color.parseColor("#0284C7");
    private static final int COLOR_STATUS_GREEN   = Color.parseColor("#10B981");
    private static final int COLOR_STATUS_AMBER   = Color.parseColor("#F59E0B");
    private static final int COLOR_STATUS_RED     = Color.parseColor("#EF4444");
    private static final int COLOR_TEXT_PRIMARY   = Color.parseColor("#F8FAFC");
    private static final int COLOR_TEXT_SECONDARY = Color.parseColor("#94A3B8");
    private static final int COLOR_TEXT_MUTED     = Color.parseColor("#64748B");

    private final Handler uiHandler = new Handler(Looper.getMainLooper());
    private final List<String> logLines = new ArrayList<>();

    private UsbManager usbManager;
    private OtgAdbBridge otgBridge;
    private BoreTunnelClient tunnelClient;

    private String localIp = "127.0.0.1";
    private int cloudTunnelPort = 0;
    private int codeSalt = 0x24;
    private boolean numericMode = false;
    private String currentCode = "----";
    private boolean technicianConnected = false;

    // Vistas principales
    private View statusDotView;
    private TextView tvHeaderBadge;
    private TextView tvCodeDisplay;
    private TextView tvCopyHint;
    private TextView tvRelayEndpoint;
    private TextView tvLanEndpoint;
    private Button btnSegmentHex;
    private Button btnSegmentNumeric;

    // Vistas de Hardware & Stepper OTG
    private TextView tvDeviceTitle;
    private TextView tvDeviceSpecs;
    private TextView tvStep1Badge;
    private TextView tvStep1Title;
    private TextView tvStep1Sub;
    private TextView tvStep2Badge;
    private TextView tvStep2Title;
    private TextView tvStep2Sub;
    private TextView tvStep3Badge;
    private TextView tvStep3Title;
    private TextView tvStep3Sub;
    private Button btnActionUsb;

    // Vistas de Acordeones
    private View guideContent;
    private TextView tvGuideToggle;
    private View logsContent;
    private TextView tvLogsToggle;
    private TextView tvLogsBadgeCount;
    private TextView tvLogsBox;

    private ObjectAnimator statusPulseAnimator;

    private final BroadcastReceiver usbReceiver = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            String action = intent.getAction();
            if (ACTION_USB_PERMISSION.equals(action)) {
                UsbDevice device = intent.getParcelableExtra(UsbManager.EXTRA_DEVICE);
                boolean granted = intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false);
                if (granted && device != null) {
                    onLog("USB", "Permiso USB Host concedido para " + getDeviceDisplayName(device));
                    otgBridge.openUsbDevice(device);
                } else {
                    onLog("AVISO", "Permiso USB OTG denegado por el usuario.");
                }
                refreshHardwareState();
            } else if (UsbManager.ACTION_USB_DEVICE_ATTACHED.equals(action)) {
                onLog("HOTPLUG", "Cable USB-C OTG conectado. Escaneando interfaz...");
                refreshHardwareState();
            } else if (UsbManager.ACTION_USB_DEVICE_DETACHED.equals(action)) {
                onLog("HOTPLUG", "Dispositivo USB-C OTG desconectado.");
                otgBridge.closeUsbDevice();
                refreshHardwareState();
            }
        }
    };

    private final Runnable periodicScanner = new Runnable() {
        @Override
        public void run() {
            refreshHardwareState();
            uiHandler.postDelayed(this, 2500);
        }
    };

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        requestWindowFeature(Window.FEATURE_NO_TITLE);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
            getWindow().setStatusBarColor(COLOR_BG_HEADER);
            getWindow().setNavigationBarColor(COLOR_BG_CANVAS);
        }

        usbManager = (UsbManager) getSystemService(Context.USB_SERVICE);
        localIp = StationCodeEncoder.detectLocalIpv4();
        otgBridge = new OtgAdbBridge(usbManager, this);
        tunnelClient = new BoreTunnelClient(otgBridge, this);

        setContentView(buildEnterpriseUi());
        updateStationCode();
        startStatusPulseAnimation();

        IntentFilter filter = new IntentFilter();
        filter.addAction(ACTION_USB_PERMISSION);
        filter.addAction(UsbManager.ACTION_USB_DEVICE_ATTACHED);
        filter.addAction(UsbManager.ACTION_USB_DEVICE_DETACHED);
        if (Build.VERSION.SDK_INT >= 33) {
            registerReceiver(usbReceiver, filter, Context.RECEIVER_NOT_EXPORTED);
        } else {
            registerReceiver(usbReceiver, filter);
        }

        onLog("INICIO", "Estación Android USB-C OTG activa (IP local: " + localIp + ")");
        onLog("TUNEL", "Iniciando enlace con Relay Mundial bore.pub...");

        tunnelClient.start();
        uiHandler.post(periodicScanner);
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        refreshHardwareState();
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        if (statusPulseAnimator != null) {
            statusPulseAnimator.cancel();
        }
        uiHandler.removeCallbacks(periodicScanner);
        try {
            unregisterReceiver(usbReceiver);
        } catch (Exception ignored) {}
        if (tunnelClient != null) {
            tunnelClient.stop();
        }
        if (otgBridge != null) {
            otgBridge.closeUsbDevice();
        }
    }

    private void startStatusPulseAnimation() {
        if (statusDotView != null) {
            statusPulseAnimator = ObjectAnimator.ofFloat(statusDotView, "alpha", 1.0f, 0.35f);
            statusPulseAnimator.setDuration(900);
            statusPulseAnimator.setRepeatCount(ValueAnimator.INFINITE);
            statusPulseAnimator.setRepeatMode(ValueAnimator.REVERSE);
            statusPulseAnimator.start();
        }
    }

    private void updateStationCode() {
        localIp = StationCodeEncoder.detectLocalIpv4();
        if (numericMode) {
            currentCode = StationCodeEncoder.encodeNumeric(localIp, cloudTunnelPort);
        } else {
            currentCode = StationCodeEncoder.encodeAlphanumeric(localIp, cloudTunnelPort, codeSalt);
        }

        if (tvCodeDisplay != null) {
            tvCodeDisplay.setText(currentCode);
            // Tamaño responsivo adaptativo que garantiza cero wrapping en 1 sola línea
            if (numericMode) {
                tvCodeDisplay.setTextSize(TypedValue.COMPLEX_UNIT_SP, 25f);
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
                    tvCodeDisplay.setLetterSpacing(0.08f);
                }
            } else {
                tvCodeDisplay.setTextSize(TypedValue.COMPLEX_UNIT_SP, 19.5f);
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
                    tvCodeDisplay.setLetterSpacing(0.04f);
                }
            }
        }

        updateSegmentButtonsStyle();

        if (tvLanEndpoint != null) {
            tvLanEndpoint.setText("LAN Wi-Fi: " + localIp + ":5555");
        }
        if (tvRelayEndpoint != null) {
            if (cloudTunnelPort > 0) {
                tvRelayEndpoint.setText("● Relay Mundial: bore.pub:" + cloudTunnelPort);
                tvRelayEndpoint.setTextColor(COLOR_STATUS_GREEN);
            } else {
                tvRelayEndpoint.setText("◌ Relay Mundial: Conectando a bore.pub...");
                tvRelayEndpoint.setTextColor(COLOR_STATUS_AMBER);
            }
        }
    }

    private void updateSegmentButtonsStyle() {
        if (btnSegmentHex == null || btnSegmentNumeric == null) return;
        if (!numericMode) {
            btnSegmentHex.setBackground(makeInteractiveBox(Color.parseColor("#1E2D48"), COLOR_BORDER_ACCENT, dp(6)));
            btnSegmentHex.setTextColor(COLOR_BORDER_ACCENT);
            btnSegmentNumeric.setBackground(makeInteractiveBox(Color.parseColor("#101726"), COLOR_BORDER_SUBTLE, dp(6)));
            btnSegmentNumeric.setTextColor(COLOR_TEXT_MUTED);
        } else {
            btnSegmentHex.setBackground(makeInteractiveBox(Color.parseColor("#101726"), COLOR_BORDER_SUBTLE, dp(6)));
            btnSegmentHex.setTextColor(COLOR_TEXT_MUTED);
            btnSegmentNumeric.setBackground(makeInteractiveBox(Color.parseColor("#1E2D48"), COLOR_BORDER_ACCENT, dp(6)));
            btnSegmentNumeric.setTextColor(COLOR_BORDER_ACCENT);
        }
    }

    private void refreshHardwareState() {
        if (usbManager == null) return;
        HashMap<String, UsbDevice> deviceList = usbManager.getDeviceList();
        if (deviceList == null || deviceList.isEmpty()) {
            updateHeaderStatus("ESPERANDO OTG", COLOR_TEXT_SECONDARY, COLOR_TEXT_MUTED);

            tvDeviceTitle.setText("Ningún teléfono conectado por cable");
            tvDeviceTitle.setTextColor(COLOR_TEXT_SECONDARY);
            tvDeviceSpecs.setText("Conecte el teléfono a reparar con un cable USB-C a USB-C (o adaptador OTG).");

            setStepState(1, StepStatus.WAITING, "Cable USB-C a USB-C", "No detectado en este puerto");
            setStepState(2, StepStatus.WAITING, "Permiso USB Host", "En espera de conexión");
            setStepState(3, StepStatus.WAITING, "Puente ADB OTG", "En espera de equipo");

            btnActionUsb.setText("ESCANEAR PUERTO USB-C OTG");
            btnActionUsb.setBackground(makeInteractiveBox(COLOR_BG_CARD, COLOR_BORDER_SUBTLE, dp(8)));
            btnActionUsb.setTextColor(COLOR_TEXT_PRIMARY);
            return;
        }

        UsbDevice targetDevice = deviceList.values().iterator().next();
        String name = getDeviceDisplayName(targetDevice);
        String vidPid = String.format(Locale.US, "%04X:%04X", targetDevice.getVendorId(), targetDevice.getProductId());
        String modeDesc = OtgAdbBridge.describeDeviceMode(targetDevice);
        boolean hasAdbIntf = OtgAdbBridge.findAdbInterface(targetDevice) != null;
        boolean hasPerm = usbManager.hasPermission(targetDevice);

        setStepState(1, StepStatus.SUCCESS, "Cable USB-C Conectado", name + " (" + vidPid + ")");

        if (!hasPerm) {
            updateHeaderStatus("PERMISO USB", COLOR_STATUS_AMBER, COLOR_STATUS_AMBER);

            tvDeviceTitle.setText(name);
            tvDeviceTitle.setTextColor(COLOR_STATUS_AMBER);
            tvDeviceSpecs.setText("ID USB: " + vidPid + " • " + modeDesc + "\nToque el botón inferior para autorizar el control USB OTG.");

            setStepState(2, StepStatus.ATTENTION, "Permiso USB Host Requerido", "Toque 'Autorizar USB OTG Ahora'");
            setStepState(3, StepStatus.WAITING, "Puente ADB OTG", "Esperando permiso USB del sistema");

            btnActionUsb.setText("AUTORIZAR USB OTG AHORA");
            btnActionUsb.setBackground(makeInteractiveBox(Color.parseColor("#92400E"), COLOR_STATUS_AMBER, dp(8)));
            btnActionUsb.setTextColor(Color.WHITE);
            return;
        }

        setStepState(2, StepStatus.SUCCESS, "Permiso USB Concedido", "Control USB Host activo en este teléfono");

        if (hasAdbIntf && !otgBridge.isUsbReady()) {
            otgBridge.openUsbDevice(targetDevice);
        }

        if (otgBridge.isUsbReady()) {
            if (technicianConnected) {
                updateHeaderStatus("EN SESIÓN", COLOR_BORDER_ACCENT, COLOR_BORDER_ACCENT);
                tvDeviceTitle.setText(name + " • EN SESIÓN REMOTA");
                tvDeviceTitle.setTextColor(COLOR_BORDER_ACCENT);
            } else {
                updateHeaderStatus("OTG LISTO", COLOR_STATUS_GREEN, COLOR_STATUS_GREEN);
                tvDeviceTitle.setText(name + " • LISTO");
                tvDeviceTitle.setTextColor(COLOR_STATUS_GREEN);
            }

            tvDeviceSpecs.setText("ID USB: " + vidPid + " • Interfaz ADB 0xFF/0x42/0x01 activa\nComparta el Código con el técnico. Si aparece aviso RSA en el otro móvil, toque 'Permitir'.");

            setStepState(3, StepStatus.SUCCESS, "Puente ADB OTG Activo", "Enlazado hacia bore.pub y LAN :5555");
            btnActionUsb.setText("REINICIAR PUENTE USB-C OTG");
            btnActionUsb.setBackground(makeInteractiveBox(Color.parseColor("#065F46"), COLOR_STATUS_GREEN, dp(8)));
            btnActionUsb.setTextColor(Color.WHITE);
        } else {
            updateHeaderStatus("DEPURACIÓN", COLOR_STATUS_AMBER, COLOR_STATUS_AMBER);

            tvDeviceTitle.setText(name + " • SIN DEPURACIÓN");
            tvDeviceTitle.setTextColor(COLOR_STATUS_AMBER);
            tvDeviceSpecs.setText("ID USB: " + vidPid + " • " + modeDesc + "\nEn el otro teléfono: active 'Depuración por USB' en Ajustes de Desarrollador.");

            setStepState(3, StepStatus.ATTENTION, "Sin Interfaz ADB", "Active 'Depuración por USB' en el otro teléfono");
            btnActionUsb.setText("VOLVER A VERIFICAR INTERFAZ ADB");
            btnActionUsb.setBackground(makeInteractiveBox(Color.parseColor("#92400E"), COLOR_STATUS_AMBER, dp(8)));
            btnActionUsb.setTextColor(Color.WHITE);
        }
    }

    private void updateHeaderStatus(String text, int textColor, int dotColor) {
        if (tvHeaderBadge != null) {
            tvHeaderBadge.setText(text);
            tvHeaderBadge.setTextColor(textColor);
        }
        if (statusDotView != null) {
            GradientDrawable gd = new GradientDrawable();
            gd.setShape(GradientDrawable.OVAL);
            gd.setColor(dotColor);
            statusDotView.setBackground(gd);
        }
    }

    private enum StepStatus { WAITING, ATTENTION, SUCCESS }

    private void setStepState(int stepNumber, StepStatus status, String title, String subtitle) {
        TextView badge;
        TextView tvTitle;
        TextView tvSub;
        if (stepNumber == 1) {
            badge = tvStep1Badge; tvTitle = tvStep1Title; tvSub = tvStep1Sub;
        } else if (stepNumber == 2) {
            badge = tvStep2Badge; tvTitle = tvStep2Title; tvSub = tvStep2Sub;
        } else {
            badge = tvStep3Badge; tvTitle = tvStep3Title; tvSub = tvStep3Sub;
        }
        if (badge == null || tvTitle == null || tvSub == null) return;

        tvTitle.setText(title);
        tvSub.setText(subtitle);

        switch (status) {
            case SUCCESS:
                badge.setText("✓");
                badge.setTextColor(Color.WHITE);
                badge.setBackground(makeCircle(COLOR_STATUS_GREEN));
                tvTitle.setTextColor(COLOR_TEXT_PRIMARY);
                tvSub.setTextColor(COLOR_STATUS_GREEN);
                break;
            case ATTENTION:
                badge.setText("!");
                badge.setTextColor(Color.WHITE);
                badge.setBackground(makeCircle(COLOR_STATUS_AMBER));
                tvTitle.setTextColor(COLOR_STATUS_AMBER);
                tvSub.setTextColor(COLOR_STATUS_AMBER);
                break;
            case WAITING:
            default:
                badge.setText(String.valueOf(stepNumber));
                badge.setTextColor(COLOR_TEXT_MUTED);
                badge.setBackground(makeCircle(Color.parseColor("#1A2333")));
                tvTitle.setTextColor(COLOR_TEXT_SECONDARY);
                tvSub.setTextColor(COLOR_TEXT_MUTED);
                break;
        }
    }

    private void requestOrReconnectUsb() {
        if (usbManager == null) return;
        HashMap<String, UsbDevice> deviceList = usbManager.getDeviceList();
        if (deviceList == null || deviceList.isEmpty()) {
            Toast.makeText(this, "No se detecta ningún teléfono en el cable USB-C.", Toast.LENGTH_SHORT).show();
            refreshHardwareState();
            return;
        }
        UsbDevice device = deviceList.values().iterator().next();
        if (!usbManager.hasPermission(device)) {
            int flags = 0;
            if (Build.VERSION.SDK_INT >= 31) {
                flags = 33554432; // PendingIntent.FLAG_MUTABLE requerido en Android 12+
            }
            Intent permIntent = new Intent(ACTION_USB_PERMISSION);
            permIntent.setPackage(getPackageName());
            PendingIntent pi = PendingIntent.getBroadcast(this, 0, permIntent, flags);
            usbManager.requestPermission(device, pi);
        } else {
            otgBridge.openUsbDevice(device);
            refreshHardwareState();
        }
    }

    private static String getDeviceDisplayName(UsbDevice device) {
        if (device == null) return "Dispositivo USB";
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
            String mfg = device.getManufacturerName();
            String prod = device.getProductName();
            if (mfg != null && prod != null) {
                if (prod.toLowerCase(Locale.US).startsWith(mfg.toLowerCase(Locale.US))) {
                    return prod.trim();
                }
                return (mfg.trim() + " " + prod.trim()).trim();
            }
            if (prod != null && !prod.trim().isEmpty()) {
                return prod.trim();
            }
        }
        return String.format(Locale.US, "Android USB (%04X:%04X)", device.getVendorId(), device.getProductId());
    }

    private void shareStationCode() {
        String msg = "Código de Estación ADBHUB-LINK:\n"
                + currentCode + "\n\n"
                + (cloudTunnelPort > 0 ? "Relay Mundial: bore.pub:" + cloudTunnelPort + "\n" : "")
                + "LAN Wi-Fi: " + localIp + ":5555\n"
                + "Ingrese este código en ADBHUB Desktop para conectar.";

        Intent sendIntent = new Intent();
        sendIntent.setAction(Intent.ACTION_SEND);
        sendIntent.putExtra(Intent.EXTRA_TEXT, msg);
        sendIntent.setType("text/plain");

        Intent shareIntent = Intent.createChooser(sendIntent, "Compartir Código ADBHUB-LINK");
        startActivity(shareIntent);
    }

    @Override
    public void onTunnelPortAssigned(final int publicPort) {
        uiHandler.post(new Runnable() {
            @Override
            public void run() {
                cloudTunnelPort = publicPort;
                updateStationCode();
                onLog("TUNEL", "Relay Mundial activo en bore.pub:" + publicPort + " -> Código: " + currentCode);
            }
        });
    }

    @Override
    public void onClientSessionStateChanged(final boolean connected, final String remotePeer) {
        uiHandler.post(new Runnable() {
            @Override
            public void run() {
                technicianConnected = connected;
                refreshHardwareState();
            }
        });
    }

    @Override
    public void onLog(final String tag, final String message) {
        uiHandler.post(new Runnable() {
            @Override
            public void run() {
                String ts = new SimpleDateFormat("HH:mm:ss", Locale.US).format(new Date());
                String line = "[" + ts + "] [" + tag + "] " + message;
                logLines.add(0, line);
                if (logLines.size() > 50) {
                    logLines.remove(logLines.size() - 1);
                }
                if (tvLogsBadgeCount != null) {
                    tvLogsBadgeCount.setText(String.valueOf(logLines.size()));
                }
                if (tvLogsBox != null) {
                    StringBuilder sb = new StringBuilder();
                    for (int i = 0; i < logLines.size(); i++) {
                        if (i > 0) sb.append('\n');
                        sb.append(logLines.get(i));
                    }
                    tvLogsBox.setText(sb.toString());
                }
            }
        });
    }

    // ========================================================================
    // CONSTRUCCIÓN PROGRAMÁTICA DE LA INTERFAZ MATERIAL 3 DARK SLATE
    // ========================================================================
    private View buildEnterpriseUi() {
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(COLOR_BG_CANVAS);

        // 1. BARRA SUPERIOR (HEADER)
        LinearLayout header = new LinearLayout(this);
        header.setOrientation(LinearLayout.HORIZONTAL);
        header.setGravity(Gravity.CENTER_VERTICAL);
        header.setBackgroundColor(COLOR_BG_HEADER);
        header.setPadding(dp(16), dp(12), dp(16), dp(12));

        // Logo oficial ADBHUB-LINK
        ImageView ivLogo = new ImageView(this);
        ivLogo.setImageResource(R.drawable.ic_app_logo);
        ivLogo.setScaleType(ImageView.ScaleType.FIT_CENTER);
        LinearLayout.LayoutParams logoLp = new LinearLayout.LayoutParams(dp(32), dp(32));
        logoLp.rightMargin = dp(8);
        ivLogo.setLayoutParams(logoLp);
        header.addView(ivLogo);

        LinearLayout brandCol = new LinearLayout(this);
        brandCol.setOrientation(LinearLayout.VERTICAL);
        LinearLayout.LayoutParams brandLp = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f);
        brandCol.setLayoutParams(brandLp);

        TextView tvTitle = new TextView(this);
        tvTitle.setText("ADBHUB-LINK");
        tvTitle.setTextColor(Color.WHITE);
        tvTitle.setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f);
        tvTitle.setMaxLines(1);
        tvTitle.setSingleLine(true);
        tvTitle.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        brandCol.addView(tvTitle);

        TextView tvSub = new TextView(this);
        tvSub.setText("CLIENTE ANDROID OTG");
        tvSub.setTextColor(COLOR_BORDER_ACCENT);
        tvSub.setTextSize(TypedValue.COMPLEX_UNIT_SP, 9.5f);
        tvSub.setMaxLines(1);
        tvSub.setSingleLine(true);
        tvSub.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        brandCol.addView(tvSub);

        header.addView(brandCol);

        // Badge de estado con indicador LED pulsante
        LinearLayout badgeBox = new LinearLayout(this);
        badgeBox.setOrientation(LinearLayout.HORIZONTAL);
        badgeBox.setGravity(Gravity.CENTER_VERTICAL);
        badgeBox.setPadding(dp(8), dp(4), dp(8), dp(4));
        badgeBox.setBackground(makeRoundedBox(COLOR_BG_RECESSED, COLOR_BORDER_SUBTLE, dp(14)));

        statusDotView = new View(this);
        LinearLayout.LayoutParams dotLp = new LinearLayout.LayoutParams(dp(7), dp(7));
        dotLp.rightMargin = dp(5);
        statusDotView.setLayoutParams(dotLp);
        statusDotView.setBackground(makeCircle(COLOR_TEXT_MUTED));
        badgeBox.addView(statusDotView);

        tvHeaderBadge = new TextView(this);
        tvHeaderBadge.setText("ESPERANDO OTG");
        tvHeaderBadge.setTextColor(COLOR_TEXT_SECONDARY);
        tvHeaderBadge.setTextSize(TypedValue.COMPLEX_UNIT_SP, 10.5f);
        tvHeaderBadge.setTypeface(Typeface.create("monospace", Typeface.BOLD));
        badgeBox.addView(tvHeaderBadge);

        header.addView(badgeBox);
        root.addView(header);

        View divider = new View(this);
        divider.setBackgroundColor(COLOR_BORDER_SUBTLE);
        root.addView(divider, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1)));

        // 2. CONTENEDOR SCROLLABLE PRINCIPAL
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        LinearLayout.LayoutParams scrollLp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f);
        scroll.setLayoutParams(scrollLp);

        LinearLayout content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setPadding(dp(14), dp(12), dp(14), dp(24));

        // ====================================================================
        // TARJETA 1: HERO CODE & CREDENCIALES DE CONEXIÓN
        // ====================================================================
        LinearLayout cardCreds = makeCardLayout();

        LinearLayout credsHeader = new LinearLayout(this);
        credsHeader.setOrientation(LinearLayout.HORIZONTAL);
        credsHeader.setGravity(Gravity.CENTER_VERTICAL);

        TextView tvCredsTitle = makeSectionLabel("CÓDIGO DE ESTACIÓN REMOTA");
        LinearLayout.LayoutParams ctLp = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f);
        tvCredsTitle.setLayoutParams(ctLp);
        credsHeader.addView(tvCredsTitle);

        // Selector segmentado de formato [ 16-HEX ] | [ 9 DÍGITOS ]
        LinearLayout segmentLayout = new LinearLayout(this);
        segmentLayout.setOrientation(LinearLayout.HORIZONTAL);
        segmentLayout.setPadding(dp(2), dp(2), dp(2), dp(2));
        segmentLayout.setBackground(makeRoundedBox(COLOR_BG_RECESSED, COLOR_BORDER_SUBTLE, dp(8)));

        btnSegmentHex = new Button(this);
        btnSegmentHex.setText("16-HEX");
        btnSegmentHex.setAllCaps(false);
        btnSegmentHex.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11f);
        btnSegmentHex.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        btnSegmentHex.setMinHeight(dp(30));
        btnSegmentHex.setMinimumHeight(dp(30));
        btnSegmentHex.setPadding(dp(8), dp(4), dp(8), dp(4));
        btnSegmentHex.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                if (numericMode) {
                    numericMode = false;
                    updateStationCode();
                }
            }
        });
        segmentLayout.addView(btnSegmentHex);

        btnSegmentNumeric = new Button(this);
        btnSegmentNumeric.setText("9 DÍGITOS");
        btnSegmentNumeric.setAllCaps(false);
        btnSegmentNumeric.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11f);
        btnSegmentNumeric.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        btnSegmentNumeric.setMinHeight(dp(30));
        btnSegmentNumeric.setMinimumHeight(dp(30));
        btnSegmentNumeric.setPadding(dp(8), dp(4), dp(8), dp(4));
        btnSegmentNumeric.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                if (!numericMode) {
                    numericMode = true;
                    updateStationCode();
                }
            }
        });
        segmentLayout.addView(btnSegmentNumeric);
        credsHeader.addView(segmentLayout);

        cardCreds.addView(credsHeader);

        TextView tvCredHint = new TextView(this);
        tvCredHint.setText("Comparta este código con su técnico para conectar sin configurar router ni VPN.");
        tvCredHint.setTextColor(COLOR_TEXT_SECONDARY);
        tvCredHint.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f);
        tvCredHint.setPadding(0, dp(4), 0, dp(10));
        cardCreds.addView(tvCredHint);

        // Caja interactiva del Código (Tap to Copy)
        LinearLayout codeBox = new LinearLayout(this);
        codeBox.setOrientation(LinearLayout.VERTICAL);
        codeBox.setGravity(Gravity.CENTER);
        codeBox.setPadding(dp(12), dp(14), dp(12), dp(14));
        codeBox.setBackground(makeInteractiveBox(COLOR_BG_RECESSED, Color.parseColor("#263854"), dp(10)));
        codeBox.setClickable(true);
        codeBox.setFocusable(true);
        codeBox.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                triggerCodeCopyAction();
            }
        });

        tvCodeDisplay = new TextView(this);
        tvCodeDisplay.setText(currentCode);
        tvCodeDisplay.setTextColor(COLOR_BORDER_ACCENT);
        tvCodeDisplay.setTextSize(TypedValue.COMPLEX_UNIT_SP, 19.5f);
        tvCodeDisplay.setTypeface(Typeface.create("monospace", Typeface.BOLD));
        tvCodeDisplay.setSingleLine(true);
        tvCodeDisplay.setMaxLines(1);
        tvCodeDisplay.setGravity(Gravity.CENTER);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            tvCodeDisplay.setAutoSizeTextTypeUniformWithConfiguration(14, 25, 1, TypedValue.COMPLEX_UNIT_SP);
        }
        codeBox.addView(tvCodeDisplay);

        tvCopyHint = new TextView(this);
        tvCopyHint.setText("Toque el recuadro para copiar código");
        tvCopyHint.setTextColor(COLOR_TEXT_MUTED);
        tvCopyHint.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11f);
        tvCopyHint.setPadding(0, dp(3), 0, dp(6));
        codeBox.addView(tvCopyHint);

        View miniLine = new View(this);
        miniLine.setBackgroundColor(COLOR_BORDER_SUBTLE);
        codeBox.addView(miniLine, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1)));

        LinearLayout endpointsRow = new LinearLayout(this);
        endpointsRow.setOrientation(LinearLayout.VERTICAL);
        endpointsRow.setPadding(0, dp(6), 0, 0);

        tvRelayEndpoint = new TextView(this);
        tvRelayEndpoint.setText("◌ Relay Mundial: Conectando a bore.pub...");
        tvRelayEndpoint.setTextColor(COLOR_STATUS_AMBER);
        tvRelayEndpoint.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12.5f);
        tvRelayEndpoint.setTypeface(Typeface.create("monospace", Typeface.BOLD));
        tvRelayEndpoint.setGravity(Gravity.CENTER);
        endpointsRow.addView(tvRelayEndpoint);

        tvLanEndpoint = new TextView(this);
        tvLanEndpoint.setText("LAN Wi-Fi: " + localIp + ":5555");
        tvLanEndpoint.setTextColor(COLOR_TEXT_SECONDARY);
        tvLanEndpoint.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f);
        tvLanEndpoint.setTypeface(Typeface.create("monospace", Typeface.NORMAL));
        tvLanEndpoint.setGravity(Gravity.CENTER);
        endpointsRow.addView(tvLanEndpoint);

        codeBox.addView(endpointsRow);
        cardCreds.addView(codeBox);

        // Fila de acciones principales: [ COPIAR CÓDIGO ] [ COMPARTIR ] [ REFRESCAR ID ]
        LinearLayout rowActions = new LinearLayout(this);
        rowActions.setOrientation(LinearLayout.HORIZONTAL);
        LinearLayout.LayoutParams raLp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        raLp.topMargin = dp(10);
        rowActions.setLayoutParams(raLp);

        Button btnCopy = makeStyledButton("COPIAR", COLOR_ACCENT_DARK, COLOR_ACCENT_BLUE);
        LinearLayout.LayoutParams bCopyLp = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1.2f);
        bCopyLp.rightMargin = dp(6);
        btnCopy.setLayoutParams(bCopyLp);
        btnCopy.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                triggerCodeCopyAction();
            }
        });
        rowActions.addView(btnCopy);

        Button btnShare = makeStyledButton("COMPARTIR", Color.parseColor("#1E2D48"), COLOR_BORDER_ACCENT);
        LinearLayout.LayoutParams bShareLp = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1.2f);
        bShareLp.rightMargin = dp(6);
        btnShare.setLayoutParams(bShareLp);
        btnShare.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                shareStationCode();
            }
        });
        rowActions.addView(btnShare);

        Button btnNewId = makeStyledButton("NUEVO", Color.parseColor("#151D2A"), COLOR_BORDER_SUBTLE);
        LinearLayout.LayoutParams bNewLp = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 0.8f);
        btnNewId.setLayoutParams(bNewLp);
        btnNewId.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                codeSalt = (codeSalt + 17) & 0xFF;
                if (codeSalt == 0) codeSalt = 0x24;
                updateStationCode();
                onLog("SEGURIDAD", "Nuevo salt generado. Código actual: " + currentCode);
            }
        });
        rowActions.addView(btnNewId);

        cardCreds.addView(rowActions);
        content.addView(cardCreds);

        // ====================================================================
        // TARJETA 2: ESTADO DEL CABLE USB-C OTG & VISUAL STEPPER
        // ====================================================================
        LinearLayout cardDevice = makeCardLayout();
        cardDevice.addView(makeSectionLabel("PUENTE CABLE USB-C A USB-C OTG"));

        tvDeviceTitle = new TextView(this);
        tvDeviceTitle.setText("Escaneando puerto USB-C OTG...");
        tvDeviceTitle.setTextColor(Color.WHITE);
        tvDeviceTitle.setTextSize(TypedValue.COMPLEX_UNIT_SP, 17f);
        tvDeviceTitle.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        tvDeviceTitle.setPadding(0, dp(6), 0, dp(2));
        cardDevice.addView(tvDeviceTitle);

        tvDeviceSpecs = new TextView(this);
        tvDeviceSpecs.setText("Conecte el teléfono a reparar usando un cable USB-C a USB-C.");
        tvDeviceSpecs.setTextColor(COLOR_TEXT_SECONDARY);
        tvDeviceSpecs.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f);
        tvDeviceSpecs.setPadding(0, 0, 0, dp(10));
        cardDevice.addView(tvDeviceSpecs);

        // STEPPER VISUAL CON INDICADORES
        LinearLayout stepperLayout = new LinearLayout(this);
        stepperLayout.setOrientation(LinearLayout.VERTICAL);
        stepperLayout.setPadding(dp(10), dp(8), dp(10), dp(8));
        stepperLayout.setBackground(makeRoundedBox(COLOR_BG_RECESSED, COLOR_BORDER_SUBTLE, dp(8)));

        // Paso 1
        LinearLayout rowS1 = new LinearLayout(this);
        rowS1.setOrientation(LinearLayout.HORIZONTAL);
        rowS1.setGravity(Gravity.CENTER_VERTICAL);
        rowS1.setPadding(0, dp(4), 0, dp(4));

        tvStep1Badge = makeCircleBadge("1");
        rowS1.addView(tvStep1Badge);

        LinearLayout textS1 = new LinearLayout(this);
        textS1.setOrientation(LinearLayout.VERTICAL);
        textS1.setPadding(dp(10), 0, 0, 0);
        tvStep1Title = new TextView(this);
        tvStep1Title.setText("Cable USB-C a USB-C");
        tvStep1Title.setTextColor(COLOR_TEXT_SECONDARY);
        tvStep1Title.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13.5f);
        tvStep1Title.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        textS1.addView(tvStep1Title);

        tvStep1Sub = new TextView(this);
        tvStep1Sub.setText("Buscando conexión física...");
        tvStep1Sub.setTextColor(COLOR_TEXT_MUTED);
        tvStep1Sub.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11.5f);
        textS1.addView(tvStep1Sub);
        rowS1.addView(textS1);
        stepperLayout.addView(rowS1);

        stepperLayout.addView(makeStepperConnector());

        // Paso 2
        LinearLayout rowS2 = new LinearLayout(this);
        rowS2.setOrientation(LinearLayout.HORIZONTAL);
        rowS2.setGravity(Gravity.CENTER_VERTICAL);
        rowS2.setPadding(0, dp(4), 0, dp(4));

        tvStep2Badge = makeCircleBadge("2");
        rowS2.addView(tvStep2Badge);

        LinearLayout textS2 = new LinearLayout(this);
        textS2.setOrientation(LinearLayout.VERTICAL);
        textS2.setPadding(dp(10), 0, 0, 0);
        tvStep2Title = new TextView(this);
        tvStep2Title.setText("Permiso USB Host");
        tvStep2Title.setTextColor(COLOR_TEXT_SECONDARY);
        tvStep2Title.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13.5f);
        tvStep2Title.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        textS2.addView(tvStep2Title);

        tvStep2Sub = new TextView(this);
        tvStep2Sub.setText("En espera");
        tvStep2Sub.setTextColor(COLOR_TEXT_MUTED);
        tvStep2Sub.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11.5f);
        textS2.addView(tvStep2Sub);
        rowS2.addView(textS2);
        stepperLayout.addView(rowS2);

        stepperLayout.addView(makeStepperConnector());

        // Paso 3
        LinearLayout rowS3 = new LinearLayout(this);
        rowS3.setOrientation(LinearLayout.HORIZONTAL);
        rowS3.setGravity(Gravity.CENTER_VERTICAL);
        rowS3.setPadding(0, dp(4), 0, dp(4));

        tvStep3Badge = makeCircleBadge("3");
        rowS3.addView(tvStep3Badge);

        LinearLayout textS3 = new LinearLayout(this);
        textS3.setOrientation(LinearLayout.VERTICAL);
        textS3.setPadding(dp(10), 0, 0, 0);
        tvStep3Title = new TextView(this);
        tvStep3Title.setText("Puente ADB OTG");
        tvStep3Title.setTextColor(COLOR_TEXT_SECONDARY);
        tvStep3Title.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13.5f);
        tvStep3Title.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        textS3.addView(tvStep3Title);

        tvStep3Sub = new TextView(this);
        tvStep3Sub.setText("En espera de equipo");
        tvStep3Sub.setTextColor(COLOR_TEXT_MUTED);
        tvStep3Sub.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11.5f);
        textS3.addView(tvStep3Sub);
        rowS3.addView(textS3);
        stepperLayout.addView(rowS3);

        cardDevice.addView(stepperLayout);

        btnActionUsb = makeStyledButton("AUTORIZAR / ESCANEAR CABLE USB-C OTG", COLOR_ACCENT_DARK, COLOR_ACCENT_BLUE);
        LinearLayout.LayoutParams actLp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        actLp.topMargin = dp(12);
        btnActionUsb.setLayoutParams(actLp);
        btnActionUsb.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                requestOrReconnectUsb();
            }
        });
        cardDevice.addView(btnActionUsb);
        content.addView(cardDevice);

        // ====================================================================
        // SECCIÓN ACORDEÓN 1: GUÍA RÁPIDA DE CONEXIÓN
        // ====================================================================
        LinearLayout cardGuide = makeCardLayout();

        LinearLayout guideHeader = new LinearLayout(this);
        guideHeader.setOrientation(LinearLayout.HORIZONTAL);
        guideHeader.setGravity(Gravity.CENTER_VERTICAL);
        guideHeader.setPadding(0, dp(2), 0, dp(2));
        guideHeader.setClickable(true);
        guideHeader.setFocusable(true);

        TextView tvGuideLabel = makeSectionLabel("GUÍA RÁPIDA DE CONEXIÓN");
        LinearLayout.LayoutParams glLp = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f);
        tvGuideLabel.setLayoutParams(glLp);
        guideHeader.addView(tvGuideLabel);

        tvGuideToggle = new TextView(this);
        tvGuideToggle.setText("▼ VER PASOS");
        tvGuideToggle.setTextColor(COLOR_BORDER_ACCENT);
        tvGuideToggle.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11.5f);
        tvGuideToggle.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        guideHeader.addView(tvGuideToggle);
        cardGuide.addView(guideHeader);

        guideContent = new LinearLayout(this);
        ((LinearLayout) guideContent).setOrientation(LinearLayout.VERTICAL);
        guideContent.setPadding(0, dp(8), 0, 0);
        guideContent.setVisibility(View.GONE); // Colapsada por defecto para máxima limpieza visual

        TextView tvGuideText = new TextView(this);
        tvGuideText.setText(
                "1. Conecte un cable USB-C a USB-C entre este teléfono y el teléfono a reparar.\n\n" +
                "2. Si no detecta conexión, baje la barra de notificaciones de este teléfono, pulse 'Sistema Android • USB' y elija 'Controlado por: Este dispositivo'.\n\n" +
                "3. Acepte la ventana emergente '¿Permitir que ADBHUB-LINK OTG acceda al dispositivo USB?'.\n\n" +
                "4. En el teléfono a reparar: active 'Depuración por USB' en Opciones de Desarrollador y acepte la huella RSA cuando el técnico se conecte."
        );
        tvGuideText.setTextColor(COLOR_TEXT_PRIMARY);
        tvGuideText.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.JELLY_BEAN) {
            tvGuideText.setLineSpacing(dp(3), 1.15f);
        }
        ((LinearLayout) guideContent).addView(tvGuideText);
        cardGuide.addView(guideContent);

        guideHeader.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                if (guideContent.getVisibility() == View.VISIBLE) {
                    guideContent.setVisibility(View.GONE);
                    tvGuideToggle.setText("▼ VER PASOS");
                } else {
                    guideContent.setVisibility(View.VISIBLE);
                    tvGuideToggle.setText("▲ OCULTAR");
                }
            }
        });

        content.addView(cardGuide);

        // ====================================================================
        // SECCIÓN ACORDEÓN 2: BITÁCORA DE AUDITORÍA OTG & RED
        // ====================================================================
        LinearLayout cardLogs = makeCardLayout();

        LinearLayout logsHeader = new LinearLayout(this);
        logsHeader.setOrientation(LinearLayout.HORIZONTAL);
        logsHeader.setGravity(Gravity.CENTER_VERTICAL);
        logsHeader.setPadding(0, dp(2), 0, dp(2));
        logsHeader.setClickable(true);
        logsHeader.setFocusable(true);

        TextView tvLogsLabel = makeSectionLabel("BITÁCORA DE AUDITORÍA");
        logsHeader.addView(tvLogsLabel);

        tvLogsBadgeCount = new TextView(this);
        tvLogsBadgeCount.setText("0");
        tvLogsBadgeCount.setTextColor(COLOR_BORDER_ACCENT);
        tvLogsBadgeCount.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11f);
        tvLogsBadgeCount.setTypeface(Typeface.create("monospace", Typeface.BOLD));
        tvLogsBadgeCount.setPadding(dp(6), dp(2), dp(6), dp(2));
        tvLogsBadgeCount.setBackground(makeRoundedBox(COLOR_BG_RECESSED, COLOR_BORDER_SUBTLE, dp(10)));
        LinearLayout.LayoutParams lbcLp = new LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        lbcLp.leftMargin = dp(8);
        tvLogsBadgeCount.setLayoutParams(lbcLp);
        logsHeader.addView(tvLogsBadgeCount);

        View emptySpacer = new View(this);
        LinearLayout.LayoutParams esLp = new LinearLayout.LayoutParams(0, 1, 1f);
        emptySpacer.setLayoutParams(esLp);
        logsHeader.addView(emptySpacer);

        tvLogsToggle = new TextView(this);
        tvLogsToggle.setText("▼ VER REGISTROS");
        tvLogsToggle.setTextColor(COLOR_BORDER_ACCENT);
        tvLogsToggle.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11.5f);
        tvLogsToggle.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        logsHeader.addView(tvLogsToggle);
        cardLogs.addView(logsHeader);

        logsContent = new LinearLayout(this);
        ((LinearLayout) logsContent).setOrientation(LinearLayout.VERTICAL);
        logsContent.setPadding(0, dp(8), 0, 0);
        logsContent.setVisibility(View.GONE); // Colapsada por defecto

        LinearLayout logsActionRow = new LinearLayout(this);
        logsActionRow.setOrientation(LinearLayout.HORIZONTAL);
        logsActionRow.setGravity(Gravity.END);

        Button btnCopyLogs = makeStyledButton("COPIAR LOGS", Color.parseColor("#151D2A"), COLOR_BORDER_SUBTLE);
        btnCopyLogs.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11f);
        btnCopyLogs.setMinHeight(dp(32));
        btnCopyLogs.setMinimumHeight(dp(32));
        btnCopyLogs.setPadding(dp(10), dp(4), dp(10), dp(4));
        btnCopyLogs.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                if (tvLogsBox != null) {
                    copyToClipboard("Logs ADBHUB-LINK", tvLogsBox.getText().toString());
                }
            }
        });
        logsActionRow.addView(btnCopyLogs);

        Button btnClearLogs = makeStyledButton("LIMPIAR", Color.parseColor("#151D2A"), COLOR_BORDER_SUBTLE);
        btnClearLogs.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11f);
        btnClearLogs.setMinHeight(dp(32));
        btnClearLogs.setMinimumHeight(dp(32));
        btnClearLogs.setPadding(dp(10), dp(4), dp(10), dp(4));
        LinearLayout.LayoutParams clLp = new LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        clLp.leftMargin = dp(6);
        btnClearLogs.setLayoutParams(clLp);
        btnClearLogs.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                logLines.clear();
                if (tvLogsBox != null) tvLogsBox.setText("");
                if (tvLogsBadgeCount != null) tvLogsBadgeCount.setText("0");
            }
        });
        logsActionRow.addView(btnClearLogs);
        ((LinearLayout) logsContent).addView(logsActionRow);

        tvLogsBox = new TextView(this);
        tvLogsBox.setTextColor(COLOR_TEXT_SECONDARY);
        tvLogsBox.setTextSize(TypedValue.COMPLEX_UNIT_SP, 11.5f);
        tvLogsBox.setTypeface(Typeface.create("monospace", Typeface.NORMAL));
        tvLogsBox.setPadding(dp(10), dp(8), dp(10), dp(8));
        tvLogsBox.setBackground(makeRoundedBox(COLOR_BG_RECESSED, COLOR_BORDER_SUBTLE, dp(6)));
        LinearLayout.LayoutParams logLp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(180));
        logLp.topMargin = dp(6);
        tvLogsBox.setLayoutParams(logLp);
        ((LinearLayout) logsContent).addView(tvLogsBox);

        cardLogs.addView(logsContent);

        logsHeader.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                if (logsContent.getVisibility() == View.VISIBLE) {
                    logsContent.setVisibility(View.GONE);
                    tvLogsToggle.setText("▼ VER REGISTROS");
                } else {
                    logsContent.setVisibility(View.VISIBLE);
                    tvLogsToggle.setText("▲ OCULTAR");
                }
            }
        });

        content.addView(cardLogs);

        scroll.addView(content);
        root.addView(scroll);

        return root;
    }

    private void triggerCodeCopyAction() {
        if (tvCodeDisplay != null) {
            tvCodeDisplay.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP);
        }
        copyToClipboard("Código ADBHUB-LINK", currentCode);
        if (tvCopyHint != null) {
            tvCopyHint.setText("✓ ¡Copiado al portapapeles!");
            tvCopyHint.setTextColor(COLOR_STATUS_GREEN);
            uiHandler.postDelayed(new Runnable() {
                @Override
                public void run() {
                    if (tvCopyHint != null) {
                        tvCopyHint.setText("Toque el recuadro para copiar código");
                        tvCopyHint.setTextColor(COLOR_TEXT_MUTED);
                    }
                }
            }, 2500);
        }
    }

    // ========================================================================
    // UTILIDADES DE DISEÑO PROGRAMÁTICO
    // ========================================================================
    private LinearLayout makeCardLayout() {
        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(dp(14), dp(14), dp(14), dp(14));
        card.setBackground(makeRoundedBox(COLOR_BG_CARD, COLOR_BORDER_SUBTLE, dp(12)));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        lp.bottomMargin = dp(12);
        card.setLayoutParams(lp);
        return card;
    }

    private TextView makeSectionLabel(String text) {
        TextView tv = new TextView(this);
        tv.setText(text);
        tv.setTextColor(COLOR_BORDER_ACCENT);
        tv.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f);
        tv.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        return tv;
    }

    private TextView makeCircleBadge(String number) {
        TextView tv = new TextView(this);
        tv.setText(number);
        tv.setTextColor(COLOR_TEXT_MUTED);
        tv.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f);
        tv.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        tv.setGravity(Gravity.CENTER);
        int size = dp(24);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(size, size);
        tv.setLayoutParams(lp);
        tv.setBackground(makeCircle(Color.parseColor("#1A2333")));
        return tv;
    }

    private View makeStepperConnector() {
        View v = new View(this);
        v.setBackgroundColor(COLOR_BORDER_SUBTLE);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(dp(2), dp(12));
        lp.leftMargin = dp(11);
        v.setLayoutParams(lp);
        return v;
    }

    private Button makeStyledButton(String text, int fillColor, int strokeColor) {
        Button btn = new Button(this);
        btn.setText(text);
        btn.setAllCaps(false);
        btn.setTextColor(Color.WHITE);
        btn.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f);
        btn.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        btn.setMinHeight(dp(44));
        btn.setPadding(dp(12), dp(8), dp(12), dp(8));
        btn.setBackground(makeInteractiveBox(fillColor, strokeColor, dp(8)));
        return btn;
    }

    private static Drawable makeInteractiveBox(int fillColor, int strokeColor, int radiusPx) {
        GradientDrawable normal = makeRoundedBox(fillColor, strokeColor, radiusPx);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
            int rippleColor = Color.argb(45, 255, 255, 255);
            ColorStateList csl = ColorStateList.valueOf(rippleColor);
            GradientDrawable mask = makeRoundedBox(Color.WHITE, 0, radiusPx);
            return new RippleDrawable(csl, normal, mask);
        }
        return normal;
    }

    private static GradientDrawable makeRoundedBox(int fillColor, int strokeColor, int radiusPx) {
        GradientDrawable gd = new GradientDrawable();
        gd.setShape(GradientDrawable.RECTANGLE);
        gd.setColor(fillColor);
        gd.setCornerRadius(radiusPx);
        if (strokeColor != 0) {
            gd.setStroke(dpToPx(1.5f), strokeColor);
        }
        return gd;
    }

    private static GradientDrawable makeCircle(int fillColor) {
        GradientDrawable gd = new GradientDrawable();
        gd.setShape(GradientDrawable.OVAL);
        gd.setColor(fillColor);
        return gd;
    }

    private static int dpToPx(float dp) {
        return Math.max(1, (int) (dp * 2f)); // Aproximación segura para stroke
    }

    private void copyToClipboard(String label, String text) {
        ClipboardManager cm = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
        if (cm != null) {
            cm.setPrimaryClip(ClipData.newPlainText(label, text));
            Toast.makeText(this, "Copiado: " + text, Toast.LENGTH_SHORT).show();
        }
    }

    private int dp(float value) {
        return (int) TypedValue.applyDimension(
                TypedValue.COMPLEX_UNIT_DIP, value, getResources().getDisplayMetrics());
    }
}
