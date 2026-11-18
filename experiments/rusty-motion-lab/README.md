# Rusty Motion Lab — etapa 03

Experimento **aislado, local y sin integración** para estudiar el tipo de
movimiento que hace expresivo a Grok Bot manteniendo la **imagen reconocible**
del Rusty amarillo en pixel art de 16 bits.

## Qué se creó

| Archivo | Responsabilidad |
| --- | --- |
| `motion.mjs` | Motor puro: estados, resortes amortiguados, mirada independiente, parpadeos, distracciones y eventos de escritura. |
| `character.mjs` | Dibujo procedural de Rusty a 96 × 96 píxeles: cabeza dorada, dos ojos oscuros ligeramente cruzados y boca de rejilla. |
| `index.html` | Laboratorio visual interactivo para experimentar sin tocar la aplicación. |
| `motion.test.mjs` | Pruebas automáticas del motor, sin bibliotecas externas. |
| `character.test.mjs` | Smoke test sin navegador del renderer procedural para todos los estados. |
| `build-preview.mjs` | Genera una vista HTML que funciona sin servidor ni módulos externos. |
| `preview-standalone.html` | Vista portátil generada, lista para abrir directamente. |
| `rusty_motion.rs` | Traducción experimental del motor de movimiento a Rust puro, aún desconectada de la aplicación. |
| `motion-spec.md` | Contrato de poses, estados, eventos y límites de la etapa 03. |

No utiliza los archivos de `assets/rusty-16bit/`, no modifica los sprites,
no integra nada con `src/`, no necesita IA, conexión a Internet ni servicios.

## Cómo probarlo

Desde la carpeta del laboratorio:

```bash
python3 -m http.server 8765 --bind 127.0.0.1
```

Abrir [http://127.0.0.1:8765/](http://127.0.0.1:8765/).

Alternativamente, ejecutar `node build-preview.mjs` y abrir
`preview-standalone.html` directamente: no requiere servidor.

Para verificar la física y las transiciones:

```bash
node --test *.test.mjs
```

El módulo Rust es independiente. En un entorno que tenga `rustc`, puede
comprobarse por separado mediante:

```bash
rustc --edition=2021 --test rusty_motion.rs -o /tmp/rusty_motion_tests
/tmp/rusty_motion_tests
```

La traducción a Rust no se integra en ninguna ruta de compilación de la app.

## Qué conviene observar

1. Dejar Rusty **Idle** durante 15 segundos: pequeñas miradas, respiración y parpadeos no periódicos.
2. Mover el cursor alrededor del robot: ojos con movimiento independiente y cabeza que acompaña suavemente.
3. Pasar a **Thinking**, **Focused**, **Listening**, **Happy** y **Sleep**: no hay cambios bruscos de fotograma.
4. Modificar velocidad e intensidad para juzgar cuándo Rusty resulta vivo y cuándo se vuelve molesto.
5. Activar **Reducir animaciones** para comprobar que las expresiones siguen siendo legibles en una pose estática.
6. Pulsar **Mirar a un lado** (izquierda/derecha): los ojos deben reaccionar antes que la cabeza y ambos regresar.
7. Pulsar **Antes de responder**: Rusty te mira de frente y se acomoda suavemente.
8. Escribir en la caja de texto: Rusty baja la mirada; al salir de ella, vuelve a su actividad previa.
9. Activar **Modelo escribe**: simula la misma atención hacia abajo sin requerir un modelo ni servicio.

## Referencia técnica (no copiada)

El equipo de Grok Bot describe un *animation spec* real que puede calibrarse
con `timing`, `springs`, `distance`, `scale` y `easing` en un playground local:

- [Designing Grok Bot with Grok Bot — Motion God](https://x.ai/bot/guides/designing-grok-bot-with-grok-bot)
- [GrokBot Animation Anatomy](https://github.com/ArvinYin1/grokbot-animation-anatomy): análisis **no oficial** del motor con SVG procedural, keyframes Bézier y amortiguación tipo spring.

No se copió código ni se incluyeron recursos de estos proyectos. Este laboratorio
implementa una aproximación original y pequeña de esos principios. Para evitar
el suavizado de SVG en el contorno pixelado, dibuja el personaje procedimentalmente
en un Canvas de **96 × 96** y lo presenta con `image-rendering: pixelated`.

## Lo que NO se ha hecho

- No se ha modificado ninguna línea de la aplicación Rust.
- No se han alterado ni reemplazado PNG ni la sprite sheet existente.
- No se ha creado integración con `AnimationController` o `egui`.
- No se han agregado dependencias a `Cargo.toml`.
- No se ha hecho commit, push, PR, merge ni deploy.

## Decisión pendiente antes de integrar

El usuario aprobó la dirección visual del laboratorio. Ya hay una primera
versión del motor en Rust y una coreografía de atención experimental en JS.
**La coreografía nueva aún no está portada a Rust.** Faltan comparación visual
fina, giro de cabeza creíble y calibración de eventos antes de integrar nada
con `egui`.
