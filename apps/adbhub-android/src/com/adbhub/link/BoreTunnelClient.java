package com.adbhub.link;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Cliente de Túnel Mundial (bore.pub:7835) + Servidor Local (:5555) para Android.
 * Permite que el Técnico se conecte desde cualquier parte del mundo usando el
 * Código de Estación (XXXX-XXXX-XXXX-XXXX) sin abrir puertos ni usar VPN.
 */
public final class BoreTunnelClient {

    public interface TunnelListener {
        void onTunnelPortAssigned(int publicPort);
        void onLog(String tag, String message);
    }

    private static final String BORE_HOST = "bore.pub";
    private static final int BORE_CONTROL_PORT = 7835;
    private static final int LOCAL_LAN_PORT = 5555;

    private static final Pattern HELLO_PORT_PATTERN = Pattern.compile("\"Hello\"\\s*:\\s*(\\d+)");
    private static final Pattern CONN_UUID_PATTERN = Pattern.compile("\"Connection\"\\s*:\\s*\"([^\"]+)\"");

    private final OtgAdbBridge bridge;
    private final TunnelListener listener;
    private final AtomicBoolean running = new AtomicBoolean(false);
    private ServerSocket lanServerSocket;
    private Socket controlSocket;

    public BoreTunnelClient(OtgAdbBridge bridge, TunnelListener listener) {
        this.bridge = bridge;
        this.listener = listener;
    }

    public void start() {
        if (running.getAndSet(true)) {
            return;
        }
        startLanServer();
        startCloudRelayLoop();
    }

    public void stop() {
        running.set(false);
        try {
            if (lanServerSocket != null) lanServerSocket.close();
        } catch (Exception ignored) {}
        try {
            if (controlSocket != null) controlSocket.close();
        } catch (Exception ignored) {}
    }

    private void startLanServer() {
        Thread t = new Thread(new Runnable() {
            @Override
            public void run() {
                try {
                    lanServerSocket = new ServerSocket();
                    lanServerSocket.setReuseAddress(true);
                    lanServerSocket.bind(new InetSocketAddress("0.0.0.0", LOCAL_LAN_PORT));
                    listener.onLog("RED", "Servidor LAN OTG escuchando en puerto " + LOCAL_LAN_PORT);

                    while (running.get() && !lanServerSocket.isClosed()) {
                        Socket client = lanServerSocket.accept();
                        String peer = client.getInetAddress().getHostAddress() + ":" + client.getPort();
                        bridge.attachTechnicianSocket(client, "LAN " + peer);
                    }
                } catch (Exception e) {
                    if (running.get()) {
                        listener.onLog("AVISO", "Puerto LAN " + LOCAL_LAN_PORT + ": " + e.getMessage());
                    }
                }
            }
        }, "adbhub-lan-server");
        t.setDaemon(true);
        t.start();
    }

    private void startCloudRelayLoop() {
        Thread t = new Thread(new Runnable() {
            @Override
            public void run() {
                while (running.get()) {
                    Socket ctrl = null;
                    try {
                        ctrl = new Socket();
                        ctrl.connect(new InetSocketAddress(BORE_HOST, BORE_CONTROL_PORT), 8000);
                        ctrl.setSoTimeout(0);
                        controlSocket = ctrl;

                        OutputStream out = ctrl.getOutputStream();
                        InputStream in = ctrl.getInputStream();

                        byte[] helloMsg = "{\"Hello\":0}\0".getBytes(StandardCharsets.UTF_8);
                        out.write(helloMsg);
                        out.flush();

                        String firstFrame = readNullTerminatedFrame(in);
                        if (firstFrame == null) {
                            throw new Exception("Respuesta vacía de bore.pub");
                        }

                        Matcher mHello = HELLO_PORT_PATTERN.matcher(firstFrame);
                        if (mHello.find()) {
                            int assignedPort = Integer.parseInt(mHello.group(1));
                            if (assignedPort > 0) {
                                listener.onTunnelPortAssigned(assignedPort);
                            }
                        }

                        while (running.get() && !ctrl.isClosed()) {
                            String frame = readNullTerminatedFrame(in);
                            if (frame == null) {
                                break;
                            }
                            Matcher mConn = CONN_UUID_PATTERN.matcher(frame);
                            if (mConn.find()) {
                                final String uuid = mConn.group(1);
                                spawnDataTunnelForUuid(uuid);
                            }
                        }
                    } catch (Exception e) {
                        if (running.get()) {
                            listener.onLog("TUNEL", "Reconectando Relay Mundial bore.pub...");
                        }
                    } finally {
                        if (ctrl != null) {
                            try {
                                ctrl.close();
                            } catch (Exception ignored) {}
                        }
                    }

                    try {
                        Thread.sleep(4000);
                    } catch (InterruptedException ignored) {}
                }
            }
        }, "adbhub-bore-control");
        t.setDaemon(true);
        t.start();
    }

    private void spawnDataTunnelForUuid(final String uuid) {
        Thread t = new Thread(new Runnable() {
            @Override
            public void run() {
                Socket dataSocket = null;
                try {
                    dataSocket = new Socket();
                    dataSocket.connect(new InetSocketAddress(BORE_HOST, BORE_CONTROL_PORT), 7000);
                    dataSocket.setTcpNoDelay(true);

                    String acceptJson = "{\"Accept\":\"" + uuid + "\"}\0";
                    OutputStream out = dataSocket.getOutputStream();
                    out.write(acceptJson.getBytes(StandardCharsets.UTF_8));
                    out.flush();

                    bridge.attachTechnicianSocket(dataSocket, "Relay Mundial (bore.pub)");
                } catch (Exception e) {
                    if (dataSocket != null) {
                        try {
                            dataSocket.close();
                        } catch (Exception ignored) {}
                    }
                }
            }
        }, "adbhub-bore-data");
        t.setDaemon(true);
        t.start();
    }

    private static String readNullTerminatedFrame(InputStream in) throws Exception {
        ByteArrayOutputStream baos = new ByteArrayOutputStream(128);
        int b;
        while ((b = in.read()) != -1) {
            if (b == 0) {
                return new String(baos.toByteArray(), StandardCharsets.UTF_8);
            }
            baos.write(b);
            if (baos.size() > 8192) {
                break;
            }
        }
        return null;
    }
}

