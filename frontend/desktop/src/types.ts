/**
 * ADBHUB-LINK Desktop — TypeScript Domain Types (Tauri 2 / React Frontend)
 * Aligned strictly with Section 13 (Interfaz de Usuario) and Section 2.6 (Samsung Download Mode).
 */

export type UsbDeviceMode =
  | "AndroidAdb"
  | "Mtp"
  | "CompositeAdbMtp"
  | "Fastboot"
  | "QualcommEdl"
  | "MediatekBrom"
  | "MediatekPreloader"
  | "SpdUnisoc"
  | "SamsungDownload"
  | "UnknownUsbDevice";

export type OdinCompatibility = "verificada" | "no verificada" | "desconocida";

export interface BusId {
  bus: number;
  port_chain: number[];
}

export interface DeviceRecord {
  id: string;
  agent_id: string;
  busid: BusId;
  vendor_id: number;
  product_id: number;
  manufacturer: string;
  product_name: string;
  serial_hash: string | null;
  device_state: string;
  usb_mode: UsbDeviceMode;
  odin_compatibility: OdinCompatibility;
  online: boolean;
  shared: boolean;
  updated_at: string;
}

export interface SamsungDownloadPanelView {
  manufacturer: "Samsung";
  mode: "Download Mode";
  usb_state: string;
  detected_driver: string;
  export_status: string;
  technician_pc_connection_status: string;
  odin_compatibility: OdinCompatibility;
  flashing_warning: string;
}

export interface DesktopActiveSessionView {
  session_id: string;
  device_id: string;
  device_name: string;
  manufacturer: string;
  vid_pid: string;
  host_machine: string;
  owner_email: string;
  usb_mode: UsbDeviceMode;
  usb_mode_label: string;
  state: string;
  started_at: string;
  duration_seconds: number;
  latency_ms: number;
  transfer_rate_kbps: number;
  bytes_out: number;
  bytes_in: number;
  urbs_completed: number;
  critical_flashing_guard: boolean;
  samsung_download_panel: SamsungDownloadPanelView | null;
  events: string[];
}

export interface DesktopDashboardSnapshot {
  connection_status: string;
  host_usbipd_installed: boolean;
  client_vhci_installed: boolean;
  available_agents_count: number;
  connected_devices_count: number;
  active_sessions_count: number;
  devices: DeviceRecord[];
  active_sessions: DesktopActiveSessionView[];
  recent_events: string[];
}

