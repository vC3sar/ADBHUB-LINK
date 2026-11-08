package com.adbhub.link;

import android.hardware.usb.UsbConstants;
import android.hardware.usb.UsbDevice;
import android.hardware.usb.UsbDeviceConnection;
import android.hardware.usb.UsbEndpoint;
import android.hardware.usb.UsbInterface;
import android.hardware.usb.UsbManager;

import java.io.EOFException;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.Socket;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * Puente de Transporte ADB de Bajo Nivel sobre Cable USB-C a USB-C (OTG).
 *
 * Traduce de forma transparente las tramas de 24 bytes (struct amessage) + payload
 * entre el socket TCP del Técnico (bore.pub o LAN :5555) y los endpoints Bulk IN / Bulk OUT
 * de la interfaz USB ADB (Class 0xFF, Subclass 0x42, Protocol 0x01) del teléfono esclavo.
 */
public final class OtgAdbBridge {

    public interface BridgeLogger {
        void onLog(String tag, String message);
        void onClientSessionStateChanged(boolean connected, String remotePeer);
    }

    private static final int ADB_HEADER_LENGTH = 24;
    private static final int MAX_ADB_PAYLOAD = 1024 * 1024; // 1 MB límite seguro por paquete
    private static final int USB_TIMEOUT_MS = 5000;

    private final UsbManager usbManager;
    private final BridgeLogger logger;
    private final Object usbWriteLock = new Object();

    private volatile UsbDevice activeDevice;
    private volatile UsbDeviceConnection activeConnection;
    private volatile UsbInterface activeInterface;
    private volatile UsbEndpoint epIn;
    private volatile UsbEndpoint epOut;
    private volatile Socket currentSocket;
    private final AtomicBoolean sessionRunning = new AtomicBoolean(false);

    public OtgAdbBridge(UsbManager usbManager, BridgeLogger logger) {
        this.usbManager = usbManager;
        this.logger = logger;
    }

    /**
     * Inspecciona un UsbDevice para encontrar su interfaz ADB (0xFF / 0x42 / 0x01).
     */
    public static UsbInterface findAdbInterface(UsbDevice device) {
        if (device == null) return null;
        int count = device.getInterfaceCount();
        for (int i = 0; i < count; i++) {
            UsbInterface intf = device.getInterface(i);
            if (intf.getInterfaceClass() == 0xFF
                    && intf.getInterfaceSubclass() == 0x42
                    && intf.getInterfaceProtocol() == 0x01) {
                return intf;
            }
        }
        return null;
    }

    /**
     * Describe el modo especial de reparación del dispositivo conectado por USB-C.
     */
    public static String describeDeviceMode(UsbDevice device) {
        if (device == null) return "No detectado";
        int count = device.getInterfaceCount();
        for (int i = 0; i < count; i++) {
            UsbInterface intf = device.getInterface(i);
            if (intf.getInterfaceClass() == 0xFF && intf.getInterfaceSubclass() == 0x42) {
                if (intf.getInterfaceProtocol() == 0x01) {
                    return "Modo ADB Activo (Listo para Puente)";
                } else if (intf.getInterfaceProtocol() == 0x03) {
                    return "Modo Fastboot / Bootloader";
                }
            }
        }
        int vid = device.getVendorId();
        int pid = device.getProductId();
        if (vid == 0x04E8 && (pid == 0x685D || pid == 0x68C3)) {
            return "Modo Samsung Download (Odin)";
        }
        if (vid == 0x05C6 && pid == 0x9008) {
            return "Modo Qualcomm EDL 9008";
        }
        if (vid == 0x0E8D && (pid == 0x0003 || pid == 0x2000)) {
            return "Modo MediaTek BROM / Preloader";
        }
        return "Conectado por USB (Activar Depuración USB en el equipo)";
    }

    public synchronized boolean openUsbDevice(UsbDevice device) {
        closeUsbDevice();
        if (device == null) return false;

        UsbInterface adbIntf = findAdbInterface(device);
        if (adbIntf == null) {
            logger.onLog("USB", "El teléfono conectado aún no expone la interfaz ADB (active Depuración por USB).");
            return false;
        }

        UsbEndpoint foundIn = null;
        UsbEndpoint foundOut = null;
        for (int i = 0; i < adbIntf.getEndpointCount(); i++) {
            UsbEndpoint ep = adbIntf.getEndpoint(i);
            if (ep.getType() == UsbConstants.USB_ENDPOINT_XFER_BULK) {
                if (ep.getDirection() == UsbConstants.USB_DIR_IN) {
                    foundIn = ep;
                } else if (ep.getDirection() == UsbConstants.USB_DIR_OUT) {
                    foundOut = ep;
                }
            }
        }

        if (foundIn == null || foundOut == null) {
            logger.onLog("ERROR", "No se encontraron endpoints Bulk IN/OUT en la interfaz ADB.");
            return false;
        }

        UsbDeviceConnection conn = usbManager.openDevice(device);
        if (conn == null) {
            logger.onLog("ERROR", "No se pudo abrir la conexión USB Host.");
            return false;
        }

        if (!conn.claimInterface(adbIntf, true)) {
            conn.close();
            logger.onLog("ERROR", "No se pudo reclamar la interfaz USB ADB en modo exclusivo.");
            return false;
        }

        this.activeDevice = device;
        this.activeConnection = conn;
        this.activeInterface = adbIntf;
        this.epIn = foundIn;
        this.epOut = foundOut;

        logger.onLog("PUENTE", "Interfaz USB-C OTG reclamada correctamente (Bulk IN/OUT listos).");
        return true;
    }

    public boolean isUsbReady() {
        return activeConnection != null && epIn != null && epOut != null;
    }

    public synchronized void closeUsbDevice() {
        sessionRunning.set(false);
        closeQuietly(currentSocket);
        currentSocket = null;

        if (activeConnection != null) {
            try {
                if (activeInterface != null) {
                    activeConnection.releaseInterface(activeInterface);
                }
            } catch (Exception ignored) {}
            try {
                activeConnection.close();
            } catch (Exception ignored) {}
        }
        activeConnection = null;
        activeInterface = null;
        activeDevice = null;
        epIn = null;
        epOut = null;
    }

    /**
     * Enlaza un socket TCP entrante (desde bore.pub o desde LAN :5555) con los endpoints
     * USB Bulk del teléfono conectado por OTG.
     */
    public void attachTechnicianSocket(final Socket tcpSocket, final String peerDescription) {
        if (!isUsbReady()) {
            if (activeDevice != null && usbManager.hasPermission(activeDevice)) {
                openUsbDevice(activeDevice);
            }
            if (!isUsbReady()) {
                logger.onLog("AVISO", "Conexión de técnico rechazada: no hay teléfono ADB listo en el cable OTG.");
                closeQuietly(tcpSocket);
                return;
            }
        }

        // Cerrar sesión previa si existía para dar prioridad a la nueva conexión del técnico
        Socket old = this.currentSocket;
        if (old != null && !old.isClosed()) {
            closeQuietly(old);
        }

        this.currentSocket = tcpSocket;
        sessionRunning.set(true);
        logger.onClientSessionStateChanged(true, peerDescription);
        logger.onLog("CONEXION", "Técnico enlazado al cable USB-C OTG (" + peerDescription + ")");

        final UsbDeviceConnection conn = this.activeConnection;
        final UsbEndpoint inEp = this.epIn;
        final UsbEndpoint outEp = this.epOut;

        // Hilo 1: TCP (Técnico) -> USB Bulk OUT (Teléfono a reparar)
        Thread tcpToUsb = new Thread(new Runnable() {
            @Override
            public void run() {
                try {
                    tcpSocket.setTcpNoDelay(true);
                    InputStream in = tcpSocket.getInputStream();
                    byte[] headerBuf = new byte[ADB_HEADER_LENGTH];

                    while (sessionRunning.get() && currentSocket == tcpSocket && !tcpSocket.isClosed()) {
                        readFully(in, headerBuf, 0, ADB_HEADER_LENGTH);

                        ByteBuffer bb = ByteBuffer.wrap(headerBuf).order(ByteOrder.LITTLE_ENDIAN);
                        int command = bb.getInt(0);
                        int dataLen = bb.getInt(12);
                        int magic = bb.getInt(20);

                        if ((command ^ 0xFFFFFFFF) != magic || dataLen < 0 || dataLen > MAX_ADB_PAYLOAD) {
                            logger.onLog("AVISO", "Trama ADB fuera de sincronía recibida por TCP; reiniciando sesión.");
                            break;
                        }

                        byte[] payload = null;
                        if (dataLen > 0) {
                            payload = new byte[dataLen];
                            readFully(in, payload, 0, dataLen);
                        }

                        synchronized (usbWriteLock) {
                            int r1 = conn.bulkTransfer(outEp, headerBuf, ADB_HEADER_LENGTH, USB_TIMEOUT_MS);
                            if (r1 < 0) {
                                throw new IOException("Fallo en bulkTransfer OUT (cabecera 24B)");
                            }
                            if (payload != null && dataLen > 0) {
                                int r2 = uploadBulkPayload(conn, outEp, payload, dataLen);
                                if (r2 < 0) {
                                    throw new IOException("Fallo en bulkTransfer OUT (payload " + dataLen + "B)");
                                }
                            }
                        }
                    }
                } catch (EOFException eof) {
                    // Cierre normal del socket TCP del técnico
                } catch (Exception e) {
                    if (sessionRunning.get() && currentSocket == tcpSocket) {
                        logger.onLog("RED", "Sesión TCP->USB finalizada: " + e.getMessage());
                    }
                } finally {
                    if (currentSocket == tcpSocket) {
                        sessionRunning.set(false);
                        closeQuietly(tcpSocket);
                        logger.onClientSessionStateChanged(false, peerDescription);
                    }
                }
            }
        }, "adbhub-tcp-to-usb");

        // Hilo 2: USB Bulk IN (Teléfono a reparar) -> TCP (Técnico)
        Thread usbToTcp = new Thread(new Runnable() {
            @Override
            public void run() {
                try {
                    OutputStream out = tcpSocket.getOutputStream();
                    byte[] headerBuf = new byte[ADB_HEADER_LENGTH];

                    while (sessionRunning.get() && currentSocket == tcpSocket && !tcpSocket.isClosed()) {
                        int n = conn.bulkTransfer(inEp, headerBuf, ADB_HEADER_LENGTH, 1000);
                        if (n < 0) {
                            // Timeout de lectura USB (normal cuando no hay datos entrantes); continuar si la sesión sigue activa
                            continue;
                        }
                        if (n != ADB_HEADER_LENGTH) {
                            continue;
                        }

                        ByteBuffer bb = ByteBuffer.wrap(headerBuf).order(ByteOrder.LITTLE_ENDIAN);
                        int command = bb.getInt(0);
                        int dataLen = bb.getInt(12);
                        int magic = bb.getInt(20);

                        if ((command ^ 0xFFFFFFFF) != magic || dataLen < 0 || dataLen > MAX_ADB_PAYLOAD) {
                            continue;
                        }

                        byte[] payload = null;
                        if (dataLen > 0) {
                            payload = readBulkPayload(conn, inEp, dataLen);
                            if (payload == null) {
                                break;
                            }
                        }

                        synchronized (out) {
                            out.write(headerBuf, 0, ADB_HEADER_LENGTH);
                            if (payload != null && dataLen > 0) {
                                out.write(payload, 0, dataLen);
                            }
                            out.flush();
                        }
                    }
                } catch (Exception e) {
                    if (sessionRunning.get() && currentSocket == tcpSocket) {
                        logger.onLog("USB", "Sesión USB->TCP finalizada: " + e.getMessage());
                    }
                } finally {
                    if (currentSocket == tcpSocket) {
                        sessionRunning.set(false);
                        closeQuietly(tcpSocket);
                        logger.onClientSessionStateChanged(false, peerDescription);
                    }
                }
            }
        }, "adbhub-usb-to-tcp");

        tcpToUsb.setDaemon(true);
        usbToTcp.setDaemon(true);
        tcpToUsb.start();
        usbToTcp.start();
    }

    private static int uploadBulkPayload(UsbDeviceConnection conn, UsbEndpoint outEp, byte[] data, int totalLen) {
        int offset = 0;
        int maxChunk = 16384;
        while (offset < totalLen) {
            int chunk = Math.min(maxChunk, totalLen - offset);
            int sent = conn.bulkTransfer(outEp, data, offset, chunk, USB_TIMEOUT_MS);
            if (sent <= 0) {
                return -1;
            }
            offset += sent;
        }
        int maxPkt = outEp.getMaxPacketSize();
        if (maxPkt > 0 && (totalLen % maxPkt) == 0) {
            // Paquete de longitud cero (ZLP) requerido por la especificación USB Bulk de Android adbd
            conn.bulkTransfer(outEp, new byte[0], 0, 200);
        }
        return offset;
    }

    private static byte[] readBulkPayload(UsbDeviceConnection conn, UsbEndpoint inEp, int totalLen) {
        byte[] result = new byte[totalLen];
        int offset = 0;
        int maxChunk = 16384;
        while (offset < totalLen) {
            int want = Math.min(maxChunk, totalLen - offset);
            int got = conn.bulkTransfer(inEp, result, offset, want, USB_TIMEOUT_MS);
            if (got <= 0) {
                return null;
            }
            offset += got;
        }
        return result;
    }

    private static void readFully(InputStream in, byte[] buf, int off, int len) throws IOException {
        int pos = 0;
        while (pos < len) {
            int n = in.read(buf, off + pos, len - pos);
            if (n < 0) {
                throw new EOFException("Fin de flujo TCP inesperado");
            }
            pos += n;
        }
    }

    private static void closeQuietly(Socket s) {
        if (s != null) {
            try {
                s.close();
            } catch (Exception ignored) {}
        }
    }
}

