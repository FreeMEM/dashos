# Bitácora: el resto de C2 (panel rápido)

Rama `panel-c2`, sacada de `revision`. Fase S10c, diseño en `docs/diseño/S10c-centro-de-control.md`.

## 2026-11-12: el modo de edición, guardado por el centro

Lo hecho:

- `usuario/src/panel_mosaicos.rs`, nuevo. La lista de mosaicos (oscuro, bloquear, volumen) y su orden. Lo usan el
  panel y el centro, así los dos aceptan lo mismo. Se guarda en una línea: los que se ven, en orden, con un espacio
  entre ellos. Los que faltan están quitados. Rechaza bytes que no son texto, mosaicos que no existen, repetidos,
  espacios de más, dos líneas y textos largos. Sin `alloc`: se prueba en el PC con
  `rustc --edition 2024 --test usuario/src/panel_mosaicos.rs` (6 pruebas, pasan).
- El centro guarda en `C/Prefs/panel`, al lado de la apariencia. Leer y guardar en Prefs pasa a dos funciones
  comunes (`leer_prefs`, `guardar_prefs`); la apariencia las usa también. Dos pedidos nuevos por su canal, solo de
  `panel` (como "modo oscuro"):
  - "panel leer": contesta el estado (0 guardado; 1 no hay; 2 no se entiende; -2 sin servicio de archivos) y la
    línea. Si no hay o está rota, la de serie.
  - "panel guardar <línea>": si la línea no vale, -1 y no toca el disco. Si vale, guarda, relee y lo dice.
  - A otro programa, -1, como cualquier pedido que no vale.
- El panel pide sus mosaicos al centro al arrancar, antes de hablar con el compositor. Si el centro no contesta (o
  no hay servicio de archivos), los de serie, y lo vuelve a pedir al abrir.
- El modo de edición: botón "Editar" (tonal) arriba a la derecha. En edición, el título es "Editar panel" y el botón
  es "Hecho" (lleno). Cada mosaico es una fila con tres botones de icono: subir, bajar y quitar. Debajo, "Quitados",
  con un "+" para volver a poner cada uno (va al final). Con "Hecho" se guarda si cambió. Si se cierra el panel con
  Super+A en edición, también.
- Los iconos (chevrones, cruz, más y menos) se dibujan con trazos suavizados dentro de `panel.rs`: la fuente no trae
  flechas ni el signo menos. El volumen tiene ahora dos botones redondos tonales, "−" y "+"; en el tope, apagados.
- El panel mide lo que su contenido. Al cambiar de altura pide otro tamaño a la capa, y al llegar el configure pinta
  en un búfer de esa altura. Son dos búferes sobre la misma memoria: uno se ve y el otro se rehace.
- Material 3: contenedor de superficie de fondo, contenedor alto en los mosaicos y filas, primario en lo encendido,
  contenedor secundario en los botones tonales. Los quitados, con borde y sin fondo.

Pruebas:

- Hostil: pide "panel leer" y "panel guardar" con uno que vale, vacío, uno que no existe, repetidos y sin espacio.
  A todo, -1. Línea nueva en `arranque_virt.txt`.
- Disco roto: `tools/disco_archivos.py` pone `C/Prefs/panel` con "volumen volumen nube". El centro dice "un mosaico
  repetido; se queda el de serie" y el panel arranca con los de serie (estado 2). La basura y los demás casos van en
  las pruebas de PC del módulo.
- Virt (`tools/prueba_uefi.sh`): tras el volumen, Editar, quitar Bloquear, Hecho. El centro lo guarda y lo relee; el
  panel dice que se ven oscuro y volumen. Otra vez Editar, poner Bloquear, subirlo y Hecho: como estaba. Al final,
  Super+A. La comprobación del disco exige `C/Prefs/panel` con "oscuro bloquear volumen". El panel dice dónde está
  cada botón (una línea por fila, numerada: una línea larga se podía mezclar con la de otro programa).
- Capturas nuevas con `CAPTURA_CENTRO`: `.panel-edicion.ppm` (en edición, Bloquear quitado) y `.panel-sin.ppm` (sin
  Bloquear).

Visto por el camino, sin tocar (no es de esta rama):

- En la prueba de base (antes de cambiar nada) faltó una vez "[ficheros] A15 sujetar sin soltar: bien". Con los
  cambios, las cuatro BIEN a la primera. Parece de tiempos.
- El compositor dice "error de protocolo de centro: número de objeto nuevo malo" al poner el modo desde el panel.
  Ya salía antes de esta rama. El modo se pone igual; es la conexión de un momento de `aplicar_modo`.

Pendiente:

- Arrastrar para ordenar (DMS lo hace así). Aquí, subir y bajar con botones.
- El teclado en el panel (Tab y flechas): no lo maneja.
- Las pruebas de PC del módulo no están en `make prueba`: tocaría `kernel/Makefile`.
- El mosaico "Salir" lo añade otro agente: basta con meterlo en `MOSAICOS` de `panel_mosaicos.rs` y en `pintar`.
