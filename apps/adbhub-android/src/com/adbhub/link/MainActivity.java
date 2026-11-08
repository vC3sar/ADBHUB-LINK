package com.adbhub.link;

import android.app.Activity;
import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.hardware.usb.UsbDevice;
import android.hardware.usb.UsbManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowManager;
import android.widget.Button;
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
 * Diseñada con estética Enterprise Dark Slate, completamente responsiva con ScrollView,
 * detección automática de cable USB-C a USB-C y túnel mundial bore.pub.
 */
public class MainActivity extends Activity implements OtgAdbBridge.BridgeLogger, BoreTunnelClient.TunnelListener {

    private static final String ACTION_USB_PERMISSION = "com.adbhub.link.USB_PERMISSION";

    // Paleta Enterprise Dark Slate
    private static final int COLOR_BG_CANVAS = Color.parseColor("#090D16");
    private static final int COLOR_BG_HEADER = Color.parseColor("#0D1320");
    private static final int COLOR_BG_CARD = Color.parseColor("#111827");
    private static final int COLOR_BG_RECESSED = Color.parseColor("#070A11");
    private static final int COLOR_BORDER_SUBTLE = Color.parseColor("#1E293B");
    private static final int COLOR_BORDER_ACCENT = Color.parseColor("#38BDF8");
    private static final int COLOR_ACCENT_BLUE = Color.parseColor("#0EA5E9");
    private static final int COLOR_ACCENT_DARK = Color.parseColor("#0369A1");
    private static final int COLOR_STATUS_GREEN = Color.parseColor("#10B981");
    private static final int COLOR_STATUS_AMBER = Color.parseColor("#F59E0B");
    private static final int COLOR_TEXT_PRIMARY = Color.parseColor("#F1F5F9");
    private static final int COLOR_TEXT_SECONDARY = Color.parseColor("#94A3B8");
    private static final int COLOR_TEXT_MUTED = Color.parseColor("#64748B");

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

    // Vistas de UI
    private TextView tvHeaderBadge;
    private TextView tvCodeDisplay;
    private TextView tvRelayEndpoint;
    private TextView tvLanEndpoint;
    private TextView tvDeviceTitle;
    private TextView tvDeviceSpecs;
    private TextView tvStep1;
    private TextView tvStep2;
    private TextView tvStep3;
    private Button btnActionUsb;
    private TextView tvLogsBox;

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

        updateStationCode();
        setContentView(buildEnterpriseUi());

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

    private void updateStationCode() {
        localIp = StationCodeEncoder.detectLocalIpv4();
        if (numericMode) {
            currentCode = StationCodeEncoder.encodeNumeric(localIp, cloudTunnelPort);
        } else {
            currentCode = StationCodeEncoder.encodeAlphanumeric(localIp, cloudTunnelPort, codeSalt);
        }
        if (tvCodeDisplay != null) {
            tvCodeDisplay.setText(currentCode);
        }
        if (tvLanEndpoint != null) {
            tvLanEndpoint.setText("LAN Wi-Fi: " + localIp + ":5555");
        }
        if (tvRelayEndpoint != null) {
            if (cloudTunnelPort > 0) {
                tvRelayEndpoint.setText("Relay Mundial: bore.pub:" + cloudTunnelPort);
                tvRelayEndpoint.setTextColor(COLOR_STATUS_GREEN);
            } else {
                tvRelayEndpoint.setText("Relay Mundial: Conectando a bore.pub...");
                tvRelayEndpoint.setTextColor(COLOR_STATUS_AMBER);
            }
        }
    }

    private void refreshHardwareState() {
        if (usbManager == null) return;
        HashMap<String, UsbDevice> deviceList = usbManager.getDeviceList();
        if (deviceList == null || deviceList.isEmpty()) {
            tvHeaderBadge.setText("ESPERANDO CABLE OTG");
            tvHeaderBadge.setTextColor(COLOR_TEXT_SECONDARY);

            tvDeviceTitle.setText("Ningún teléfono detectado por cable USB-C");
            tvDeviceTitle.setTextColor(COLOR_TEXT_SECONDARY);
            tvDeviceSpecs.setText("Conecte ambos teléfonos con un cable USB-C a USB-C (o adaptador OTG).");

            tvStep1.setText("[1] Cable USB-C a USB-C: No detectado");
            tvStep1.setTextColor(COLOR_TEXT_SECONDARY);

            tvStep2.setText("[2] Permiso USB Host: En espera");
            tvStep2.setTextColor(COLOR_TEXT_SECONDARY);

            tvStep3.setText("[3] Puente ADB OTG: En espera de equipo");
            tvStep3.setTextColor(COLOR_TEXT_SECONDARY);

            btnActionUsb.setText("ESCANEAR PUERTO USB-C OTG");
            return;
        }

        UsbDevice targetDevice = deviceList.values().iterator().next();
        String name = getDeviceDisplayName(targetDevice);
        String vidPid = String.format(Locale.US, "%04X:%04X", targetDevice.getVendorId(), targetDevice.getProductId());
        String modeDesc = OtgAdbBridge.describeDeviceMode(targetDevice);
        boolean hasAdbIntf = OtgAdbBridge.findAdbInterface(targetDevice) != null;
        boolean hasPerm = usbManager.hasPermission(targetDevice);

        tvStep1.setText("[OK] Cable USB-C Detectado: " + name + " (" + vidPid + ")");
        tvStep1.setTextColor(COLOR_STATUS_GREEN);

        if (!hasPerm) {
            tvHeaderBadge.setText("AUTORIZAR PERMISO USB");
            tvHeaderBadge.setTextColor(COLOR_STATUS_AMBER);

            tvDeviceTitle.setText(name + "  [PERMISO PENDIENTE]");
            tvDeviceTitle.setTextColor(COLOR_STATUS_AMBER);
            tvDeviceSpecs.setText("ID USB: " + vidPid + " • " + modeDesc + "\nToque el botón inferior para autorizar el control USB OTG.");

            tvStep2.setText("[!] Permiso USB Host: Toque 'Autorizar USB OTG'");
            tvStep2.setTextColor(COLOR_STATUS_AMBER);
            tvStep3.setText("[3] Puente ADB OTG: Esperando permiso USB");
            tvStep3.setTextColor(COLOR_TEXT_SECONDARY);

            btnActionUsb.setText("AUTORIZAR USB OTG AHORA");
            return;
        }

        tvStep2.setText("[OK] Permiso USB Host: Concedido en este teléfono");
        tvStep2.setTextColor(COLOR_STATUS_GREEN);

        if (hasAdbIntf && !otgBridge.isUsbReady()) {
            otgBridge.openUsbDevice(targetDevice);
        }

        if (otgBridge.isUsbReady()) {
            if (technicianConnected) {
                tvHeaderBadge.setText("TÉCNICO CONECTADO EN VIVO");
                tvHeaderBadge.setTextColor(COLOR_BORDER_ACCENT);
            } else {
                tvHeaderBadge.setText("PUENTE OTG LISTO");
                tvHeaderBadge.setTextColor(COLOR_STATUS_GREEN);
            }

            tvDeviceTitle.setText(name + "  [PUENTE OTG ACTIVO]");
            tvDeviceTitle.setTextColor(COLOR_STATUS_GREEN);
            tvDeviceSpecs.setText("ID USB: " + vidPid + " • Interfaz ADB 0xFF/0x42/0x01 lista\nComparta el Código de Estación con el técnico. Si aparece el aviso de huella RSA en el otro teléfono, toque 'Permitir'.");

            tvStep3.setText("[OK] Puente ADB OTG: Activo hacia bore.pub y puerto 5555");
            tvStep3.setTextColor(COLOR_STATUS_GREEN);
            btnActionUsb.setText("REINICIAR PUENTE USB-C OTG");
        } else {
            tvHeaderBadge.setText("ACTIVAR DEPURACIÓN USB");
            tvHeaderBadge.setTextColor(COLOR_STATUS_AMBER);

            tvDeviceTitle.setText(name + "  [SIN DEPURACIÓN USB]");
            tvDeviceTitle.setTextColor(COLOR_STATUS_AMBER);
            tvDeviceSpecs.setText("ID USB: " + vidPid + " • " + modeDesc + "\nEn el teléfono a reparar: active 'Depuración por USB' en Opciones para Desarrolladores.");

            tvStep3.setText("[!] Puente ADB OTG: Active 'Depuración por USB' en el otro móvil");
            tvStep3.setTextColor(COLOR_STATUS_AMBER);
            btnActionUsb.setText("VOLVER A VERIFICAR INTERFAZ ADB");
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
                flags = 33554432; // PendingIntent.FLAG_MUTABLE requerido en Android 12+ para USB Manager
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
                if (logLines.size() > 45) {
                    logLines.remove(logLines.size() - 1);
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
    // CONSTRUCCIÓN PROGRAMÁTICA DE LA INTERFAZ ENTERPRISE DARK SLATE
    // ========================================================================
    private float getAdaptiveScale() {
        android.util.DisplayMetrics dm = getResources().getDisplayMetrics();
        float widthDp = dm.widthPixels / dm.density;
        // Escala responsiva autoadaptativa entre 0.92x (teléfonos compactos) y 1.35x (tablets/foldables)
        return Math.max(0.92f, Math.min(1.35f, widthDp / 390f));
    }

    private float sp(float baseSp) {
        return baseSp * getAdaptiveScale();
    }

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

        LinearLayout brandCol = new LinearLayout(this);
        brandCol.setOrientation(LinearLayout.VERTICAL);
        LinearLayout.LayoutParams brandLp = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f);
        brandCol.setLayoutParams(brandLp);

        TextView tvTitle = new TextView(this);
        tvTitle.setText("ADBHUB-LINK");
        tvTitle.setTextColor(Color.WHITE);
        tvTitle.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(17.5f));
        tvTitle.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        brandCol.addView(tvTitle);

        TextView tvSub = new TextView(this);
        tvSub.setText("ESTACIÓN CLIENTE ANDROID • CABLE USB-C OTG");
        tvSub.setTextColor(COLOR_BORDER_ACCENT);
        tvSub.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(10f));
        tvSub.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        brandCol.addView(tvSub);

        header.addView(brandCol);

        tvHeaderBadge = new TextView(this);
        tvHeaderBadge.setText("ESPERANDO CABLE OTG");
        tvHeaderBadge.setTextColor(COLOR_TEXT_SECONDARY);
        tvHeaderBadge.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(10f));
        tvHeaderBadge.setTypeface(Typeface.create("monospace", Typeface.BOLD));
        tvHeaderBadge.setPadding(dp(10), dp(5), dp(10), dp(5));
        tvHeaderBadge.setBackground(makeRoundedBox(COLOR_BG_RECESSED, COLOR_BORDER_SUBTLE, dp(6)));
        header.addView(tvHeaderBadge);

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
        content.setPadding(dp(14), dp(14), dp(14), dp(20));

        // TARJETA 1: CREDENCIALES DE ESTACIÓN (CÓDIGO + RELAY MUNDIAL)
        LinearLayout cardCreds = makeCardLayout();
        cardCreds.addView(makeSectionLabel("CREDENCIALES DE ESTA ESTACIÓN OTG"));

        TextView tvCredHint = new TextView(this);
        tvCredHint.setText("Comparta este código con su técnico. Permite conexión desde cualquier parte del mundo sin VPN.");
        tvCredHint.setTextColor(COLOR_TEXT_SECONDARY);
        tvCredHint.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(12f));
        tvCredHint.setTypeface(Typeface.create("sans-serif", Typeface.NORMAL));
        tvCredHint.setPadding(0, dp(2), 0, dp(10));
        cardCreds.addView(tvCredHint);

        LinearLayout codeBox = new LinearLayout(this);
        codeBox.setOrientation(LinearLayout.VERTICAL);
        codeBox.setGravity(Gravity.CENTER);
        codeBox.setPadding(dp(12), dp(14), dp(12), dp(14));
        codeBox.setBackground(makeRoundedBox(COLOR_BG_RECESSED, Color.parseColor("#263854"), dp(8)));

        TextView tvCodeCaption = new TextView(this);
        tvCodeCaption.setText("CÓDIGO DE EMPAREJAMIENTO MUNDIAL");
        tvCodeCaption.setTextColor(COLOR_TEXT_MUTED);
        tvCodeCaption.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(10.5f));
        tvCodeCaption.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        tvCodeCaption.setGravity(Gravity.CENTER);
        codeBox.addView(tvCodeCaption);

        tvCodeDisplay = new TextView(this);
        tvCodeDisplay.setText(currentCode);
        tvCodeDisplay.setTextColor(COLOR_BORDER_ACCENT);
        tvCodeDisplay.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(23f));
        tvCodeDisplay.setTypeface(Typeface.create("monospace", Typeface.BOLD));
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
            tvCodeDisplay.setLetterSpacing(0.06f);
        }
        tvCodeDisplay.setGravity(Gravity.CENTER);
        tvCodeDisplay.setPadding(0, dp(4), 0, dp(6));
        codeBox.addView(tvCodeDisplay);

        tvRelayEndpoint = new TextView(this);
        tvRelayEndpoint.setText("Relay Mundial: Conectando a bore.pub...");
        tvRelayEndpoint.setTextColor(COLOR_STATUS_AMBER);
        tvRelayEndpoint.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(11.5f));
        tvRelayEndpoint.setTypeface(Typeface.create("monospace", Typeface.BOLD));
        tvRelayEndpoint.setGravity(Gravity.CENTER);
        codeBox.addView(tvRelayEndpoint);

        tvLanEndpoint = new TextView(this);
        tvLanEndpoint.setText("LAN Wi-Fi: " + localIp + ":5555");
        tvLanEndpoint.setTextColor(COLOR_TEXT_SECONDARY);
        tvLanEndpoint.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(11.5f));
        tvLanEndpoint.setTypeface(Typeface.create("monospace", Typeface.NORMAL));
        tvLanEndpoint.setGravity(Gravity.CENTER);
        tvLanEndpoint.setPadding(0, dp(2), 0, 0);
        codeBox.addView(tvLanEndpoint);

        cardCreds.addView(codeBox);

        Button btnCopyCode = makeStyledButton("COPIAR CÓDIGO DE ESTACIÓN", COLOR_ACCENT_DARK, COLOR_ACCENT_BLUE);
        LinearLayout.LayoutParams copyLp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        copyLp.topMargin = dp(10);
        btnCopyCode.setLayoutParams(copyLp);
        btnCopyCode.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                copyToClipboard("Código ADBHUB-LINK", currentCode);
            }
        });
        cardCreds.addView(btnCopyCode);

        LinearLayout rowBtns = new LinearLayout(this);
        rowBtns.setOrientation(LinearLayout.HORIZONTAL);
        LinearLayout.LayoutParams rowLp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        rowLp.topMargin = dp(8);
        rowBtns.setLayoutParams(rowLp);

        Button btnNewId = makeStyledButton("NUEVO ID", Color.parseColor("#182234"), COLOR_BORDER_SUBTLE);
        LinearLayout.LayoutParams b1Lp = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f);
        b1Lp.rightMargin = dp(4);
        btnNewId.setLayoutParams(b1Lp);
        btnNewId.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                numericMode = false;
                codeSalt = (codeSalt + 17) & 0xFF;
                if (codeSalt == 0) codeSalt = 0x24;
                updateStationCode();
                onLog("SEGURIDAD", "Nuevo ID de 16 caracteres generado: " + currentCode);
            }
        });
        rowBtns.addView(btnNewId);

        Button btnNineDigits = makeStyledButton("MODO 9 DÍGITOS", Color.parseColor("#182234"), COLOR_BORDER_SUBTLE);
        LinearLayout.LayoutParams b2Lp = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f);
        b2Lp.leftMargin = dp(4);
        btnNineDigits.setLayoutParams(b2Lp);
        btnNineDigits.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                numericMode = !numericMode;
                updateStationCode();
            }
        });
        rowBtns.addView(btnNineDigits);

        cardCreds.addView(rowBtns);
        content.addView(cardCreds);

        // TARJETA 2: ESTADO DEL DISPOSITIVO EN EL CABLE USB-C A USB-C
        LinearLayout cardDevice = makeCardLayout();
        cardDevice.addView(makeSectionLabel("TELÉFONO CONECTADO POR CABLE USB-C A USB-C"));

        tvDeviceTitle = new TextView(this);
        tvDeviceTitle.setText("Escaneando puerto USB-C OTG...");
        tvDeviceTitle.setTextColor(Color.WHITE);
        tvDeviceTitle.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(15f));
        tvDeviceTitle.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        tvDeviceTitle.setPadding(0, dp(6), 0, dp(4));
        cardDevice.addView(tvDeviceTitle);

        tvDeviceSpecs = new TextView(this);
        tvDeviceSpecs.setText("Conecte el teléfono a reparar usando un cable USB-C a USB-C.");
        tvDeviceSpecs.setTextColor(COLOR_TEXT_SECONDARY);
        tvDeviceSpecs.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(12f));
        tvDeviceSpecs.setTypeface(Typeface.create("sans-serif", Typeface.NORMAL));
        tvDeviceSpecs.setPadding(0, 0, 0, dp(10));
        cardDevice.addView(tvDeviceSpecs);

        tvStep1 = makeChecklistRow("[1] Cable USB-C a USB-C: Buscando...");
        cardDevice.addView(tvStep1);

        tvStep2 = makeChecklistRow("[2] Permiso USB Host: En espera");
        cardDevice.addView(tvStep2);

        tvStep3 = makeChecklistRow("[3] Puente ADB OTG: En espera");
        cardDevice.addView(tvStep3);

        btnActionUsb = makeStyledButton("AUTORIZAR / ESCANEAR CABLE USB-C OTG", COLOR_ACCENT_DARK, COLOR_ACCENT_BLUE);
        LinearLayout.LayoutParams actLp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        actLp.topMargin = dp(10);
        btnActionUsb.setLayoutParams(actLp);
        btnActionUsb.setOnClickListener(new View.OnClickListener() {
            @Override
            public void onClick(View v) {
                requestOrReconnectUsb();
            }
        });
        cardDevice.addView(btnActionUsb);
        content.addView(cardDevice);

        // TARJETA 3: GUÍA RÁPIDA CABLE USB-C A USB-C
        LinearLayout cardGuide = makeCardLayout();
        cardGuide.addView(makeSectionLabel("GUÍA RÁPIDA: CONEXIÓN CON CABLE USB-C A USB-C"));

        TextView tvGuide = new TextView(this);
        tvGuide.setText(
                "1. Conecte el cable USB-C a USB-C entre este teléfono (Puente) y el teléfono a reparar.\n" +
                "2. Si no lo detecta al instante, baje la barra de notificaciones de este teléfono, toque la notificación 'Sistema Android • USB' y elija 'Controlado por: Este dispositivo'.\n" +
                "3. Acepte el aviso '¿Permitir que ADBHUB-LINK OTG acceda al dispositivo USB?'.\n" +
                "4. En el teléfono a reparar: active 'Depuración por USB' y acepte la ventana de huella RSA cuando el técnico se conecte."
        );
        tvGuide.setTextColor(COLOR_TEXT_PRIMARY);
        tvGuide.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(12f));
        tvGuide.setTypeface(Typeface.create("sans-serif", Typeface.NORMAL));
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.JELLY_BEAN) {
            tvGuide.setLineSpacing(dp(3), 1.15f);
        }
        tvGuide.setPadding(0, dp(6), 0, 0);
        cardGuide.addView(tvGuide);
        content.addView(cardGuide);

        // TARJETA 4: REGISTRO DE ACTIVIDAD EN VIVO
        LinearLayout cardLogs = makeCardLayout();
        cardLogs.addView(makeSectionLabel("BITÁCORA DE AUDITORÍA OTG & RED"));

        tvLogsBox = new TextView(this);
        tvLogsBox.setTextColor(COLOR_TEXT_SECONDARY);
        tvLogsBox.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(10.5f));
        tvLogsBox.setTypeface(Typeface.create("monospace", Typeface.NORMAL));
        tvLogsBox.setPadding(dp(10), dp(10), dp(10), dp(10));
        tvLogsBox.setBackground(makeRoundedBox(COLOR_BG_RECESSED, COLOR_BORDER_SUBTLE, dp(6)));
        LinearLayout.LayoutParams logLp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        logLp.topMargin = dp(6);
        tvLogsBox.setLayoutParams(logLp);
        cardLogs.addView(tvLogsBox);

        content.addView(cardLogs);
        scroll.addView(content);
        root.addView(scroll);

        return root;
    }

    private LinearLayout makeCardLayout() {
        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(dp(14), dp(14), dp(14), dp(14));
        card.setBackground(makeRoundedBox(COLOR_BG_CARD, COLOR_BORDER_SUBTLE, dp(10)));
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
        tv.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(11f));
        tv.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        return tv;
    }

    private TextView makeChecklistRow(String text) {
        TextView tv = new TextView(this);
        tv.setText(text);
        tv.setTextColor(COLOR_TEXT_SECONDARY);
        tv.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(11.5f));
        tv.setTypeface(Typeface.create("monospace", Typeface.BOLD));
        tv.setPadding(dp(10), dp(8), dp(10), dp(8));
        tv.setBackground(makeRoundedBox(COLOR_BG_RECESSED, COLOR_BORDER_SUBTLE, dp(6)));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        lp.topMargin = dp(5);
        tv.setLayoutParams(lp);
        return tv;
    }

    private Button makeStyledButton(String text, int fillColor, int strokeColor) {
        Button btn = new Button(this);
        btn.setText(text);
        btn.setAllCaps(false);
        btn.setTextColor(Color.WHITE);
        btn.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp(12.5f));
        btn.setTypeface(Typeface.create("sans-serif-medium", Typeface.BOLD));
        btn.setPadding(dp(12), dp(10), dp(12), dp(10));
        btn.setBackground(makeRoundedBox(fillColor, strokeColor, dp(7)));
        return btn;
    }

    private static GradientDrawable makeRoundedBox(int fillColor, int strokeColor, int radiusPx) {
        GradientDrawable gd = new GradientDrawable();
        gd.setShape(GradientDrawable.RECTANGLE);
        gd.setColor(fillColor);
        gd.setCornerRadius(radiusPx);
        gd.setStroke(2, strokeColor);
        return gd;
    }

    private void copyToClipboard(String label, String text) {
        ClipboardManager cm = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
        if (cm != null) {
            cm.setPrimaryClip(ClipData.newPlainText(label, text));
            Toast.makeText(this, "Copiado al portapapeles: " + text, Toast.LENGTH_SHORT).show();
        }
    }

    private int dp(float value) {
        return (int) TypedValue.applyDimension(
                TypedValue.COMPLEX_UNIT_DIP, value, getResources().getDisplayMetrics());
    }
}

