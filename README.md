<div align="center">

# 🤖 Rusty Desk

### Tu pequeño compañero de escritorio. Con píxeles, carácter y alguna que otra cara de sueño.

**Un asistente de escritorio hecho en Rust, con alma de videojuego de 16 bits.**

![Rusty en movimiento](assets/rusty-16bit/preview.gif)

![Rust](https://img.shields.io/badge/Rust-2021-b74121?style=flat-square&logo=rust&logoColor=white)
![egui](https://img.shields.io/badge/UI-egui%20%2B%20eframe-273955?style=flat-square)
![Windows](https://img.shields.io/badge/Windows-en%20desarrollo-226bb4?style=flat-square)
![Linux](https://img.shields.io/badge/Linux-en%20desarrollo-f0a015?style=flat-square)
![Local first](https://img.shields.io/badge/Datos-SQLite%20local-31b59e?style=flat-square)

</div>

---

Rusty no quiere ser otra ventana que abrís, usás y olvidás. Quiere acompañarte mientras trabajás: te ayuda a organizar tareas, concentrarte, escuchar música y conversar con modelos de IA. Y, de vez en cuando, **te mira de reojo, cabecea o reacciona si lo molestás**.

Tiene una estética retro de 16 bits, un marco de acero oxidado y tres formas de vivir en tu escritorio.

## 🕹️ Tres formas de tener a Rusty cerca

| Modo | Qué hace |
| --- | --- |
| **Ventana principal** | Todas las herramientas, el chat y la configuración. |
| **Isla** | Vista compacta para consultar el estado, controlar el enfoque y acceder a la música. |
| **Notch** | Se esconde contra el borde de la pantalla y deja a Rusty asomándose; puede reaparecer al pasar el mouse, si activás esa opción. |

Los marcos se dibujan con una **textura 9-slice**: las esquinas pixeladas no se estiran y los bordes acompañan el tamaño de cada modo.

## ✨ Qué podés hacer

- **💬 Charlar con IA.** Configurá un proveedor OpenAI-compatible o Gemini. También podés usar un modelo local mediante [LM Studio](https://lmstudio.ai/), sin depender de una API de pago.
- **📝 Organizar tareas y hábitos.** Creá, completá y seguí tus pendientes; registrá hábitos cotidianos.
- **⏳ Entrar en foco.** Temporizador Pomodoro, pausas y tareas vinculadas a las sesiones de concentración.
- **🎵 Escuchar música.** Reproducción de URLs de audio y listas `.m3u` / `.m3u8`, con controles también en la isla. La disponibilidad de formatos y controles depende del sistema.
- **🔊 Escuchar efectos de 16 bits.** Once señales sonoras originales para tareas, mensajes, enfoque y errores: estéreo a 44,1 kHz, notas superpuestas y ecos discretos. Podés desactivarlas o reemplazarlas con tus propios archivos WAV.
- **🗂️ Ordenar archivos.** Herramientas de organización del escritorio y conversión a Markdown.
- **🔎 Consultar información.** Integraciones opcionales para búsqueda web, lectura de páginas, clima y tipos de cambio.
- **🧠 Dreaming (experimental).** Un modelo configurable, independiente del asistente principal, puede analizar historial de conversaciones y generar recuerdos almacenados localmente en SQLite. Está desactivado de forma predeterminada y requiere configuración explícita.
- **🤖 Ver a Rusty reaccionar.** Animaciones de mirada, movimiento, sueño, alegría, errores, reconexiones y clics repetidos, sin consumir tokens para animarse.

> **Privacidad:** las tareas, conversaciones y datos persistidos se guardan localmente en SQLite. Si habilitás una IA o herramienta remota, las solicitudes correspondientes sí pueden salir de tu computadora. Dreaming no envía historial a servicios remotos salvo que lo autorices expresamente.

## 🚀 Ejecutarlo

### Requisitos

- [Rust y Cargo](https://rustup.rs/) instalados.
- Windows o Linux con entorno de escritorio compatible.
- Conexión a Internet solo para descargar dependencias y utilizar proveedores en línea; un servidor local de IA es opcional.

```bash
git clone https://github.com/gustavololli/rusty-desk.git
cd rusty-desk
cargo run --release
```

Para desarrollar o ejecutar pruebas:

```bash
cargo fmt --check
cargo test
cargo check
```

> **Estado del proyecto:** desarrollo activo (v0.3.0). Las funciones de integración con el sistema operativo pueden comportarse de manera distinta en Windows y Linux. Si algo falla, abrí un issue con el sistema, la versión y los pasos para reproducirlo.

### Conectar LM Studio (ejemplo)

1. Abrí LM Studio, cargá un modelo y activá su servidor local.
2. En la configuración de Rusty, seleccioná el proveedor **OpenAI-compatible**.
3. Indicá la URL base `http://127.0.0.1:1234/v1`.
4. Elegí el identificador exacto del modelo que tenés cargado.

El endpoint de chat utilizado es `/v1/chat/completions`. No necesitás una clave real para un servidor local que no exige autenticación.

### Dreaming: memoria separada

La idea es simple: **el modelo que conversa no tiene que procesar sus propios recuerdos**. Podés asignar un modelo diferente, local o por API, para el proceso Dreaming. La memoria se guarda en SQLite y el asistente puede recuperar contexto relevante cuando corresponde.

Dreaming es opcional; si lo desactivás, el resto de Rusty sigue funcionando.

## ⌨️ Algunos comandos

Escribí `/ayuda` dentro del chat para consultar la lista completa.

| Comando | Acción |
| --- | --- |
| `/tarea Preparar presentación` | Crea una tarea. |
| `/tareas` | Muestra las tareas. |
| `/pomodoro 25` | Inicia un bloque de concentración. |
| `/pausa` | Pausa el Pomodoro. |
| `/agua` | Registra agua. |
| `/clima Córdoba` | Consulta el clima. |
| `/buscar Rust egui` | Busca en la web, si configuraste un proveedor. |

## 🛠️ Herramientas del asistente

Rusty ejecuta acciones de verdad. El modelo propone llamadas; Rust valida el nombre, los parámetros, el riesgo y si la herramienta está disponible. El modelo no corre código ni puede confirmarse a sí mismo.

| Riesgo | Ejemplos | Confirmación |
| --- | --- | --- |
| Low | Leer tareas, estado del Pomodoro, play/pausa/volumen del reproductor, clima | No |
| Medium | Crear o editar tareas, registrar hábitos, guardar un recuerdo | No |
| High | Borrar una tarea, mover o deshacer archivos del escritorio | Sí, en el chat |

El volumen que cambia es el del reproductor de Rusty (la sesión de `ffplay` / Windows), no el volumen general del sistema ni los efectos de sonido. Pausar música no está disponible en Windows: ahí se puede detener. No se inventan playlists ni estaciones.

Proveedores:

| Proveedor | Tool calling |
| --- | --- |
| Gemini | Nativo (`functionDeclarations` / `functionCall` / `functionResponse`) |
| OpenAI, OpenRouter, Groq | Nativo (`tools` / `tool_calls` en chat completions) |
| Ollama, llama.cpp, LM Studio y otros compatibles | Se prueba. Si el servidor rechaza `tools`, Rusty lo dice y no interpreta el texto como una llamada |
| Mock local | Devuelve llamadas estructuradas sin red |

En Configuración se puede dejar en automático, forzar o desactivar. Un turno admite varias llamadas en paralelo y varias rondas seguidas, con un tope de 6 rondas y 75 segundos. Se puede cancelar desde el chat.

La búsqueda web sigue usando el motor existente (Tavily, luego LangSearch u otro proveedor configurado). Esos resultados son datos no confiables: no cambian permisos ni instrucciones.

## 🧱 Cómo está armado

```text
src/
├── ui/            # egui, isla, notch, mascota, sprites y marcos 9-slice
├── ai/            # Proveedores de modelos y tool calling
├── agent.rs       # Bucle de herramientas, límites y persona
├── dreaming.rs    # Procesamiento independiente de memoria
├── persistence/   # SQLite local
├── platform.rs    # Integraciones según sistema operativo
├── music.rs       # Reproducción y controles
├── tools/         # Comandos y herramientas
└── main.rs        # Arranque de la aplicación

assets/
├── rusty-16bit/   # Sprites y animaciones de Rusty
└── frames/        # Marco pixelado 9-slice
```

Rusty está construido sobre **Rust + egui/eframe**, con **SQLite** para persistencia. La interfaz y las animaciones son locales: no necesitan una conexión a IA para moverse.

### 🔊 Identidad sonora

Los efectos de Rusty tienen un estilo inspirado en las consolas de 16 bits. Se sintetizan localmente y se guardan en caché como archivos **WAV estéreo PCM de 16 bits / 44.100 Hz**; no requieren servicios externos ni generación por IA. Los archivos personalizados con el mismo nombre que los efectos internos tienen prioridad y se buscan en la carpeta `sounds` del directorio de configuración de la aplicación.

## 🧪 Contribuir

¿Encontraste un bug, querés aportar un sprite o mejorar el soporte para tu sistema operativo? Los [issues](https://github.com/gustavololli/rusty-desk/issues) y pull requests son bienvenidos.

Antes de enviar cambios, ejecutá `cargo fmt --check`, `cargo test` y `cargo check` en tu plataforma. No incluyas claves de API ni datos privados de tu base local.

---

<div align="center">

**Hecho con Rust, café y cariño por los videojuegos viejos.** 🧡

*Rusty puede dormirse. Tus ideas, no.*

</div>
