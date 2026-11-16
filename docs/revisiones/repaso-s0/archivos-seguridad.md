# Repaso contra S0: archivos, seguridad, paquetes y arranque

Del 2026-11-15. Solo lectura de `docs/diseño/` en el worktree `revision`. No se cambió nada del repo.

Las dos preguntas de S0 ("Amigable de 2026, con fondo para informáticos"):

- **A**: ¿lo usa sin ayuda alguien que no es informático?
- **B**: ¿puede un informático ver cómo funciona y cambiarlo sin leer el código?

Cada hallazgo dice: qué dice el diseño (con cita), qué pregunta falla y el cambio que se propone. Las citas son
`documento:línea`.

Aviso: varios hallazgos tocan decisiones ya aprobadas por el usuario (S6b decisiones 5 y 9, la guardia de S28). Van
marcados con "decidido". No son errores: son choques con el principio nuevo, que es posterior.

---

## S4b. Organización

**H1. El cajón de datos de cada aplicación no está en el árbol de `Home:`.**
- Dice: el árbol de "Cada usuario, su mundo" (S4b:193-205) lista `Prefs`, `C`, `Libs`, `Paquetes`, `Escritorio`,
  `Documentos`, `Amiga`, `Linux`. No sale `Datos/`. Pero S6:61 y S6b:181 ponen los datos de cada aplicación en
  `Home:Datos/<paquete>`.
- Falla: A. S0 pide que no haya "datos de un programa que el usuario no pueda encontrar".
- Cambio: añadir `Datos/` al árbol, con icono y un cajón por programa con el nombre que se ve en el escritorio.

**H2. El cajón de la caja Amiga está en tres sitios.**
- Dice: `Home:Amiga/` (S4b:203); `Home:Datos/caja-amiga`, visto como `Caja:` (S28:219-221); hoy
  `Estado/Servicios/caja-amiga` (S28:219).
- Falla: A y B. El usuario no sabe dónde guarda sus cosas la caja.
- Cambio: elegir uno (propuesta: `Home:Amiga`, que se ve como `Caja:` dentro de la caja) y corregir los otros dos
  documentos.

**H3. Activar un monitor o un teclado "arrastrándolo a Devs/" no se puede hacer.**
- Dice: lo de `Storage/` "se activa arrastrándolo a su cajón de Devs/, como en el Amiga" (S4b:159-160). Pero `SYS:` es
  de solo lectura (S4b:35) y `DEVS:` no tiene capa del usuario (S4b:53).
- Falla: A y B. El gesto que se promete falla, y no hay otro sitio donde escribir.
- Cambio: dar a `DEVS:` una capa de la máquina que se pueda escribir (por ejemplo `Estado:Devs`, con acción de
  administrador). Arrastrar escribe ahí. Lo mismo para las listas de montaje de `DEVS:DOSDrivers` (S4b:89).

**H4. Los ajustes de la máquina no dicen que sean ficheros de texto.**
- Dice: "Lo que es de la máquina (hora, red, actualizaciones) no está en `Home:`: va en `Estado:` y cambiarlo es
  acción de administrador" (S4b:210-212). No dice formato, ni cajón, ni orden de Shell.
- Falla: B. S0 pide que los ajustes sean texto en `ENVARC:`.
- Cambio: decir que son ficheros de texto en una capa de máquina de `ENVARC:` (por ejemplo `Estado:Prefs`), que todos
  leen y que se escriben con acción de administrador, desde Ajustes o desde el Shell.

**H5. Parte de los ajustes sigue en IFF, que no se lee a mano.**
- Dice: idioma, fuentes, modo de pantalla, teclado y puntero "siguen en el formato IFF del Amiga" (S4b:78-80, y
  abierto en S4b:232).
- Falla: B. Son justo los ajustes que más se tocan, y no son texto.
- Cambio: que el fichero de texto sea el que manda y el IFF se genere solo para la caja. Si no, al menos una orden que
  lo vuelque a texto y lo cargue de texto.

**H6. No se puede ver de qué capa sale un fichero ni qué asignaciones tiene un programa.**
- Dice: cada proceso tiene su vista de los nombres (S4b:23-27) y cada asignación tiene tres capas (S4b:46-57). No hay
  orden ni ventana para mirar la vista de un proceso, ni para saber qué capa ganó.
- Falla: B. S0 pide que "se pueda ver qué asignaciones tiene cada programa".
- Cambio: `Assign` enseña las capas de cada nombre; `Which` dice de qué capa sale un fichero; y la vista de otro
  proceso propio se pide por su PID (también en el Monitor del sistema).

**H7. "Volver a los valores de fábrica" borra y no se deshace.**
- Dice: "es borrar la capa del usuario" (S4b:81-82 y 211).
- Falla: A. S0 pide deshacer antes que preguntar.
- Cambio: en vez de borrar, apartar la capa a un cajón con fecha (o una instantánea) y ofrecer "Deshacer". Y poder
  volver a fábrica un solo ajuste, no todos.

**H8. `SYS:S/Startup-Sequence`: dos documentos dicen cosas distintas.**
- Dice: en S4b es "de la máquina y lo hace el sistema antes de que entre nadie" (S4b:158, 207-208). En S14 "existe,
  pero para la caja Amiga" (S14:102-103). Y S28:157 dice que la caja no tiene `S:`.
- Falla: B. No se sabe qué guion manda en el arranque ni dónde mirarlo.
- Cambio: decidirlo en un sitio. Propuesta: el arranque de la máquina lo lleva el gestor de servicios (S14), y
  `Startup-Sequence` es solo de la caja y vive en su cajón.

---

## S5. Sistema de archivos

**H9. Las instantáneas existen, pero no se dice cómo las usa nadie.**
- Dice: "`Usuarios:` con instantáneas periódicas a elegir, como Time Machine" (S5:111, y S5:117-118). No hay ventana,
  ni orden, ni sitio donde verlas. S28:295-297 confirma que hoy "no hay orden para leer un fichero de una
  instantánea" y que volver deshace el volumen entero.
- Falla: A y B. Es la base de "deshacer" en todo el grupo y no tiene cara.
- Cambio: cada instantánea se ve como un cajón de solo lectura (por ejemplo `Home:Instantáneas/<fecha>`), para copiar
  un fichero con el ratón. En el menú del icono, "Versiones anteriores". Y una orden `Instantanea` (listar, crear,
  copiar de, volver).

**H10. Un fichero dañado se detecta, pero no se dice qué ve el usuario.**
- Dice: sumas de comprobación "para detectar datos estropeados en vez de leerlos sin saberlo" (S5:11).
- Falla: A y B. Falta el aviso y falta el rastro.
- Cambio: aviso en llano ("Este fichero está dañado. Recuperar de una copia anterior") y una línea en el registro con
  el fichero y el bloque.

**H11. No queda claro si un disco que se enchufa se monta solo.**
- Dice: los otros sistemas de archivos "se montan desde `DEVS:DOSDrivers`" (S5:96).
- Falla: A, si hace falta una lista de montaje para un pendrive.
- Cambio: decir que lo enchufado se monta solo y sale en el escritorio. `DOSDrivers` queda para lo que no es corriente.

**H12. Compresión y cifrado del disco entero "a elegir", sin decir dónde.**
- Dice: "zstd a elegir" (S5:72); "Cifrar el disco entero es una opción aparte" (S5:92).
- Falla: B, leve.
- Cambio: nombrar el ajuste (fichero de texto) y la orden con la que se cambia.

---

## S6. Seguridad

**H13. El registro de seguridad no tiene sitio, formato ni orden.**
- Dice: "Registro de seguridad: inicios de sesión, confirmaciones de administrador, permisos concedidos y avisos del
  Enforcer" (S6:131-132). Nada más.
- Falla: B. S0 pide "registros legibles".
- Cambio: texto, una línea por hecho, en `Estado:Registros/seguridad`. Cada usuario lee lo suyo; un administrador,
  todo. Una orden para leerlo y filtrarlo, y la misma lista en Ajustes.

**H14. No hay modo de desarrollo: `SYS:` no lo cambia nadie.**
- Dice: "`SYS:` no lo cambia ni un administrador. Solo las actualizaciones del sistema, firmadas" (S6:54-55). La clave
  que firma `SYS:` es solo la de DashOS y está fuera de la lista de `Paq` (S8:142, 151).
- Falla: B. S0 pide poder sustituir una pieza "en su capa o con un modo de desarrollo explícito". Para `C:` y `LIBS:`
  hay capa. Para un servicio, un manejador de `L:`, un controlador o el kernel, no hay ninguna de las dos.
- Cambio: un modo de desarrollo explícito. Un administrador lo activa con su contraseña; deja montar una capa encima
  de `SYS:` (o arrancar una imagen firmada con una clave propia). Mientras está activo se ve una marca fija en
  pantalla, queda en el registro y se quita desde el menú de arranque.

**H15. Los permisos se revisan "en Prefs" y en ningún otro sitio.**
- Dice: "El usuario puede revisarlo y quitarlo en `Prefs`" (S6:85).
- Falla: B. Una función que solo existe en una ventana.
- Cambio: ver H18.

**H16. Listar y matar procesos no tiene orden de Shell nombrada.**
- Dice: "Hay un servicio de procesos que los lista" y se le pide matar un PID (S6:40-43). El árbol solo trae el
  "Monitor del sistema" (S4b:161-162).
- Falla: B.
- Cambio: nombrar las órdenes en `C:` (por ejemplo `Status` y `Break`, como en el Amiga).

**H17. Cada acción de administrador pide la contraseña (decidido).**
- Dice: "Para hacer algo de administrador hay que confirmarlo con la contraseña" (S6:51); "Cada acción se confirma.
  Sin vale durante 5 minutos" (S6b:262, decisión 5).
- Falla: A. Varias acciones seguidas son varias contraseñas. S0 pide no cansar a preguntas.
- Cambio: sin tocar la decisión, que una tanda cuente como una acción. Instalar diez paquetes de un plan de `Paq`, o
  crear un usuario con sus grupos, es una ventana con la lista y una contraseña.

---

## S6b. Usuarios y permisos

**H18. Lo concedido se guarda donde el usuario no lo puede leer, y en dos sitios según el documento.**
- Dice: "se guarda por usuario en `Home:Prefs/Permisos/`, con `sesiones` de dueño y modo `0700`: ni el Shell lo puede
  tocar" (S6b:220-221). S28:280 lo pone en `Estado/Servicios/sesiones/permisos`.
- Falla: B. Es un ajuste que no se lee, sin orden de Shell. Y está en `Home:` pero cerrado a su dueño.
- Cambio: fichero de texto que el usuario lee y solo `sesiones` escribe. Una orden `Permiso` (listar, quitar, dar).
  Quitar es libre; dar pasa por `guardian`. Y un solo sitio en los dos documentos.

**H19. La ficha de un proceso no se puede mirar entera.**
- Dice: `sesiones` contesta "PID, usuario, aplicación, nombre y consumo" (S6b:275-276). La ficha tiene además perfil,
  permisos, raíces y tabla de nombres (S6b:54-63, 158-163). Los hijos sin ficha no se ven uno a uno (S6b:279-280).
- Falla: B. S0 pide ver "asignaciones, permisos y memoria de cada programa".
- Cambio: la orden de procesos y el Monitor enseñan la ficha entera de los procesos propios: perfil, permisos,
  raíces y nombres. Y adelantar la lista de hijos de un grupo.

**H20. Un permiso que el paquete no declara se niega sin rastro.**
- Dice: "¿Lo declara el paquete? Si no, se niega sin preguntar" (S6b:212).
- Falla: A y B. El programa falla y nadie sabe por qué.
- Cambio: una línea en el registro ("Editor pidió la red y su paquete no la declara") y, si el programa lo enseña, un
  mensaje en llano con el detalle a un clic.

**H21. Varios permisos no tienen un texto en llano.**
- Dice: la lista trae `plazo` ("la llave de la clase de plazo del planificador"), `arexx:<puerto>`, `volumenes`,
  `red-escuchar` (S6b:225-235). El único ejemplo de pregunta es "Editor quiere usar la red" (S6b:214).
- Falla: A. "Quiere usar la clase de plazo" es jerga.
- Cambio: añadir a la tabla una columna con la frase que ve el usuario. Por ejemplo: "quiere prioridad para que el
  sonido no se corte", "quiere dar órdenes a Tachyon", "quiere recibir conexiones de otros aparatos".

**H22. El programa suelto "pregunta todo" y avisa de que "no está firmado".**
- Dice: "pregunta todo, con aviso de que no está firmado" (S6b:247; igual en S8:129).
- Falla: A. Muchas preguntas seguidas y una palabra de informático.
- Cambio: una sola pregunta al abrirlo, en llano ("Este programa no viene del catálogo. Se abre encerrado: solo ve lo
  que tú le des"). Lo demás, en el primer uso, como cualquier aplicación.

**H23. Las aplicaciones encerradas no cargan de `Home:Libs`, y no hay forma explícita de probar una pieza propia
(decidido).**
- Dice: regla 6 y decisión 9 (S6b:320-322, 402); S4b:59-61. La salida real es lanzar desde el Shell, que hereda
  `Home:`, `Work:` y red sin preguntas (S6b:249-250). S29:214-215 ya dice que eso deja leer y cifrar todo.
- Falla: B. La capa del usuario no vale para estos programas, y la alternativa es quitar todo el encierro.
- Cambio: un modo de desarrollo por aplicación: el usuario marca una aplicación y esa sí carga de su capa. Se ve en
  su ficha y queda en el registro. Y una orden para lanzar desde el Shell un programa encerrado, para probarlo como lo
  verá el usuario.

**H24. Crear y borrar cuentas solo tiene ventana.**
- Dice: programa `Usuarios` en `SYS:System` (S4b:161-162) y "`Prefs` de usuarios" (S6b:386). Las cuentas sí son texto
  legible (S6b:101-103): eso cumple.
- Falla: B. Falta la orden.
- Cambio: una orden `Usuario` (crear, borrar, grupos) que pide la misma acción de administrador.

**H25. Borrar una cuenta no se deshace.**
- Dice: un administrador puede "borrar la cuenta" (S6b:112). El volumen cifrado se pierde.
- Falla: A. Un "¿seguro?" donde cabe deshacer.
- Cambio: borrar deja la cuenta "retirada" (el campo `estado` ya existe, S6b:101) y guarda el volumen cerrado unos
  días. Después se borra solo.

**H26. Contraseña, código en papel y frase secreta: mucho para el primer arranque.**
- Dice: código de recuperación "para apuntar en papel" (S6b:110); "una imagen o frase que el usuario eligió al crear
  la cuenta... Si no sale, no es el sistema" (S6b:270-271).
- Falla: A, leve. Tres cosas que entender antes de ver el escritorio, y una defensa que depende de que el usuario se
  fije.
- Cambio: elegir una imagen de una rejilla, sin teclear nada. Una frase en llano que explique para qué. Y poder sacar
  un código nuevo desde Ajustes, con la contraseña.

---

## S8. Paquetes

**H27. El almacén de versiones "no sale en el explorador" y no se dice cómo mirarlo.**
- Dice: "viven en un almacén que solo toca `Paq` y que no sale en el explorador" (S8:191-192). Dónde vive y el formato
  de la tabla están "por diseñar" (S8:221).
- Falla: B. S0 lo nombra como ejemplo de "ordenado, no escondido": no estorba, "pero se puede mirar si se pide".
- Cambio: el almacén tiene nombre y se lee (no se escribe). Sale con "Mostrar todos los ficheros". La tabla de cada
  generación es un fichero de texto.

**H28. Lo que `Paq` hace en Ajustes no tiene todo su par en el Shell.**
- Dice: órdenes `Instalar`, `Quitar`, `Buscar`, `Actualizar` y `Volver` (S8:54-55). Pero elegir una generación de la
  lista (S8:195-196) y limpiar a mano (S8:198, "Ajustes dice cuánto ocupa y deja limpiar") no tienen orden.
- Falla: B.
- Cambio: añadir `Paq Generaciones`, `Paq Volver <n>`, `Paq Limpiar` y `Paq Info <nombre>` (versión, permisos que
  pide, ficheros).

**H29. Repositorios y claves de confianza: no se dice dónde están ni cómo se cambian.**
- Dice: "las que añada un administrador" (S8:56-57; S6:101). No hay fichero ni orden.
- Falla: B.
- Cambio: la lista de repositorios y las claves, en ficheros de texto en `Estado:`, un fichero por clave. Órdenes
  `Paq Repos` y `Paq Claves`. Añadir sigue siendo acción de administrador.

**H30. No se dice dónde se instala de serie.**
- Dice: `Paquetes:` (de todos, con administrador) o `Home:` (del usuario, sin él) (S8:43, 135; S4b:213).
- Falla: A. Si lo de serie es `Paquetes:`, cada instalación pide contraseña.
- Cambio: de serie, en `Home:` y sin contraseña. "Para todos los usuarios" queda como opción a un paso.

**H31. "Si dos paquetes ofrecen la misma biblioteca, el gestor pregunta."**
- Dice: S8:59-60.
- Falla: A. Es una pregunta que solo sabe contestar un informático.
- Cambio: sin pregunta: manda la prioridad del repositorio. Quien quiera otra, la elige con una orden de `Paq`.

**H32. Aviso para reiniciar un programa que no hace falta.**
- Dice: "Si el programa está abierto, se avisa para reiniciarlo" (S8:102). Pero "un programa abierto sigue con su
  versión hasta que se cierra" (S8:212).
- Falla: A, leve. Un aviso de más, y los dos textos chocan.
- Cambio: quitar el aviso. La versión nueva entra la próxima vez que se abre. Avisar solo si es de seguridad.

**H33. El instalador de Amiga pide algo que DashOS no trae.**
- Dice: "Hace falta el `Installer` del AmigaOS del usuario (DashOS no lo trae): sin él, el paquete se instala como
  cajón" (S8:121).
- Falla: A, leve. El usuario no sabe por qué un programa quedó a medias.
- Cambio: mensaje en llano cuando pasa, con qué falta y dónde ponerlo. Lo descartado por el instalador (S8:120) se
  guarda en un fichero de texto junto al paquete, no solo "se enseña".

---

## S14. Arranque

**H34. Un servicio solo puede vivir en `SYS:` o en un paquete.**
- Dice: "Viven en `SYS:Servicios` y en `Paquetes:*/Servicios`" (S14:67-68). No dice dónde se apunta qué está
  activado.
- Falla: B. Para añadir o cambiar un servicio hay que hacer un paquete. No hay capa propia.
- Cambio: dos capas más: de la máquina (`Estado:Servicios`, con administrador) y de la sesión (`Home:Servicios`). Y lo
  activado o desactivado, en un fichero de texto. Decir que el fichero del servicio es texto.

**H35. El registro de los servicios no dice dónde se guarda ni en qué formato.**
- Dice: "va a un registro en memoria, que se guarda en disco con calma" (S14:98). Hay `servicio registro` (S14:73):
  eso cumple.
- Falla: B. No se sabe si es texto ni dónde está. Y si la máquina cae antes de guardar, se pierde lo que más importa.
- Cambio: texto en `Estado:Registros/<servicio>`. Lo de un servicio que se cae se escribe en el momento.

**H36. El menú de arranque temprano está escondido tras una tecla.**
- Dice: "con una tecla al arrancar" (S14:105-109, 138-139). El arranque dura menos de un segundo (S14:8).
- Falla: A. Quien no lo sabe no lo encuentra, y no da tiempo a pulsar.
- Cambio: que salga solo tras dos arranques fallidos. Y una entrada "Reiniciar en modo recuperación" en el menú de
  apagar. La tecla se queda para quien la conoce.

**H37. Si una actualización falla y el sistema vuelve atrás solo, no se dice que avise.**
- Dice: "Si no llega, el vigilante reinicia y el firmware vuelve solo a la ranura buena" (S14:119).
- Falla: A y B. Pasa algo importante sin aviso y sin rastro.
- Cambio: al entrar, un aviso en llano ("La actualización no arrancó bien. Sigues con la versión de antes") y el
  motivo en el registro.

**H38. Cumple en lo demás.** La orden `servicio` y `Prefs/Servicios` hacen lo mismo (S14:72-74). Publicar un puerto
se hace con orden o en `Prefs/Red` (S14:89-90).

---

## S20. RAD:, ADF y HDF

**H39. RAD: hay que configurarlo, y su fichero no tiene dónde escribirse.**
- Dice: "El tamaño se elige en las preferencias, como el MountList del Amiga (`DEVS:DOSDrivers/RAD`), y lo lee el
  arranque" (S20:21-22). El tamaño de serie está abierto (S20:108). `DEVS:` no tiene capa que se escriba (S4b:53).
- Falla: A y B. No sirve recién instalado, y el ajuste apunta a un sitio de solo lectura.
- Cambio: un tamaño de serie (o apagado de serie, sin estorbar). El ajuste, en la capa de máquina de H3. Decir en la
  ventana que el cambio entra al reiniciar.

**H40. Al desmontar una imagen se pregunta si guardar o tirar los cambios.**
- Dice: "Al desmontar se elige: guardar los cambios en la imagen o tirarlos" (S20:54-56).
- Falla: A. Una pregunta en cada desmontaje, y tirar no se deshace.
- Cambio: sin pregunta. La capa de cambios se queda al lado de la imagen, como un fichero que se ve, y se vuelve a
  usar al montar. "Aplicar a la imagen" y "Descartar" son acciones del menú del icono; lo descartado va a la papelera.

**H41. Montar, desmontar y "meter en DF0:" no tienen orden de Shell.**
- Dice: "Doble clic en un ADF o un HDF" (S20:50); la caja cambia el disco "cuando se pide" (S20:52-53).
- Falla: B.
- Cambio: órdenes para montar una imagen (con solo lectura y con nombre de aparato), desmontarla y meterla en `DF0:`.
  Así un guion cambia de disquete.

---

## S28. Malware del Amiga

**H42. La papelera de la caja está donde el usuario no la encuentra.**
- Dice: "La de la caja es `Estado/Servicios/caja-amiga.papelera`... Está fuera de sus raíces: la caja no la ve ni la
  vacía" (S28:291-293). No tiene cupo (S28:184-185). No hay ventana ni orden para recuperar.
- Falla: A y B. Es el "deshacer" de la caja y el usuario no puede usarlo.
- Cambio: que lo borrado por la caja vaya a la papelera del usuario (S4b:201), con una marca "de la caja Amiga". Con
  cupo y limpieza sola.

**H43. La pregunta de la guardia caduca a los 30 segundos y deja la caja parada (decidido).**
- Dice: "sin respuesta en 30 s es no"; un no vale "para todo lo de ese programa, sin volver a preguntar"; mientras
  tanto la caja "no atiende al compositor" (S28:272-273, 283-287).
- Falla: A. Quien no está delante se encuentra una instalación rota a medias.
- Cambio: con la sesión delante, la pregunta espera. Si caduca, un aviso que se queda ("actualiza.exe no pudo cambiar
  viejo. Volver a intentar"). Y guardar siempre la versión anterior del ejecutable en la papelera, para poder
  deshacer aunque se diga que sí.

**H44. Lo machacado por la caja no se recupera.**
- Dice: "Hasta entonces, lo machacado (no borrado) por la caja no se recupera" (S28:299-300). El documento lo
  reconoce como agujero 4 (S28:184).
- Falla: A (no hay deshacer).
- Cambio: no esperar a la instantánea del cajón. Cuando la caja vacía o reescribe un fichero que ya existía, el
  servicio de archivos manda la versión anterior a la papelera, igual que al borrar.

**H45. El aviso del bloque de arranque habla con jerga.**
- Dice: el servicio dice "toca lo que tocaban los virus (con qué: `SetFunction`, `DoIO`, `CoolCapture`,
  `KickTagPtr`...)" (S28:266-268).
- Falla: A. S0 pide mensajes en llano con el detalle a un clic.
- Cambio: "Este disquete arranca con un programa que no es el normal. Aquí no hace nada. Cuidado si lo usas en un
  Amiga de verdad." Las funciones, en "Detalles".

**H46. Qué ve la caja de serie: S28 y S6b no dicen lo mismo.**
- Dice: en S28 la caja tiene su cajón y pide cada volumen con `guardian` ("La caja Amiga quiere usar Work:")
  (S28:193-195). En S6b la caja principal tiene `Home:`, `Work:` y red de serie (S6b:246, 249; S6:89-91).
- Falla: A y B. No se sabe si un programa de Amiga ve los documentos del usuario.
- Cambio: decidir cuál manda y corregir el otro. La de S28 pregunta una vez por volumen y es más segura.

**H47. La excepción de modo supervisor "por programa" no dice dónde se pone.**
- Dice: "modo usuario, con excepción por programa" (S28:263).
- Falla: B, leve.
- Cambio: un tooltype en el icono del programa. Se ve, se edita y viaja con él.

---

## S29. Malware de Linux y de DashOS

**H48. El cortafuegos de la caja corta sin avisar, y solo apunta los 32 primeros cortes.**
- Dice: "lo cortado se cuenta y las primeras 32 cosas distintas se dicen en el registro" (S29:65). Abrir la red local
  es `Reglas::red_local`, en el código (S29:66). El permiso `red-local` es solo una propuesta (S29:74-75, 353).
- Falla: A y B. La impresora no va desde Linux y nadie dice por qué. Y después de 32 no queda rastro.
- Cambio: el primer corte hacia la red local lanza la pregunta de `red-local` en llano. Las reglas, en un fichero de
  texto con sus valores de serie. El registro resume lo repetido, pero no deja de apuntar.

**H49. Cerrar el puerto 25 y limitar el caudal "de serie", sin decir cómo se abre.**
- Dice: S29:71-72, 75, 371.
- Falla: B. S0: lo otro queda "a un paso, nunca quitado".
- Cambio: son líneas del mismo fichero de reglas de H48. Cambiarlas es acción de administrador, por orden o en
  Ajustes.

**H50. "Volver a ayer" y volver a una instantánea de `Home:` son todo o nada, con contraseña.**
- Dice: "Volver a ayer en `Prefs` de la caja" (S29:46); "Volver a una instantánea es acción de administrador (o del
  dueño del volumen, con su contraseña)" (S29:272).
- Falla: A y B. Para recuperar un fichero hay que deshacer todo. Volver no se deshace. Y solo hay ventana.
- Cambio: lo de H9 (copiar un fichero de una instantánea, sin contraseña). Antes de volver entera, se toma otra
  instantánea de lo de ahora, para poder deshacer el volver. Y la misma orden `Instantanea` para el disco de la caja.

**H51. Lo de `WBStartup` "pregunta todo".**
- Dice: "que lo de `WBStartup` lo lance `sesiones` con la ficha de un programa suelto (pregunta todo) y que la primera
  vez se avise" (S29:253-254).
- Falla: A. Lo que el usuario puso a propósito le pregunta al entrar.
- Cambio: lo que el usuario arrastra con el ratón no pregunta. Solo avisa lo que apareció sin él: "X se ha puesto
  para abrirse al entrar. Dejar / Quitar", una vez.

**H52. La lista de "no ejecutar" no dice qué ve el usuario ni si tiene salida.**
- Dice: "revocar paquetes malos desde el índice firmado (una lista de no ejecutar que `sesiones` respete)" (S29:237).
- Falla: A y B. Un programa deja de abrirse sin explicación, y no hay vuelta.
- Cambio: aviso en llano con el motivo. Una excepción explícita de administrador, que queda en el registro.

**H53. "Restablecer DashOS" vacía `Paquetes:` sin vuelta.**
- Dice: "deja `SYS:` de fábrica, vacía `Paquetes:` y conserva los volúmenes de los usuarios" (S29:287).
- Falla: A. Un "¿seguro?" donde cabe deshacer: ya hay generaciones (S8:184-186).
- Cambio: restablecer es una generación nueva vacía. La anterior se guarda los 30 días de siempre.

**H54. Los avisos del compositor no dicen dónde se leen.**
- Dice: "el compositor lo dice (pidió pegar sin tecla ni clic: no se le da)" (S29:88). El permiso de portapapeles por
  caja, solo "en `Prefs`" (S29:91).
- Falla: B, leve.
- Cambio: va al registro de H13. El permiso de portapapeles es un permiso más de la orden `Permiso` (H18).

---

## Los cinco hallazgos más importantes del grupo

1. **Lo concedido no se puede leer y está en dos sitios (H18, H15).** S6b:220-221 lo guarda en `Home:` con modo
   `0700` para `sesiones`; S28:280 lo pone en `Estado:`. Solo se ve en `Prefs`. Cambio: un fichero de texto que el
   usuario lee, una orden `Permiso` y un solo sitio.

2. **Los registros no tienen sitio, formato ni orden (H13, H35, H48, H20, H37).** S6:131 y S14:98 los nombran y nada
   más. El cortafuegos apunta solo 32 cortes. Varias cosas pasan sin rastro: un permiso negado, una vuelta atrás del
   arranque. Cambio: texto en `Estado:Registros/`, una orden para leerlos y la misma lista en Ajustes.

3. **Falta el modo de desarrollo explícito (H14, H23, H34).** `SYS:` no lo cambia nadie (S6:54), las aplicaciones
   encerradas no cargan de la capa del usuario (S6b:320) y un servicio solo vive en `SYS:` o en un paquete (S14:68).
   La única salida hoy es el Shell, que quita todo el encierro. Cambio: un modo de desarrollo que se activa a
   propósito, se ve en pantalla y queda en el registro; y capas de máquina y de sesión para los servicios.

4. **Las instantáneas no tienen cara (H9, H50, H44).** Son el "deshacer" de todo el grupo (S5:111, S29:272) y no hay
   forma de sacar un fichero de una: S28:295-297 lo confirma. Cambio: cada instantánea se ve como un cajón de solo
   lectura, "Versiones anteriores" en el menú del icono y una orden `Instantanea`.

5. **Hay datos que el usuario no encuentra (H42, H1, H2, H27).** La papelera de la caja Amiga está en
   `Estado/Servicios/` (S28:291-293); `Home:Datos` no sale en el árbol de S4b; el cajón de la caja está en tres
   sitios; el almacén de `Paq` "no sale en el explorador" (S8:191). Va contra "ordenado, no escondido". Cambio: todo
   con un sitio con nombre en `Home:` o en un volumen que se lee, y el almacén visible con "Mostrar todos los
   ficheros".

Fuera de los cinco, pero de fondo: **falta una capa de la máquina que se pueda escribir** (H3, H4, H39). `DEVS:` y los
ajustes de la máquina apuntan a sitios de solo lectura o sin definir.

**Total: 53 hallazgos** (H1 a H54; H38 es un "cumple", no un fallo).

## Lo que no se comprobó

- Solo se leyeron los nueve documentos del grupo y el apartado de S0. De S27 solo se miraron por encima las líneas
  254-289, que ya prevén órdenes de Shell para los ajustes. Puede que S10, S10c, S13, S27 o S32 ya resuelvan alguno de
  estos hallazgos (por ejemplo, el centro de control para los permisos).
- No se miró el código. Donde un documento dice "hecho", se tomó tal cual.
