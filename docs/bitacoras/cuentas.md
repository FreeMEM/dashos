# Bitácora: las cuentas de usuario de verdad (cuentas)

Rama `cuentas`, sacada de `revision` (d45a8281). S6b ("Dónde se guardan las cuentas" y "Administradores") y S36
(decisión 1, aceptada el 2026-11-16: una tanda es una acción, con una contraseña). Papel: implementa. No se leyó código
ajeno a DashOS.

## 2026-11-16

### Lo que había

- Las cuentas, fijas en `sesiones`: `CUENTAS = [(1000, "Ana"), (1001, "Carla")]` y `ADMIN = [1000]`. El nombre era
  también el del volumen cifrado (su `Home:`). El centro de control tenía su propia copia (1000 Ana, 1001 Carla).
- Tanda de administrador (rama `admin-desarrollo`) con tres acciones: comprobar, matar, registro.
- El servicio de archivos hacía volúmenes cifrados (`HACER_VOLUMEN`) con una contraseña, sin código de recuperación,
  y con la raíz del que lo pide, 0755.

### 1. Las cuentas, en `Estado:Cuentas/`

- **Biblioteca** `usuario/src/cuentas.rs` (sin dependencias, probada en el PC): leer `usuarios`
  (`nombre:uid:gid:nombre completo:volumen:estado`) y `grupos` (`nombre:gid:miembros`), comprobar, cambiar (crear,
  retirar, devolver, meter, sacar, quitar del todo) y escribir. Nombres: 1 a 16 letras ASCII, cifras, `-` o `_`,
  empezando por letra, sin los reservados (los volúmenes y asignaciones del sistema, `admin`, `sistema`...), únicos
  sin distinguir mayúsculas (también contra los volúmenes). uid de 1000 a 65.535, gid igual (su grupo propio). Topes:
  32 cuentas, 16 grupos, 16 miembros, 4 KB por fichero. El siguiente número es el mayor más uno (no se reusa el de una
  retirada).
- **Lo que no vale no tumba nada**: una línea mala (campos, números, estado, repetida) o la última sin salto de línea
  (fichero truncado) se salta y se cuenta; las buenas se quedan. `sesiones` lo dice en la consola y en
  `Estado:Registros/seguridad`.
- **`sesiones`**: las lee al arrancar (`cargar_cuentas`), con un cliente propio de archivos sobre `Estado/Cuentas`
  (`HUECO_SUYO`: el cajón es suyo, 0755; los ficheros, 0644; uno que no es suyo no se lee). `CUENTAS` y `ADMIN` ya no
  existen: todo lo que los miraba (entrar, volver, salir, lanzar, `Home:`, `RAM:`, lo concedido, el registro, la
  tanda, bloquear, delante, guardar ajustes, leer el registro) usa la lista leída, por cuatro funciones que no dejan
  salir referencias (`nombre_de`, `volumen_de`, `para_entrar`, `es_admin`). Entrar usa el volumen de la cuenta (antes,
  el nombre tecleado era el volumen); una cuenta borrada o que no existe da lo mismo que una contraseña mala.
- **Escribir** (`sd::guardar_cuenta`): de una sola escritura (una transacción del servicio): si lo nuevo es más corto,
  se rellena con un comentario hasta el largo de antes y después se recorta. Un corte de luz deja lo de antes o lo nuevo
  entero, nunca lo nuevo con el final de lo de antes (que podría devolver una línea borrada). Si no se guarda, la
  acción falla y las cuentas en memoria vuelven a como estaban.
- **El centro de control** (monitor del sistema) lee los nombres de `Estado:Cuentas/usuarios` al entrar al monitor
  (antes, su propia copia de Ana y Carla).
- **El disco de pruebas** trae `Estado/Cuentas/usuarios` con Ana y Carla y `grupos` con `admin:10:Ana` (de
  `sesiones`, usuario 1), y a propósito cuatro líneas malas (un uid repetido, una sin campos, un nombre con ñ y la
  última cortada): todas las pruebas de antes siguen igual.

### 2. Las acciones de cuentas, en la tanda

- `tanda.rs`: `crear <nombre> <uid> [nombre completo]` (uid 0: el siguiente), `borrar <nombre>`, `devolver <nombre>`,
  `meter <nombre> <grupo>` y `sacar <nombre> <grupo>`, con su frase en llano ("Crear la cuenta "Berta" (Berta Gil),
  número 1002, con su volumen cifrado"). Una cuenta nueva por tanda (su contraseña va en la ventana).
- **Se prueban antes de preguntar**: `sesiones` hace la tanda entera sobre una copia de las cuentas, en orden (crear y
  luego meter en un grupo vale; meter y luego crear, no). Si una no se podría hacer, no sale la ventana y se dice por
  qué (estado `cuentas::Fallo::estado`, de -101 en adelante). Al hacerla de verdad se vuelve a mirar.
- **Crear**: con la contraseña buena de un administrador (activo, del grupo `admin` del fichero), `sesiones` pide al
  servicio de archivos el volumen cifrado con un cliente que tiene el usuario de la cuenta nueva y el derecho de
  administrar: su raíz es suya desde el principio, se pone 0700 y se cierra. `HACER_VOLUMEN` con `CON_RECUPERACION`
  (nuevo) hace además la ranura del código de recuperación, del azar del servicio (24 letras sin las que se confunden,
  120 bits; `cuentas::codigo_recuperacion`), y lo contesta en la ventana. Si no se puede guardar la cuenta, el volumen
  se quita.
- **La contraseña de la cuenta nueva y el código, solo en `guardian`**: la ventana de la tanda tiene una caja más
  ("Para la nueva") cuando crea una cuenta; Intro con una caja vacía pasa a la siguiente. Hecha la cuenta, `sesiones`
  manda el código a `guardian` ("*codigo"), que lo enseña en su ventana para apuntarlo en papel; el programa que la
  pidió no ve ni la contraseña ni el código. 8 letras como mínimo.
- **Borrar** = retirar: no entra, su volumen queda cerrado, se puede devolver 30 días (`PLAZO_RETIRADA_S`). Al
  arrancar, las vencidas se borran del todo con su volumen (`sd::quitar_volumen`). Sin hora creíble (la Pi 4 sin
  reloj), no se borra (el plazo no se podría contar) ni vence nada. Ni la cuenta propia (de quien lo pide ni de quien
  lo confirma), ni la del último administrador, ni una con la sesión abierta.
- Cada cambio, una línea en `Estado:Registros/seguridad` (además de la de la tanda): "crea la cuenta Berta (usuario
  1002), con su volumen cifrado", "borra la cuenta...", con quién lo pidió y con la contraseña de quién.

### 3. La orden `Usuario`

`ordenes/src/base/usuario.rs` (`ACCION,NOMBRE,MAS/M,UID/K/N`): `Usuario` o `Usuario LISTA` (número, nombre completo,
grupos y, si está borrada, hasta cuándo se puede devolver), `Usuario CREAR Berta "Berta Gil" +fotos [UID n]`,
`Usuario BORRAR Carla`, `Usuario DEVOLVER Carla`, `Usuario GRUPOS Carla` (ver) y `Usuario GRUPOS Carla +fotos -admin`.
Arma la tanda y la pasa por `sesiones` y `guardian` como `Administrar` (que ahora también explica los fallos de
cuentas). Va también como programa suelto (`orden-usuario`).

### Un hallazgo: el Argon2 de verdad tumbaba el servicio de archivos

La primera vuelta en QEMU creaba el volumen con `Cost::NORMAL` (64 MiB, el "medio segundo en una Pi 4" de S6b): el
servicio de archivos pidió 64 MB, topó con el tope de su grupo (64 MB), entró en pánico y cayó (lo relanzó `inicio`,
pero lo que tenía abierto se perdió). Nadie lo había usado antes (S6b lo tenía en "lo que no se comprobó"). Ahora
`HACER_VOLUMEN` cifrado usa `Cost::SERVICE` (nuevo en `dashfs`): 16 MiB y 3 pasadas, más que el mínimo de OWASP para
Argon2id (12 MiB y 3). Crear una cuenta (dos ranuras) tarda 236 ms en QEMU. `Cost::NORMAL` sigue en las herramientas
de Linux.

### Otro: el código se borraba antes de copiarlo

En la segunda vuelta, los 14 primeros caracteres del código salían como ceros: `sesiones` borraba la contraseña de la
ventana después de la orden, cuando ya estaba el código en su sitio. La comprobación del disco lo vio (el código de la
consola no abría el volumen). Arreglado: el servicio ya borra la contraseña al copiarla.

### Pruebas

- PC: `usuario/src/cuentas.rs` (11: las de las pruebas, líneas malas que no tiran las buenas, ficheros truncados y
  enormes, grupos malos, nombres hostiles (`:`, saltos de línea, largos, ñ, reservados, repetidos con otras
  mayúsculas), números usados o fuera de rango, llenas, retirar y devolver (la propia, el último administrador, el
  plazo con un reloj de antes), grupos, 5.000 ficheros al azar que se leen, escriben y releen igual, los fallos como
  estados y el código de recuperación) y `tanda.rs` (19 con las de cuentas: lo que vale, lo hostil, las frases), en
  `make prueba`. `dashfs`: `made_with_a_recovery_code` (abre con la contraseña y con el código, no con otra; la raíz,
  de la persona). `cargo test` de `dashfs` (184) y `shell` (71): bien.
- QEMU (`make arranque-virt`), en el guion del Shell: `Usuario`; `Usuario CREAR Berta "Berta Gil" +fotos` (la ventana
  con las tres cajas; el guion teclea la contraseña de Ana y la de Berta; el código sale en la ventana de `guardian` y
  el guion pulsa Intro); `GRUPOS`, `BORRAR` (la lista dice hasta cuándo se puede devolver), `DEVOLVER`, `GRUPOS -fotos`
  (el grupo vacío desaparece). Lo hostil, sin ventana: nombre repetido con otras mayúsculas, con `:`, con salto de línea,
  de 40 letras, `SYS`, `UID 1001`, la cuenta propia, la última administradora, una que no existe, dos cuentas nuevas en
  una tanda, meter antes de crear; escribir, borrar o crear ficheros en `SYS:Estado/Cuentas` (protegido). Con ventana:
  la contraseña mala de un administrador, la buena de Carla (no es administradora) y una contraseña corta para la
  cuenta nueva: Dora no se crea. En `Programas/espera` (lo hostil de la tanda, sin `Usuario` delante): las mismas
  listas malas directas a `sesiones`, y `HACER_VOLUMEN` y `QUITAR_VOLUMEN` sin el derecho de administrar (-12).
- El disco de pruebas, a propósito con cuatro líneas malas: `sesiones` las salta y lo dice, y Ana y Carla entran.
- Tras la prueba, en Linux (`disco_archivos.py --comprobar`): `usuarios` y `grupos` como los dejó el guion (Ana,
  Carla y Berta; `admin:10:Ana`), nada más en el cajón; y el volumen de Berta, en una copia del disco, abre con su
  contraseña y con el código que enseñó `guardian` (en las pruebas lo escribe también en la consola), y no con la de
  Ana ni con el código sin guiones.
- Resultados del árbol final: `make prueba`, `make arranque-virt`, `make arranque-virt-2`, `make arranque-virt-linux` y
  `make arranque-virt-shell`: bien a la primera (en la máquina había pruebas de otros agentes a la vez). Antes, dos
  vueltas de `arranque-virt` con los dos fallos de arriba (Argon2 y el código).
- Capturas (fuera de git): `~/recuperar/geosian-7d/cuentas-capturas/` (la ventana de la tanda con la caja de la
  cuenta nueva y la del código).

### Lo medido

Crear una cuenta (volumen con dos ranuras de Argon2id de 16 MiB y 3 pasadas): 236 ms en QEMU. `sesiones` se queda
esperando mientras (como con cualquier orden al servicio de archivos). Las órdenes: `C/Ordenes` crece con `Usuario`.

### Pendiente del usuario

- **El plazo de una cuenta borrada**: 30 días (S6b: "por decidir"). Borrar del todo pasa al arrancar, si la hora es
  creíble.
- **El coste de Argon2 de los volúmenes que hace el sistema**: 16 MiB y 3 pasadas, porque 64 MiB no caben en el
  servicio de archivos. La otra salida es subir el tope de su grupo (y que el montón devuelva lo grande); S6b decía
  "medio segundo en una Pi 4, 64 MB".
- **La contraseña de una cuenta nueva**: la teclea el administrador en la ventana de `guardian` (no la ve el programa),
  una vez, sin repetirla; 8 letras o más.
- **El código de recuperación solo en `guardian`**: el programa que pide la cuenta no lo ve. En la imagen de pruebas
  `guardian` lo escribe en la consola (la prueba lo usa); en la de usar, no.
- **Los nombres**: de 1 a 16 letras sin acentos, cifras, `-` y `_`, empezando por letra; los reservados (volúmenes y
  asignaciones del sistema, `admin`, `sistema`, `nadie`, `root`). La orden `Usuario` y sus palabras (`LISTA`,
  `CREAR`, `BORRAR`, `DEVOLVER`, `GRUPOS`, `+grupo`/`-grupo`, `UID`), y "Para la nueva" en la ventana.
- **Una cuenta nueva por tanda**.
- **Un grupo para compartir que se queda vacío se quita** (`admin` no); meter en uno que no existe lo crea (gid desde
  el 100).

### Lo que queda

- Entrar con el código de recuperación y poner contraseña nueva (no existe aún: el código abre el volumen, pero no hay
  pantalla que lo pida), y cambiar la propia contraseña.
- Los grupos de más en la ficha y en el alta de archivos (hoy solo el gid propio): ser de `fotos` aún no da nada.
- El programa `Usuarios` de `SYS:System` (S4b) y la lista de cuentas en la pantalla de entrada.
- El primer arranque (un asistente que crea la primera cuenta, S6b): sin `Estado:Cuentas/usuarios` nadie entra.
- Un número de cuenta no se reusa mientras haya una retirada con él; una vez borrada del todo, si era la mayor, sí.
- `Estado/` del disco de pruebas es de Ana (todo el disco lo es): podría mover `Estado/Cuentas` y poner otro; `sesiones`
  no lo leería (no es suyo) y nadie entraría. En un disco de verdad, `Estado/` será del sistema.
- `archivos` cae si una orden pide más memoria de la que le cabe (aquí, Argon2): debería contestar un error (regla 7
  de S6b). No lo he tocado: es de quien lleva `archivos`.
- Un corte de luz entre hacer el volumen y escribir la cuenta deja un volumen sin cuenta (crear otra con ese nombre da
  "ya existe" del servicio).

### Lo que no se comprobó

- Entrar como Berta de verdad (en QEMU): se comprobó en Linux que su volumen abre con su contraseña y con el código.
- Que la raíz del volumen nuevo quede 0700 (lo pone `sesiones` con el usuario de la cuenta; no hay prueba que lo lea).
- El borrado del todo al vencer el plazo (necesita un disco con una cuenta retirada hace más de 30 días): solo la
  prueba de PC de `vencida`.
- La ventana con el teclado real (el guion teclea por el monitor de QEMU), ni en la Pi.
- Crear una cuenta en la Pi (el tiempo de Argon2 allí).
