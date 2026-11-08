# AUDITORÍA INICIAL UI/UX — ECOSISTEMA ADBHUB-LINK
**Fecha:** 2026-11-08  
**Auditor:** Senior Product Designer & Design Systems Architect  
**Estado:** Diagnóstico Pre-Implementación

---

## 1. Resumen Ejecutivo del Diagnóstico

Tras la inspección profunda del código fuente en los 4 componentes visuales del ecosistema (**Windows Desktop**, **Windows Agent**, **Linux Client** y **Android Client**), se han identificado severas discrepancias tipográficas, saturación de elementos en pantalla, valores dispersos no estandarizados y problemas de jerarquía visual.

A continuación se detalla el desglose por severidad y plataforma.

---

## 2. Hallazgos Críticos y Severidad

| ID | Hallazgo | Severidad | Plataformas Afectadas | Archivos Afectados |
|---|---|---|---|---|
| **ISS-01** | **Micro-tipografía Ilegible:** Presencia masiva de textos con tamaños entre 9.0 px y 11.5 px (subtítulos, descripciones, badges, botones secundarios y telemetría). | **CRÍTICA** | Windows Desktop, Windows Agent, Linux, Android | `apps/adbhub-desktop/src/gui.rs`<br>`apps/adbhub-agent/src/gui.rs`<br>`DIST-RELEASES/Linux-Cliente/adbhub-client.py`<br>`apps/adbhub-android/src/com/adbhub/link/MainActivity.java` |
| **ISS-02** | **Falta de Tokens Centralizados:** Tamaños de fuente, paddings y espaciados codificados como números mágicos dispersos (`size(10.5)`, `dp(6)`, `pady=2`). | **ALTA** | Todas las plataformas | Todo el codebase de GUI |
| **ISS-03** | **Títulos de Pantalla Desproporcionados / Diminutos:** El título principal de la ventana tenía 14–17 px, careciendo del peso e impacto visual de un software comercial moderno (debería ser 28–32 px). | **ALTA** | Todas las plataformas | `gui.rs`, `adbhub-client.py`, `MainActivity.java` |
| **ISS-04** | **Botones con Áreas Táctiles y de Clic Insuficientes:** Botones con paddings muy reducidos (ej. `small_button` de 11px o botones móviles con menos de 40dp de altura). | **ALTA** | Windows, Linux, Android | `apps/adbhub-agent/src/gui.rs`<br>`MainActivity.java` |
| **ISS-05** | **Espaciados Inconsistentes:** Se mezclaban espaciados aleatorios (3.0, 5.0, 7.0, 9.5) rompiendo el ritmo vertical del grid de 4/8 puntos. | **MEDIA** | Windows Desktop, Agent, Android | `gui.rs`, `MainActivity.java` |
| **ISS-06** | **Inconsistencia de Estados Vacíos (Empty States):** La ausencia de dispositivos USB mostraba mensajes en color apagado sin una llamada a la acción clara ni ilustración/icono representativo. | **MEDIA** | Windows Desktop, Agent | `apps/adbhub-desktop/src/gui.rs`<br>`apps/adbhub-agent/src/gui.rs` |

---

## 3. Detalle por Plataforma

### 3.1 Windows Desktop (`apps/adbhub-desktop/src/gui.rs`)
- **Títulos:** Título de sección "MÓDULOS DE TRABAJO" fijado en `9.5px` (prácticamente invisible en monitores 1080p y 1440p).
- **Subtítulos de Navegación:** `size(9.5)` y `size(10.0)`.
- **Hero Input:** La barra superior para ingresar el ID de 16 caracteres requiere un tamaño tipográfico de `TYPO_INPUT` (15–16 px) con botones de acción ("CONECTAR AHORA") en `14–15 px bold`.
- **Ficha Técnica & Telemetría:** Atributos como ABI, SDK, Build ID y SoC Platform estaban en `10.5px`, obligando al técnico a forzar la vista.

### 3.2 Windows Agent (`apps/adbhub-agent/src/gui.rs`)
- **Encabezado:** Título principal en `15.5px` y subtítulo del host en `10.5px`.
- **Tarjetas de Estado USB:** Mensajes como "ESPERANDO CABLE USB" y checklist de pasos en `10.0–11.5px`.
- **Botones de Conexión:** Botones "Actualizar USB" y "Aislar Red" en `11.5px`.

### 3.3 Linux Client (`DIST-RELEASES/Linux-Cliente/adbhub-client.py`)
- **Tipografía Base:** `f_badge` en 9px, `f_body` en 9px, `f_header` en 10px. Deben migrarse a la escala estándar: Título 26–28px, Secciones 20–22px, Cuerpo 15px, Botones 14px bold.
- **Botones Tkinter:** Los botones de "COPIAR CÓDIGO DE ESTACIÓN" y "RECONECTAR PUENTE" tenían `font=("DejaVu Sans", 9, "bold")`.

### 3.4 Android Client (`apps/adbhub-android/src/com/adbhub/link/MainActivity.java`)
- **Header:** `tvTitle` en 17.5sp, `tvSub` en 10sp.
- **Código Hero:** Excelente visibilidad del código, pero las etiquetas explicativas y el checklist estaban entre 10sp y 11.5sp.
- **Botones:** `btnActionUsb` en 12.5sp y paddings que no garantizaban los 48dp mínimos de área táctil.

---

## 4. Plan de Acción Inmediato

1. Crear e inyectar el módulo de constantes `DesignTokens` con los colores semánticos, espaciados en múltiplos de 4 y la escala tipográfica estandarizada (`TYPO_DISPLAY`, `TYPO_PAGE_TITLE`, `TYPO_SECTION_TITLE`, `TYPO_SUBSECTION`, `TYPO_BODY`, `TYPO_BUTTON`, `TYPO_LABEL`, `TYPO_MONO_CODE`).
2. Refactorizar completamente `apps/adbhub-desktop/src/gui.rs` para eliminar todo texto inferior a 13px y elevar los títulos principales a 22–28px.
3. Refactorizar `apps/adbhub-agent/src/gui.rs` bajo la misma identidad visual y escala tipográfica.
4. Refactorizar `DIST-RELEASES/Linux-Cliente/adbhub-client.py` y `adbhub-client.py` aplicando los tokens del Design System en Tkinter.
5. Refactorizar `apps/adbhub-android/src/com/adbhub/link/MainActivity.java` elevando todos los `sp` a la escala 14–30sp con touch targets de 48dp+.
6. Recompilar los binarios de Windows, Linux y el APK de Android, verificando en dispositivo físico y PC.

---

## 5. Auditoría Posterior y Matriz de Resolución (Post-Implementación)

### 5.1 Matriz de Estado de Hallazgos

| ID | Hallazgo Original | Estado | Resolución Técnica Implementada |
|---|---|---|---|
| **ISS-01** | Micro-tipografía Ilegible (< 12 px) | **RESUELTO** | Se erradicaron todos los tamaños < 13 px en los 4 codebases. La escala base ahora oscila entre `TYPO_LABEL` (13.0 px / 14 sp) y `TYPO_DISPLAY` (32.0 px / 28 sp). |
| **ISS-02** | Números mágicos dispersos | **RESUELTO** | Implementación de `DESIGN_SYSTEM.md` y centralización de tokens (`TYPO_*`, `SPACE_*`, `RADIUS_*`) en Rust, Tkinter y Java. |
| **ISS-03** | Títulos de pantalla diminutos | **RESUELTO** | Elevados a `TYPO_PAGE_TITLE` (28 px / 22 sp) y `TYPO_SECTION_TITLE` (22 px / 19 sp). |
| **ISS-04** | Botones con áreas de clic insuficientes | **RESUELTO** | Alturas mínimas de 32–36 px en desktop y `minHeight(48dp)` con padding `16dp x 12dp` en Android (WCAG AA). |
| **ISS-05** | Ritmo y espaciados rotos | **RESUELTO** | Grid estricto basado en múltiplos de 4 (`SPACE_XS`: 4px, `SPACE_SM`: 8px, `SPACE_MD`: 16px, `SPACE_LG`: 24px, `SPACE_XL`: 32px). |
| **ISS-06** | Inconsistencia en Empty States | **RESUELTO** | Empty states con tarjeta visual elevada, tipografía 18–22 px, llamada a la acción clara y vector minimalista de dispositivo. |

### 5.2 Comparativa Antes vs. Después de Tipografía

| Elemento Visual | Tamaño Previo (Auditado) | Tamaño Nuevo (Design System) | Cumplimiento WCAG |
|---|---|---|---|
| **Título Principal (Windows / Linux)** | 14.0 – 15.5 px | **28.0 px** (`TYPO_PAGE_TITLE`) | AAA |
| **Título Principal (Android)** | 17.5 sp | **22.0 – 26.0 sp** | AAA |
| **Títulos de Sección** | 10.5 – 12.0 px | **22.0 px** (`TYPO_SECTION_TITLE`) | AAA |
| **Subtítulos y Headers** | 9.5 – 10.5 px | **18.0 px** (`TYPO_SUBSECTION`) | AAA |
| **Cuerpo de Texto Principal** | 9.0 – 11.0 px | **15.0 px** (`TYPO_BODY`) | AAA |
| **Texto Secundario / Metadatos** | 8.0 – 9.5 px | **14.0 px** (`TYPO_BODY_SMALL`) | AA |
| **Etiquetas / Badges** | 9.0 – 10.0 px | **13.0 px / 13.5 sp** (`TYPO_LABEL`) | AA |
| **Botones Principales** | 11.0 – 11.5 px (25-30px h) | **14.5 px / 15 sp (36-48px h)** | AAA |
| **Hero Monospace (Código Estación)** | 15.0 – 18.5 px | **20.0 – 28.0 px** (`TYPO_MONO_CODE`) | AAA |

### 5.3 Módulos Nuevos Añadidos

- **Módulo 05: Compartir APK con Cliente (QR Vectorial Nativo):**
  - Generación de código QR embebido mediante la biblioteca `qrcode` (sin dependencias externas de red).
  - Enlace oficial a MediaFire: `https://www.mediafire.com/file/5vh10fh39e0vcwi/adbhub-client.apk/file`.
  - Botón integrado para copiar enlace directo al portapapeles.

### 5.4 Control de Compilación y Verificación

1. **Windows Desktop (`adbhub-desktop.exe`):** Compilación release sin advertencias, tamaño 18.6 MB, verificado con `cargo check` y `cargo build --release`.
2. **Windows Agent (`adbhub-agent.exe`):** Compilación release sin advertencias, tamaño 16.2 MB, verificado con `cargo check` y `cargo build --release`.
3. **Linux Client (`adbhub-client.py`):** Tkinter DPI-aware, escalado autoadaptativo con clamps `[0.95x - 1.30x]`, sincronizado en raíz y `DIST-RELEASES`.
4. **Android Client (`adbhub-client.apk`):** Compilado con AAPT2 + JDK 21 + D8 + Zipalign + Apksigner (v2 & v3 schemes). Instalado exitosamente en terminal físico TECNO KM5 (Android 14).

