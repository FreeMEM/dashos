# S36. Repaso de los diseños contra "amigable de 2026, con fondo para informáticos"

Pedido por el usuario el 2026-11-15. Se leyeron 39 diseños contra las dos preguntas de S0 (¿lo usa sin ayuda quien no
es informático?, ¿puede un informático verlo y cambiarlo sin leer el código?). Salieron 162 hallazgos. El detalle,
documento a documento y con cita, está en `docs/revisiones/repaso-s0/` (tres ficheros). Aquí va lo que se repite, como
reglas comunes, y lo que tiene que decidir el usuario.

Límites del repaso: cada lector solo leyó su grupo, así que algún "no lo dice" puede estar resuelto en otro documento;
de S18 y de los diseños del kernel solo se leyó lo que toca al usuario; no se miró el código. Los diseños de la rama
`x86` (S30, S31, S34) no entraron.

## Reglas comunes que salen del repaso

Son consecuencia directa del principio de S0. Los diseños se corrigen para cumplirlas.

1. **Cada ajuste es un fichero de texto en `ENVARC:` que manda.** La ventana de Ajustes y una orden del Shell escriben
   ese mismo fichero; editarlo a mano vale igual. Si hace falta un formato binario por compatibilidad (IFF del Amiga),
   se genera del texto. Hay una orden para leer y poner un ajuste suelto. Protegido (solo con la ventana de confianza o
   con contraseña) queda solo lo que es de seguridad: permisos, cuentas y ajustes de la máquina.
   Afecta a: S10c, S27, S3 ("memoria amiga"), S13 (red), S7 y S18 (cajas), S26 (tema).
2. **Un solo registro, legible.** Texto en `Estado:Registros/`, una orden para leerlo y la misma lista en Ajustes. Todo
   lo que el sistema para, reinicia, niega o deshace deja una línea con programa, motivo y hora: un permiso negado, una
   vuelta atrás del arranque, un controlador o la GPU reiniciados, un programa que topa con su cupo, lo que mata Linux
   por memoria, cada ciclo de dormir. Aviso en pantalla, en llano, solo si el usuario lo nota o si se repite.
   Afecta a: S6, S6b, S14, S12, S25, S29, S35, S22, S3, hilos.
3. **Los permisos se leen.** Lo concedido está en un solo sitio, en texto que su dueño puede leer, con una orden
   `Permiso` (listar, dar, quitar). Ver los permisos propios es libre; cambiarlos sigue pidiendo confirmación de
   confianza. Afecta a: S6, S6b, S28 (hoy lo ponen en dos sitios distintos), S10c.
4. **Modo de desarrollo explícito.** Para sustituir una pieza del sistema, probar un servicio propio o una imagen de
   dominio propia: se activa a propósito, se ve siempre en pantalla, queda en el registro y vive en la capa del usuario
   o de la máquina, sin tocar `SYS:`. Hoy la única salida es el Shell, que quita todo el encierro.
   Afecta a: S6, S6b, S14, S29, S35.
5. **Las instantáneas tienen cara.** Cada una se ve como un cajón de solo lectura; "Versiones anteriores" en el menú de
   un icono saca un fichero suelto; hay una orden `Instantanea`. Volver entero sigue existiendo. Afecta a: S5, S28, S29.
6. **Cada dato, un sitio con nombre.** Nada del usuario vive donde no lo pueda encontrar: la papelera de la caja Amiga,
   `Home:Datos`, el cajón de cada caja (hoy en tres sitios según el documento). Lo interno no estorba, pero se ve con
   "Mostrar todos los ficheros". Afecta a: S4b, S28, S8 (ya corregido).
7. **Las cajas se dejan mirar y mandar.** El disco de la caja Linux se ve como un volumen con nombre (solo lectura
   desde fuera); una orden `Caja` da estado, arranca, para y ejecuta dentro. Los dominios de controladores se listan
   con su memoria y su tope, y su registro sale a un fichero. Afecta a: S7, S13, S35.
8. **Lo que falta se ofrece, no se explica.** Si algo no va de serie (un códec, un controlador), al necesitarlo sale un
   aviso en llano con un solo botón. Afecta a: S33, S32.
9. **Lo que existe se ve.** Los menús de un programa se enseñan también con el clic normal en la barra; el botón
   derecho queda como atajo del Amiga. Afecta a: S10.
10. **Dormir e hibernar no pierden sin avisar.** Si hay imagen de hibernación, se arranca con la generación que la
    hizo; al despertar se dice qué programas no guardaron. Afecta a: S22, S8.

## Contradicciones entre documentos

- **Controladores de Linux**: S12b dio por aceptado LKL (el kernel de Linux como biblioteca dentro de un proceso de
  DashOS) y `hilos-de-usuario.md` se diseñó para él; S35, escrito después, pone los controladores en máquinas virtuales
  y no nombra LKL. S35 se escribió sin cotejar S12b. **Decidido el 2026-11-16**: vale S35 y LKL se retira.
- **Quién cambia el tema**: S26 dice que solo el centro de control; S10 promete cambiarlo por guion y S10b tiene un
  servicio de apariencia. Con la regla 1 gana "fichero de texto, ventana y orden".
- **"Pantallas" o "espacios de trabajo"**: S10 y S23 usan un nombre y unas teclas; S27, otros. **Decidido el
  2026-11-16**: tres nombres para tres cosas, y las teclas de S27.

## Por decidir el usuario

Las cuatro están **decididas** (2026-11-16). El detalle, en "Las cuatro decisiones", abajo.

1. Tres hallazgos chocan con decisiones ya tomadas; no se tocan si no se dice: la contraseña en cada acción de
   administrador; que las aplicaciones encerradas no carguen de `Home:Libs`; la guardia contra malware de Amiga, que
   pregunta. **Decidido**: las tres se quedan, con un ajuste cada una (decisión 1).
2. LKL o dominios de controladores (o cada uno para algo). **Decidido**: máquinas virtuales (S35); LKL se retira
   (decisión 2).
3. "Pantallas" o "espacios de trabajo", y sus teclas. **Decidido**: monitor, espacio de trabajo y pantalla, cada uno
   su nombre; mandan las teclas de S27 (decisión 3).
4. Juegos del Amiga recién instalado: hoy hay que poner una ROM y elegir modo de caja. La propuesta del repaso es
   llevar una ROM libre de serie y que la caja elija sola; llevar una ROM es una decisión de licencia y de la regla "sin
   ROMs en el repositorio". **Decidido**: la ROM de AROS, en la imagen de `SYS:` y nunca en git (decisión 4).

## Las cuatro decisiones (aceptadas por el usuario el 2026-11-16)

Estudio pedido por el usuario. Aceptó las cuatro recomendaciones el 2026-11-16, y están aplicadas a los documentos
(ver "Aplicado después: las cuatro decisiones").

### 1. Las tres decisiones ya tomadas: se quedan, con un ajuste cada una

- **Contraseña en cada acción de administrador.** Se queda sin "vale durante 5 minutos": esa ventana de tiempo es
  justo lo que aprovecha un programa malo para colar su acción detrás de la del usuario. Ajuste: **una tanda es una
  acción**. Instalar diez paquetes de un plan de `Paq`, o crear un usuario con sus grupos, es una sola ventana que
  enseña la lista entera y pide la contraseña una vez. Lo que no esté en la lista enseñada no se hace.
- **Las aplicaciones encerradas no cargan de `Home:Libs`.** Se queda: si no, cualquier programa del Shell deja una
  biblioteca que se cuela en todas las aplicaciones. Ajuste: **modo de desarrollo por aplicación** (regla 4). El
  usuario marca una aplicación y esa sí carga de su capa; marcarla pide la confirmación de `guardian` (si no, la
  marcaría el mismo programa malo), se ve en su ficha y queda en el registro. Y una orden para lanzar desde el Shell
  un programa encerrado, para probarlo como lo verá el usuario.
- **La guardia de la caja Amiga, que pregunta.** Se queda preguntar. Ajustes: con la sesión delante la pregunta
  **espera**, sin el plazo de 30 segundos; el plazo existe porque hoy la caja se queda parada mientras pregunta, y lo
  que hay que arreglar es eso (el programa 68k espera, la caja sigue atendiendo su ventana). Con la sesión bloqueada
  o sin nadie, es "no", con un aviso que se queda: "actualiza.exe no pudo cambiar viejo. Volver a intentar". Y la
  versión anterior del ejecutable se guarda siempre en la papelera, también cuando se dice que sí: así se puede
  deshacer.

### 2. LKL o máquinas virtuales: máquinas virtuales (S35), y LKL se retira

| | LKL (S12b) | Dominio en máquina virtual (S35) |
|---|---|---|
| Qué Linux corre | Una variante fuera del kernel oficial (`arch/lkl`), mantenida aparte | El kernel oficial, sin tocar |
| Lo que hay que escribir | La capa que pide `arch/lkl` y, además, cómo llegan a ese Linux el bus, las interrupciones y el DMA del aparato de verdad | Pasar el aparato a la máquina y servirlo por virtio |
| Lo que DashOS ya tiene | Hilos en los procesos | El hipervisor (la caja Linux) y los controladores virtio, probados |
| Seguir las versiones de Linux | Depende de que el proyecto LKL siga | Se cambia el kernel y ya |
| Frontera de licencia | Código GPL enlazado con nuestra capa, en la zona GPL | Un kernel aparte que solo habla virtio: la más limpia |
| Protección del propio Linux | La de un proceso | La de un proceso y, además, el código del kernel protegido desde fuera (capa 3) |
| Memoria | Sin medir | Sin medir (objetivo 48 MB) |

Decidido: **máquinas virtuales**. La razón que pesa: LKL se pensó para usar la pila de red y los sistemas de
ficheros de Linux dentro de un programa, no para llevar aparatos de verdad; lo que S12b quería ahorrar (escribir la
unión con cada bus) habría que escribirlo igual, y sobre un Linux que no es el oficial. Comprobado: LKL sigue fuera
del kernel oficial (las propuestas para meterlo llegan hasta 2021). No comprobado: si hoy admite varios procesadores
o aparatos reales; lo que se recuerda es que no, pero no se ha verificado.

Lo que cambia con la decisión:
- S12b queda sustituido por S35 en lo de LKL; lo de red de S12b (SACK, `amissl.library` nativa) sigue.
- Los hilos de usuario no se pierden: hacen falta igual.
- **El Wi-Fi de la Pi** (SDIO, sin IOMMU) era lo primero de S12b. Con S35: controlador propio (el chip es "fullmac",
  casi todo lo hace su firmware) o dominio sin barrera de DMA. Sigue por decidir con la placa delante.
- **Corrección a S35**: "con 1 GB o menos, sin dominios" es demasiado tajante. La Pi 3 (1 GB) y el THEA1200 (512 MB)
  dependen del Wi-Fi. Mejor: el dominio de red entra si su tope medido cabe (48 MB en 1 GB cabe); los de gráficas, no.
- Si la medida del paso 1 de S35 sale mal (muy por encima de 48 MB), se vuelve a mirar. LKL no tendría por qué
  gastar menos: es el mismo kernel.

### 3. "Pantallas" o "espacios de trabajo": son tres cosas y cada una lleva su nombre

- **Monitor**: el aparato físico.
- **Espacio de trabajo**: lo de todos los días, del 1 al 9, cada uno con su disposición (S27, ya hecho en el
  compositor). Es la palabra que ve el usuario.
- **Pantalla**: solo la pantalla propia de un programa de Amiga (`OpenScreen`, con su resolución y su paleta, S23). Se
  ve como un espacio de trabajo más, que es lo que S27 ya dice.

Teclas: mandan las de S27, que son las que existen (Super+1..9, Super+arriba y abajo). Amiga+M y Amiga+N se quedan
como atajo del Amiga para lo mismo (pasar al siguiente y al anterior), y el gesto de arrastrar una pantalla hacia
abajo se conserva. S10 y S23 se corrigen para decirlo así. S27 usa a veces "pantalla" por "monitor": se corrige.

### 4. ROM para los juegos del Amiga: sí, la de AROS, en la imagen pero no en el repositorio

- Qué es: la ROM de AROS para 68k es libre (licencia APL) y se puede repartir. No es la de Commodore.
- No rompe la sala limpia: esa regla es no leer su código para escribir el nuestro. Repartir su binario no lo es.
- No rompe "sin ROMs en el repositorio": la imagen de `SYS:` la baja al construirse, con su huella comprobada, y
  dice de dónde sale el código fuente (lo pide su licencia). En git no entra.
- Cómo se usa: si el usuario tiene su Kickstart, manda el suyo (y sigue siendo la referencia de las pruebas). Si no,
  la caja arranca con la de AROS sin preguntar. La caja elige sola el modo ("máquina entera" al primer acceso a los
  chips) y la configuración de cada juego se guarda en texto junto a él.
- Lo que se dice claro: con la ROM de AROS no funcionarán todos los juegos. Cuántos, no se sabe: no está medido.
  Cuando uno falle, el aviso lo dice en llano y ofrece poner un Kickstart propio.
- Cuándo: no hay prisa. Los chips del Amiga (S18) están diseñados, no hechos; esto se hace en su fase 2. S18 ya lo
  traía como pregunta 8 ("probarlo en la fase 2 como extra"): la recomendación es que deje de ser un extra.

## Orden

Primero se corrigen los documentos con las reglas 1 a 10 (rama `repaso-s0`, revisada antes de unir). Luego, al hacer
cada pieza, su prueba incluye las dos preguntas: que exista la orden de Shell y el fichero de texto, además de la
ventana.

## Aplicado (2026-11-16)

Rama `repaso-s0`. De los 162 hallazgos se aplicaron 145 (9 de ellos solo en parte) en 39 documentos. El texto se
cambió en su sitio. Donde no había nada decidido se dice qué tiene que existir (el fichero, la orden, el registro) y
el formato queda "por diseñar al hacerlo".

Nombres comunes, los mismos en todos: órdenes `Ajuste`, `Registro`, `Permiso`, `Instantanea`, `Caja`, `Status`,
`Break`, `Usuario`, `Avisos`, `Sonido`, `ModoMonitor`, `Dormir` e `Hibernar`; cajón `Estado:Registros/`; "modo de
desarrollo"; "Mostrar todos los ficheros". Quién define cada cosa: S4b, las capas y los cajones; S6, el registro y el
modo de desarrollo; S6b, los permisos; S5, las instantáneas; S7, la orden `Caja`; S27 (apartado 7), la orden `Ajuste`.

Dos sitios que estaban repetidos quedan en uno, el que encaja con S4b:

- **Los permisos concedidos**: `Home:Prefs/Permisos/` (S6b). Son de cada usuario y viajan con su volumen; `Estado:`
  es para lo que no es de ningún usuario. S28, S10c y U6 lo citan. El código de hoy los guarda en el cajón de
  `sesiones`: queda dicho como estado.
- **El cajón de la caja Amiga**: `Home:Amiga` (S4b), que dentro de la caja se ve como `Caja:`. S28 y S6b lo citan.

Nombres nuevos y provisionales: `Estado:Prefs` y `Estado:Devs` (la capa de la máquina de `ENVARC:` y de `DEVS:`),
`Home:Instantáneas/` y el volumen `Linux:`.

| Documento | Reglas | Qué se puso |
|---|---|---|
| S4b | 1, 2, 6 | Capa de la máquina; cada ajuste es texto y el IFF se genera de él; `Assign` y `Which` enseñan las capas; `Datos/`, `Prefs/Permisos/` e `Instantáneas/` en el árbol de `Home:`; volver a fábrica se deshace |
| S5 | 1, 2, 5 | Las instantáneas como cajón, "Versiones anteriores" y la orden `Instantanea`; aviso y registro de un fichero dañado; lo enchufado se monta solo |
| S6 | 2, 3, 4 | El registro único y la orden `Registro`; el modo de desarrollo; `Status` y `Break`; la orden `Permiso` |
| S6b | 2, 3, 6 | Un solo sitio de permisos, legible; la frase en llano de cada permiso; la ficha entera de un proceso; la orden `Usuario`; borrar una cuenta se deshace unos días |
| S8 | 1, 6, 10 | Órdenes de `Paq` para todo lo de Ajustes; repositorios y claves en texto; sin pregunta por la biblioteca repetida; con imagen de hibernación, la generación que la hizo |
| S14 | 2, 4 | Registro de servicios en `Estado:Registros/`; capas de servicios con el modo de desarrollo; el menú de arranque sale solo tras dos fallos; aviso de la vuelta atrás |
| S20 | 1, 6 | El ajuste de RAD: en la capa de la máquina; desmontar no pregunta y lo descartado va a la papelera; órdenes de Shell |
| S28 | 2, 3, 5, 6 | La papelera del usuario para lo borrado y lo machacado; el aviso del bloque de arranque en llano; cita a S6b y a S4b |
| S29 | 1, 2, 4, 5 | Reglas del cortafuegos de la caja en texto; el registro no deja de apuntar; copiar de una instantánea sin volver entero; restablecer se deshace |
| S4 | 2, 6, 7 | Cajón de las ROM y aviso si falta una biblioteca; "Abrir aparte"; el perfil con la orden `Caja`; aviso al llegar a "memoria amiga"; nota del hilo único por caja |
| S7 | 1, 2, 7 | El disco como volumen `Linux:`; la orden `Caja`; ajustes de la caja en texto; registro de la caja; aviso si Linux mata por memoria |
| S13 | 1, 2 | Ajustes de red en texto; órdenes de Wi-Fi y de conexiones; registro de lo cortado; la pregunta de escuchar, en llano |
| S17 | 2 | Aviso si un programa PPC no puede correr; la aplicación de QEMU elige la máquina sola |
| S18 | 1, 6 | El modo de caja sale solo; la configuración en texto junto al juego; partidas y estados guardados donde se ven |
| S25 | 1, 2 | Registro de reinicios de la GPU; quién la tiene; ajuste para forzar la CPU |
| S11 | 1 | Puerto ARexx de serie por aplicación; ajustes en `ENVARC:`; herramienta de traza; aviso si falta MUI |
| S12 | 2, 7 | Registro de reinicios; los controladores salen en `Status` |
| S12b | (nota) | Línea de cabecera: contradicción con S35 (después, sustituido en lo de LKL: ver abajo) |
| S35 | 2, 4, 7 | Línea de cabecera (después, la decisión: ver abajo); los dominios se listan y sacan su registro; imagen propia solo en modo de desarrollo; aviso a la tercera vez |
| S21 | 1, 2 | Orden y registro de qué chip hace cada trabajo; ajuste para forzar la CPU |
| S24 | 2, 8 | Lista llana de lo que aún no funciona; la línea de la CPU, al registro |
| S1 | 3 | Llamada de solo lectura que lista las llaves de un proceso |
| S2 | 1 | `Status` y `Break`; las tareas de cada caja; "de fondo" como tooltype; el perfil de energía en texto |
| S3 | 1, 2 | Registro del Enforcer; "memoria amiga" manda desde su fichero; topar con el cupo deja rastro |
| S16 | 2 | Lo cobrado a cada grupo se ve con `Status`; el error se llama `E_MEMORIA`, como en el código |
| hilos | 2 | El fin no normal de un proceso va al registro; `Status` enseña los hilos |
| S10 | 1, 9 | Los menús, a la vista con el clic normal; la vista general con el ratón; la orden `Avisos` |
| S10b | 2, 3 | Los poderes se leen con `Permiso`; aviso y registro del escritorio mínimo; apagar con cuenta atrás |
| S10c | 1, 3 | Los ajustes del usuario se cambian por fichero y por orden; protegido solo lo de seguridad |
| S27 | 1 | La orden `Ajuste`; el texto manda sobre el IFF; "Deshacer cambios"; textos del panel en llano; zona horaria de serie |
| S26 | 1, 2 | El dueño del tema es el servicio de apariencia; aviso con la línea que falla; excepciones con el ratón |
| S23 | 1, 9 | "Probar con Workbench clásico"; la ficha como tooltypes; ver en ventana desde el menú |
| S9 | 1 | La orden `ModoMonitor`; el modo nuevo se prueba y vuelve solo |
| S19 | 1, 2 | Aviso al cambiar de salida; regla de salida de serie; la orden `Sonido`; registro de quién grabó |
| S19b | 1, 2 | Ventana de cables y grafo en texto; período con nombres llanos; registro de plugins quitados |
| S22 | 2, 10 | Se arranca con la generación de la imagen; aviso de lo que no guardó; `Dormir` e `Hibernar`; el sitio de la imagen |
| U6 | 2, 3 | Botón "Cambiar de usuario"; aviso y registro de lo negado a la sesión de detrás; cita a S6b |
| S32 | 2, 8 | El plan como fichero de texto y registro; aviso de IOMMU con un botón; guía para encoger; deshacer en vez de doble confirmación |
| S33 | 2, 8 | Aviso con un solo botón si falta un códec; orden y registro de por dónde se decodifica |

## Aplicado después: las cuatro decisiones (2026-11-16)

Con las cuatro decisiones aceptadas se aplicaron siete hallazgos que estaban en "No aplicado", y lo que quedaba a
medias de S4-1. Con ellos son 152 aplicados de 162.

| Hallazgo | Documentos | Qué se puso |
|---|---|---|
| H17 | S6, S6b, S8 | Sin "vale durante 5 minutos". Una tanda es una acción: una ventana con la lista entera y una contraseña; lo que no está en la lista no se hace |
| H23 | S6b, S6, S4b, S29 | Las encerradas siguen sin cargar de `Home:Libs`. Modo de desarrollo por aplicación, con confirmación de `guardian`, a la vista en su ficha y en el registro; y una orden para lanzar desde el Shell un programa encerrado |
| H43 | S28 | Con la sesión delante la pregunta espera, sin plazo; la caja no se para mientras pregunta; sin nadie es "no", con aviso que se queda y "Volver a intentar"; la versión anterior va siempre a la papelera. Dicho como por hacer: hoy el código tiene el plazo y la parada |
| S12-3, S12b-1 | S35, S12b, S12, S13, hilos, S19b, RESUMEN | Máquinas virtuales; LKL se retira. S12b marcado como sustituido en lo de LKL (lo de red sigue). Tabla comparativa y porqué en S35. El dominio de red entra si su tope medido cabe; los de gráficas, desde 4 GB. Los hilos siguen, pero ya no por LKL |
| S27-4 | S10, S23, S27, S10b, S10c, S9, S4, RESUMEN | "Monitor" el aparato, "espacio de trabajo" lo de todos los días, "pantalla" solo la propia de un programa de Amiga. Teclas de S27; Amiga+M y N como atajo; se conserva arrastrar hacia abajo |
| S18-2, S4-1 | S18, S4, S32, PLAN | La ROM de AROS en la imagen de `SYS:`, bajada al construir, nunca en git. Manda el Kickstart del usuario si lo hay. La caja elige sola el modo. Se dice que no irán todos los juegos. Fase 2 de S18 |

Con el cambio de nombres, la categoría de Ajustes y el programa de `SYS:Prefs` pasan a llamarse "Monitores", y el
poder de S10b, "Espacios de trabajo". La orden de S9 se llama `ModoMonitor` (usuario, 2026-11-16).

## No aplicado

Diez hallazgos enteros y la mitad de otros ocho. Quedan pendientes de que decida el usuario.

Decisión nueva de seguridad, o cambia algo decidido con el usuario:

- **H26** (S6b): elegir la imagen de una rejilla en vez de la frase. Cambia la defensa contra la ventana falsa.
- **H30** (S8): instalar de serie en `Home:` y sin contraseña. Cambia quién necesita al administrador.
- **H46** (S28, S6b): qué ve la caja Amiga de serie. Los dos documentos siguen sin decir lo mismo.
- **H51** (S29): que lo de `WBStartup` no pregunte.
- **S10b-5**: un escritorio de la caja Linux con poderes.
- **U6-1**: apagar con otra sesión abierta sin contraseña de administrador (decisión 7 de U6).
- **U6-4**: pausar la música de la sesión de detrás (decisión 2 de U6, ya hecha como silencio).
- **S32-4**: que `Paq` instale el controlador de NVIDIA. El apartado está decidido el 2026-11-15.
- En parte: **H22** (se puso el aviso en llano; sigue preguntando todo), **H48** (la pregunta de `red-local`),
  **H52** (la excepción de administrador), **U6-2** (la lista de cuentas; se puso el botón), **S35-4** (quién cambia
  la tabla de aparatos), **S35-5** (dejar de reiniciar el dominio), **hilos-2** (reparto por grupo, pregunta 3).

Otros motivos:

- **H8**: `Startup-Sequence` dice una cosa en S4b y otra en S14 y S28. Hay que elegir, y no es un cambio de texto.
- **S27-7**: que no se pueda apagar es el estado de hoy; subirlo de prioridad es cosa de la hoja de ruta.
- **S11-4**, en parte: repartir MUI depende de su licencia. Se puso solo el aviso.

Dudas que salieron al aplicar:

1. Los permisos pasan de modo `0700` a legibles por su dueño (regla 3). Escribir sigue siendo solo de `sesiones`.
2. El modo de desarrollo es superficie nueva de seguridad. Solo se dice qué tiene que cumplir; el detalle va con
   revisión. Lo mismo la llamada del kernel que lista las llaves de un proceso (S1).
3. S4b tenía abierta la pregunta 3 con "IFF para lo compartido" como recomendado. Con la regla 1 queda "todo en texto
   y el IFF se genera". Es un cambio de recomendación.
4. Una aplicación encerrada solo ve su cajón: cómo escribe sus ajustes en `ENVARC:` (S11) queda por diseñar.
5. S4b dice a la vez que `/home/<usuario>` es `Home:` y que `Home:Linux` es el /home de la caja Linux. Ya estaba así;
   no se tocó.
6. Las órdenes de S21 (qué chip hace cada trabajo), S24 (describir la máquina), S33 (clases de DataTypes) y S20
   (montar imágenes) quedan sin nombre.
7. Resuelta el 2026-11-16: se llama `ModoMonitor`. Antes: la orden `Pantalla` (S9) cambia el modo de un monitor. Con los tres nombres le tocaría llamarse de otra forma,
   pero "Monitor" choca con el Monitor del sistema. Se deja como está hasta que se decida. Tampoco se ha tocado
   "modo de pantalla" ni `Prefs/ScreenMode`, que son nombres del Amiga.
8. En S18, pasar un programa de la caja nativa a máquina entera "al primer acceso a los chips" obliga a volver a
   abrirlo: cómo se hace sin que el usuario lo note queda por diseñar.
9. `AGENTS.md` (regla 2 y la nota de `DASH_KICKSTART`) no se ha tocado: sigue diciendo "nada de código de la ROM en
   el repositorio", que la decisión no rompe. Si conviene nombrar ahí la ROM de AROS, es cosa del usuario.
