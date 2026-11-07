# Auditoría de cierre de la fase 4 de S16, 2026-11-06

Hecha por un agente con lectura independiente. Mira los commits C45 a C51 (primera parte), de `853c9b7` a `fa1f0c2`,
y lo que tocan de `kernel/` y de los servicios de `usuario/`. Los números de línea son de `fa1f0c2`. Este fichero no se
actualiza: es la foto de ese día.

No se ha cambiado código del repositorio. Para medir A1 hice una copia aparte (fuera del repositorio), con un
programa hostil en lugar de `tiempo` y una medida de tiempo en `temporizadores::repartir`, y la arranqué en QEMU con
`make prueba`. Lo demás está leído, no ejecutado.

Mientras escribía llegó `919e255` (C51, segunda parte: la ventana de entrada del PCIe). Lo he leído por encima; ver
"Qué no he mirado".

Modelo de amenaza, el mismo de la fase 3: un programa hostil en EL0, dentro de un grupo con cupo, con cualquier
llamada, valor e instrucción, y con las llaves que le den. Varios procesos hostiles que colaboran. Un cliente hostil de
un servicio. Un controlador con llave de registros, de interrupción o de permiso DMA es de confianza (S16, D), pero
puede tener fallos o morir a medias.

"Comprobado" quiere decir comprobado leyendo el código, y "medido", ejecutado en la copia. "Plausible" quiere decir que
el camino existe, pero depende de una carrera, del hardware o de tiempos que no he medido. "A comprobar" quiere decir
que no estoy seguro de que pase.

## Estado de la fase 4

| Parte | Estado | Dónde y por qué |
|---|---|---|
| C45. Quitar proyecciones | A medias | `memoria_quitar` y el borrado con su TLB están bien, y la carrera entre proyectar y revocar está bien cerrada (ver "Revisado y correcto"). Pero la lista de proyecciones de un intermediario no se limpia nunca: tras 1024 proyecciones, nadie más puede proyectar por él (M2). Y revocar puede tener que borrar un millón de páginas con las IRQ enmascaradas (M3). |
| C46. Memoria DMA | Bien, con reservas | Permiso solo del kernel, derecho de escribir, tope de 4 MB, cobro al cupo antes de tomar páginas, a cero y fuera de la caché, y sin caché al proyectar para escribir. Pero sus páginas vuelven al repartidor al soltarse aunque el aparato siga escribiendo en ellas (M5). Detalles en B6 y B7. |
| C47. Programa de inicio | Bien, con una reserva | El kernel solo da llaves a `inicio`, y `inicio` lanza cada servicio en su grupo con copias de sus llaves. La reserva: el compositor recibe toda la memoria de la VideoCore (M4). |
| C48. Consola como servicio | A medias | El kernel ya no espera a la serie con la consola en marcha (N2 cerrado), y el pánico escribe directo. Pero la marca "│ " de N6 se puede saltar (B2), cualquier proceso puede llenar el registro y hacer perder las líneas del kernel (B4), y `escribir` calcula mal lo que reserva (B3). |
| C49. USB fuera y temporizadores | El USB bien; los temporizadores mal | El USB es un proceso y el kernel ya no lo toca. Los temporizadores son la parte más débil de la fase: un grupo cualquiera deja el núcleo 0 con las IRQ enmascaradas casi todo el tiempo (A1, medido); un temporizador sobrevive a la revocación de la llave con que se creó y escribe sin marca (A2); un aviso de una vez se pierde si la cola está llena (M1). |
| C50. Buzón, compositor, HVS y V3D fuera | Bien en el kernel; a medias en los servicios | En el kernel, `Objeto::Firmware` solo lo crea el kernel y `proyectar_firmware` vuelve a mirar que no toca la RAM; no queda `unsafe` fuera de `base` salvo las pruebas. En los servicios: un cliente tumba el buzón con un solo mensaje (M6), y el compositor tiene más poder del que necesita (M4). La GPU, sin probar. |
| C51, primera parte | Bien | Solo mueve pruebas a `lanzador`. No añade nada que vea un proceso. |

## Fallos nuevos

### Crítica

Ninguno. No he encontrado forma de que un proceso sin privilegios tumbe el kernel, lea o escriba memoria que no es
suya, o se salga de su cupo con lo que da esta fase.

### Alta

**A1. Los temporizadores dejan el núcleo 0 con las IRQ enmascaradas casi todo el tiempo.** Medido en QEMU.

- `temporizadores.rs:91-116`: en cada tic en que vence alguno, `repartir` recorre **todos** los temporizadores del
  sistema, de todos los grupos, con el cerrojo global `TODOS` tomado. Todo cerrojo enmascara las IRQ (`cerrojo.rs`,
  C35). Por cada uno: `upgrade`, su cerrojo, y quizá un `Arc` y un `push`.
- Corre en el repartidor, en el núcleo 0, que es donde llegan todas las interrupciones de aparatos
  (`interrupciones.rs:10-11` y `61`).
- `temporizadores.rs:53-54`: crear uno hace `retain` de la lista entera, también con `TODOS` tomado. Crear N es
  cuadrático.
- El único límite es el cupo: 256 bytes por temporizador (`cupo.rs:28`). Las llaves se guardan en la tabla (1024) o
  dentro de mensajes en colas de canales (128 por canal). En un grupo de 16 MB caben unos 40.000.

Escenario, desde un grupo cualquiera y sin llaves especiales:

1. `canal_crear` da `x`, el canal al que avisarán todos (su cola se llena y da igual).
2. Repetir: `canal_crear` da `(c0, c1)`; 16 veces, crear 8 temporizadores periódicos de 1 µs sobre `x` y mandarlos en
   un mensaje por `c0`. Cada canal guarda 128.
3. Desde ese momento, cada tic del núcleo 0 recorre la lista entera con las IRQ enmascaradas, y después intenta
   escribir un aviso por cada uno.

Lo medido en la copia, en QEMU (que va más lento que la Pi; lo que vale es la forma):

| Temporizadores | Con `TODOS` tomado (IRQ enmascaradas) | Cada pasada entera |
|---|---|---|
| 1 | 0,2 ms | 0,3 ms |
| 11.000 | 8 a 12 ms | 19 a 22 ms |
| 49.000 | 33 a 36 ms | 81 a 85 ms |

- Con 20.480 temporizadores, uno de 100 ms de otro proceso llegó a los 128 y 145 ms (sin carga, 109 ms).
- Crear esos 20.480 tardó 8 segundos.
- Con 49.000, cada pasada dura más que el tic (10 ms): el repartidor no descansa nunca y el núcleo 0 pasa la mitad del
  tiempo sin atender interrupciones.

En la Pi irá más rápido, pero sigue siendo trabajo sin tope que pone un grupo y paga todo el sistema: todas las
interrupciones de aparatos y todos los temporizadores de los demás. Rompe la decisión E (trabajo acotado).

Arreglo propuesto:

- Un montículo ordenado por vencimiento, para que cada tic solo toque los vencidos, y sin `retain` al crear.
- Un tope de temporizadores por grupo o por proceso, en número y no solo en bytes (por ejemplo 64 por proceso).
- Un tope de avisos por pasada, dejando el resto pendiente para la siguiente.
- Y quizá un periodo mínimo de un tic: hoy uno de 1 µs cuesta lo mismo que uno de 10 ms, pero obliga a pasar por el
  repartidor en cada tic.

**A2. Un temporizador sobrevive a la revocación y escribe sin marca.** Comprobado leyendo.

- `llamadas.rs:423` y `431`: `temporizador_crear` resuelve la llave con `ver` y guarda el `Arc<Canal>` desnudo. No
  guarda el intermediario ni la marca de la llave.
- `temporizadores.rs:119`: cada aviso va con `marca: 0`, la de lo que escribe el kernel.
- El temporizador no tiene una punta: no depende de que siga abierto el lado de quien lo creó.

Escenario, con un servidor S y un cliente hostil K:

1. S da a K una copia revocable y marcada del lado por el que le escriben sus clientes (lo que recomienda
   `kernel.md:197-198`).
2. K crea un temporizador periódico sobre esa llave y pasa la llave del temporizador a un cómplice, o la deja dentro de
   un mensaje en la cola de otro canal.
3. S revoca. La copia de K da `E_LLAVE`, pero el temporizador sigue escribiendo en la cola de S, cada tic, con marca 0.
4. Matar a K, o a su grupo, no lo para: `grupo_matar` cierra los canales que creó el grupo (C44), y el de S no es suyo.

Consecuencias:

- Rompe dos promesas de `kernel.md`: la revocación ("dan `E_LLAVE`, estén donde estén", líneas 225-226) y la marca como
  identidad del que escribe (líneas 196-198). Los mensajes de K llegan como si fueran del kernel.
- Con unos cuantos, la cola de S está siempre llena y sus demás clientes reciben `E_LIMITE`. La única defensa de S es
  cerrar su lado y perder a todos sus clientes.
- Es el B2 de la fase 3 (interrupción que sigue tras revocar), pero ahora sin ninguna llave especial.

Arreglo: que el temporizador lleve la marca de la llave con que se creó, y los intermediarios por los que pasó
(`ver_con_intermediarios`), y que se pare al ver uno revocado. Lo mismo para `interrupcion_asociar`.

### Media

**M1. Un aviso de una vez se pierde si la cola está llena.** Comprobado leyendo.

- `temporizadores.rs:103`: un temporizador de una vez pone `vence = u64::MAX` al vencer, antes de escribir.
- `temporizadores.rs:120-123`: si `escribir` falla (cola llena), solo se suma a `perdidos`. Con `vence` en
  `u64::MAX` no vuelve a vencer, y `perdidos` no sale nunca.
- `temporizadores.rs:107`: si `avisos.try_reserve` falla, el aviso se pierde también en los periódicos, sin sumarse a
  `perdidos`.

Escenario: un servidor espera en un mismo canal las peticiones y un plazo de una vez, como recomienda `kernel.md:246`
y como hace el USB con la interrupción. Un cliente llena la cola (16 mensajes) justo cuando vence el plazo. El aviso
no llega nunca y el servidor espera para siempre. `kernel.md:240-243` da a entender que el aviso se reintenta.

Arreglo: si no se pudo escribir, dejar `vence` en el próximo tic (o `ahora`) en vez de `u64::MAX`, también en los de
una vez, y reintentar hasta entregarlo o hasta que se suelte el temporizador.

**M2. La lista de proyecciones de un intermediario se llena y no se vacía.** Comprobado leyendo.

- `objetos.rs:100-108`: `apuntar_proyeccion` añade (proceso, dirección) y solo quita las de procesos muertos. Tope de
  1024 (`objetos.rs:117`).
- `llamadas.rs:371-374`: `memoria_quitar` no la toca.
- `llamadas.rs:355-362`: si falla la proyección, o un intermediario posterior, lo ya apuntado se queda.

Escenario: un servidor da a varios clientes copias de una misma revocable de memoria (todas pasan por el mismo
intermediario). Un cliente hace `memoria_proyectar` + `memoria_quitar` 1024 veces. Desde ahí, todos los que pasan por
ese intermediario reciben `E_LIMITE` al proyectar. Lo mismo le pasa, sin mala fe, a un programa largo que proyecta y
quita un búfer compartido (lo que hará A7 de Dash con `memoria_quitar`).

De paso: si un proceso proyectó con la revocable en una dirección, la quitó y después proyecta el mismo objeto en la
misma dirección con la llave original, revocar le quita esa proyección legítima (`objetos.rs:90-93`). `kernel.md:228`
dice que las hechas con la original siguen.

Arreglo: guardar en cada proyección del `Espacio` los intermediarios por los que se hizo, y borrarla de sus listas en
`quitar_proyeccion`. O, al menos, al apuntar, quitar las entradas cuyo proceso ya no tiene ese objeto en esa dirección.

**M3. Revocar puede borrar un millón de páginas con las IRQ enmascaradas.** Plausible: el camino está comprobado; el
tiempo, no medido.

- `objetos.rs:86-97`: `revocar` recorre todas las proyecciones apuntadas (hasta 1024) y quita cada una.
- `espacio.rs:300-308`: cada página es una escritura y un `tlbi vae1is`, que va a todos los núcleos. Un objeto de 4 MB
  son 1024 páginas.
- Toda llamada corre con las IRQ enmascaradas.

Escenario: un cliente con una revocable de 4 MB la proyecta 1024 veces en direcciones distintas (le cuesta unos 8 MB
de tablas de su cupo). Cuando el servidor revoca, su núcleo hace un millón de invalidaciones seguidas con las IRQ
enmascaradas. Es el cliente quien elige cuánto trabajo hace el núcleo del servidor. Rompe la decisión E.

Arreglo: con más de unas decenas de páginas por espacio, un solo `tlbi aside1is` al final en lugar de uno por página.
Y bajar el tope de proyecciones por intermediario, o partir el trabajo en trozos.

**M4. El compositor recibe toda la memoria de la VideoCore.** Comprobado leyendo. Es de diseño.

- `kernel/src/inicio.rs:74-75` y `31`: la llave "vc" cubre desde el fin de la RAM hasta 1 GB y va al compositor.
- `objetos.rs:33-36` lo dice: quien la tiene puede escribir en toda la memoria de la VideoCore, también la del propio
  firmware.
- La VideoCore manda en el chip: ve toda la RAM. Escribir en su memoria es, en la práctica, poder sobre todo el
  sistema, más que un controlador con DMA.

S16 (D) solo acepta como piezas de confianza a los controladores con DMA. El compositor no lo es, y es el servicio que
más entradas ajenas va a recibir (los clientes de Wayland, S9). Hoy solo le llegan eventos del USB y respuestas del
buzón, así que el riesgo es pequeño; con clientes, sería alta.

Arreglo: que el compositor reciba solo la pantalla. Por ejemplo, que el buzón (o `inicio`) tenga la llave entera y,
al reservar la pantalla, dé una llave recortada a sus páginas. Para eso hace falta una llamada que recorte una llave
de registros o de firmware a un subtramo de páginas enteras.

**M5. La memoria DMA vuelve al repartidor aunque el aparato siga escribiendo en ella.** Plausible: depende del
aparato.

- `memoria.rs:445-453`: al soltarse el último `Arc`, las páginas se liberan sin más.
- Un controlador que muere (fallo, `proceso_matar`, `grupo_matar`) deja su aparato como estaba: un canal del DWC2
  habilitado, o la V3D con su tabla de MMU en memoria DMA.
- Las páginas de `Dma1G` vuelven al montón del primer GB, del que sale la memoria DMA de los otros controladores: el
  mensaje del buzón, por ejemplo.
- El permiso de la GPU es de la zona normal (`kernel/src/inicio.rs:103`). Sus páginas pueden volver como montón del
  kernel o memoria de otro proceso.

Escenario: el USB muere en medio de una transferencia. Su página DMA pasa al buzón para su próximo mensaje. El DWC2
termina la transferencia y escribe ahí, entre la comprobación de etiquetas y el envío al firmware: el filtro del buzón
no sirve. Con la GPU, una V3D que sigue trabajando escribe en páginas del kernel.

S16 D hace de confianza a los controladores con DMA, pero un fallo o una muerte del controlador no es mala fe, y aquí
basta eso.

Arreglo: no devolver al repartidor las páginas DMA de un permiso hasta que el aparato se haya parado. Por ejemplo,
dejarlas "en cuarentena" en el permiso hasta que un controlador nuevo de ese aparato diga que lo ha reiniciado. O
que el kernel sepa parar el DMA de cada aparato al morir su controlador.

**M6. Un cliente tumba el buzón con un solo mensaje.** Comprobado leyendo.

- `buzon.rs:119-123`: lee con hueco para una sola llave, y ante cualquier error de `canal_leer` acaba (`return 0`).
- `llamadas.rs:283-285`: un mensaje con más llaves que el hueco da `E_TAMANO`.

Escenario: un cliente del canal "buzon" (hoy el compositor y la GPU) manda un mensaje con dos llaves. El buzón recibe
`E_TAMANO` y termina. Desde ahí nadie puede hablar con el firmware. El compositor tiene el mismo patrón en su canal de
entrada (`compositor.rs:249-251`), aunque ahí solo escribe el USB.

Otros dos problemas del mismo servicio:

- Todos los clientes comparten una copia sin marca del mismo extremo (`usuario/src/bin/inicio.rs:82`). El buzón no
  puede saber quién le pide qué, y no puede tener una lista por cliente.
- Cualquier cliente puede usar las etiquetas de la pantalla (0x00040001, 0x00048003 y 0x00048004). La GPU, o un
  cliente futuro, puede hacer que el firmware cambie de sitio la pantalla mientras el compositor sigue escribiendo en
  la vieja, dentro de la memoria de la VideoCore. A comprobar qué hay ahí después.

Arreglo: leer con hueco para 8 llaves y cerrar las que sobren; acabar solo con `E_CERRADO`. Dar a cada cliente una
copia marcada, y decidir las etiquetas por cliente (la pantalla, solo el compositor).

### Baja

**B1. La comprobación de ciclos no mira dentro de una revocable que está dentro de un revocador.** Comprobado leyendo.
`forma_ciclo` (`objetos.rs:157`) resuelve un revocador, pero si dentro hay una `Revocable`, cae en `_ => false`.
Escenario: `canal_crear` da `(a, b)`; `(rb1, rev1) = llave_revocable(b)`; `(rb2, rev2) = llave_revocable(rb1)`;
cerrar `b`, `rb1` y `rev1`; mandar `rev2` por `a`; cerrar `rb2` y `a`. El lado 1 queda vivo solo dentro de su propia
cola. Es el ciclo de M1 de la fase 3, con un paso más. C44 lo suelta al matar el grupo, así que no se pierde para
siempre, pero el cupo queda gastado hasta entonces. Arreglo: resolver también `Revocable` dentro de `forma_ciclo`.

**B2. La marca "│ " de N6 se puede saltar.** Comprobado leyendo. `llamadas.rs:170-186`: poner la marca se decide
con `a_media_linea`, que es de cada proceso, no del registro. Escenario: un proceso escribe "x" sin salto de línea;
después escribe otro proceso o el kernel (sus líneas acaban en salto); el primero escribe "C32: bien\n". Como para él
sigue a media línea, sale sin marca y al principio de una línea. `prueba_qemu.sh` lo tomaría por un veredicto.
Arreglo: llevar en `registro.rs` si el anillo está a principio de línea y quién escribió lo último, y poner la marca
según eso.

**B3. `escribir` reserva menos de lo que puede necesitar.** Comprobado leyendo. `llamadas.rs:167` reserva
`2n + 4` bytes, pero 256 saltos de línea seguidos dan 5 bytes cada uno ("│ " son 4). `push_str` crece entonces sin
`try`. Solo cae si el montón está agotado. Arreglo: reservar `5n`.

**B4. Cualquier proceso puede llenar el registro.** Comprobado leyendo. `registro.rs:37-51`: con el anillo lleno, lo
nuevo se pierde. La consola lo saca a 115.200 baudios, unos 11 KB/s. Un proceso que escribe sin parar mantiene el
anillo lleno y las líneas del kernel (avisos de fallos, por ejemplo) se pierden. Arreglo: un tope por proceso, o
dejar una parte del anillo solo para el kernel.

**B5. Memoria del kernel sin cobrar en los temporizadores.** Comprobado leyendo; el efecto, visto en la copia.

- La lista global `TODOS` crece con el número de temporizadores vivos a la vez y no encoge.
- `repartir` pide en cada pasada un `Vec` de 32 bytes por aviso (`temporizadores.rs:93`): 1,5 MB con 45.000.
- Si el último `Arc` de un temporizador se suelta dentro de `repartir` (`temporizadores.rs:99`), su canal se vacía
  ahí, con `TODOS` tomado.

En la copia, tras el experimento, la comprobación de C43 dio "MAL": 64 páginas menos al final de la tanda. Lo más
probable es este crecimiento del montón; no lo he seguido hasta el final. Se arregla con lo de A1.

**B6. Una proyección de solo lectura de memoria DMA va con caché.** Comprobado leyendo. `espacio.rs:276-280`: sin
escribir, el permiso es `SoloLectura`, normal con caché. Quien la lee puede ver datos viejos de lo que escribió el
aparato, y el mismo objeto queda con atributos distintos en dos proyecciones. Arreglo: un permiso de solo lectura sin
caché para la memoria DMA.

**B7. `nueva_dma` no respeta la reserva del kernel.** Comprobado leyendo. `memoria.rs:370` usa `reservar`, que no mira
`RESERVA` (`pagina_usuario` sí, `memoria.rs:310`). Con el permiso de la GPU, de la zona normal, puede tomar las
últimas páginas del kernel. Lo acota el cupo. Arreglo: la misma comprobación que `pagina_usuario` en la zona normal.

**B8. La llave del buzón da la página entera.** A comprobar en la placa. `kernel/src/inicio.rs` redondea a la página
0xFE00B000, que en la BCM2711 tiene también el controlador de interrupciones antiguo, el temporizador ARM y otros
buzones. No sé si con el GIC en marcha alguno de ellos puede molestar (por ejemplo, mandar una interrupción como FIQ).
Si puede, convendría un objeto de registros de menos de una página que el kernel lea y escriba por el proceso, o
dejarlo dicho como en la serie (`kernel.md`, "Limitaciones conocidas").

**B9. El sondeo de la V3D, en la Pi.** A comprobar. `kernel/src/inicio.rs:98` lee el puente de la V3D en el arranque.
En QEMU da un fallo de bus y se recoge. En la Pi, si el dominio de potencia de la V3D está apagado, leer puede colgar
el bus en vez de dar un fallo, y el arranque se quedaría parado.

**B10. Llega un aviso después de cerrar el temporizador.** Comprobado leyendo. `repartir` guarda un `Arc` del
temporizador en `avisos` y escribe fuera del cerrojo (`temporizadores.rs:118-124`). Si quien lo tiene cierra su última
llave entre medias, llega un aviso más. `kernel.md:245` dice que se para al soltar la llave. Basta con decirlo, o mirar
una marca de "soltado" antes de escribir.

**B11. Documentación desfasada.**

- La tabla de `llamadas.rs:6-30` acaba en la 23: falta la 24, `temporizador_crear`.
- `kernel.md:310` dice que el kernel lanza los controladores con la llave 1 de registros y la 2 de interrupción. Ya
  no lanza ninguno: todo llega desde `inicio`.
- `kernel.md:232-235` dice que la memoria DMA es del primer GB. El permiso de la GPU es de la zona normal.
- `kernel.md:100-101` no nombra la GPU entre los servicios.

## Revisado y correcto

- **`memoria_quitar` (C45).** Solo actúa en el espacio propio y solo en una proyección que empieza justo ahí. Borra las
  entradas y hace `tlbi vae1is` con el ASID de cada página, y un `dsb ish` al final, antes de soltar el objeto
  (`espacio.rs:291-313`). Un acceso de otro núcleo en curso ya ha acabado cuando la invalidación termina. Los registros
  y la memoria del firmware no están en `proyectados`, así que no se quitan por aquí, como dice `kernel.md`.
- **Carrera entre proyectar y revocar (C45).** Proyectar apunta antes de hacerlo y mira después; revocar vacía el
  objeto antes de tomar la lista. En todos los órdenes, o lo quita quien revoca o lo quita quien proyecta. Si los dos
  lo intentan, uno no encuentra nada. La comprobación de que es el mismo objeto evita quitar otra proyección que haya
  ocupado el sitio. Las cadenas de intermediarios se cubren: cada proyección queda apuntada en todos.
- **Permiso DMA y `memoria_dma` (C46).** Solo lo crea el kernel. Pide el derecho de escribir. Cobra antes de tomar
  páginas y lo devuelve si falla. Las páginas salen de la zona del permiso, se ponen a cero y se limpian hasta el punto
  de coherencia. La zona `Dma1G` está por debajo del piso, que ni el montón ni los procesos tocan (C38). No hay
  desbordamientos con tamaños hostiles: el tamaño se mira antes (`llamadas.rs:684`).
- **Memoria del firmware (C50).** Solo la crea el kernel, a partir del árbol (`dtb.rs`, `memoria_vc`), y comprueba que
  queda fuera de la RAM. `proyectar_firmware` lo vuelve a mirar: alineación, por debajo de 4 GB y fuera de todos los
  tramos de RAM (`memoria.rs:201-204`, `espacio.rs:318-330`). Si hay más tramos que huecos, el último se ensancha, así
  que nunca se queda RAM sin apuntar. No he encontrado forma de proyectar RAM del kernel ni de otro proceso con ella.
  `proyectar_dispositivo` sigue sin admitir nada por debajo de 0xFC00_0000.
- **Temporizadores: aritmética.** El plazo y el periodo se limitan a un día antes de pasarlos a cuentas, con `u128`.
  `vence` suma con saturación, y `veces * periodo` no pasa de `ahora - vence + periodo`. `perdidos` satura.
- **Temporizadores: avisos y carreras con el tic.** El tic solo mira y marca en el núcleo 0, y el repartidor mira y se
  bloquea con las IRQ enmascaradas en ese mismo núcleo: no se pierde ningún despertar. Crear uno baja `PROXIMO` con
  `TODOS` tomado, el mismo que usa `repartir` para calcularlo. El orden de cerrojos es tabla de llaves, `TODOS` y el
  del temporizador, y no he encontrado ningún camino que lo dé la vuelta.
- **Temporizadores y ciclos.** `forma_ciclo` los incluye (salvo el caso de B1). `soltar` los saca sin recursión. Un
  temporizador no tiene una punta, así que no mantiene abierto ningún lado: al cerrarse las puntas, las colas se
  vacían y el temporizador se suelta. No he encontrado un ciclo propio de los temporizadores.
- **Registro (C48).** `registro_leer` pide la llave del registro, que solo da el kernel a `inicio`, y el derecho de
  leer. Copia como mucho 4 KB, mira el tramo de usuario y mira la marca de muerto al despertar. El cerrojo del anillo
  se toma después del de la serie, y nunca al revés; el planificador no imprime con su cerrojo tomado. No he
  encontrado un bloqueo mutuo.
- **El kernel deja la serie (C48, N2).** Con la consola en marcha, `kprint` solo escribe en el anillo. El pánico
  escribe directo.
- **`inicio` (C47).** El kernel solo da llaves a `inicio` (`kernel/src/inicio.rs`). Las interrupciones, solo del
  tramo de aparatos y con un solo dueño. `inicio` lanza cada servicio en su grupo de 16 MB con copias de sus llaves
  (no las mueve), y uno al que le falta un aparato no se lanza. El paquete es de solo lectura.
- **Servicios y llaves.** Ningún servicio recibe la llave del registro salvo la consola, ni memoria del firmware salvo
  el compositor, ni permiso DMA salvo el buzón, el USB y la GPU. El buzón valida los mensajes en su propia memoria,
  después de copiarlos, así que el cliente no puede cambiarlos entre la comprobación y el envío. Las etiquetas de
  poner relojes solo admiten el de la V3D. El bloque PM de la GPU (el perro guardián) está dicho en `servicios.md`.
- **`unsafe`.** Fuera de `kernel/src/base/` solo queda en `pruebas.rs`, que no va en la imagen normal.

## Qué no he mirado o no he podido comprobar

- Solo he medido A1, en QEMU y en una copia. Los tiempos de M3 son estimados. Nada se ha probado en una Pi 4: M5, B8 y
  B9 dependen del hardware.
- La biblioteca `dashos::graficos` (HVS, V3D, listas) y el servicio `gpu`, solo por encima. Corren en EL0 y no
  reciben nada de clientes todavía.
- El análisis de descriptores del USB (`usb.rs`) lo he leído buscando índices fuera de rango con datos del aparato y no
  he visto ninguno, pero no lo he probado con un aparato hostil.
- `919e255` (C51, segunda parte), que llegó durante la auditoría: lo he leído por encima. La ventana de entrada del
  PCIe queda en el primer GB con desplazamiento 0. Ojo: ese primer GB incluye la memoria de la VideoCore (M4), así que
  el VL805, como la SD y el USB, sigue llegando a ella y, a través de ella, a todo. Es lo que acepta S16 D, pero
  conviene decirlo junto al cierre de la ventana.
- Las pruebas de C51 (`lanzador.rs`, `pruebas.rs`), solo por encima.

## Segunda vuelta (arreglos)

Hecha el 2026-11-07 sobre tres commits: `919e255` (ventana de entrada del PCIe), `2869322` (arreglos de A1, A2, M1, M2
y M3) y `59de692` (arreglos de M4, M5 y M6, con las llamadas 25 `registros_recortar` y 26 `dma_liberar`). Los números
de línea son de `59de692`. Mismo método: leer el camino entero. C1 y A3 están medidos en QEMU, en una copia del
repositorio fuera de él, con un programa hostil en lugar de `tiempo`; lo demás está leído.

### Qué cierra cada arreglo

| Hallazgo | Estado | Por qué |
|---|---|---|
| A1 | Cerrado lo de las IRQ; abre A3 | El montículo y el tope de 64 por pasada acotan el trabajo con `COLA` tomado. Pero el tope reparte mal: los temporizadores de un grupo retrasan segundos los de todos (A3, medido). |
| A2 | Cerrado; abre C1 | El temporizador y la interrupción llevan la marca de la llave y se paran al ver revocado un intermediario. Pero guardar los intermediarios (`via`) mete un camino de soltar recursivo que tumba el kernel (C1, medido). |
| M1 | Cerrado | Un aviso de una vez que no cabe se vuelve a programar al tic siguiente. Queda la cuenta (B12). |
| M2 | Cerrado el caso de la auditoría; abre A4 | La purga quita lo que ya no está proyectado. Pero también quita lo que está a punto de proyectarse, y con eso una proyección escapa a la revocación (A4). |
| M3 | Cerrado | Con más de 32 páginas, un solo `tlbi aside1is`. |
| M4 | A medias | El compositor ya no recibe toda la memoria de la VideoCore. Pero las llaves de pantalla viejas siguen valiendo, y el recorte se hace con valores de la respuesta que no se comprueban (M8). |
| M5 | Cerrado el reúso; abre M7 | La memoria DMA soltada no vuelve al repartidor hasta `dma_liberar`. Pero la cuarentena no se cobra y no tiene tope (M7). |
| M6 | Cerrado | El buzón lee con hueco para 8 llaves, cierra las de más y solo acaba con `E_CERRADO`. Clientes con marca y etiquetas por cliente. Quedan detalles (B16). |
| B1 | Cerrado | `forma_ciclo` resuelve también una revocable (`objetos.rs:165-166`). |

### Crítica

**C1. Una cadena de temporizadores con llave revocable tumba el kernel.** Medido en QEMU: pánico por pila del kernel
desbordada con 20 eslabones.

- `temporizadores.rs:31`: el temporizador guarda en `via` los `Arc<Revocable>` de la llave del canal (A2). Un
  intermediario sin revocar tiene dentro su objeto: la punta del canal.
- `objetos.rs:315-323`: `soltar` saca el destino de un temporizador sin recursión, pero después lo suelta con la
  vía normal de Rust. Al soltar `via`, si era el último dueño del intermediario, se suelta la punta.
- `objetos.rs`, `Punta::drop`: cerrar un lado vacía su cola y llama otra vez a `soltar`. Si en esa cola hay otro
  temporizador como el primero, se repite. Es recursión otra vez, como el C1 y el hallazgo 3 de auditorías anteriores.

Escenario, desde un grupo cualquiera y sin llaves especiales. Por cada eslabón i:

1. `canal_crear` da `(a, b)`; `(ra, rev) = llave_revocable(a)`; `cerrar(a)`.
2. `t = temporizador_crear(ra, un día, 0)`; `cerrar(ra)`; `cerrar(rev)`. Ahora el lado `a` solo vive dentro del
   intermediario, que solo vive en `t.via`.
3. Mandar `t` por el `b` del eslabón anterior (va a la cola del lado `a` anterior) y cerrar ese `b`. `forma_ciclo` no
   lo para: el destino de `t` es otro canal.

Al final se cierra la llave del primer temporizador. Suelta su intermediario, que cierra su lado `a`, que suelta el
temporizador siguiente, y así hasta el fondo, todo en la pila del kernel. Con 20 eslabones (unos 620 KB de cupo) la
pila llega a la guarda: "pila del kernel desbordada" en `soltar` (`objetos.rs:287`) y el kernel se para.

Lo mismo vale para `Interrupcion` (`interrupciones.rs`, campo `via`), aunque ahí hace falta una llave de interrupción
por eslabón.

Arreglo propuesto: que `soltar` trate el temporizador y la interrupción enteros, metiendo en la lista de objetos lo que
llevan en `via` (como ya hace con el objeto de un intermediario). O guardar en `via` un `Weak<Revocable>`: si el
intermediario ya no existe, nadie puede revocarlo, y basta con mirar que el `Weak` siga vivo o tratarlo como revocado.
Con `Weak`, además, el temporizador deja de sujetar el lado del canal (B13).

### Alta

**A3. Los temporizadores de un grupo retrasan segundos los de todos los demás.** Medido en QEMU.

- `temporizadores.rs:163`: cada pasada saca como mucho 64 avisos, y el repartidor solo pasa una vez por tic.
- El montículo está ordenado por vencimiento. Un periódico atendido se vuelve a programar en el futuro, pero los que no
  caben en la pasada se quedan con su vencimiento viejo, por delante de cualquiera que venza después.
- Un temporizador cuesta 4 KB de cupo y su periodo mínimo es un tic.

Escenario: un grupo de 64 MB crea 12.800 temporizadores periódicos de 10 ms (guardados en colas de canales, como en
A1). Cada tic vencen todos y solo salen 64. Medido: un temporizador de 100 ms de otro proceso llegó a los 2,13 s (sin
carga, 0,11 s). Con 16 MB serían unos 0,5 s. Le pasa a todo el sistema: la espera del USB (10 ms) y el `Delay` de
Dash, por ejemplo.

A1 era trabajo sin tope con las IRQ enmascaradas; ahora el trabajo está acotado, pero el reparto no es justo.

Arreglo propuesto: reparto por grupo (por ejemplo, una cola por grupo y turno entre grupos en cada pasada), o un tope
de temporizadores vencidos por grupo y pasada. Y, si quedan vencidos al acabar la pasada, volver a marcar `PENDIENTE`
para seguir sin esperar al tic, con el trabajo de cada pasada acotado como ahora.

**A4. Una proyección puede escapar a la revocación.** Plausible: es una carrera. El camino está comprobado.

- `llamadas.rs:353-359`: `memoria_proyectar` apunta la proyección en los intermediarios y después la hace, con el
  cerrojo del espacio tomado solo en ese segundo paso.
- `objetos.rs:109`: `apuntar_proyeccion` purga cada entrada cuyo proceso no tenga ya esa proyección en su espacio
  (`proyectada_via`). Una entrada recién apuntada, cuya proyección aún no se ha hecho, no la tiene.

Escenario con dos procesos que colaboran, P y Q, cada uno con una copia de la misma revocable de memoria (la misma
copia duplicada pasa por el mismo intermediario):

1. P llama a `memoria_proyectar`: apunta (P, va).
2. Antes de que P proyecte, Q llama a `memoria_proyectar` en otro núcleo. Su purga ve que P no tiene nada en `va` y
   borra (P, va).
3. P proyecta. Nadie ha revocado todavía, así que vuelve con 0.
4. Más tarde, el dueño revoca. La lista ya no tiene (P, va): la proyección de P se queda. P sigue leyendo y
   escribiendo esa memoria después de la revocación.

Es fácil de repetir: Q proyecta y quita sin parar mientras P proyecta en muchas direcciones. Basta con que una gane la
carrera. Rompe lo que dice `kernel.md` de revocar memoria (C45).

Arreglo propuesto: no purgar entradas "pendientes". Por ejemplo, que cada entrada lleve un estado (pendiente o hecha),
que P la marque como hecha después de proyectar, y que la purga solo quite las hechas que ya no estén. O quitar la
entrada desde `quitar_proyeccion`, que sabe por qué intermediarios se hizo, en vez de purgar a ciegas.

### Media

**M7. La cuarentena DMA no se cobra ni tiene tope.** Comprobado leyendo.

- `memoria.rs:488-499`: la memoria DMA soltada va a la cuarentena de su permiso, y su `Cargo` se devuelve al grupo.
  Las páginas siguen fuera del repartidor, sin cobrar a nadie, hasta que alguien llame a `dma_liberar`.
- Los controladores solo llaman a `dma_liberar` al arrancar (`usb.rs:595`, `gpu.rs:103`, `buzon.rs:148`).
- El permiso de la GPU es de la zona normal, y `nueva_dma` no respeta la reserva del kernel (B7).

Escenario: el servidor de la GPU crea y suelta búferes, que es su trabajo normal (cada búfer es un objeto DMA). Cada
búfer soltado se queda en la cuarentena. Con el tiempo se come la zona normal, después la reserva del kernel, y el
montón del kernel falla: el kernel cae. Con el USB o el buzón, que piden una vez, no pasa; con el servidor de Mesa
(S9), sí. Un controlador con un fallo que cree y suelte en bucle lo hace en segundos.

Además, `dma_liberar` devuelve toda la cuarentena de una vez con las IRQ enmascaradas (hasta toda la zona), y solo
tiene sentido si el aparato entero se ha reiniciado: no sirve para un búfer suelto mientras el aparato sigue en marcha.

Arreglo propuesto: cobrar la cuarentena al grupo del permiso (o al del controlador) hasta que se libere, con un tope;
dar al controlador una forma de devolver un búfer concreto cuando sabe que el aparato ya no lo usa; y respetar la
reserva en `nueva_dma` (B7).

**M8. Las llaves de pantalla viejas siguen valiendo, y el recorte se fía de la respuesta.** Comprobado leyendo lo del
buzón; lo que hace el firmware, a comprobar en la Pi.

- `buzon.rs:236-255`: `recortar_pantalla` saca la base y el tamaño de la etiqueta de reservar pantalla, después de
  `mandar`, sin mirar el código de la respuesta (`p[1] == 0x80000000`) ni el bit de respuesta de la etiqueta. Si el
  firmware no llega a escribir esa etiqueta (por ejemplo, porque rechaza el mensaje entero), los valores son los que
  puso el cliente, y el cliente elige qué trozo de la memoria de la VideoCore recibe.
- Cada vez que el compositor reserva la pantalla recibe una llave nueva. Las viejas no se revocan, y aunque se
  revocaran, lo ya proyectado con `registros_proyectar` no se quita. Si el compositor cambia el tamaño (0x00048003,
  0x00048004) y vuelve a reservar, el firmware puede mover la pantalla, y el compositor se queda con llaves y
  proyecciones de memoria que el firmware ya usa para otra cosa.

M4 queda a medias: el compositor ya no tiene toda la memoria de la VideoCore de entrada, pero puede ir juntando trozos.

Arreglo propuesto: recortar solo con `p[1] == 0x80000000`, el bit de respuesta puesto y una longitud de respuesta de 8
bytes; dar la llave de pantalla una sola vez (o no dejar cambiar el tamaño después); y, si hace falta cambiarla, que
el kernel pueda quitar las proyecciones de memoria del firmware al revocar, como hace con la memoria normal (C45).

### Baja

**B12. Un aviso de una vez reintentado dice que venció dos veces.** Comprobado leyendo. `temporizadores.rs:175-183` y
`205`: al reintentarlo, cuenta 1 más lo que quedó en `perdidos` (el 1 del intento fallido). Arreglo: en los de una vez,
no sumar a `perdidos` al reprogramar.

**B13. El temporizador sujeta abierto el lado del canal.** Comprobado leyendo. Con `via` (`temporizadores.rs:31`), el
temporizador tiene el intermediario y, con él, la punta por la que escribe. Si el cliente se va y deja la llave del
temporizador en otro sitio, el servidor no recibe `E_CERRADO` por ese lado. Antes no pasaba. Se arregla con el `Weak`
de C1.

**B14. Cuentas de la cola de temporizadores.** Comprobado leyendo.

- `temporizadores.rs:169`: las entradas de temporizadores ya soltados se sacan sin contar para el tope de 64. Su número
  lo acota la purga de `nuevo` (`len > 2 · vivos + 256`, línea 106), así que es trabajo acotado por los temporizadores
  vivos de todo el sistema, pero no por pasada.
- `temporizadores.rs:128-133`: el `Drop` resta de `vivos` también cuando `nuevo` falló antes de sumar.
- `temporizadores.rs:180-181`: si `programar` falla sin memoria, un periódico se para para siempre sin aviso.

**B15. La purga de `apuntar_proyeccion` trabaja mucho con cerrojos tomados.** Comprobado leyendo. `objetos.rs:109`: por
cada una de hasta 256 entradas, toma el cerrojo del espacio de ese proceso y recorre sus hasta 1024 proyecciones: unas
260.000 comparaciones con las IRQ enmascaradas en cada proyección por una revocable. Además, el `Arc<Proceso>` de
`upgrade` puede ser el último y soltar un proceso entero dentro del cerrojo del intermediario. Se arregla con lo de A4.

**B16. Detalles del buzón.** Comprobado leyendo.

- `inicio.rs:94` (usuario): el mensaje de directorio de cada cliente no mira el resultado. Si un cliente ya lanzado
  llena la cola del buzón (16 mensajes) antes de que `inicio` lance al siguiente, el siguiente queda como "Otro" y no
  puede pedir lo suyo (la GPU, su reloj).
- `buzon.rs:166`: con un error que se repite (`E_LIMITE` con la tabla llena, `E_MEMORIA`), el mensaje vuelve a la cola
  y el buzón lo vuelve a leer sin fin, gastando CPU.
- `buzon.rs:148`: el buzón libera la cuarentena al arrancar. Si el anterior murió con una petición en el firmware, la
  VideoCore puede escribir la respuesta en una página ya devuelta. Ventana muy pequeña.

**B17. Recortar una revocable da una llave que no se revoca.** Comprobado leyendo. `llamadas.rs:718-745`:
`registros_recortar` resuelve la llave con `ver` y crea un objeto nuevo, sin intermediario. Con `desde = 0` y el
tamaño entero, es una copia no revocable de una llave revocable de registros o de memoria del firmware. Hoy pesa poco,
porque revocar tampoco quita lo ya proyectado de registros, pero conviene que la copia recortada pase por los mismos
intermediarios.

**B18. La ventana del PCIe se puede volver a abrir.** Comprobado leyendo (`919e255`). `pcie.rs:46-77` cierra la ventana
de entrada al primer GB en el arranque, antes de repartir memoria. No he visto fallos en el orden de las escrituras
(BAR2 antes que `SCB0_SIZE`, como dice la especificación). Pero quien reciba algún día los registros del controlador
PCIe (0xFD50_0000) puede volver a abrirla. Conviene dejar escrito que esa llave no se da nunca (el xHCI solo necesita
sus BAR de salida), y que el primer GB que sigue viendo el VL805 incluye la memoria de la VideoCore. Sin probar: QEMU
no emula el PCIe.

### Revisado y correcto en la segunda vuelta

- **Cerrojos de los temporizadores.** El orden es tabla de llaves, `COLA` y el del temporizador. `repartir` suelta el
  del temporizador antes de volver a tomar `COLA` (M1), y escribe en los canales fuera de `COLA`. El `Drop` del
  temporizador toma `COLA`, y no he encontrado ningún sitio donde se suelte el último `Arc` con `COLA` tomado: en
  `repartir` van al lote y se sueltan fuera, y en `nuevo` la guarda se suelta antes. Quitar el destino de un temporizador
  revocado dentro de `COLA` puede soltar el último `Arc<Canal>`, pero en ese momento sus dos lados ya están cerrados y
  sus colas vacías, así que no se suelta nada más.
- **Revocación y marca (A2).** Los avisos llevan la marca de la llave del canal. Un temporizador o una interrupción
  cuya llave se revoca deja de avisar en el siguiente vencimiento. La interrupción, además, queda enmascarada.
- **M3.** Con muchas páginas se borran las entradas y se invalida el ASID entero una vez, con sus barreras.
- **`dma_liberar` y `registros_recortar`.** Piden el tipo de llave correcto; `dma_liberar`, el derecho de escribir.
  El recorte exige páginas enteras, tamaño mayor que 0 y quedar dentro del original (`checked_add`). Una llave de
  memoria del firmware recortada se sigue comprobando contra la RAM al proyectarla.
- **Cuarentena y reúso.** Las páginas de un objeto DMA solo entran en la cuarentena al soltarse su último `Arc`, cuando
  ya no las proyecta nadie. Cada aparato tiene su permiso y su cuarentena, y el USB y la GPU liberan la suya después de
  reiniciar su aparato, con el búfer nuevo ya pedido antes.
- **Directorio del buzón.** Los clientes reciben copias marcadas, que no se pueden volver a marcar. Un mensaje sin
  marca solo lo puede mandar quien tiene la llave sin marca, que es `inicio`. Los temporizadores y las interrupciones
  ya no escriben con marca 0. No he encontrado forma de que un cliente se haga pasar por otro.
