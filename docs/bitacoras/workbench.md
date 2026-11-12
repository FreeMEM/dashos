# Bitácora: el Workbench nativo, primera versión (S10)

Rama `workbench`, sacada de `revision`. Diseño en `docs/diseño/S10-escritorio.md` ("Workbench", "Aspecto") y
`docs/diseño/S6b-usuarios-y-permisos.md` (raíces, permisos, `Home:`).

## 2026-11-12: estudio, sin código todavía

La sesión se cortó por el cupo semanal de uso antes de escribir código. Queda lo estudiado y lo decidido, para que la
siguiente vuelta empiece por el programa.

### Lo que hay que tocar y dónde

- Programa nuevo `usuario/src/bin/workbench.rs` (y su entrada en `usuario/Cargo.toml`).
- `kernel/src/inicio.rs`: una línea en la lista `PROGRAMAS`, solo en `virt`, con los canales "ventanas", "archivos" y
  "sesiones". Sus llaves serían 1 ventanas, 2 archivos y 3 sesiones (el orden de los canales es el de su primera
  aparición en la lista, como en la barra). Ponerla al final de los de `virt` (tras `caida-cliente`): si va antes,
  cambian los números (marcas) de los que vienen detrás. Hoy hay 35 programas; `inicio` solo vigila y relanza los
  servicios con número menor de 32, así que no debe empujar a `caida` (33) más allá: no lo empuja si va detrás.
- `usuario/src/bin/inicio.rs`, `FICHAS`: la ficha del workbench.
- `usuario/src/bin/compositor.rs`, `CON_CAPAS`: añadir "workbench" (las capas van por el nombre que da `inicio`).
- `usuario/src/bin/sesiones.rs`, `LANZADORES`: añadir "workbench" para que lance de `Programas/` como la persona de
  delante, igual que la barra (`lanzables::de_programas`). Es lo único que toca de `sesiones`.
- `tools/disco_archivos.py`: un cajón privado de otra persona dentro de `C` (por ejemplo `C/DeCarla/`, de 1001:1001,
  0700, con `duenos ... camino=`) y un nombre raro más; y sus líneas en la lista de `--comprobar`.
- `tools/prueba_uefi.sh` y `kernel/pruebas/arranque_virt.txt`: la fase del workbench al final, tras la de la barra
  (`barra == 8`), con `continue` mientras dura para que no corra "lo de siempre".

### Decisiones tomadas (para confirmar)

1. **Capa de abajo (1), no de fondo (0)**, anclada a los cuatro lados, tamaño 0 y zona 0: el compositor la pone en el
   hueco que deja la barra (no tapa ni se mete bajo ella). Búfer ARGB transparente salvo los iconos: un fondo de
   pantalla de `fondo-c3` en la capa 0 queda debajo y se ve. Sin teclado (interactividad 0): un clic en el escritorio
   no quita el teclado a la ventana (lo comprueba `Escena::raton`: una capa sin teclado no lo toma).
2. **Iconos de volúmenes arriba a la derecha.** El guion mueve el ratón en diagonal (10, 5) y hace clic cada 0,65 s
   desde arriba a la izquierda; abajo a la derecha se queda clavado en la esquina. Arriba a la derecha no pasa.
   `tools/captura_ventanas.py` mira el fondo en (5, 60): ahí la capa es transparente.
3. **Raíces**: ficha como la del selector (usuario 1000, Ana, con el derecho de repartir, raíz del disco) y, al
   empezar, cederse a sí mismo la raíz 1 = el disco solo para leer ("Sistema") y la raíz 2 = `C` para leer y escribir
   ("Home", provisional). Todo se busca desde esas dos, nunca desde la 0. Motivo: el `Home:` de verdad es el volumen
   cifrado "Ana", y abrir un volumen por nombre exige el derecho de administrar; en el alta no sirve porque al
   arrancar está cerrado. Hasta que `sesiones` le ceda el volumen al entrar Ana (cambio en `sesiones`, fuera de esta
   rama), `Home` es `C`. El icono de `Home` sale solo si `ACTIVA` (orden 18 de `sesiones`) dice que la sesión de Ana
   está abierta: así cumple "si está abierto".
   Alternativa más estrecha: ficha con raíz `C` (como el centro), sin repartir; entonces no hay icono de sistema.
4. **Ventanas de cajón** con el kit: arriba el camino y dos botones (Iconos/Lista e Información); el contenido, un
   hueco que pinta y atiende el propio programa (rejilla de iconos o lista con nombre, tamaño y fecha). Varias
   ventanas por una sola conexión, cada una con sus objetos de Wayland (base 100 + 10 por ventana) y su memoria.
   Tamaño fijo en la primera versión. La pantalla de `virt` es 800x600 (560 bajo la barra).
5. **Información**: otra ventana del kit con nombre, tipo, tamaño, fecha, dueño ("usuario 1000 (tú)": los nombres de
   las cuentas no salen de `sesiones`), el comentario en un campo con etiqueta, y una rejilla de casillas: filas
   Dueño/Grupo/Otros y columnas Leer/Escribir/Ejecutar/No borrar (bits 16, 32, 64 del Amiga), más una fila con Guion
   (4), Puro (2), Archivado (1) y Residente (8). Guardar manda `COMENTAR` y `PROTEGER`; sin ser el dueño o por la raíz
   de solo lectura, todo desactivado y un texto que lo dice (el servicio contestaría -12).
6. **Nombres raros**: como el selector, fuera los vacíos, ".", ".." y los que llevan '/', ':' o '|'. Al abrir, se busca
   por la asa del cajón y un solo nombre, y se mira el tipo de nuevo.
7. **Doble clic** con `kit::controles::DOBLE_MS` (500 ms) y la hora de los eventos de Wayland.

### Pendiente (todo)

- Escribir `workbench.rs`, los iconos (cajón, fichero, programa, volumen, en Material 3 con los tonos del tema) y los
  cambios de arriba.
- Las pruebas en `virt`: iconos en pantalla, doble clic en un cajón, cambiar a lista, Información y cambiar un
  comentario; y las hostiles (no ve ni abre `C/DeCarla` ni `Estado/Servicios/sesiones`; no enseña ni abre nombres
  con '/', ':' o "..").
- Capturas en `capturas-agente/`.
- `make prueba` y `make arranque-virt` no se han pasado en esta rama: no hay cambios de código.
