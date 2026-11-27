# Bitácora: cambio rápido de usuario (U6)

Rama `usuarios-u6`, sacada de `revision`. Lo que faltaba de U6/R19 en `docs/REVISION_LOOP.md`. Diseño en
`docs/diseño/U6-cambio-rapido.md`.

## 2026-11-12

### Diseño

- Escrito `docs/diseño/U6-cambio-rapido.md`. La sesión de quien no está delante sigue en marcha, con su volumen
  abierto; no ve pantalla ni recibe teclado, no pregunta al usuario, no pide acciones de administrador y no bloquea.
- `sesiones` decide qué sesión está delante. El compositor la seguirá (paso 2).
- Siete decisiones para que las confirme el usuario, al final del documento.

### Paso 1: `sesiones` y `guardian`

- `sesiones` apunta la sesión de delante: la del último que entró o volvió con su contraseña. Lo dice en una línea
  ("delante, la sesión de Carla; detrás, 1").
- Entrar en una sesión ya abierta es volver: se comprueba la contraseña sin reabrir el volumen.
- `PROCESOS`: cada programa ve los de su usuario y los del sistema. `guardian` ve todos.
- `MATAR` a un proceso de otra persona da "no existe" (-1), como si no estuviera. Queda en el registro.
- Desde una sesión de detrás: pedir un servicio sin permiso da -2 sin preguntar; una acción de administrador, -2;
  bloquear, -2. Todo al registro.
- Orden nueva, `ACTIVA` (18): delante 1, detrás 0, sin sesión -2.
- `guardian`: en la pantalla de bloqueo, Esc pasa a "Cambiar de usuario" (nombre y contraseña). Otra vez Esc vuelve.
  Al pie, una pastilla con la tecla. El kit no da Esc: se mira el código de la tecla (1), como hace el centro.
- `guardian` decía "pantalla pintada" con cada sync de un repintado entero. Con Esc salía dos veces y el guion contaba
  mal. Ahora, una vez por pantalla.

### La prueba en virt

- Programa nuevo en el disco, `Programas/cambio`. Hace de testigo si su `Home:` es de Ana y de espía si es de Carla.
- `guardian`, tras entrar Ana: lanza `cambio` como Ana, bloquea, y el guion pulsa Esc y entra como Carla. Lanza
  `cambio` como Carla y espera a que acabe. Bloquea otra vez; Esc, y vuelve Ana. Espera a que acabe el testigo.
- La espía (Carla, con Ana detrás): ve 16 procesos, ninguno de otra persona. Mata a ciegas 111 números (los de
  `inicio` y los de `sesiones`, el de Ana entre ellos): ninguno muere y todos dicen "no existe". Entrar, volver y salir
  como Ana dan -12; lanzar como Ana, -2. Abrir el volumen de Ana con la contraseña buena, verlo y cerrarlo dan -12. Su
  fichero, por 18 caminos (su `Home:`, subiendo, por el disco y por raíces que no tiene): ninguno llega.
- El testigo (Ana): escribe `Home:testigo.txt`. Detrás sigue escribiendo; pedir `eco` y bloquear dan -2 sin preguntar.
  Al volver Ana, el fichero tiene las dos partes.
- `disco_archivos.py --comprobar`: `Programas/cambio` en el árbol, y lo que escribió el testigo no está en claro en el
  disco. Primero lo buscaba por el nombre y un texto fijo, y los encontraba: estaban en el propio programa, que está
  en el disco. Ahora el testigo escribe 48 letras que calcula al correr (con `black_box`, para que el compilador no
  las deje hechas), y se busca eso.
- El guion cuenta las pantallas de bloqueo: las del bloqueo por tiempo y Super+L pasan a ser la cuarta y la quinta.
- Capturas en `capturas-agente/` (fuera de git): el bloqueo con "Esc: cambiar de usuario" y "Cambiar de usuario" con
  la frase de Carla.

### Pruebas

- `make arranque-virt`, primera vuelta: el cambio salió bien, pero la cuenta de pantallas del guion fallaba (el doble
  "pintada"). Arreglado.
- Segunda y tercera vuelta, con el PC a una carga de 16 a 60 por otras sesiones (amiberry): todas las líneas de U6
  bien, el panel rápido al final bien. Fallaron cosas de antes del cambio y que no toca este paso: `[hostil] A7
  préstamos` (no proyecta lo prestado) una vez, y Ajustes maximizado y el selector otra.
- `make prueba`: C14 (mide tiempos) MAL dos veces con la carga a 50-60; lo demás bien.
- Con la carga a 13: `make prueba` BIEN y `make arranque-virt` BIEN en las líneas; la comprobación del disco falló por
  lo del texto en claro de arriba.

## Pendiente

- Paso 2: el compositor por sesiones. Hoy un programa lanzado por `sesiones` no tiene llave del compositor, así que no
  hay ventanas de una sesión que ocultar. Los que lanza `inicio` con el usuario de Ana (`centro`, `selector`) se ven
  también con Carla delante.
- Paso 3: sonido de detrás en silencio; preguntas de detrás en espera en vez de negadas; lista de cuentas en el
  bloqueo; acción de administrador "cerrar la sesión de otro".
- Los permisos concedidos son por programa, no por programa y usuario.
- Probar en la placa.
