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

## Integración futura

Este commit agrega **assets únicamente**. No modifica la apariencia actual
del avatar ni el motor de animaciones para que se pueda comparar con calma.
`AnimationController` puede seguir decidiendo el estado y el renderizador
podría elegir el frame correspondiente en el manifiesto.

El pack es un primer prototipo visual: vale la pena verlo a los tamaños
reales del modo mascota (64 / 80 / 104 px) antes de decidir si reemplaza
al Rusty esférico.
