//! # `Setup.exe` — ADBHUB-LINK Standalone Windows Setup Executable
//!
//! Provides an interactive and automated single-click `.exe` installer.
//! Prompts the user to select the installation target:
//!   [1] PC del Cliente / Taller (ADBHUB-LINK Agent)
//!   [2] PC del Técnico (ADBHUB-LINK Desktop)
//!   [3] Instalación Completa (Ambos componentes)

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn is_elevated() -> bool {
    Command::new("net")
        .args(["session"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn relaunch_as_admin() -> bool {
    let current_exe = env::current_exe().unwrap_or_else(|_| PathBuf::from("Setup.exe"));
    let ps_cmd = format!("Start-Process -FilePath '{}' -Verb RunAs", current_exe.display());
    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_cmd])
        .status();
    status.map(|s| s.success()).unwrap_or(false)
}

fn find_file_in_candidates(candidates: &[PathBuf]) -> Option<PathBuf> {
    for c in candidates {
        if c.exists() {
            return Some(c.clone());
        }
    }
    None
}

fn run_command(desc: &str, program: &str, args: &[&str]) -> bool {
    println!("  [+] {desc}...");
    let status = Command::new(program).args(args).status();
    match status {
        Ok(s) => {
            if s.success() {
                println!("      OK.");
                true
            } else {
                println!("      Notice: Exit code {:?}", s.code());
                false
            }
        }
        Err(e) => {
            println!("      Warning: {e}");
            false
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallTarget {
    AgentOnly,
    DesktopOnly,
    Full,
}

fn prompt_install_target() -> InstallTarget {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--agent" || a == "-a") {
        return InstallTarget::AgentOnly;
    }
    if args.iter().any(|a| a == "--desktop" || a == "-d") {
        return InstallTarget::DesktopOnly;
    }
    if args.iter().any(|a| a == "--full" || a == "-f") {
        return InstallTarget::Full;
    }

    println!("\n  ¿Que componente deseas instalar en esta computadora?\n");
    println!("  [1] PC del Cliente / Taller (ADBHUB-LINK Agent)");
    println!("      -> Para la computadora donde se conecta fisicamente el telefono.");
    println!("      -> Instala el Agente detector de USB y el driver de exportacion usbipd-win.");
    println!();
    println!("  [2] PC del Tecnico (ADBHUB-LINK Desktop)");
    println!("      -> Para la estacion del tecnico donde corre Odin, QFIL, ADB, etc.");
    println!("      -> Instala el driver virtual UDE y abre el panel de control grafico.");
    println!();
    println!("  [3] Instalacion Completa (Ambos componentes)");
    println!("      -> Recomendado para pruebas locales o computadoras todo-en-uno.");
    println!();
    print!("  Seleccione una opcion [1, 2 o 3] (Por defecto: 3): ");
    let _ = io::stdout().flush();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        match input.trim() {
            "1" => return InstallTarget::AgentOnly,
            "2" => return InstallTarget::DesktopOnly,
            "3" | "" => return InstallTarget::Full,
            _ => {
                println!("  Opcion no reconocida. Continuando con instalacion Completa (3).");
                return InstallTarget::Full;
            }
        }
    }

    InstallTarget::Full
}

fn main() {
    println!("================================================================================");
    println!(" ADBHUB-LINK v0.2.0 — Instalador Oficial Windows (.EXE)");
    println!(" Plataforma Profesional de USB Passthrough Remoto para Reparacion Movil");
    println!("================================================================================");

    // 1. Check Administrator Privileges and auto-elevate
    if !is_elevated() {
        println!("\n[ADBHUB-LINK] Solicitando permisos de Administrador para configurar drivers...");
        if relaunch_as_admin() {
            return;
        } else {
            eprintln!("[ERROR] No se pudo elevar a Administrador. Por favor ejecute como Administrador.");
            let _ = Command::new("cmd").args(["/c", "pause"]).status();
            return;
        }
    }

    let target = prompt_install_target();

    let root_dir = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Try finding root project dir if run from target/release or installer
    let project_root = if root_dir.join("installer").exists() {
        root_dir.clone()
    } else if root_dir
        .parent()
        .map(|p| p.join("installer").exists())
        .unwrap_or(false)
    {
        root_dir.parent().unwrap().to_path_buf()
    } else if root_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("installer").exists())
        .unwrap_or(false)
    {
        root_dir.parent().unwrap().parent().unwrap().to_path_buf()
    } else {
        root_dir.clone()
    };

    let packages_dir = project_root.join("installer").join("packages");
    let usbipd_candidates = [
        packages_dir.join("usbipd-win.msi"),
        root_dir.join("packages").join("usbipd-win.msi"),
        root_dir.join("usbipd-win.msi"),
    ];
    let usbipd_msi = find_file_in_candidates(&usbipd_candidates).unwrap_or_else(|| packages_dir.join("usbipd-win.msi"));

    let win2_candidates = [
        packages_dir.join("usbip-win2.exe"),
        root_dir.join("packages").join("usbip-win2.exe"),
        root_dir.join("usbip-win2.exe"),
    ];
    let usbip_win2_exe = find_file_in_candidates(&win2_candidates).unwrap_or_else(|| packages_dir.join("usbip-win2.exe"));

    let base_install_dir = PathBuf::from(r"C:\Program Files\ADBHUB-LINK");
    let agent_dir = base_install_dir.join("Agent");
    let desktop_dir = base_install_dir.join("Desktop");

    let install_agent = matches!(target, InstallTarget::AgentOnly | InstallTarget::Full);
    let install_desktop = matches!(target, InstallTarget::DesktopOnly | InstallTarget::Full);

    println!("\n[Paso 1/4] Instalacion de Controladores Kernel Firmados");
    println!("--------------------------------------------------------------------------------");

    if install_agent {
        let usbipd_installed = Path::new(r"C:\Program Files\usbipd-win\usbipd.exe").exists();
        if !usbipd_installed {
            if usbipd_msi.exists() {
                let msi_str = usbipd_msi.to_string_lossy().to_string();
                run_command(
                    "Instalando driver de exportacion Host (dorssel.usbipd-win WHQL)",
                    "msiexec.exe",
                    &["/i", &msi_str, "/qn", "/norestart"],
                );
            } else {
                println!("  [!] Paquete usbipd-win.msi no encontrado en {:?}", packages_dir);
            }
        } else {
            println!("  [OK] Driver Host usbipd-win ya instalado.");
        }
    }

    if install_desktop {
        let usbip_installed = Path::new(r"C:\Program Files\usbip-win2\usbip.exe").exists()
            || Path::new(r"C:\Windows\System32\drivers\usbip2_ude.sys").exists();
        if !usbip_installed {
            if usbip_win2_exe.exists() {
                let exe_str = usbip_win2_exe.to_string_lossy().to_string();
                run_command(
                    "Instalando driver virtual UDE para Tecnico (vadimgrn.usbip-win2)",
                    &exe_str,
                    &["/S", "/v/qn"],
                );
            } else {
                println!("  [!] Paquete usbip-win2.exe no encontrado en {:?}", packages_dir);
            }
        } else {
            println!("  [OK] Driver Tecnico usbip-win2 UDE ya instalado.");
        }
    }

    println!("\n[Paso 2/4] Configuracion de Reglas de Windows Defender Firewall");
    println!("--------------------------------------------------------------------------------");
    if install_agent {
        run_command(
            "Apertura de puerto local Host USB/IP (TCP 3240)",
            "netsh.exe",
            &[
                "advfirewall",
                "firewall",
                "add",
                "rule",
                "name=ADBHUB-LINK Host USB/IP",
                "dir=in",
                "action=allow",
                "protocol=TCP",
                "localport=3240",
            ],
        );
    }
    if install_desktop {
        run_command(
            "Apertura de puerto local Desktop UI (TCP 3420)",
            "netsh.exe",
            &[
                "advfirewall",
                "firewall",
                "add",
                "rule",
                "name=ADBHUB-LINK Desktop UI",
                "dir=in",
                "action=allow",
                "protocol=TCP",
                "localport=3420",
            ],
        );
    }
    run_command(
        "Apertura de tunel seguro QUIC Relay (UDP 4433)",
        "netsh.exe",
        &[
            "advfirewall",
            "firewall",
            "add",
            "rule",
            "name=ADBHUB-LINK QUIC Relay",
            "dir=out",
            "action=allow",
            "protocol=UDP",
            "remoteport=4433",
        ],
    );

    println!("\n[Paso 3/4] Despliegue de Binarios Oficiales");
    println!("--------------------------------------------------------------------------------");
    let agent_candidates = [
        root_dir.join("adbhub-agent.exe"),
        project_root.join(r"target\release\adbhub-agent.exe"),
        project_root.join(r"target\debug\adbhub-agent.exe"),
    ];
    let desktop_candidates = [
        root_dir.join("adbhub-desktop.exe"),
        project_root.join(r"target\release\adbhub-desktop.exe"),
        project_root.join(r"target\debug\adbhub-desktop.exe"),
    ];

    if install_agent {
        let _ = fs::create_dir_all(&agent_dir);
        if let Some(src) = find_file_in_candidates(&agent_candidates) {
            let dest = agent_dir.join("adbhub-agent.exe");
            if fs::copy(&src, &dest).is_ok() {
                println!("  [OK] Agente desplegado en: {}", dest.display());
            }
        }

        let agent_bin_str = agent_dir.join("adbhub-agent.exe").to_string_lossy().to_string();
        let tr_arg = format!("\"{agent_bin_str}\" watch --mobile-only");
        run_command(
            "Registro de Servicio en segundo plano para el Agente",
            "schtasks.exe",
            &[
                "/Create",
                "/TN",
                "ADBHUB-LINK-Agent",
                "/TR",
                &tr_arg,
                "/SC",
                "ONLOGON",
                "/RL",
                "HIGHEST",
                "/F",
            ],
        );

        // Create desktop shortcut for Agent
        let ps_agent_shortcut = format!(
            "$ws = New-Object -ComObject WScript.Shell; \
             $sc = $ws.CreateShortcut(\"$([Environment]::GetFolderPath('CommonDesktopDirectory'))\\ADBHUB-LINK Agent.lnk\"); \
             $sc.TargetPath = \"{}\"; \
             $sc.WorkingDirectory = \"{}\"; \
             $sc.Description = \"ADBHUB-LINK Mobile USB Passthrough Agent (Cliente)\"; \
             $sc.Save()",
            agent_dir.join("adbhub-agent.exe").to_string_lossy(),
            agent_dir.to_string_lossy()
        );
        run_command(
            "Creacion de acceso directo del Agente en el Escritorio",
            "powershell.exe",
            &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_agent_shortcut],
        );
    }

    if install_desktop {
        let _ = fs::create_dir_all(&desktop_dir);
        let _ = fs::create_dir_all(desktop_dir.join("data"));
        if let Some(src) = find_file_in_candidates(&desktop_candidates) {
            let dest = desktop_dir.join("adbhub-desktop.exe");
            if fs::copy(&src, &dest).is_ok() {
                println!("  [OK] Desktop desplegado en: {}", dest.display());
            }
        }

        // Create desktop shortcut via PowerShell
        let ps_shortcut_cmd = format!(
            "$ws = New-Object -ComObject WScript.Shell; \
             $sc = $ws.CreateShortcut(\"$([Environment]::GetFolderPath('CommonDesktopDirectory'))\\ADBHUB-LINK Desktop.lnk\"); \
             $sc.TargetPath = \"{}\"; \
             $sc.WorkingDirectory = \"{}\"; \
             $sc.Description = \"ADBHUB-LINK Professional Remote USB Passthrough for Mobile Repair\"; \
             $sc.Save()",
            desktop_dir.join("adbhub-desktop.exe").to_string_lossy(),
            desktop_dir.to_string_lossy()
        );
        run_command(
            "Creacion de acceso directo en el Escritorio",
            "powershell.exe",
            &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_shortcut_cmd],
        );
    }

    println!("\n[Paso 4/4] Verificacion Final del Sistema");
    println!("--------------------------------------------------------------------------------");
    let agent_exe = agent_dir.join("adbhub-agent.exe");
    if agent_exe.exists() {
        let _ = Command::new(&agent_exe).arg("doctor").status();
    }

    if install_desktop {
        let desktop_exe = desktop_dir.join("adbhub-desktop.exe");
        if desktop_exe.exists() {
            println!("\n  [+] Iniciando ADBHUB-LINK Desktop GUI (Ventana del Tecnico)...");
            let _ = Command::new(&desktop_exe).spawn();
        }
    } else if install_agent {
        let agent_exe = agent_dir.join("adbhub-agent.exe");
        if agent_exe.exists() {
            println!("\n  [+] Iniciando ADBHUB-LINK Agent GUI (Ventana del Cliente)...");
            let _ = Command::new(&agent_exe).spawn();
        }
    }

    println!("\n================================================================================");
    println!(" ¡INSTALACION COMPLETADA CON EXITO!");
    if install_desktop {
        println!(" - Se ha abierto la interfaz gráfica 'ADBHUB-LINK Desktop' en tu pantalla.");
        println!(" - Acceso directo 'ADBHUB-LINK Desktop' creado en tu Escritorio.");
    }
    if install_agent {
        println!(" - El Agente Host esta activo y listo para detectar telefonos conectados.");
    }
    println!("================================================================================");
    println!("\nPresione cualquier tecla para finalizar...");
    let _ = Command::new("cmd").args(["/c", "pause >nul"]).status();
}
