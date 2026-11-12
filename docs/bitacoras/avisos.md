# Bitácora: avisos (notificaciones, S10)

Rama `avisos`, sacada de `revision`. Diseño en `docs/diseño/S10-escritorio.md` ("Lanzador, notificaciones y
Commodities") y `docs/diseño/U6-cambio-rapido.md`.

## 2026-11-12: cortada al empezar (cupo de uso)

Se paró por el cupo semanal antes de escribir código. Solo hay esto:

- La rama avanzó a `revision` (755875c, con `u6-paso2` dentro), porque `u6-paso2` cambia `sesiones`, `inicio` y el
  compositor, que son las piezas que usan los avisos.
- El diseño, ya pensado tras leer el código. Sin probar nada.

### Diseño previsto

**Quién es quién.** Igual que el compositor y el sonido en U6 paso 2:

- `avisos` entra en `SIGUEN_SESIONES` de `usuario/src/bin/inicio.rs` y de `sesiones.rs`. Así `inicio` da a
  `sesiones` una copia sin marca de su canal (`CANAL_SISTEMA`), y `sesiones` le manda `AVISO_PERSONA` (marca, usuario,
  nombre) y `AVISO_DETRAS`.
- Los programas de `inicio` llegan con su entrada de directorio (24 bytes, sin marca: marca y nombre).
- Solo cuenta lo que llega con marca 0 (de `inicio` o `sesiones`). Un programa que mande, con su marca, algo con
  forma de entrada de directorio o de `AVISO_PERSONA` no cambia su nombre: es la prueba hostil del nombre.
- El aviso no lleva campo "de quién": el servicio pone el nombre de la marca. Cada entrada guarda si es del sistema
  (marca de `inicio`) o lanzado del disco (se enseña con su nombre de fichero).
- Lo lanzado por `sesiones` pide la llave con `SERVICIO "avisos"` (`REPARTE`, con la llave de `CANAL_SISTEMA`, sin
  entrada de directorio).

**Protocolo** (`usuario/src/protocolo/avisos.rs`, sin `alloc`, probado con `rustc --test` en `make prueba`):

- `AVISAR`: urgencia, largo del título (hasta 48 bytes), largo del texto (hasta 160) y los textos, en un mensaje de
  256. Contesta estado, número del aviso y el nombre que puso el servicio (así el programa de prueba comprueba que no
  se puede hacer pasar por otro). Largos que no cuadran con el mensaje: error, y el servicio sigue. Bytes no válidos o
  de control: se cambian por "?" (y la barra vuelve a limpiar al pintar).
- Solo para la barra (nombre `barra` de `inicio`, con `lanzables::del_sistema`): `SUSCRIBIR` (con una llave para el
  timbre), `LEER` (un aviso por mensaje, desde un número), `LEIDOS` y `BORRAR_TODO`.
- El `Historial` va en el mismo fichero: 50 avisos, como mucho 8 por programa (un programa que inunda solo se pisa a
  sí mismo), un ritmo por programa (más de N por minuto: rechazado) y una tabla de remitentes acotada.

**La barra.** Un proceso solo espera en un canal. El servicio escribe el timbre en el mismo canal por el que la barra
lee al compositor, con una copia de ese extremo marcada (hace falta un `Cliente::conectar_con_puerta(marca)` en
`usuario/src/ventana_cliente.rs`, que guarde aparte los mensajes con esa marca en vez de tirarlos). Un solo timbre
pendiente hasta que la barra pida algo: no llena la cola del compositor. Al sonar, la barra pide lo nuevo y enseña un
"toast" (capa arriba a la derecha bajo la barra, unos 4 s, se cierra con un clic), una campana con punto si hay sin
leer y, al pulsarla, el historial en una capa como el panel, con "Borrar todo".

**Sesiones de detrás.** Los avisos de una persona con su sesión detrás se guardan y no salen a la de delante ni en su
historial. Al volver su dueña, salen en el historial con la campana marcada, sin toast. Lo del sistema sale a quien
esté delante.

**Pruebas previstas en virt.** `avisador` (de `inicio`, canal "avisos"): un aviso bueno, el intento de cambiarse el
nombre, largos falsos, bytes malos y una inundación. `Programas/avisa` lanzado desde el lanzador como Ana (permiso fijo
"avisa avisos"): sale su toast (captura), punto en la campana, se abre el historial, y lo de `avisa` sigue ahí tras la
inundación. `cambio`, con Ana detrás, avisa: no sale a Carla y sí al volver Ana. "No molestar" en el panel, al final
del guion.

### Pendiente: todo

- Servicio `usuario/src/bin/avisos.rs`, protocolo e historial con sus pruebas de PC en `kernel/Makefile`.
- Líneas de `kernel/src/inicio.rs` (el servicio antes de la barra; el canal "avisos" a la barra y a `avisador`).
- `inicio.rs` y `sesiones.rs` de usuario (`SIGUEN_SESIONES`, `REPARTE`, `PERMISOS`).
- La barra (timbre, toast, campana, historial) y el cambio en `ventana_cliente.rs`.
- Programas de prueba, disco (`tools/disco_archivos.py`), guion y esperados; "no molestar" (opcional).

No se ejecutó ninguna prueba: no hay código nuevo.
