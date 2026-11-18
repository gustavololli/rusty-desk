# Rusty — robot amarillo en pixel art de 16 bits

Pack de sprites para experimentar con una nueva apariencia de Rusty, inspirado en
la cabeza amarilla con ojos oscuros ligeramente cruzados y boca tipo rejilla.

## Contenido

- `rusty-idle.png`: sprite maestro (96 × 96, RGBA).
- `rusty-spritesheet.png`: 4 columnas × 14 filas, 384 × 1344 píxeles.
- `frames/<estado>/00.png` a `03.png`: 56 frames individuales, todos de 96 × 96.
- `manifest.json`: nombres de estados, filas, frames y duración por fotograma.
- `preview.gif`: recorrido de algunas expresiones, sólo para inspección.

Los 14 nombres de estado coinciden con el enum `AnimationState` de Rust:
`Idle`, `Blink`, `LookAround`, `Sleep`, `WakeUp`, `Thinking`, `Listening`,
`Happy`, `Celebrate`, `Focused`, `Error`, `Glitch`, `PomodoroFinished` y
`ListeningMusic`.

## Regeneración

Requiere Node.js e ImageMagick. Desde la raíz del repositorio:

```bash
node tools/generate_rusty_sprites.mjs
```

El generador es determinista: misma forma, paleta y posición entre frames.
No llama a servicios de IA ni necesita API keys. Las imágenes PNG tienen
resolución **nativa** 96 × 96; al dibujarlas desde egui se recomienda usar
`TextureOptions::NEAREST` para conservar el pixelado.

## Integración

Este pack es el avatar de Rusty Desk. `AnimationController` elige el estado
y `src/ui/sprites.rs` elige el frame de la sprite sheet (8 FPS, filtro
nearest). No hace falta volver a generar los PNG para compilar: el sheet
está embebido en el binario.

Para regenerar los PNG, el GIF y el manifest:

```bash
node tools/generate_rusty_sprites.mjs
```
