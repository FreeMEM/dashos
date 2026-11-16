# Ajustes en texto, avisos de cambio y la orden `Ajuste`

Rama `ajustes-texto`, desde `revision` (2344aad5). Encargo: la regla 1 de S36 ("cada ajuste es un fichero de texto en
`ENVARC:` que manda; la ventana y la orden escriben ese mismo fichero; editarlo a mano vale igual"), con los avisos de
cambio en `archivos` que dejó pendientes la bitácora de idiomas, `ENV:` vigilado, un formato de texto común, la orden
`Ajuste` y el centro de control escribiendo esos ficheros. Diseños de partida: S0 ("Amigable de 2026, con fondo para
informáticos"), S36 (reglas 1 y 2), S27 (apartado 7), S4b (`ENV:`, `ENVARC:`, "Prefs") y las bitácoras de asignaciones
(`RAM:` en memoria) e idiomas.

## Diseño en tres frases

Un ajuste es una clave de un fichero de texto (`clave = valor`, comentarios con `#` o `;`) en `ENV:` (en uso, en el
`RAM:` de la sesión) y en `ENVARC:` (guardado, en `Home:Prefs/Env-Archive`); quien lo cambia toca solo su línea, y donde
hace falta un IFF del Amiga (`Sys/locale.prefs`) se genera del texto. El servicio de archivos avisa de los cambios
(`VIGILAR`): un timbre por vigilancia y una cuenta de cambios que se pide aparte, así que no se pierde ninguno ni se
llena ninguna cola. El centro de control vigila `ENV:` y aplica en marcha el idioma y el tema, los cambie la orden
`Ajuste`, un editor u otro programa; el tema lo reparte el compositor y lo cambian la barra y los programas del kit.

## 1. El formato: `idioma::texto` (`usuario/src/ajustes_texto.rs`)

Lo que ya había no era un formato: una palabra por fichero (`Prefs/zona`, `Prefs/Red/hora`, `C/Prefs/fondo`) o palabras
con espacios (`C/Prefs/apariencia`: "oscuro fondo 3d6f88 ..."), y un `hosts` con `#`. Se eligió el de siempre:

```
# Idioma y país (Ajustes > Idioma y región; la orden `Ajuste idioma`).
# Este fichero manda: Sys/locale.prefs, el que lee la caja Amiga, se genera de él.
idiomas = english, español
pais = great_britain
```

- Una cosa por línea: ajuste, comentario (`#` o `;`, como los guiones del Amiga) o blanco. Sin comentarios al final de
  una línea (un color `#3d6f88` es un valor).
- La clave: letras, cifras, `_` y `-`, hasta 64 bytes, sin distinguir mayúsculas. Sin punto: la orden nombra un ajuste
  como `fichero.clave`.
- El valor: lo de tras el `=`, sin los espacios de los lados, sin bytes de control, hasta 1 KB.
- Clave repetida: vale la primera (como en los catálogos); las demás se señalan.
- Una línea que no vale no rompe el fichero: se guarda tal cual, no cuenta y se señala con su número.
- Cambiar un ajuste toca solo su línea (lo de antes del valor se queda: la clave, los espacios, el `=`); uno nuevo va
  al final. Comentarios, orden, `\r\n` y el salto del final se conservan: leer y escribir sin cambiar nada da los mismos
  bytes.
- No vale entero (y quien lee se queda con lo de serie): lo que no es UTF-8, más de 64 KB o de 2.048 líneas.

Vive en el crate `idioma` como `idioma::texto` (`kit::idioma::texto` para los programas): usa `alloc`, y la biblioteca
`dashos` no lo enlaza (se probó: los programas sin montón dejaban de enlazar).

Pruebas (en el `cargo test` de `idioma`): leer y escribir igual, cambiar sin perder nada, repetidas, y hostil (no UTF-8,
enorme, demasiadas líneas, claves y valores malos, llenar hasta el tope, 2.000 ficheros de bytes al azar que se leen y
se vuelven a escribir igual).

## 2. Avisos de cambio en `archivos` (`VIGILAR`, `NO_VIGILAR`, `AVISOS`)

Como `StartNotify` del Amiga. Órdenes **80, 81 y 82** (números altos para no chocar con otras ramas). Referencia en
`docs/referencia/servicios.md`; la lógica, sin nada de DashOS, en `usuario/src/vigilancia.rs` (pruebas de PC en
`make prueba`); pedir una vigilancia desde un programa, `dashos::vigilar::vigilar` (va aparte del protocolo, que se
incluye en pruebas de PC sin DashOS).

- `VIGILAR` sobre una asa (fichero o cajón) con una llave: la **puerta** por la que el servicio toca el timbre (16 bytes:
  `VIGI`, número de la vigilancia, cambios). Normalmente una copia marcada del canal donde ya espera el cliente: un
  proceso solo espera en un canal.
- **Un timbre por vigilancia** hasta que el cliente pide los avisos (`AVISOS`), que dan por cada una el número, las
  marcas (cambio, borrado, fuera, movido) y **cuántos cambios**, y los ponen a cero. Mil escrituras seguidas son un
  timbre y una cuenta de mil. Lo que espera un cliente es un número por vigilancia, nunca una cola: no se desborda.
- **No se pierde nada**: el cambio que llega tras pedir los avisos vuelve a tocar. Quien pide los avisos y después lee
  ve siempre lo último.
- Puerta llena: el timbre se guarda y se reintenta con un temporizador (20 ms). Puerta cerrada (quien vigilaba ya no
  lee): la vigilancia se quita. El cliente se va: se quitan todas.
- Topes: 64 por cliente; con 64 clientes, 4.096 como mucho en el servicio. Uno que pide miles no deja sin sitio a otro.
- Hace falta poder leer lo vigilado (un cajón, listarlo): vigilar no enseña nada que no se pueda leer.
- Qué cuenta: de un fichero, escribir, recortar, comentar, proteger, fechar, renombrar (`MOVIDO`) o borrar (`BORRADO`, y
  acaba: el número de nodo se puede volver a usar); de un cajón, crear, borrar o renombrar dentro y cambiar un fichero de
  dentro. Un volumen quitado, cerrado o vuelto a una instantánea acaba sus vigilancias con `FUERA`; el `RAM:` de una
  sesión que se va, igual. Copiar un árbol a `RAM:` (`RAM_COPIAR`, al entrar) es un cambio en todas las de ese volumen.
- Para saber de qué cajón es un fichero que se escribe, el servicio apunta el cajón de cada asa que da al buscar o
  crear (el nodo de RedoxFS no guarda a su padre).

Pruebas en QEMU (`Programas/ram`, como Ana y Carla): ver "Pruebas".

## 3. `ENV:` vigilado: el idioma y el tema en marcha

- **Idioma**: `ENV:Sys/locale` (texto, claves `idiomas` y `pais`) manda; `Sys/locale.prefs` (el IFF del Amiga) se genera
  de él cuando lo escribe la orden `Ajuste` (con los datos del país de su `.country` y la diferencia con Greenwich de la
  zona del sistema). Lo nativo (`Idiomas::cargar`) mira, en orden, `ENV:Sys/locale`, `ENV:Sys/locale.prefs`,
  `ENVARC:Sys/locale` y `ENVARC:Sys/locale.prefs`: un texto sin idiomas ni país (solo comentarios) deja paso al IFF de su
  capa; uno roto manda (lo de serie), como ya pasaba con el IFF. El nombre `Sys/idioma` no se usó: lo usa como dato de
  prueba `Programas/ram`.
- **Tema**: `ENV:Escritorio/apariencia`, claves `modo` (claro u oscuro) y `acento` (violeta, azul, verde, naranja o
  fondo).
- **El centro de control** (que ya era el servicio del tema, S26) vigila `ENV:`, `ENV:Sys` y `ENV:Escritorio` con una
  copia de su "yo:centro" como puerta. Antes de entrar nadie no hay `ENV:`: lo reintenta cada 2 s hasta que lo hay, y
  otra vez si la sesión se va. Con un aviso, pide los avisos, carga otra vez el idioma (sin mirar firmas: la fecha va en
  segundos y dos cambios del mismo tamaño en el mismo segundo no se distinguen) y, si el tema de `ENV:` es otro, lo pone
  en el compositor, que lo reparte: la barra y los programas del kit (Ajustes) cambian sin reiniciar.

## 4. La orden `Ajuste` (`ordenes/src/base/ajuste.rs`, en `SYS:C/Ordenes`)

```
Ajuste LISTA                          los que conoce, con su valor y su fichero
Ajuste idioma                         el de ENV: (o, si no está, el guardado)
Ajuste idioma "english, español"      en ENV:
Ajuste tema oscuro GUARDAR            en ENV: y en ENVARC:
Ajuste Escritorio/barra.segundos sí   cualquier fichero.clave dentro de ENV:
```

Plantilla `NOMBRE,VALOR,GUARDAR/S,LISTA/S` (S27 decía "`Ajuste LEE clave` / `PON clave valor` con `USE`/`SAVE`, sintaxis
por diseñar"; el encargo pidió esta). Conoce `idioma`, `pais`, `tema`, `acento` y `zona`, y comprueba lo que se pone: un
idioma con nombre de fichero válido (y avisa si no tiene `.language`), un país con su `.country`, un tema y un acento de
la lista. Escribe en la primera capa de la asignación (`RAM:Env`, `Home:Prefs/Env-Archive`), creando los cajones que
falten, sobre lo que había (sus comentarios se quedan). Si algo no vale, no escribe nada. Un fichero que no es texto o
que pasa de 64 KB no se pisa: lo dice y pide arreglarlo o borrarlo.

**Lo protegido**: la zona horaria es de la máquina (`SYS:Prefs/zona`): se lee, pero ponerla se niega ("pide la
confirmación de `guardian`, y esta orden aún no sabe pedirla"). Permisos y cuentas no son ajustes de esta orden.

## 5. El centro de control

Vigila `ENV:` y aplica el idioma y el tema en marcha (apartado 3). Y la apariencia ya no va a `C/Prefs/apariencia`:
está en `ENV:Escritorio/apariencia` (en uso) y `ENVARC:Escritorio/apariencia` (guardada), en texto (`modo`, `acento`
y, con la paleta del fondo, `semilla` y `esquemas`, lo calculado), el mismo fichero que escribe la orden `Ajuste`. Un
solo sitio: `C/Prefs/apariencia` ya no se lee ni se escribe, y la comprobación del disco tras la prueba exige que no
esté.

- **Guardar sin tener el `Home:`** (decidido con el coordinador, 2026-11-16): el centro no tiene raíz en el `Home:`
  cifrado (bloquearía cerrar la sesión). Escribe `ENV:` (su raíz de `RAM:` pasa a ser de leer y escribir) y, para
  guardar, pide a `sesiones` la orden nueva **`GUARDAR_ENV` (90)**, que hace `archivos` con la orden nueva
  **`RAM_GUARDAR` (120)**: copia `Env/<fichero>` del `RAM:` de la sesión a `Prefs/Env-Archive/<fichero>` del volumen del
  usuario (cajones 0700, fichero 0600, suyos). Condiciones: solo los programas de `GUARDAN_AJUSTES` (hoy el centro, por
  el nombre que da `inicio`; uno del disco con ese nombre no vale); siempre la sesión de quien lo pide (el mensaje no
  nombra ninguna); solo nombres que no salen del cajón (`archivos::ajuste_valido`, comprobado en `sesiones` y otra vez
  en `archivos`); hasta 64 KB y solo ficheros. Lo negado va al registro de seguridad de `sesiones` (`R_AJUSTE`, 90); el
  registro común de `registro-permisos` tiene su hueco marcado en el mismo sitio. Lo de la máquina sigue pidiendo
  `guardian` (la orden `Ajuste` no lo pone; `GUARDAR_ENV` solo llega al `Home:` del usuario).
- **La ventana** (Apariencia): lo que se elige se ve ya y va a `ENV:`; tres botones, como en las Prefs del Amiga:
  **Guardar** (a `ENVARC:`, por `sesiones`), **Usar** (se queda en `ENV:`, sin guardar) y **Cancelar** (vuelve a como
  estaba al abrir). Cerrar con cambios sin Guardar ni Usar es Guardar (S27). El panel rápido (modo claro u oscuro)
  escribe `ENV:` y guarda.
- **Antes de entrar nadie** no hay `ENV:` ni `ENVARC:` del usuario: vale la de fábrica (`SYS:Prefs/Env-Archive`, por
  `ENVARC:`) o la de serie, y lo que se elige queda en uso sin guardarse ("en uso (sin sesión: no se guarda)"). En la
  prueba, toda la parte de Apariencia de la ventana pasa antes de entrar Ana: por eso sus líneas lo dicen así. Al entrar,
  manda lo que haya en su `ENV:`.
- El explorador y el Workbench no necesitan vigilar el tema: lo reciben del compositor (`dashos_tema_v1`) y cambian en
  marcha como la barra. El fondo, el panel, el monitor y el aspecto de las cajas siguen en `C/Prefs/*` (no eran del
  encargo).

## Pruebas

Resultado sobre lo último: `make prueba`, `arranque-virt` (a la segunda: la primera, la ventana de demostración de
ReAction de la caja Amiga salió encima de la del centro justo cuando el guion iba a pulsar Quitar, y el guion se
quedó esperando; no se repitió), `arranque-virt-2`, `arranque-virt-linux` y `arranque-virt-shell`: bien.

- PC: `cargo test` de `idioma` (los del formato, `Sys/locale` que manda, la lista de idiomas), `shell` y la de
  `vigilancia.rs` (ráfaga, puerta llena y cerrada, borrado y fuera, miles y el que se va).
- QEMU (`make arranque-virt`), en `Programas/ram`:
  - Ana vigila `ENV:Sys` y `ENV:Sys/teclado`: 200 escrituras seguidas, 2 timbres y cuentas [200, 200]; pedidos los
    avisos, uno más suena otra vez; crear y borrar en el cajón cuentan para él; borrar lo vigilado avisa y la vigilancia
    acaba.
  - Hostil: vigilar `SYS:DeCarla` (de Carla, 0700) -12; una asa mala -5; sin puerta -1; una puerta cerrada se quita
    sola; 63 vigilancias con la misma puerta y nadie leyendo: llegan los 63 timbres (por el reintento); 5.000 pedidas:
    1 sí y 4.999 sin sitio, y el servicio sigue. Se va con 64 puestas: Carla, después, ve que se soltaron.
- QEMU, en el guion del Shell (`S/Prueba-Shell`, como Ana): `Ajuste LISTA`, idioma en inglés y vuelta al castellano
  (el centro cambia de idioma en marcha), un país que no hay, un idioma que no vale, un tema que no existe, la zona (se
  niega), un nombre que no hay, un fichero que no es texto y uno enorme (no se tocan), un `fichero.clave` con GUARDAR, y
  el tema con la orden, a mano con `Echo` y otra vez con la orden (el centro lo aplica; Ajustes y la barra lo dicen).

## Medido (QEMU `virt`, con otras pruebas corriendo en la máquina; una vuelta, sin repetir)

| Qué | Tiempo |
|---|---|
| 200 escrituras de un byte en `ENV:Sys/teclado`, sin nadie vigilando | 101,7 ms |
| Las mismas, con el fichero y su cajón vigilados (y su timbre) | 113,7 ms (otra vuelta: 96,8 y 110,8) |
| 5.000 `VIGILAR` seguidos (4.999 sin sitio) | 444 a 454 ms (unos 90 µs cada uno, con el viaje) |

La diferencia de la segunda fila (unos 60 µs por escritura) está dentro de lo que varía QEMU con la máquina cargada:
no se separó del ruido. Sin nadie vigilando, cada orden que cambia algo hace una búsqueda en un mapa vacío. "Antes" no
se midió con el servicio de antes de esta rama.

## Decisiones pendientes del usuario

1. **Formato de los ajustes**: `clave = valor`, `#` y `;` de comentario, sin comentarios al final de línea, la primera
   clave repetida vale, sin distinguir mayúsculas en la clave.
2. **La sintaxis de `Ajuste`**: la del encargo (`Ajuste nombre valor GUARDAR`, `Ajuste LISTA`), no la de S27 (`LEE`,
   `PON`, `USE`, `SAVE`). Los nombres cortos (`idioma`, `pais`, `tema`, `acento`, `zona`) y la forma `fichero.clave`.
3. **Nombres de fichero**: `Sys/locale` (texto) junto a `Sys/locale.prefs` (IFF), y `Escritorio/apariencia`.
4. **Un fichero roto no se pisa** con la orden: se pide arreglarlo o borrarlo. La otra opción, escribirlo de nuevo, pierde
   lo que alguien escribió.
5. **El texto manda siempre sobre el IFF**: si un programa 68k escribe `Sys/locale.prefs`, lo nativo no lo ve mientras
   haya `Sys/locale` con idiomas o país.
6. **Vigilar pide poder leer** (un cajón, listarlo), y el tope es de 64 por programa.
7. **La zona horaria es de la máquina** (como `hora` y `red`, S4b): la orden la lee y se niega a ponerla.
8. **Quién puede pedir `GUARDAR_ENV`**: solo el centro de control. La orden `Ajuste` no lo necesita (el Shell tiene el
   `Home:`).
9. **La ventana sobrescribe un `Escritorio/apariencia` que no se entiende** al elegir (es una elección entera); la
   orden `Ajuste`, en cambio, no pisa un fichero roto.
10. **Antes de entrar nadie, lo elegido en Apariencia no se guarda** (no hay de quién).

## Lo que queda

- Los botones Guardar, Usar y Cancelar de la ventana **no se pulsan en la prueba**: el guion pasa por Apariencia antes
  de entrar Ana, sin `ENV:`. Guardar por `sesiones` sí se prueba (el modo del panel, ya dentro), y lo hostil también.
  Probarlos pide mover el tramo del centro en `guardian` y en el guion tras la entrada, que cambia mucho de lo
  esperado (el tema de las pantallas de bloqueo, por ejemplo).
- Lo que se ve de la apariencia antes de entrar nadie (la pantalla de entrada) debería ser un ajuste de la máquina
  (`Estado:Prefs`, con `guardian`), no lo último elegido.
- Los demás ficheros del centro (`C/Prefs/panel`, `fondo`, `monitor`, `cajas`) a `ENV:`/`ENVARC:`, igual.
- Regenerar `Sys/locale.prefs` cuando el texto se edita a mano (hoy solo lo hace la orden): un servicio de ajustes que
  vigile y lo rehaga; y el sentido contrario (lo que escribe el `Locale` de 68k, al texto).
- La barra no cambia de idioma: es del sistema (usuario 1), común a todas las sesiones, y no tiene el `ENV:` de nadie.
  Cambia de tema en marcha (por el compositor). La zona de la barra tampoco se vigila aún (es de la máquina y nada la
  escribe sin `guardian`).
- `Ajuste` con la confirmación de `guardian` para lo de la máquina, y la capa de la máquina (`Estado:Prefs`).
- Los demás programas que leen idioma (explorador, Workbench) no vigilan aún: lo ven al abrirse.
- Avisos de un fichero abierto por una asa cedida (`CEDER`) a quien vigila su cajón: el servicio no sabe su cajón.
