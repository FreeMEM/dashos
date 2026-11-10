# Revisión: implantar mejoras y usuarios (plan y bitácora)

Encargo del usuario (2026-11-10): implantar `docs/REVISION_ARQUITECTURA.md` y `docs/diseño/S6b-usuarios-y-permisos.md`,
con todas sus decisiones aprobadas, "como mejor creas", en loop. Papel de esta sesión: implementa (no lee Linux ni
desensamblados).

Worktree `.worktrees/revision`, rama `revision` (sale de `multitarea`). Commits locales: nada hacia fuera sin permiso.

## Cómo se trabaja

- Cada paso: medir antes si toca rendimiento (`make -C kernel medir`, `tools/medir.py`), cambiar, `make -C kernel
  prueba` y `make -C kernel arranque-virt`, apuntar aquí y commit local.
- Cada arreglo de seguridad o estabilidad entra con su prueba hostil, que falla antes y pasa después.
- Emulador bajo `flock /run/user/1000/dash-emu-red.lock`. Parar solo procesos propios por PID.
- Al 90 % de `~/.claude-ftapias/state/uso-7d-ftapias.txt` se para.
- Quien cambie una llamada o una orden pone al día `docs/referencia/kernel.md` o `servicios.md` en el mismo commit.

## Orden

Primero lo que hace falta para probar (cliente hostil), luego lo pequeño y seguro, luego permisos en archivos (U1, que
no depende de nada), luego lo grande. Las medidas de rendimiento esperan a tener el banco fiable (B1 a B3).

| Paso | Qué | Puntos | Estado |
|---|---|---|---|
| R1 | Cliente hostil en las pruebas del arranque | B6 | pendiente |
| R2 | Servicios: mensajes vacíos, fugas de llaves, giros, topes de Wayland, tecla perdida | E1 a E5 | pendiente |
| R3 | Kernel: dormidos sin tope, temporizador puntual, interrupción con la cola llena, aviso perdido | K9 a K12, K15 | pendiente |
| R4 | Llaves de aparato con derechos mínimos, USB hostil, foco del teclado, TIC del sonido | S4, S7, S8 | pendiente |
| R5 | Clase de plazo con llave y admisión que cuenta a los dormidos | S3, K8 | pendiente |
| R6 | Archivos con ficha: dueño, grupo y modo, raíces, ceder, administrar | U1, A13 | pendiente |
| R7 | Banco fiable | B1 a B4 | pendiente |
| R8 | Archivos, lo rápido: cerrar sin guardar, menos viajes | A1 a A6 | pendiente |
| R9 | Aviso de muerte y plazo al leer | K7 | pendiente |
| R10 | Azar de la placa y arranque endurecido | S5, S6 | pendiente |
| R11 | Kernel sin pánicos por memoria y llamadas largas troceadas | S1, S2 | pendiente |
| R12 | Supervisión de servicios y biblioteca común | E6 a E9 | pendiente |
| R13 | Discos ajenos en un proceso lector aparte | S6b regla 4 | pendiente |
| R14 | `sesiones`: cuentas, fichas, lanzar desde disco, procesos | U2 | pendiente |
| R15 | Entrada, bloqueo y volumen cifrado por usuario | U3 | pendiente |
| R16 | Permisos por aplicación | U4 | pendiente |
| R17 | `guardian` y `selector` | U5 | pendiente |
| R18 | Camino caliente del kernel; archivos, lo grande | K1 a K5, A7 a A14 | pendiente |
| R19 | `Prefs` de usuarios, registro de seguridad, PID en el kernel | U6 | pendiente |

K6 (ida y vuelta con paso directo) espera a medir en la Pi real (B5).

## Bitácora

### 2026-11-10

- Decisiones del usuario apuntadas en S6, S6b, S4b, RESUMEN y en el informe. La sonda de medidas de `multitarea` se
  quitó (parche guardado fuera del repo).
