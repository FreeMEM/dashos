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
