package com.adbhub.link;

import java.net.Inet4Address;
import java.net.InetAddress;
import java.net.NetworkInterface;
import java.util.Enumeration;

/**
 * Codificador criptográfico de Estación ADBHUB-LINK (compatible bit a bit con
 * AnyDeskCode::encode_station y AnyDeskCode::encode_numeric_station en Rust).
 */
public final class StationCodeEncoder {

    private static final char[] HEX_ARRAY = "0123456789ABCDEF".toCharArray();

    private StationCodeEncoder() {}

    public static String encodeAlphanumeric(String localIp, int cloudTunnelPort, int saltByte) {
        int[] octets = parseIpv4(localIp);
        int salt = (saltByte & 0xFF) != 0 ? (saltByte & 0xFF) : 0x42;
        int port = cloudTunnelPort & 0xFFFF;

        int[] raw = new int[] {
            (0xAD ^ salt) & 0xFF,
            salt & 0xFF,
            (octets[0] ^ salt ^ 0x5A) & 0xFF,
            (octets[1] ^ salt ^ 0xA5) & 0xFF,
            (octets[2] ^ salt ^ 0x3C) & 0xFF,
            (octets[3] ^ salt ^ 0xC3) & 0xFF,
            (((port >>> 8) & 0xFF) ^ salt ^ 0x7E) & 0xFF,
            ((port & 0xFF) ^ salt ^ 0xE7) & 0xFF
        };

        StringBuilder sb = new StringBuilder(19);
        for (int i = 0; i < raw.length; i++) {
            int b = raw[i];
            sb.append(HEX_ARRAY[(b >>> 4) & 0x0F]);
            sb.append(HEX_ARRAY[b & 0x0F]);
            if (i == 1 || i == 3 || i == 5) {
                sb.append('-');
            }
        }
        return sb.toString();
    }

    public static String encodeNumeric(String localIp, int cloudTunnelPort) {
        if (cloudTunnelPort > 0) {
            int p = cloudTunnelPort & 0xFFFF;
            return String.format("900-%03d-%03d", p / 1000, p % 1000);
        }
        int[] octets = parseIpv4(localIp);
        if (octets[0] == 192 && octets[1] == 168) {
            return String.format("192-%03d-%03d", octets[2], octets[3]);
        }
        return String.format("%03d-%03d-%03d", octets[0], octets[2], octets[3]);
    }

    public static String detectLocalIpv4() {
        try {
            Enumeration<NetworkInterface> interfaces = NetworkInterface.getNetworkInterfaces();
            String fallback = "127.0.0.1";
            while (interfaces != null && interfaces.hasMoreElements()) {
                NetworkInterface nif = interfaces.nextElement();
                if (!nif.isUp() || nif.isLoopback()) {
                    continue;
                }
                Enumeration<InetAddress> addrs = nif.getInetAddresses();
                while (addrs.hasMoreElements()) {
                    InetAddress addr = addrs.nextElement();
                    if (addr instanceof Inet4Address && !addr.isLoopbackAddress()) {
                        String ip = addr.getHostAddress();
                        if (ip != null) {
                            if (ip.startsWith("192.168.") || ip.startsWith("172.")) {
                                return ip;
                            }
                            fallback = ip;
                        }
                    }
                }
            }
            return fallback;
        } catch (Exception e) {
            return "127.0.0.1";
        }
    }

    private static int[] parseIpv4(String ip) {
        if (ip != null) {
            String[] parts = ip.trim().split("\\.");
            if (parts.length == 4) {
                try {
                    return new int[] {
                        Integer.parseInt(parts[0]) & 0xFF,
                        Integer.parseInt(parts[1]) & 0xFF,
                        Integer.parseInt(parts[2]) & 0xFF,
                        Integer.parseInt(parts[3]) & 0xFF
                    };
                } catch (NumberFormatException ignored) {}
            }
        }
        return new int[] { 127, 0, 0, 1 };
    }
}

