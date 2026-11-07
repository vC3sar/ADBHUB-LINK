; ==============================================================================
; ADBHUB-LINK — Master Windows Setup Compiler Script (Inno Setup 6)
; Plataforma profesional de USB Passthrough remoto para reparación de teléfonos
; ==============================================================================

#define MyAppName "ADBHUB-LINK"
#define MyAppVersion "0.2.0"
#define MyAppPublisher "ADBHUB Technologies"
#define MyAppURL "https://github.com/adbhub-link"

[Setup]
AppId={{5E97D4F3-8821-4E64-9665-F6B616BC0872}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist
OutputBaseFilename=ADBHUB-LINK-Setup-v{#MyAppVersion}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[Types]
Name: "full"; Description: "Instalación Completa (Desktop Técnico + Agent Host)"
Name: "technician"; Description: "Estación del Técnico (Solo Desktop y VHCI Client)"
Name: "agent"; Description: "PC del Cliente / Taller (Solo Agent Host de Exportación)"
Name: "custom"; Description: "Personalizada"; Flags: iscustom

[Components]
Name: "desktop"; Description: "ADBHUB-LINK Desktop (Aplicación para Técnicos con Panel Samsung Odin)"; Types: full technician
Name: "agent"; Description: "ADBHUB-LINK Agent (Servicio de Detección y Exportación USB)"; Types: full agent

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Components: desktop
Name: "autostart_agent"; Description: "Iniciar ADBHUB-LINK Agent automáticamente con Windows"; Components: agent

[Files]
; Drivers empaquetados para instalación offline automática 100% silenciosa
Source: "..\packages\usbipd-win.msi"; DestDir: "{tmp}"; Flags: ignoreversion deleteafterinstall; Components: agent
Source: "..\packages\usbip-win2.exe"; DestDir: "{tmp}"; Flags: ignoreversion deleteafterinstall; Components: desktop
; Binarios compilados en target\release (o target\debug como fallback)
Source: "..\..\target\release\adbhub-desktop.exe"; DestDir: "{app}\Desktop"; DestName: "adbhub-desktop.exe"; Components: desktop; Flags: ignoreversion
Source: "..\..\target\release\adbhub-agent.exe"; DestDir: "{app}\Agent"; DestName: "adbhub-agent.exe"; Components: agent; Flags: ignoreversion
Source: "..\..\frontend\desktop\index.html"; DestDir: "{app}\Desktop\frontend"; Components: desktop; Flags: ignoreversion
Source: "..\..\docs\*"; DestDir: "{app}\Docs"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\ADBHUB-LINK Desktop"; Filename: "{app}\Desktop\adbhub-desktop.exe"; Components: desktop
Name: "{group}\ADBHUB-LINK Agent Doctor"; Filename: "{app}\Agent\adbhub-agent.exe"; Parameters: "doctor"; Components: agent
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\ADBHUB-LINK Desktop"; Filename: "{app}\Desktop\adbhub-desktop.exe"; Tasks: desktopicon; Components: desktop

[Run]
; Instalación 100% silenciosa y automática de drivers empaquetados
Filename: "msiexec.exe"; Parameters: "/i ""{tmp}\usbipd-win.msi"" /qn /norestart"; Flags: runhidden; Components: agent; Check: not IsUsbipdInstalled
Filename: "{tmp}\usbip-win2.exe"; Parameters: "/S /v/qn"; Flags: runhidden; Components: desktop; Check: not IsUsbipWin2Installed

; Configuración de Firewall para Agente y Desktop
Filename: "netsh.exe"; Parameters: "advfirewall firewall add rule name=""ADBHUB-LINK Host USB/IP"" dir=in action=allow protocol=TCP localport=3240"; Flags: runhidden; Components: agent
Filename: "netsh.exe"; Parameters: "advfirewall firewall add rule name=""ADBHUB-LINK QUIC Relay"" dir=out action=allow protocol=UDP remoteport=4433"; Flags: runhidden
Filename: "netsh.exe"; Parameters: "advfirewall firewall add rule name=""ADBHUB-LINK Desktop UI"" dir=in action=allow protocol=TCP localport=3420"; Flags: runhidden; Components: desktop

; Registro de tarea programada de inicio para Agent
Filename: "schtasks.exe"; Parameters: "/Create /TN ""ADBHUB-LINK-Agent"" /TR """"{app}\Agent\adbhub-agent.exe"" watch --mobile-only"" /SC ONLOGON /RL HIGHEST /F"; Flags: runhidden; Tasks: autostart_agent; Components: agent

; Lanzar Desktop al finalizar si se instaló
Filename: "{app}\Desktop\adbhub-desktop.exe"; Description: "{cm:LaunchProgram,ADBHUB-LINK Desktop}"; Flags: nowait postinstall skipifsilent; Components: desktop

[UninstallRun]
Filename: "schtasks.exe"; Parameters: "/Delete /TN ""ADBHUB-LINK-Agent"" /F"; Flags: runhidden
Filename: "netsh.exe"; Parameters: "advfirewall firewall delete rule name=""ADBHUB-LINK Host USB/IP"""; Flags: runhidden
Filename: "netsh.exe"; Parameters: "advfirewall firewall delete rule name=""ADBHUB-LINK QUIC Relay"""; Flags: runhidden
Filename: "netsh.exe"; Parameters: "advfirewall firewall delete rule name=""ADBHUB-LINK Desktop UI"""; Flags: runhidden

[Code]
function IsUsbipdInstalled(): Boolean;
begin
  Result := FileExists(ExpandConstant('{commonpf}\usbipd-win\usbipd.exe')) or
            FileExists(ExpandConstant('{sys}\drivers\VBoxUsb.sys'));
end;

function IsUsbipWin2Installed(): Boolean;
begin
  Result := FileExists(ExpandConstant('{commonpf}\usbip-win2\usbip.exe')) or
            FileExists(ExpandConstant('{sys}\drivers\usbip2_ude.sys'));
end;

