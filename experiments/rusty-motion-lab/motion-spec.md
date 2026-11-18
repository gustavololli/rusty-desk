# Rusty Motion — contrato visual, etapa 03

Esta especificación permite revisar **cómo debe sentirse Rusty** sin acoplar
el motor de movimiento a `eframe`, `egui` ni a los sprites actuales.

## Contrato entre comportamiento y dibujo

El motor recibe `dt`, un estado semántico, una posición opcional del puntero
y las preferencias de movimiento. Devuelve una pose con valores continuos:

| Canal | Unidad | Qué controla |
| --- | --- | --- |
| `head_x`, `head_y` | píxeles lógicos | Desplazamiento de la cabeza. |
| `tilt_degrees` | grados | Inclinación lateral leve. |
| `scale_x`, `scale_y` | factor | Squash/stretch muy sutil. |
| `left_eye_x`, `left_eye_y` | píxeles lógicos | Mirada del ojo izquierdo. |
| `right_eye_x`, `right_eye_y` | píxeles lógicos | Mirada del ojo derecho, independiente. |
| `eye_open` | 0.045–1.16 | Apertura común de los párpados. |
| `mouth_open` | 0–1 | Apertura extra de la rejilla, sin reemplazarla. |

El renderer debe dibujar la cabeza amarilla, los ojos grandes ligeramente
cruzados y la rejilla **con la misma silueta, colores y proporciones** del
Rusty que ya elegimos. El motor de movimiento nunca decide colores o texturas.

## Estados actuales del experimento

| Estado | Cabeza | Ojos | Boca |
| --- | --- | --- | --- |
| Idle | Respiración de ~0.65 px, inclinación de <1° | Curiosidad y parpadeos espaciados | Rejilla neutral |
| Thinking | -4.5° y -1.1 px | Arriba y hacia un costado | Neutral |
| Listening | +4.2° | Atiende al interlocutor | Rejilla con pulsos leves |
| Focused | +1.2° | Entrecerrados | Neutral |
| Happy | -2.5° con rebote amortiguado | Expresión feliz | Rejilla levemente abierta |
| Error | Retroceso breve y -6° | Más abiertos, mirada lateral | Neutral |
| Sleep | -7.5° y respiración lenta | Cerrados casi por completo | Neutral |

Estos números son **puntos de partida editables**, no una promesa estética.
Si un movimiento se siente demasiado fuerte, debemos reducirlo; no agregar
efectos para disimularlo.

## Reglas que hacen sentir vivo al personaje

1. **Curiosidad pausada:** espera 2–6 segundos entre cambios de mirada; un
   generador pseudoaleatorio con semilla evita loops perfectamente repetidos.
2. **Parpadeo natural:** cierre y apertura en ~205 ms; ocasional segundo
   parpadeo, nunca un ciclo constante con el mismo intervalo.
3. **Ojos antes que cabeza:** los ojos reaccionan más rápido
   (resorte a 1.4× la frecuencia base, cabeza a 0.67×).
4. **No todo se mueve junto:** cada ojo tiene su resorte; la boca tiene otro;
   la cabeza tarda algo más en acomodarse.
5. **Transición con inercia:** los objetivos de pose cambian al recibir un
   evento, pero la pose visible converge gradualmente.
6. **Movimiento reducido:** no hay respiración, inclinación, persecución del
   mouse ni parpadeos automáticos. La expresión semántica se conserva.
7. **Modo local:** sin red, sin IA, sin reloj oculto ni tareas de fondo; todo
   avanza exclusivamente cuando el llamador entrega un `dt`.

## Coreografías experimentales (JavaScript)

| Evento simulado | Movimiento | Fin |
| --- | --- | --- |
| `noticeSomething('left'/'right')` | Ojos se desvían, cabeza los sigue con demora | Vuelve a reposo después de ~2.25 s |
| `setState('thinking')` | Cabeza inclinada y mirada arriba al costado | Al cambiar de estado |
| `prepareResponse()` | Devuelve la mirada al frente, cabeza hace pequeño gesto | Vuelve a reposo después de ~1.65 s |
| Movimiento combinado | Ojos a 1.4× y cabeza a 0.67× del resorte base | Siempre activo |
| `setTyping('user', true)` | Mira abajo hacia la caja de texto del usuario | `setTyping('user', false)` |
| `setTyping('model', true)` | Mira abajo, leve inclinación diferente | `setTyping('model', false)` o `prepareResponse()` |

En esta etapa los eventos de escritura **son simulados por controles del
laboratorio**. No hay escucha de teclado global, procesos, servicios, APIs ni
conexión con un modelo real. Si el usuario y el modelo escriben simultáneamente,
ambos flags se preservan y el movimiento mantiene prioridad de escritura.
Las expresiones `sleep` tienen prioridad visual sobre estos eventos.

La coreografía ya está implementada en `motion.mjs` y el HTML portátil;
`rusty_motion.rs` sigue siendo una primera traducción **de la etapa 02**:
estos nuevos eventos todavía no se portaron a Rust.

## Qué aprendemos del diseño de Grok Bot

El equipo de Grok Bot describe un archivo de especificaciones de movimiento
que permite ajustar directamente `springs`, `timing`, `distance`, `scale` y
`easing`. Un análisis independiente de su código describe clips Bézier,
interpolación geométrica y una máquina de estados. Este experimento implementa
**principios similares con código propio**; no incluye archivos ni código
extraídos de Grok Bot.

- [Fuente de diseño (xAI)](https://x.ai/bot/guides/designing-grok-bot-with-grok-bot)
- [Análisis independiente](https://github.com/ArvinYin1/grokbot-animation-anatomy)

## Lo que deliberadamente falta

- La pose 3/4 de cabeza (rotación aparente en profundidad). Ahora hay
  **inclinación y desplazamiento**, no un giro 3D real.
- Párpados independientes, cejas y pupilas: Rusty tiene ojos tipo cápsula
  oscuros; agregarlos sin perder el diseño requiere una decisión visual.
- Coreografías complejas multi-etapa (reconocer → pensar → responder) con
  prioridades de eventos y cancelación explícita.
- Eventos conectados con tareas, Pomodoro, voz o música.
- Un renderer nativo para egui.

**El siguiente paso no debe ser integrar**: primero revisar la fidelidad de
los ojos, hacer una pose 3/4 creíble y calibrar la secuencia de reacciones.
