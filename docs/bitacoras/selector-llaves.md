# El selector para todos, "Guardar como" y la tabla de llaves

Rama `selector-llaves` (desde `revision` 41e5b81). Los tres pendientes que dejó la sesión de Dash al hacer las
ventanas de Dash en DashOS (`docs/bitacoras/dash-ventanas.md`).

## Qué hay

### 1. La tabla de llaves: qué es cada llave (pendiente 3)

- Quien lanza un programa dice al kernel el nombre de cada llave que le da (llamada 12 con el bit 16 de `x3`,
  `CON_NOMBRES`: tras los números de llave van 16 bytes de nombre por llave). El kernel los guarda con el proceso
  (`Proceso::nombres`, 129 bytes, sin montón) y el programa los lee con la llamada nueva 56 (`llaves_nombres`). El
  programa no puede cambiar su tabla. Sin el bit, todo como antes (tabla vacía).
- `dashos::llaves` (`usuario/src/llaves.rs`): `Tabla::leer()`, `buscar("ventanas")`, `nombre(n)`;
  `dashos::proceso_crear_con_nombres`. No se fía de lo que llega: solo cuentan los nombres enteros.
- `inicio` nombra todo lo que da: los aparatos como en el paquete de arranque, los canales por su nombre (el paquete
  lleva ahora los nombres de los canales tras las cabeceras de los programas), "yo:canal" la copia para escribirse a
  sí mismo y "archivos.altas" la copia sin marca que recibe `sesiones`. `sesiones`: "sesiones", "archivos",
  "ventanas", "selector". La tabla entera, en `docs/referencia/servicios.md` ("La tabla de llaves").
- Por qué en el kernel y no como una llave más con la tabla dentro: una llave más cambia cuántas trae el programa, y
  el runtime de Dash de hoy decide por esa cuenta (con 5 dejaría de encontrar el selector en la prueba de `inicio`).
  Así ningún programa que hay ve nada distinto: los números de siempre no cambian.
- kit-c: el compositor es la llave que se llama "ventanas" en la tabla, y manda sobre `dash_ui_usar_llave` (el Dash
  de hoy la llama con lo que adivina; con las 4 llaves de `sesiones` adivina mal). Función nueva `dash_llave(nombre)`.

### 2. `sesiones` da el selector (pendiente 1)

- `inicio` manda a `sesiones` una copia sin marca del canal "selector" (como las del compositor y el sonido).
- Al lanzar, `sesiones` da la llave del selector (la cuarta, "selector") a los programas **de la persona del
  selector** y antes se lo presenta (marca y nombre, como la entrada de directorio de `inicio`). Al acabar o matarlo,
  le dice que se fue. Si `inicio` relanza el selector, se los presenta otra vez.
- Quién, según S6b: "sin pedir nada: su cajón, su paquete, ventanas propias, sonido de salida y lo que el usuario le
  entregue". El selector es cómo el usuario entrega: lo recibe todo programa de una persona (perfiles aplicación,
  suelto, y lo demás que lance `sesiones` como ella). No los servicios (no tienen usuario que elija) ni, hoy, los de
  otra persona: el selector corre como Ana y enseña lo de Ana; dárselo al programa de Carla sería enseñarle a Carla lo
  de Ana. Falta un selector por sesión (apuntado abajo).
- El selector solo atiende a quien le presentaron. "raiz" (ceder `C` entero) queda solo para el `hostil` de `inicio`:
  con el selector en manos de cualquier programa era leer todo `C` sin que el usuario eligiera nada.

### 3. El protocolo del selector: nombre, guardar, patrón (pendiente 2)

- Protocolo nuevo (`usuario/src/protocolo/selector.rs`, documentado en `docs/referencia/servicios.md`, "Selector de
  ficheros"): peticiones "SEL2" con orden (abrir, guardar, carpeta, cancelar, consulta), nombre propuesto y patrón;
  respuesta con estado, asa, banderas y **el nombre de lo elegido**. Las peticiones de antes ("abrir", "carpeta")
  siguen igual: el `hostil` y la prueba de Dash no cambian.
- **Solo el nombre, no el camino** (decisión, con S6b): el camino dice cómo se llaman los cajones del usuario y dónde
  guarda sus cosas, y un programa encerrado no tiene por qué saberlo ("si cae una aplicación gana su cajón y los
  ficheros que el usuario le cedió, no el resto de `Home:`"). El nombre sí: es parte de lo que el usuario entrega
  (título de la ventana, formato por la extensión).
- **Guardar**: campo "Nombre" con lo que propone el programa, botón "Guardar". El selector crea el fichero (vacío) o,
  si existe, pregunta bajo el campo ("ya existe. ¿Sustituir?") y el botón pasa a "Sustituir"; Intro no sustituye. Se
  cede la asa de ese fichero para leer y escribir, y nada del cajón. Si existía llega como estaba, con la bandera
  `EXISTIA` (el selector no borra datos: quien guarda lo deja del tamaño de lo que escribe).
- **Nombres**: uno solo. Sin '/', ':', '|', letras de control, "." ni "..", y 107 bytes como mucho. Lo mismo para lo
  que teclea el usuario (se dice bajo el campo) que para lo que propone el programa (el campo empieza vacío).
- **Patrón**: "*.txt;*.md", con '*', '?' y "#?"; solo decide qué se enseña. Sin vueltas exponenciales.
- **Varios a la vez**: una petición por programa y cuatro esperando; las demás, ocupado. El selector mira su canal
  mientras tiene la ventana abierta: quien pidió puede cancelar, y si acaba, la ventana se cierra.
- Carpeta (nueva): en el primer hueco de raíz libre de 15 a 8 (lo lanzado por `sesiones` ya tiene la 3). La de antes
  prueba la 3 y, si está ocupada, esos.
- **Fallo encontrado y arreglado en `archivos`**: con una asa cedida solo para leer se podía recortar el fichero
  (`TAMANO`) y cambiarle comentario, protección y fecha si era del mismo usuario (el tope de una asa cedida era leer y
  escribir). Ahora el tope de una asa cedida son los derechos con que se cedió. Con el `hostil` no se veía porque es
  de otro usuario; con `sesiones` dando el selector a los programas de Ana, sí.

## Pruebas

- PC (`make prueba`): `usuario/src/llaves.rs` (5: buscar, sin tabla, truncada, nombres raros) y
  `usuario/src/protocolo/selector.rs` (7: peticiones y respuestas malas, nombres, patrones, patrón hostil).
- Kernel (`make prueba`, `padre` e `hijo`): un hijo con llaves con nombre las encuentra; sin nombres, tabla vacía;
  búfer corto o ajeno; un nombre de menos o de más; 9 llaves; nombres que no se pueden leer (no se crea nada y la llave
  no se mueve).
- QEMU (`make arranque-virt`): `pide` (del sistema y de Carla) y el `cambio` de Carla nacen sin selector; el de Ana lo
  tiene y el selector lo conoce. `Programas/guarda` (nuevo, lo lanza el guion desde la barra, lo último de la prueba):
  encuentra el selector por su nombre; "raiz", petición mal formada, crear en `C` por su cuenta; un camino como
  nombre, dos peticiones a la vez, cancelar con la ventana abierta, nombres con ':', ".." y de 200 bytes; guarda
  "informe.txt", lo sustituye (antes, un nombre vacío en el campo) y lo abre con patrón solo para leer (no lo escribe,
  recorta ni fecha). La comprobación del disco mira que `C/informe.txt` tenga lo segundo.

## Lo que tiene que cambiar el runtime de Dash (`~/Projects/FreeMEMLang/runtime/dashos/`, no tocado)

Con el kit-c nuevo y sin tocar Dash ya funciona todo lo de hoy (kit-c busca "ventanas" en la tabla; archivos sigue
en la 2 y el selector en la 4 con los dos lanzadores). Para dejar de adivinar:

1. `plat.c`: quitar la cuenta de llaves (`_dash_keys_at_start`, `keys_at_start`) y poner una función que lea la tabla
   una vez: `dash_os_call(56, (long)buf, 128, 0, 0, 0, out)` devuelve cuántos nombres hay (0 o negativo: sin tabla) y
   deja en `buf` 16 bytes por llave, rellenos con ceros; la llave `i + 1` se llama `buf[16 * i ..]`. Una función
   `long dash_plat_key(const char *nombre)` que devuelva el número o 0. (Con kit-c enlazado vale `dash_llave(nombre)`,
   pero los programas sin ventana no lo enlazan.)
2. `plat.c`: `dash_plat_windows_key()` puede devolver `dash_plat_key("ventanas")`; ya no hace falta llamar a
   `dash_ui_usar_llave` (con tabla, kit-c no le hace caso).
3. `dos.c`: `ARCH_LLAVE` pasa a ser `dash_plat_key("archivos")` y `SELECTOR_LLAVE`, `dash_plat_key("selector")`; sin
   selector (0), `dash_plat_choose` da -1. Quitar `keys_at_start != SELECTOR_LLAVE`.
4. `dos.c`, `dash_plat_choose`: pasar al protocolo nuevo (cabecera `kit-c/include/kit_c.h` no lo lleva: es un mensaje).
   Petición: "SEL2", orden u32 (1 abrir, 2 guardar, 3 carpeta), u16 largo del nombre propuesto, u16 largo del patrón,
   4 ceros, y los dos textos; con una llave para la respuesta. Respuesta: i32 estado (0 bien, -100 cancelado, -101
   ocupado), u32 asa, u32 banderas (1: ya existía), u32 largo y el nombre. Así `file_open_dialog` puede devolver el
   nombre ("Elegido:nombre") y `dialog_file_at` con guardar pide la orden 2 (un fichero, no una carpeta): la asa
   llega para leer y escribir; si la bandera 1 viene puesta, dejarlo del tamaño de lo escrito (`TAMANO`, orden 10).
   Esperar sin plazo (el usuario puede tardar) o mandar la orden 4 (cancelar) si el programa acaba antes.
5. La carpeta: el hueco de raíz es el que contesta el selector (ya no es siempre el 3).

## Pendiente del usuario

- Que el programa reciba solo el nombre de lo elegido y no el camino (hecho así).
- A quién da `sesiones` el selector: hoy, a los programas de la persona del selector, sin preguntar ni declarar.
- Al sustituir, el selector no vacía el fichero: lo hace quien guarda. (La otra opción: que llegue siempre vacío.)

## Lo que queda

- Un selector por sesión (hoy uno, de Ana, lanzado por `inicio`, que empieza en `C`): hasta entonces los programas
  de Carla no lo tienen. Y que empiece en `Home:` con los volúmenes y las asignaciones.
- El cliente no puede soltar una carpeta cedida como raíz: como mucho 8 por programa.
- Si quien pidió ya no lee la respuesta, la asa ya está cedida (se suelta cuando acabe).
- Un programa puede pedir el selector una y otra vez (de una en una): no hay límite de ritmo.
- Un botón para ver todos los ficheros cuando hay patrón.
- La caja Amiga (ASL) y el explorador no usan aún el protocolo nuevo.
- `docs/bitacoras/dash-ventanas.md` sigue diciendo que el runtime cuenta las llaves: cambiarlo cuando Dash lo cambie.

## Bitácora

- 2026-11-15: hecho todo lo de arriba (commits e5f8a1b y b84b7c5). Sobre b84b7c5: `make prueba`, `make arranque-virt`
  (con el reinicio, el disco y la prueba de Dash con ventana, sin tocar), `make arranque-virt-2` y
  `make arranque-virt-linux`, todo bien, una vuelta cada uno (`arranque-virt`, dos: la primera antes de los últimos
  retoques de kit-c y de la carpeta antigua). No medido: rendimiento (un programa nuevo paga una llamada al leer su
  tabla; lanzar, 16 bytes más por llave).
- No comprobado: la carpeta nueva (orden 3) y la antigua desde un programa de `sesiones` (el hueco distinto del 3); el
  selector relanzado por `inicio` con programas de `sesiones` vivos; un programa de Dash lanzado desde el disco (no
  hay ninguno en la imagen); el filtro de asignaciones de la rama `asignaciones` no se ha tocado. En la Pi, nada.
