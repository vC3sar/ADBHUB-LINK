# ADBHUB-LINK — Sistema Global de Diseño Multiplataforma (Design System)
**Versión:** 2.0.0  
**Autor:** Senior Product Designer, UI/UX Architect & Design Systems Engineer  
**Plataformas Objetivo:** Windows (Desktop & Agent), Linux (Client), Android (Client OTG)

---

## 1. Filosofía & Dirección Artística

El sistema de diseño de **ADBHUB-LINK** proyecta una estética de ingeniería de software comercial de primer nivel (inspirada en la precisión y sobriedad de **Linear, Raycast, GitHub Desktop y Vercel**).

### Principios Fundamentales
1. **Legibilidad Innegociable:** La información crítica nunca debe presentarse en tamaños diminutos (< 12px/sp). La jerarquía visual guía la mirada del operador y del cliente sin esfuerzo cognitivo.
2. **Consistencia Sistemática:** Cero valores mágicos en el código. Toda dimensión de espaciado, radio, grosor, color y escala tipográfica proviene estrictamente de tokens centralizados.
3. **Sobriedad & Rendimiento:** Cero glassmorphism recargado, degradados invasivos, sombras pesadas o animaciones innecesarias. Geometrías claras, bordes nítidos, alto contraste WCAG AA y rendimiento instantáneo a 60+ FPS.
4. **Adaptación Nativa:** El mismo ADN visual (paleta Dark Slate, tipografía sans-serif moderna, componentes estructurados) adaptado a las convenciones de interacción física de cada plataforma (ratón y teclado en Windows/Linux, gestos táctiles de 48dp+ en Android).

---

## 2. Design Tokens Centralizados

### 2.1 Paleta de Color (Semántica & Accesibilidad WCAG AA)

| Token | Hex / RGB | Rol en la Interfaz | Ratio de Contraste vs Fondo |
|---|---|---|---|
| `BG_CANVAS` | `#090D16` | Fondo principal de la ventana / lienzo base | N/A (Base) |
| `BG_SURFACE` | `#0D1320` | Barras de herramientas, sidebars y footers | 1.1:1 |
| `BG_CARD` | `#111827` | Superficie de tarjetas y paneles de trabajo | 1.3:1 |
| `BG_RECESSED` | `#070A11` | Cajas de código, terminales, inputs y fondos hundidos | 0.9:1 |
| `BORDER_SUBTLE` | `#1E293B` | Separadores estructurales y bordes pasivos | Contraste de borde 3.2:1 |
| `BORDER_MUTED` | `#334155` | Bordes en elementos interactivos hovered/activos | Contraste de borde 4.5:1 |
| `BORDER_ACCENT` | `#38BDF8` | Bordes activos, indicadores de foco y resaltes | 7.8:1 |
| `TEXT_PRIMARY` | `#F1F5F9` | Títulos principales, códigos de conexión y valores clave | **15.4:1 (Cumple WCAG AAA)** |
| `TEXT_SECONDARY` | `#94A3B8` | Subtítulos, etiquetas secundarias y descripciones | **7.2:1 (Cumple WCAG AA)** |
| `TEXT_MUTED` | `#64748B` | Información contextual auxiliar y placeholders | **4.6:1 (Cumple WCAG AA)** |
| `ACCENT_PRIMARY` | `#0EA5E9` | Botones de acción primaria, enlaces y focos | 6.8:1 |
| `ACCENT_DARK` | `#0369A1` | Estado pressed o fondo contenido de botones | 3.8:1 |
| `STATUS_SUCCESS` | `#10B981` | Enlace conectado, depuración autorizada y APK listo | 7.5:1 |
| `STATUS_WARNING` | `#F59E0B` | Esperando permiso USB/RSA y enlace conectando | 8.2:1 |
| `STATUS_ERROR` | `#EF4444` | Fallo de conexión o desconexión abrupta | 5.9:1 |

---

### 2.2 Escala Tipográfica Responsive Unificada

La escala tipográfica erradica el texto microscópico y define tamaños exactos y proporcionales:

| Token Semántico | Windows / Linux (px) | Android (sp) | Peso Tipográfico | Rol y Uso |
|---|---:|---:|---|---|
| `TYPO_DISPLAY` | 32 px | 30 sp | Bold (700) | Hero display, Código de emparejamiento XXXX |
| `TYPO_PAGE_TITLE` | 28 px | 26 sp | Bold (700) | Título de la estación / pantalla principal |
| `TYPO_SECTION_TITLE` | 22 px | 22 sp | Bold (700) | Encabezados de módulos y tarjetas maestras |
| `TYPO_SUBSECTION` | 18 px | 18 sp | SemiBold (600) | Títulos de secciones intermedias y diálogos |
| `TYPO_BODY_LARGE` | 16 px | 16 sp | Regular / Medium | Instrucciones clave y valores destacados |
| `TYPO_BODY` | 15 px | 15 sp | Regular (400) | Párrafos informativos, guías paso a paso |
| `TYPO_BODY_SMALL` | 14 px | 14 sp | Regular (400) | Descripciones secundarias y bitácoras |
| `TYPO_LABEL` | 13 px | 13 sp | Bold (600) | Badges de estado, labels de formulario |
| `TYPO_BUTTON` | 14 px | 14 sp | Bold (600) | Texto de botones interactivos |
| `TYPO_INPUT` | 15 px | 15 sp | Mono / Medium | Campos de texto, IP, puertos y consola |
| `TYPO_MONO_CODE` | 18 px | 18 sp | Bold (700) | Código de estación y valores de hardware |

> [!IMPORTANT]
> **Regla de Oro:** Ningún texto funcional puede ser inferior a 13 px / 13 sp. Queda terminantemente prohibido el uso de tamaños 9px, 9.5px, 10px o 10.5px que degraden la legibilidad del técnico o usuario final.

---

### 2.3 Escala de Espaciado (Grid de 4 Puntos)

Todos los paddings, margins, gaps e inner_margins respetan múltiplos de 4:

```
SPACE_1  = 4 px    // Separación mínima entre icono y texto
SPACE_2  = 8 px    // Espacio entre elementos en filas compactas
SPACE_3  = 12 px   // Padding interno de botones y badges
SPACE_4  = 16 px   // Padding estándar de tarjetas y contenedores
SPACE_5  = 20 px   // Espaciado vertical entre tarjetas maestras
SPACE_6  = 24 px   // Margen de sección o separación de paneles
SPACE_8  = 32 px   // Separación mayor entre bloques principales
SPACE_10 = 40 px   // Header spacing
SPACE_12 = 48 px   // Altura mínima de área táctil (Android touch targets)
```

---

### 2.4 Geometría, Bordes & Elevación

- **Radios de Borde (Corner Radius):**
  - Botones y Badges: `6 px`
  - Tarjetas y Contenedores: `8 px`
  - Paneles de Código y Modales: `10 px`
- **Grosor de Bordes (Border Thickness):**
  - Pasivo / Sutil: `1.0 px` (`BORDER_SUBTLE`)
  - Interactivo / Hover / Focus: `1.5 px` (`BORDER_ACCENT`)
  - Hero QR Card: `2.0 px` (`BORDER_ACCENT`)
- **Sombras:**
  - Desactivadas por defecto en favor de elevación semántica mediante luminosidad de superficie (`BG_CANVAS` $\rightarrow$ `BG_SURFACE` $\rightarrow$ `BG_CARD`).

---

## 3. Adaptaciones Específicas por Plataforma

### 3.1 Windows Desktop & Agent (`eframe` / `egui`)
- Soporte nativo para escalado DPI de Windows (100%, 125%, 150%, 200%).
- Cero ciclos recursivos de zoom (`set_zoom_factor` eliminado de repintados).
- Tipografía del sistema: `Segoe UI Variable`, `Segoe UI`, `Cascadia Code`.
- Scroll vertical elástico y cuadrículas adaptativas de 2 columnas que se contraen o expanden según la resolución.

### 3.2 Linux Client (`Tkinter` / `ADBHUB-LINK.sh`)
- Tipografía moderna: `Inter`, `Ubuntu`, `DejaVu Sans`, `Cascadia Code`.
- Widgets nativos con estilos ttk oscuros y contraste garantizado.
- Botones de acción directa con altura generosa (`pady=8`) y fuentes de 14–15 px.

### 3.3 Android Client (`com.adbhub.link` / `MainActivity.java`)
- Áreas táctiles con altura mínima de 48 dp (Google Material Accessibility).
- Unidades de densidad independiente (`dp`) para layouts y unidades escalables (`sp`) para texto.
- Tipografía `sans-serif-medium` y espaciado de caracteres (`letterSpacing = 0.05f`) para el código de 16 caracteres.
