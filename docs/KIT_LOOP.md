# Bucle del kit de interfaz (S11)

Orden del usuario (2026-11-09): tras el compositor Wayland (E1, `docs/WAYLAND_LOOP.md`), el kit de interfaz en Rust;
el sonido (S19) detrás. Diseño: S11 (el modelo de MUI, nativo: clases, atributos, avisos entre objetos, grupos que se
colocan solos, ventanas que se adaptan a la fuente) y S10 (aspecto Material 3: esquinas redondeadas, superficies con
tono, tipografía y espaciado de Material; claro y oscuro). El kit dibuja en superficies de Wayland (S9).

## Dónde está

- Copia de trabajo `.worktrees/kit`, rama `kit`, desde la del compositor (`wayland`).
- `kit/`: la biblioteca (no_std con alloc), probada en el PC dibujando en memoria.

## Decisiones tomadas por defecto (cambiables)

- **Fuente: Roboto** (la de Material 3; licencia Apache 2.0, se puede llevar en el repositorio).
- **Rasterizar TrueType con `fontdue`** (MIT o Apache 2.0, sin std), en vez de escribir un rasterizador: es lo más
  grande del kit y está resuelto.
- **Rust primero** (S11 dice Dash para las aplicaciones; Dash nativo para ARM es A7, que va aparte). La API del kit
  se piensa para que los builtins de MUI de Dash la puedan usar después tal cual.

## Pasos

| Paso | Qué | Prueba | Estado |
|------|-----|--------|--------|
| K1 | Lienzo: rectángulos, esquinas redondeadas, bordes, mezcla con alfa, recortes | Píxeles en el PC | hecho |
| K2 | Texto: Roboto con `fontdue`, caché de glifos, medir y dibujar líneas en UTF-8 | Medidas y píxeles; ñ, acentos | |
| K3 | Objetos al modo MUI: clases, atributos (poner y leer), avisos entre objetos (`notify`) | Un deslizador que mueve un texto por aviso | |
| K4 | Grupos que se colocan solos: horizontal y vertical, mínimo, máximo y pesos, espaciado de Material | Medidas al píxel en varios tamaños | |
| K5 | Controles: texto, botón, casilla, campo de texto, deslizador, ciclo, lista | Cada uno, dibujado y con su entrada | |
| K6 | Tema Material 3: colores de un tono base (claro y oscuro), estados (encima, pulsado, foco) | Colores de los estados | |
| K7 | Ventana de Wayland: la superficie, el tamaño que pide el compositor, cuadros a su ritmo, ratón y teclado al objeto con foco | En QEMU, una aplicación de prueba con captura comparada | |

## Bitácora

### 2026-11-09

- K1: `kit/src/lienzo.rs`. Píxeles ARGB8888 premultiplicados (los de `wl_shm`); colores dados sin premultiplicar
  y mezclados encima. Rectángulos, esquinas redondeadas suavizadas (4 × 4 muestras en cada píxel de la curva),
  bordes hacia dentro, recortes apilados. Pruebas: rellenar y mezclar (rojo a medias bajo negro a medias), alfa sobre
  transparente, esquinas (fuera, dentro, un píxel que cruza la curva a medias, simétrico en las cuatro), borde y
  recorte. La primera fórmula de la cobertura estaba mal; se rehízo en octavos de píxel.

