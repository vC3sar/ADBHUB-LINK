# 🚀 GUÍA INTERACTIVA DE CONEXIÓN REMOTA — ADBHUB-LINK

> **Tutorial rápido paso a paso para conectar la Estación del Cliente con la Estación del Técnico desde cualquier parte del mundo.**

---

```
  [ 👤 CLIENTE ]                                       [ 💻 TÉCNICO ]
  1. Conecta el teléfono por USB                       1. Abre adbhub-desktop.exe
  2. Abre la aplicación                                2. Pega el código en la barra superior
  3. Copia el CÓDIGO (16 car.) ─── WhatsApp / Chat ───> 3. Pulsa [CONECTAR AHORA]
                                                               │
  [ 📱 PANTALLA DEL TELÉFONO ]                                 │
  4. Salta aviso: "¿Permitir depuración por USB?" <─────────────┘
  5. Marca "Permitir siempre" y toca [ACEPTAR]
  ─────────────────────────────────────────────────────────────
  ⚡ ¡ENLACE ACTIVO! El técnico ya tiene control total del móvil.
```

---

## 🟢 CASO 1: El Cliente usa COMPUTADORA (Windows o Linux)

### 👤 Parte 1: Lo que hace el CLIENTE
1. Conecta el teléfono a la PC mediante su cable USB normal (con la pantalla desbloqueada).
2. Abre el programa:
   * **En Windows:** Abre la carpeta `Windows-Cliente` y da doble clic a **`adbhub-agent.exe`** (no requiere instalación).
   * **En Linux:** Abre una terminal en la carpeta y escribe:
     ```bash
     bash ADBHUB-LINK.sh
     ```
3. En el panel izquierdo verá un recuadro oscuro con su código de 16 caracteres:
   > Ejemplo: **`8924-BE29-19BC-30DB`**
4. Presiona el botón azul **"COPIAR CÓDIGO DE ESTACIÓN"** y se lo envía al técnico.

---

### 💻 Parte 2: Lo que hace el TÉCNICO
1. Abre **`adbhub-desktop.exe`** en su computadora.
2. En la barra superior de entrada (**ID / IP**):
   * Da clic en el botón **"Pegar"** (o presiona `Ctrl + V`).
   * Presiona **Enter** o da clic en **"CONECTAR AHORA"**.
3. El indicador cambiará a color ámbar: *"RESOLVIENDO TÚNEL..."*.

---

### 📱 Parte 3: Confirmación en el Teléfono (3 segundos)
1. En la pantalla del celular del cliente aparecerá la ventana oficial de Android:
   > *"¿Permitir depuración por USB desde esta computadora?"*
2. El cliente marca la casilla: **☑ Permitir siempre desde esta computadora** y toca **Aceptar**.
3. **¡Listo!** En la pantalla del técnico la barra cambiará a verde: **"CONECTADO"**.
   * Aparecerá la ficha técnica completa: Modelo, Versión de Android, Procesador y Batería.
   * El teléfono responderá inmediatamente en herramientas como **UnlockTool, Chimera, SamFirm, QFIL o en la pestaña "02 Consola ADB"**.

---

---

## 📱 CASO 2: El Cliente NO tiene computadora (Puente OTG Celular a Celular)

*Útil cuando el cliente solo tiene dos teléfonos (Celular A personal y Celular B a reparar).*

### 📲 Parte 1: Lo que hace el CLIENTE
1. En el **Celular A** descarga e instala **`adbhub-client.apk`**:
   * **Enlace directo MediaFire:** [Descargar adbhub-client.apk](https://www.mediafire.com/file/5vh10fh39e0vcwi/adbhub-client.apk/file)
   * *(El técnico también puede mostrar el código QR en pantalla en el módulo `05 Compartir APK (QR)` para escanearlo con la cámara).*
2. Conecta un cable **USB-C a USB-C** (o adaptador OTG) entre ambos celulares:
   * Una punta en el **Celular A** (Puente con la app abierta).
   * La otra punta en el **Celular B** (el teléfono a reparar).
3. En el Celular A saldrá el aviso: *"¿Permitir que ADBHUB-LINK OTG acceda al dispositivo USB?"* $\rightarrow$ Toca **Aceptar**.
4. Copia el **CÓDIGO DE EMPAREJAMIENTO MUNDIAL** que aparece en pantalla y se lo envía al técnico.

### 💻 Parte 2: Lo que hace el TÉCNICO
1. En su PC abre **`adbhub-desktop.exe`**.
2. Pega el código recibido y presiona **"CONECTAR AHORA"**.

### 📱 Parte 3: Confirmación
1. En la pantalla del **Celular B** salta el aviso: *"¿Permitir depuración por USB?"* $\rightarrow$ Toca **Aceptar**.
2. **¡Listo!** El técnico controla el Celular B a distancia a través del Celular A.

---

---

## 💬 Plantilla de mensaje para enviar a clientes por WhatsApp

### Opción A (Cliente con PC):
```text
¡Hola! Para realizar el servicio técnico remoto en tu teléfono:

1. Conecta tu celular con el cable USB a tu computadora (con la pantalla desbloqueada).
2. Abre el archivo "adbhub-agent.exe" (no necesita instalarse).
3. Presiona el botón "COPIAR CÓDIGO DE ESTACIÓN" y envíame el código que aparece.
4. En cuanto me conecte, en la pantalla de tu celular aparecerá un mensaje que dice "¿Permitir depuración por USB?". Marca la casilla "Permitir siempre" y toca ACEPTAR.

Con eso quedamos conectados para comenzar el trabajo.
```

### Opción B (Cliente sin PC - Celular a Celular):
```text
¡Hola! Para realizar el servicio técnico en tu teléfono sin necesidad de computadora:

1. Descarga e instala esta app en tu celular personal:
https://www.mediafire.com/file/5vh10fh39e0vcwi/adbhub-client.apk/file
2. Conecta un cable USB-C a USB-C entre ambos celulares.
3. Abre la app, acepta el permiso USB y envíame el código de 16 caracteres que aparece en pantalla.
4. En el celular a reparar, saldrá un aviso: toca "Permitir siempre" y ACEPTAR.
```

---

## ❓ Preguntas Frecuentes y Solución Rápida

* **¿Se necesita abrir puertos en el módem / router?**  
  *No.* La plataforma utiliza un túnel inverso que atraviesa routers y cortafuegos automáticamente.
* **¿Funciona si tengo VPN activada?**  
  *Sí.* El código de 16 caracteres incluye el puerto del relay mundial, por lo que se enlaza sin restricciones de red local.
* **¿Qué pasa si el cliente desconecta el cable por accidente?**  
  Basta con volver a enchufar el cable. La app del técnico cuenta con el botón *"Reconectar Enlace"* para reanudar la sesión en 1 segundo.

