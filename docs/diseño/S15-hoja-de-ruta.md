# S15. Hoja de ruta

## El problema

Las tareas S0 a S14 dicen cómo es DashOS. Falta decir en qué orden se construye, con hitos que se puedan enseñar, y
qué hace falta para la primera versión que arranca en una Pi 4 real.

Tres reglas para ordenar:

- **Cada hito se puede enseñar**: un registro por el puerto serie, una captura o un vídeo corto. Nada de "el kernel
  está al 60 %".
- **Primero lo que se puede hacer en QEMU** (S12). La Pi 4 se compra al terminar la fase Base (decisión del usuario).
- **Lo más arriesgado, cuanto antes.** Si algo no funciona, mejor saberlo al principio.

## Lo que ya está hecho (2026-11-03)

- Emu68 compila con las toolchains de Bootlin y arranca en QEMU `raspi4b` en big-endian, con los cuatro núcleos
  (`patches/emu68/`, `docs/arranque-emu68-qemu-raspi4b.log`).
- El JIT de Emu68 funciona separado, como programa de Linux big-endian en `qemu-aarch64_be` (`jit-host/`).
- Un kernel mínimo en Rust arranca en QEMU `raspi4b` en big-endian, lee la CPU y comprueba que se pueden tener
  procesos little-endian (`docs/diseño/pruebas/rust-be/`).

## Tres líneas de trabajo en paralelo

| Línea | Quién | Qué |
|---|---|---|
| Sistema | Sesiones de DashOS | Kernel, servicios, controladores, caja Amiga, escritorio |
| Dash | Sesiones de Dash (regla 4) | Backend ARM big-endian, modo sin sistema, builtins para DashOS |
| Análisis | Sesiones de análisis (regla 1) | Especificaciones de la sala limpia: exec, dos, graphics, layers, intuition, gadtools; y de los controladores sin manual |

La línea de análisis puede empezar ya, porque no depende de nada: lee el Kickstart y escribe especificaciones con sus
pruebas del oráculo. Cuando el sistema llegue a cada biblioteca, la especificación ya estará escrita.

## Los hitos

### H1. Llamada nativa desde el 68k (en `jit-host/`)

Un programa 68k llama a una función de una "biblioteca" cuya tabla de saltos lleva la instrucción de línea A (S4). La
función nativa recibe los registros, escribe por la salida y devuelve un valor en D0.

- Riesgo que despeja: que el JIT de Emu68 se deje enganchar sin romperse.
- Se enseña: la salida del programa en la terminal.

### H2. El kernel mínimo (QEMU `raspi4b`)

Rust, big-endian. Serie, temporizador, GIC, MMU, los cuatro núcleos, hilos, planificador con las dos clases (S2),
canales, capacidades (S1), objetos de memoria (S3) y procesos en EL0. Un primer proceso de usuario que escribe, y dos
procesos que se hablan.

- Riesgo que despeja: el micronúcleo en big-endian con varios núcleos.
- Se enseña: el registro por el puerto serie.

### H3. La base (QEMU `raspi4b`)

Controladores de la SD, de la pantalla del firmware y del USB DWC2 (teclado y ratón, S12). Sistema de archivos a
partir de RedoxFS, primero solo leyendo (S5). Gestor de servicios (S14). Un compositor mínimo que habla Wayland y
dibuja por CPU (S9), con una ventana de prueba que se mueve con el ratón.

- Riesgo que despeja: la cadena entera, de la tarjeta a la pantalla, con controladores en procesos.
- Se enseña: una captura de QEMU con una ventana y el puntero.
- **Al terminar este hito se compra la Pi 4.**

### H4. La caja Amiga, primera versión

La caja como proceso, con el JIT dentro, exec nativo (tareas, señales, puertos, memoria con dueño, testigo de la
caja) y dos.library sobre el sistema de archivos (S4, S4b). El banco del oráculo: el mismo binario 68k en FS-UAE con
el Kickstart real y en DashOS, con la misma salida.

- Riesgo que despeja: exec en la caja y la comparación con el oráculo.
- Se enseña: un programa Dash compilado para 68k que escribe en la consola de DashOS lo mismo que en FS-UAE.

### H5. La caja Amiga con ventanas

graphics, layers, intuition y gadtools nativos. Bibliotecas de la ROM del usuario para lo que falte (S4). MUI y
ReAction de 68k en el JIT. Ventanas Amiga como ventanas del compositor (S4, S10). Enforcer (S3).

- Riesgo que despeja: que los programas Amiga reales abran ventanas y funcionen.
- Se enseña: Forja o Inferencia (68k, MUI) con su ventana en el escritorio de DashOS.

### H6. La placa (Pi 4 real)

Arranque en la Pi 4 con `tryboot` (S14). PCIe y USB VL805, red GENET, pila smoltcp y TLS (S13), HVS y HDMI con
sincronía vertical y las dos salidas (S9), sonido. `bsdsocket.library` nativa en la caja.

- Riesgo que despeja: el hardware real sin manual, y las medidas de S0.
- Se enseña: un vídeo de la Pi 4 arrancando al escritorio, y Tachyon (68k) navegando dentro de la caja.
- **Aquí se miden por primera vez las metas de S0**: arranque, memoria en reposo, fluidez con carga.

### H7. Dash nativo y el escritorio

Backend ARM de dashc (línea Dash). Motor de interfaz con el modelo de MUI y aspecto Material 3, con las caras de
ReAction y GadTools (S11). Escritorio: Workbench, pantallas, mosaico, barra, lanzador, notificaciones (S10). Usuarios,
permisos, selector de ficheros y cifrado del cajón (S5, S6).

- Riesgo que despeja: el mismo programa Dash en los dos sistemas.
- Se enseña: Forja compilado en nativo junto a Forja de 68k en la caja.

### H8. Paquetes y actualizaciones

`Paq`, repositorios firmados, transacciones con instantáneas, imagen de `SYS:` firmada, actualización A/B con vuelta
atrás (S8, S14). El cliente de Hangar en la caja, y después en nativo.

- Riesgo que despeja: actualizar sin romper.
- Se enseña: una actualización que falla a propósito y vuelve sola a la versión anterior.

### Primera versión: DashOS 0.1

**H1 a H8.** Arranca en una Pi 4 real hasta el escritorio, con red, con la caja Amiga ejecutando programas de
Workbench, MUI y ReAction, con aplicaciones nativas en Dash, y con paquetes y actualizaciones seguras.

### H9. La caja Linux (DashOS 0.2)

Hipervisor, virtio, Arch Linux ARM dentro, ventanas por un intermediario de Wayland, `Home:` compartido (S7).

- Se enseña: Firefox en una ventana del escritorio de DashOS.

### Después

GPU V3D con Mesa (S9), Wi-Fi y Bluetooth (S12), relibc y programas de C portados (S7), GTK y Qt en nativo (S11),
Warp3D y MiniGL en la caja (S9), `amissl.library` nativa (S13), y la Pi 5, después de comprobar que su CPU admite
big-endian (PLAN).

## Lo que más puede torcerse

| Riesgo | Dónde se sabe | Qué se haría |
|---|---|---|
| El JIT no se deja enganchar o falla con varias tareas | H1, H4 | Cambiar Emu68 más a fondo o, en el peor caso, otro JIT |
| El HVS y el HDMI sin manual cuestan demasiado | H6 | Quedarse más tiempo con la pantalla del firmware |
| La CPU de la Pi 5 no admite big-endian | Antes de la Pi 5 | DashOS se queda en la Pi 4 y placas con big-endian |
| La meta de arranque o de memoria no se cumple | H6 | Revisar S0 con números reales |
| El backend ARM de Dash se retrasa | H7 | Las bibliotecas de la caja empiezan en C con punteros de 32 bits a mano |

## Lo que queda abierto

1. ¿Este orden, con la primera versión en H8 y la caja Linux en la 0.2? Recomendado: sí.
2. ¿Arrancar ya la línea de análisis (especificaciones del Kickstart en sala limpia), en paralelo? Recomendado: sí,
   empezando por exec.
3. ¿Avisar ya a las sesiones de Dash del backend ARM, o esperar a H4? Recomendado: avisar ya, porque es largo y nada
   de lo suyo depende de DashOS.

## Cómo encaja

Es el orden de todo lo anterior. Sustituye la lista de fases de `PLAN.md`.
