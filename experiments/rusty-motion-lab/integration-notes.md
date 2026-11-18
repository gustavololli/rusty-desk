# Integración de prueba · `feature/rusty-lifelike-motion`

Esta rama parte de `origin/main` actualizado (incluida la integración previa
de los sprites de 16 bits). Conserva toda la aplicación, las animaciones y los
PNG existentes. No se añade ningún servicio externo.

## Conexiones implementadas

- `src/ui/pet/motion.rs`: motor nativo de resortes para inclinación,
  desplazamiento, escala y dirección de la mirada.
- `AnimationController` incorpora la pose continua en cada `AnimationFrame`.
- `sprites.rs` aplica una transformación suave a **la textura original**.
  Las expresiones se siguen seleccionando desde el atlas 16-bit.
- Eventos de curiosidad y hover inician una distracción lateral.
- `AiReplyReady` inicia un gesto para mirar al frente antes de responder.
- El texto pendiente del chat y la espera de la respuesta IA activan una
  inclinación hacia abajo. Solo se lee estado de la aplicación, **nunca teclado
  global** ni otras ventanas.
- La cadencia solicitada pasa a ~60 FPS para interpolar los movimientos,
  respetando `reduce_animations`.
- Clics sobre Rusty en modo mascota: 1–2 = feliz, 3–5 seguidos = risa
  (celebración), 6 o más = fastidio temporal (ceño fruncido). La cuenta se
  reinicia después de 1.5 segundos sin clics. Dormido se despierta al primer
  toque, sin contar ese toque como una molestia.
- El clic simple ya no cambia de modo: **doble clic para abrir Isla**;
  el menú contextual con clic derecho sigue ofreciendo Isla y asistente.
- Al entrar en una sección realmente vacía (Tareas, Hábitos, Música, Chat
  nuevo, Markdown sin documento), Rusty hace **una** mirada curiosa y muestra
  una interrogación. No repite el gesto en cada frame; respeta la opción
  `auto_animations` y no interrumpe una reacción más importante.
- Organizador: solo reacciona al **resultado exitoso** de `Previsualizar`
  cuando no hay archivos. Focus sin temporizador y Settings no se interpretan
  erróneamente como pantallas vacías.

## Limitación visual conocida

Las imágenes actuales representan cabeza **y ojos en una sola textura**.
Por eso el movimiento de cabeza es realmente continuo, pero la mirada lateral
usa las variantes ya disponibles en el sprite sheet; todavía no hay pupilas
o párpados separados que se puedan animar a distinta velocidad. La mirada
hacia abajo se representa con una inclinación y desplazamiento de cabeza,
no con una nueva pose de ojos. No conviene falsear ese resultado.

Esta primera implementación permite juzgar el movimiento en el programa real
sin sacrificar los sprites existentes. Si la dirección convence, el siguiente
paso visual será separar los ojos en capas pixel-art manteniendo el diseño.

## Comprobación

- JS del laboratorio: `node --test experiments/rusty-motion-lab/*.test.mjs`.
- Rust (en una máquina con toolchain): `cargo fmt --check`, `cargo test` y
  `cargo run` para validar con el renderer real.

No se cambió `main`, no se hizo merge ni deploy.
