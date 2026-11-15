# Bitácora: cuatro decisiones pequeñas del usuario (2026-11-13)

Rama `decisiones-1311`, sacada de `revision` (022e9d1). Las decisiones están en la última entrada de
`docs/REVISION_LOOP.md`. Una cosa por commit, cada una con su prueba.

## 1. Apagar y reiniciar

- Kernel: llamada 51, `apagar(permiso, modo)`: 0 apaga, 1 reinicia. Pide un objeto nuevo, el permiso de apagar
  (`Objeto::PermisoApagar`), que solo crea el kernel y da a `inicio` (aparato "apagar", tipo 10, el último del
  paquete). Antes de apagar duerme 300 ms: así la consola saca lo último del registro por el puerto serie.
  - Con PSCI si el árbol lo trae (SYSTEM_OFF 0x84000008, SYSTEM_RESET 0x84000009), por `hvc` o `smc` como dice el
    árbol (lo mismo que para encender los núcleos). Vale para QEMU `virt`, UEFI y la Pi 5: su firmware carga el BL31,
    que da PSCI. En la Pi 5, SYSTEM_OFF deja la placa parada; que se corte la corriente lo decide la EEPROM
    (`POWER_OFF_ON_HALT`), que no se toca.
  - Sin PSCI (la Pi 4 y la Pi 3 con su firmware de serie): el perro guardián del bloque PM. Reiniciar: WDOG a 10 tics
    y RSTC en reinicio completo. Apagar: antes, la partición 63 en PM_RSTS, que el firmware entiende como "quedarse
    parada". **Sin probar en una Pi**: allí aún no hay `sesiones`, así que nadie tiene el permiso.
- `sesiones` ya tenía sus 8 llaves (el tope de `proceso_crear`): el permiso le llega de `inicio` por su canal, sin
  marca (`PERMISO_APAGAR`, 23), solo para escribir y sin derecho de transferirlo. Si `sesiones` se relanza, otra vez.
- `sesiones`, orden `APAGAR` (22): solo del panel (por el nombre que da `inicio`). Modos: apagar, reiniciar, cerrar
  sesión. Con otras sesiones abiertas además de la de delante, pregunta antes a `guardian` ("*apagar" o "*reiniciar");
  sin `guardian`, no. Lo hecho y lo negado van al registro de seguridad (`R_APAGAR`; el centro lo enseña como
  "apagar"), que se guarda en el disco antes de apagar. Cerrar sesión se lo pide a `guardian` ("*salir"), que hace
  `SALIR` y pone la pantalla de entrada.
- `guardian`: el diálogo de teclado sirve también para confirmar ("¿Reiniciar el equipo?", "Hay otras sesiones
  abiertas y se cerrarán."). Al acabar su prueba ya no sale: se queda atendiendo preguntas (`atender`).
- Panel: botón de encendido a la derecha de la cabecera (Editar y el candado se corren a la izquierda). Abre el menú
  de apagado en lugar de los controles: Apagar, Reiniciar, Cerrar sesión y Bloquear.
- Pruebas hostiles (`hostil`): la llamada 51 con 64 números de llave y los dos modos, todas con error; y `APAGAR` a
  `sesiones` sin ser el panel, -2.
- Prueba de QEMU: `make arranque-virt` pasa `REINICIO=pruebas/arranque_virt_reinicio.txt` a `tools/prueba_uefi.sh`.
  Cuando ya salió todo lo esperado (y la captura), el guion abre el panel, pulsa el botón de encendido y Reiniciar;
  `guardian` pregunta (Carla sigue detrás de Ana) y el guion contesta que sí. Se espera a las líneas de ese fichero y a
  un segundo "DashOS en marcha". El tiempo de la prueba pasa de 300 a 360 s.
- Sin probar en el guion: Apagar (en QEMU acabaría la prueba igual que con `-no-reboot`) y Cerrar sesión.

## 2. El estado de la red, solo para leer, para el panel

- Canal nuevo "red-estado", que lee el servicio de red (lo nombra tras "red"). Lo atiende un hilo aparte
  (`atender_estado`): solo contesta `ESTADO`, con una foto que deja el bucle principal en `FOTO` en cada vuelta (la
  configuración, las conexiones en total y las tramas; sin las suyas ni los rechazos). Lo demás se tira y sus llaves se
  cierran. Con esa llave no se llega al bucle de la pila: no hay forma de abrir una conexión.
- `inicio`: el panel recibe "red-estado" en lugar de "red" (sigue siendo su llave 5). Ya no tiene el permiso de red.
- Prueba hostil, en el panel al arrancar: pide `CONECTAR` (al eco del PC) con su llave y no recibe nada en 1 s. El
  servicio lo dice ("solo se contesta ESTADO: orden 1 rechazada"). Y el panel sigue viendo la red conectada.

## 3. 1 GB para el invitado Linux con 8 GB o más

- La regla de serie (`dashos::maquina::memoria_invitado`, en `usuario/src/lib.rs`; la usan `inicio` para el cupo de la
  caja y `caja-linux`): 128 MB con menos de 2 GB, 256 MB con 2 GB, 512 MB con 4 GB y ahora 1 GB con 8 GB o más (con la
  misma holgura de 128 MB por debajo de cada umbral). `DASHOS_CAJA_LINUX_MB` sigue mandando si se pone.
- 1 GB son justo los 64 trozos de 16 MB que admite una vista (`memoria_vista`, `MAX_ORIGENES`).
- `make arranque-virt-wayland` (8 GB): la caja dice "hasta 1024 MB para el invitado" (esperados de `-wayland` y de su
  variante de Firefox al día).

## 4. Fila "Ampliar ventanas Amiga" en Ajustes

- Centro de control, Apariencia: una fila nueva tras "Programas de las cajas", "Ampliar ventanas Amiga: automático".
  Cada vez que se elige pasa a la siguiente: automático, 1x, 2x, 3x y otra vez automático. Escribe `escala amiga N` en
  `Prefs/cajas` (automático es no poner la línea), relee para comprobar y se lo da al compositor (`CAJAS` de
  `dashos_tema_v1`), igual que el aspecto de las cajas. La regla automática no cambia (3x desde 3200x1800, 2x desde
  1600x900, 1x debajo).
- Los fondos van una fila más abajo. El guion los busca por su nombre, no por su sitio; el doble clic por posición es
  en los colores, que están antes. Así que el guion no se descoloca.
- Prueba (`make arranque-virt`): tras los fondos, el guion elige la fila una vez (de automático a 1x; en 800 x 600 la
  automática ya es 1x, así que la caja Amiga se ve igual). Se espera "Ampliar ventanas Amiga: 1x; guardado: 0
  (30 bytes); releído: sí", y `disco_archivos.py --comprobar` espera `C/Prefs/cajas` de 30 bytes.
- Esto resuelve el punto 1 de "Lo que falta" de la escala en `docs/bitacoras/caja-amiga.md`.

## Pruebas (2026-11-13)

- `cargo test` de `wayland`: 94 bien. `cargo check` del kernel para la Pi 4, la Pi 3 y la Pi 5: sin errores ni avisos.
- `make prueba`: "BIEN: esperado.txt" y "BIEN: cargador hunk".
- `make arranque-virt`: las cinco BIEN y "BIEN: arranque_virt_reinicio.txt": el panel pide reiniciar, `sesiones`
  pregunta (1 sesión más abierta), el guion dice que sí, el kernel reinicia y QEMU vuelve a arrancar (otro "DashOS en
  marcha" a los 242 s). La primera vez falló la prueba hostil del panel: miraba `k` tras una lectura con error (la
  llamada no lo toca); arreglado en el commit siguiente.
- `-vm`, `-linux`, `-2`, `-diag`, `-escritorio`, `-usb`, `-arch` y `-gpu`: BIEN. `-wayland`: BIEN en 17 min, con
  "hasta 1024 MB para el invitado" (Firefox con 449 MB, la RAM del invitado 668 de 995 MB).
- Sin probar: Apagar y Cerrar sesión desde el panel, y el apagado en una Pi de verdad.
