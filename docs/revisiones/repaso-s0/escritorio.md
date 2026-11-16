# Repaso contra S0: grupo "escritorio"

Del 2026-11-15. Solo lectura de `docs/diseño/` en el worktree `revision`. No se cambió nada del repo.

El principio (S0, "Amigable de 2026, con fondo para informáticos") da dos preguntas:

- **A**: ¿lo usa sin ayuda alguien que no es informático?
- **B**: ¿puede un informático ver cómo funciona y cambiarlo sin leer el código?

Las citas van con el número de línea del documento (L). Total: 50 hallazgos en 13 documentos.

## S10. Escritorio

Cumple en lo gordo: Prefs con ficheros de texto en `ENVARC:` que se releen solos (L176-177) y puerto ARexx (L122-125, L178).

1. **Los menús solo salen con el botón derecho.** "con el botón derecho, los menús del programa activo" (L85-86). Falla **A**: quien viene de un móvil no los encuentra, y nada en la barra dice que existen. Cambio: la barra enseña los títulos de los menús (o un botón "Menú") también con el clic normal; el botón derecho se queda como atajo del Amiga.
2. **La vista general solo tiene tecla.** "Vista general de todas las ventanas y pantallas, con una tecla" (L70); la disposición "por teclado se cambia al momento" (L72). Falla **A** ("todo se puede hacer con el ratón"). Cambio: un botón en la barra o una esquina activa de serie para la vista general, y el cambio de disposición en el menú de la barra de la pantalla.
3. **"Se recuerda" no dice dónde.** "Si una aplicación se dejó flotando, la próxima vez vuelve a flotar en el mismo sitio" (L59). Falla **B**: es un dato del sistema que no se puede encontrar ni borrar. Cambio: decir que va en un fichero de texto de `ENVARC:` (una línea por programa) y que borrar la línea lo olvida.
4. **El historial de avisos no se puede leer desde fuera.** "Notificaciones con historial" (L93; el servicio, en S10b L149-150). Falla **B**: no hay orden ni fichero. Cambio: una orden `Avisos` (listar, borrar, mandar uno) sobre el servicio, y el historial como texto legible del usuario.

## S10b. Escritorios

Cumple en el canal de órdenes con ARexx (L221-233) y en el aspecto como texto que se relee (L264-265).

1. **Los poderes no se pueden mirar.** "Los poderes los guarda el sistema, en un registro que solo cambia Prefs [...] Nunca salen de un fichero que el usuario o un programa puedan escribir" (L198-199). Que no se escriba a mano es correcto. Falla **B** porque tampoco dice que se pueda leer, y S0 pide "ver qué permisos tiene cada programa". Cambio: una orden `Poderes` de solo lectura y una copia legible del registro; cambiar sigue pidiendo la confirmación del compositor, también desde el Shell.
2. **Un guion sin permiso no mueve ventanas, y no dice cómo se le da.** "un guion sin ese permiso no mueve ventanas" (L230-231). Falla **B**: el primer guion de cualquiera falla sin camino claro. Cambio: decir cómo se concede (la pregunta de permisos la primera vez, como a un programa) y que el error del guion lo diga en llano.
3. **Tres caídas y sale el escritorio mínimo, sin explicar nada.** "Si cae tres veces en un minuto, el compositor lanza el escritorio mínimo" (L373-375). Falla **A y B**: el usuario ve otra barra y no sabe por qué; no se nombra ningún registro. Cambio: un aviso en llano ("Tu escritorio falló y se cambió por el básico. Volver a probar"), con el detalle a un clic y una línea en el registro del gestor de servicios.
4. **El menú de apagado confirma en vez de dejar deshacer.** "bloquear, cerrar sesión, suspender, reiniciar, apagar, con confirmación" (L437). Falla **A** ("deshacer antes que preguntar"). Cambio: bloquear y suspender, sin pregunta; apagar y reiniciar, con una cuenta atrás corta que se cancela. Preguntar solo si hay algo sin guardar.
5. **Una "opción avanzada" con poderes "dados a mano".** Pregunta 8: "no de serie; quizá como opción avanzada, con los poderes dados a mano" (L500-501). Choca con "sin modo experto aparte". Cambio: que un escritorio de la caja Linux pase por la misma pregunta de poderes que cualquier paquete de escritorio, en el mismo panel.

## S10c. El centro de control

1. **Los ajustes solo se cambian en la ventana.** "Nadie cambia un ajuste desde fuera [...] el canal 'ajustes' solo abre una categoría; cambiar algo lo hace el usuario en la ventana" (L31-32). Falla **B** de lleno: S0 dice "si una función solo existe en una ventana, está sin acabar". Además contradice a S27, que pone los ajustes en ficheros de `ENVARC:` y da órdenes de Shell (S27 L253-255, L289-290). Cambio: reescribir el punto. Lo que es del usuario se cambia por fichero y por orden; solo lo que toca permisos, cuentas o la máquina pasa por `guardian`, venga de la ventana o del Shell.
2. **Los permisos solo los ve Ajustes.** Prueba de C1: "Un programa que no es Ajustes no puede listar ni quitar permisos" (L41). Falla **B**: S0 pide poder ver los permisos de cada programa. Cambio: listar los propios es libre para el usuario (orden `Permisos`); quitar o dar desde el Shell pide la confirmación de `guardian`.

## S27. Un escritorio "como DMS"

La sección 7 es la que mejor cumple del grupo: una sola cara, ficheros de texto, presets y Shell.

1. **Parte de los ajustes sigue en binario.** "Ficheros de texto legibles en `ENVARC:` para lo nativo (los que lee la caja Amiga siguen en IFF)" (L267), y son idioma, país, fuentes, modo de pantalla, teclado, puntero y paleta (L291-293). Falla **B**: justo los más básicos no se leen ni se editan a mano. Cambio: el texto manda y el `.prefs` en IFF se genera de él (y al revés si lo escribe un programa 68k); o, como poco, una orden que los vuelque y los cargue como texto.
2. **Desde el Shell solo se carga un fichero entero.** "`Idioma FROM fichero USE` o `SAVE`" (L289-290). Falla **B**: no hay forma de leer o poner un ajuste suelto en un guion. Cambio: una orden común (`Ajuste LEE clave`, `Ajuste PON clave valor`, con `USE` o `SAVE`) que valga para todas las categorías.
3. **El mismo editor se comporta de dos maneras.** "En la ventana de Ajustes, cerrar es Guardar (como DMS); en el editor suelto, los tres botones" (L286). Falla **A** ("los mismos gestos en todo el sistema"), y en Ajustes no queda forma de volver atrás. Cambio: en los dos sitios, el cambio se aplica al momento y hay un "Deshacer cambios" visible; Usar y Guardar quedan en un menú para quien los quiera.
4. **Dos nombres y dos juegos de teclas para lo mismo.** S10 y S23 lo llaman "pantallas" (S10 L38, S23 L30) con Amiga + M / N o Super + flechas (S10 L41). S27 lo llama "espacios de trabajo" (L122, L141) con Super+1..9 (L166) y Super+arriba y abajo (L146). Además Super+Coma mete una ventana en la columna (L132) y es el atajo de Ajustes en DMS (L249): Ajustes se queda sin atajo dicho. Falla **A**. Cambio: elegir un solo nombre para el usuario y una tabla única de atajos, en un solo documento.
5. **Cambiar de espacio solo lo puede pedir la barra.** "petición `espacio` solo de la barra" (L192). Falla **B**, y no casa con el canal de órdenes de S10b (L225-226). Cambio: la misma petición por el canal de órdenes y ARexx, con el poder "Pantallas".
6. **Textos del panel con jerga.** "Sin control aquí" para el brillo (L210-211); "Conectada · 10.0.2.15", "Sin dirección" (L212-213). Falla **A**. Cambio: "Esta pantalla no deja cambiar el brillo"; "Conectada" o "Con cable, pero sin Internet", y la dirección en el detalle.
7. **No se puede apagar.** "Menú de apagado: no. No hay en DashOS forma de apagar ni de reiniciar" (L221-222). Falla **A** hoy. Es un estado, con la pregunta 8 abierta (L228-230). Cambio: subirlo de prioridad; sin apagar, no "sirve recién instalado".
8. **La hora sale en UTC.** "la hora es UTC: no hay zona horaria" (L110). Falla **A** hoy; va en la oleada 3 (L168). Cambio: la zona, de serie, sacada del idioma y país elegidos al instalar.

## S26. El aspecto de las cajas

El formato de `Prefs/cajas` (L72-77) es un buen ejemplo de ajuste legible.

1. **El tema solo lo cambia el centro de control.** "El servicio del tema es el centro de control [...] es el único al que el compositor deja cambiarla" (L60-62). Falla **B**: no hay tema ni fondo por guion. Contradice a S10 ("Por ARexx, para cambiar el tema o el fondo con un guion", L178) y a S10b, que tiene un servicio de apariencia sin pantalla (L153, L386-388). Cambio: el servicio de apariencia de S10b es el dueño; vigila el fichero de `ENVARC:` y lo aplica, y la ventana y el Shell escriben ese fichero.
2. **Las excepciones se ponen a mano y la ventana solo las cuenta.** "Las excepciones se ponen a mano en el fichero por ahora [...] El centro solo dice cuántas hay" (L79-82). Falla **A**. Cambio: adelantar la casilla "Aspecto original" en Información (paso 5, L181), y que Apariencia liste las excepciones con un botón para quitarlas.
3. **Un fichero mal escrito no dice qué línea falla, ni dónde está.** "lectura que rechaza todo lo que no vale"; "un texto que no vale es un error de protocolo" (L152-155). Las rutas salen como `Prefs/apariencia` y `Prefs/cajas` (L55, L58), no como `ENVARC:`. Falla **B**. Cambio: dar la ruta en `ENVARC:`, decir que se relee al guardar, y que el fallo deje un aviso con la línea y siga con el ajuste anterior.
4. **Algunos programas no cambian hasta reabrirlos, sin avisar.** "GTK 4, libadwaita, Qt y foot lo ven al abrirse otra vez" (L128-130). Falla **A**: el usuario cambia el tema y ve medio escritorio sin cambiar. Cambio: hasta que esté el portal, un aviso corto ("Algunos programas de Linux lo verán al abrirlos de nuevo").

## S23. Las pantallas del Amiga

1. **La compatibilidad la tiene que diagnosticar el usuario.** "Se elige en las propiedades del programa (como 'modo de compatibilidad' en Windows)" (L50-54). Falla **A** para los programas que no están en la lista de conocidos. Cambio: si un programa 68k pide algo que el Workbench grande no da o se cae al abrir, ofrecer "Probar con Workbench clásico" en un clic, y recordar la respuesta.
2. **No dice dónde se guardan la ficha, el filtro ni la promoción.** Ficha (L51-54), filtro "monitor de la época" (L74-75) y promoción "por programa" (L78-79). Falla **B**. Cambio: que sean tooltypes del icono del programa (texto, visibles en Información y editables desde el Shell), y la lista de conocidos, un fichero de texto en `SYS:`.
3. **Pasar a ventana, solo con una tecla.** "con la opción de verlas en ventana con una tecla" (L135-136). Falla **A**. Cambio: la misma opción en el menú de la barra de esa pantalla.

## S9. Gráficos

Casi todo es interno. Dos puntos en "Pantallas y monitores":

1. **Cambiar de modo sin red de seguridad.** "`Prefs/ScreenMode` elige resolución, frecuencia y escala" (L101). No dice qué pasa si el modo nuevo deja la pantalla en negro. Falla **A**. Cambio: el modo se prueba unos segundos y vuelve solo al anterior si no se acepta. Es un deshacer, no un "¿seguro?".
2. **No hay orden de Shell ni se dice el formato de `DEVS:Monitors`.** "tiene un fichero por salida" (L100). Falla **B**. Cambio: decir que son texto, y una orden (`Pantalla LISTA`, `Pantalla HDMI-0 1920x1080 60`).

## S19. El sonido

Cumple en el micrófono: permiso por programa y marca que no se puede tapar (L124-127).

1. **Si cae un controlador, el sonido cambia de salida sin decir nada.** "Si un controlador cae, el servicio sigue y pasa a otra salida" (L79-80). Falla **A**: lo que iba por los auriculares puede salir por los altavoces. Cambio: aviso en llano con la salida nueva, una línea en el registro, y no saltar nunca solo de una salida privada a los altavoces.
2. **No hay regla de qué salida se usa recién instalado.** Solo "elegir por dónde sale" (L27). Falla **A**. Cambio: una regla de serie (lo último que se enchufa gana, y se recuerda por aparato) con el cambio a un clic en la barra.
3. **Volumen y salida, solo en la barra.** "La barra lo enseña con los programas que suenan" (L81). Falla **B**: no hay orden ni ARexx. Cambio: una orden `Sonido` (volumen, silencio, salida, por programa) y el mismo servicio por ARexx.
4. **No queda rastro de quién grabó.** La marca solo se ve mientras se graba (L125-126). Falla **B** ("nada pasa sin dejar rastro"). Cambio: una línea en un registro legible: qué programa, desde cuándo y hasta cuándo.

## S19b. Sonido de estudio

1. **Las conexiones del grafo no tienen ni ventana ni orden.** "Conexiones libres, que se cambian sin cortar el sonido" (L24-25). El documento no dice cómo las hace el usuario. Falla **A y B**. Cambio: una ventana de cables que se arrastran, una orden (`Sonido CONECTA a b`) y el grafo guardable como fichero de texto.
2. **El período se elige en muestras.** "64 muestras para tocar, 256 por defecto, más para ahorrar energía. Lo eligen `Prefs/Sonido` y el programa" (L28-29; S19 L77-78). Falla **A**. Cambio: automático de serie (baja cuando un programa lo pide y sube solo); en Prefs, nombres llanos ("Normal", "Para tocar", "Ahorro") y la cifra en el detalle.
3. **Lo que falla o se ignora no deja rastro.** Un plugin colgado se quita "con un fundido" (L76-77); las llamadas de JACK sin sentido "se aceptan y no hacen nada" (L89-90). Falla **B**. Cambio: aviso en llano cuando se quita un plugin, y una línea de registro en los dos casos.

## S22. Suspender e hibernar

Cumple en el fondo: dormirse no se cuelga y un aparato que falla se avisa (L41-42).

1. **Una actualización tira la sesión hibernada.** "Si algo no cuadra (otro kernel tras una actualización [...]), arranque normal [...] Se pierde la sesión abierta, y se avisa" (L79-80). Falla **A**: el usuario pierde lo abierto por algo que no hizo. Cambio: si hay imagen, arrancar con la generación que la hizo (S8) y pasar a la nueva en el siguiente arranque limpio.
2. **Lo que no se guardó a tiempo se pierde sin decir qué.** "Se sigue sin ellos" (L52); "lo que falte se pierde como si se fuera la luz" (L54). Falla **A y B**. Cambio: al despertar, un aviso que nombre los programas que no contestaron, y el registro de cada ciclo (qué se paró a la fuerza, qué aparato falló, qué lo despertó).
3. **No hay orden ni se dice cómo se pide dormir.** El documento no nombra orden, botón, tapa ni tiempo sin uso; las fuentes de despertar son "botón, teclado, alarma" (L56). Falla **B**. Cambio: órdenes `Dormir` e `Hibernar` (con `HASTA hora` para la alarma) y los ajustes de energía como fichero de texto.
4. **El sitio de la imagen no tiene dueño.** "un área propia en el disco, del tamaño de la memoria en uso" (L83). S32 no la reserva. Falla **A** si hibernar falla por falta de sitio. Cambio: que el instalador la reserve o que sea un volumen elástico, y si no cabe, decirlo antes y ofrecer suspender.

## U6. Cambio rápido de usuario

El modelo de sesiones es claro y las garantías están bien escritas.

1. **Apagar con otra sesión abierta pide un administrador.** "Apagar con otras sesiones abiertas: aviso y contraseña de administrador" (L146-147). Falla **A**: en un ordenador de casa, quien no es administrador no puede apagarlo. Cambio: ofrecer siempre hibernar (guarda las sesiones de todos y no pide nada); la contraseña, solo para apagar de verdad.
2. **Cambiar de usuario es Esc y escribir el nombre.** "Esc pasa a 'Cambiar de usuario': se escribe el nombre y luego la contraseña" (L33-35); la lista queda "más adelante" (L41) y con duda (L142-143). Falla **A**. Cambio: un botón visible "Cambiar de usuario" y la lista de cuentas de serie; esconderla es un ajuste para quien lo necesite.
3. **Lo que se negó a la sesión de detrás no se cuenta al volver.** "Sus preguntas de permisos y sus acciones de administrador se niegan" (L20). Falla **A y B**: el programa de Ana falla y ella no sabe por qué. Cambio: al volver, un aviso con lo que se pidió mientras no estaba, y que quede en el registro de `sesiones`; adelantar la espera del paso 3.
4. **La música de detrás sigue sonando en silencio.** "Sus anillos se siguen leyendo a su ritmo" (L22, L77-78). Falla **A**: al volver, la canción o el vídeo ya han avanzado. Cambio: al pasar a detrás, el servicio de música manda pausa a lo que suena; el silencio queda para lo que no sepa pausarse.

## S32. El instalador

Cumple en lo principal: plan con dibujo antes de tocar (L21-23), copia de las tablas para deshacer (L37-39) y versión de órdenes (L43).

1. **Sin hueco libre, manda al usuario a Windows.** "pedir al usuario que lo encoja desde Windows y usar el hueco" (L28-29); primera versión, "solo usar hueco libre o un disco entero" (L31). Falla **A**: quien tiene Windows y no es informático se queda entre borrarlo todo o no instalar. Cambio: como poco, una guía paso a paso con dibujos dentro del instalador; y poner fecha a encoger NTFS cuando sea seguro.
2. **Sin IOMMU, el aparato no funciona y no se dice.** "Sin IOMMU, un aparato servido así alcanza toda la memoria: no se hace" (L94-95). Falla **A**: muchos PC lo traen apagado y el usuario solo ve que no hay Wi-Fi. Cambio: que el instalador lo mire y lo diga en llano ("Para usar el Wi-Fi hay que encender una opción de la placa. Te digo cómo"), con el detalle a un clic.
3. **Borrar una partición ajena lleva "doble confirmación".** (L48). Falla **A** ("deshacer antes que preguntar"). Cambio: una sola pregunta que diga qué se pierde, y un "Deshacer" que repone la tabla guardada mientras no se haya escrito encima.
4. **El controlador de NVIDIA se instala con el gestor de la caja Linux.** "dentro de la caja Linux, con su gestor de paquetes (como en Arch)" (L88-89). Falla **A**. Cambio: que `Paq` lo proponga como los demás controladores (L90-92) y haga él la instalación dentro de la caja.
5. **No hay registro ni plan como fichero.** El plan solo se enseña (L21-23). Falla **B**. Cambio: el plan es un fichero de texto que se puede guardar, revisar y pasar al instalador para instalar sin manos; y lo hecho queda en un registro legible en el disco nuevo.

## S33. Códecs

La idea de DataTypes cumple A: el programa no sabe de códecs.

1. **Recién instalado, el vídeo más corriente no se oye o no se ve.** "H.264, HEVC y AAC: por el decodificador de la máquina [...] Por software, no van en `SYS:`: un paquete opcional" (L18-20). La Pi 5 no tiene H.264 por hardware (L36), y ninguna fila de la tabla decodifica AAC (L33-39). El respaldo es "la caja Linux con mpv" (L21). Falla **A** ("sirve recién instalado"). Cambio: al abrir uno de esos ficheros, un aviso en llano con un solo botón ("Instalar lo que falta" o "Abrirlo con el reproductor de Linux"), sin que el usuario sepa qué es un códec.
2. **No se ve por dónde se decodifica.** "cada clase elige dónde decodifica" (L11-12). Falla **B**: no se puede saber si va por hardware o por CPU, ni forzarlo. Cambio: una orden que liste las clases y su camino, un ajuste de texto para forzar CPU, y una línea de registro al abrir.

## Los cinco más importantes del grupo

1. **S10c, L31-32 y L41: los ajustes y los permisos solo existen en la ventana.** Es la contradicción más directa con S0 (pregunta B) y también con S27 sección 7. Hay que reescribir esos dos puntos: fichero y orden para lo del usuario, `guardian` solo para permisos, cuentas y máquina; ver permisos es libre.
2. **S33, L18-21: H.264 y AAC fuera de serie.** Recién instalado, un vídeo del móvil no se oye, y en la Pi 5 no se ve (pregunta A). Falta el camino de un clic para el que no sabe qué es un códec.
3. **S10, L85-86: los menús escondidos tras el botón derecho.** Es el gesto del Amiga, pero nadie de 2026 lo encuentra (pregunta A). De serie tienen que verse; el botón derecho se queda.
4. **S22, L79-80 y L52-54: hibernar pierde la sesión tras una actualización, y dormir pierde datos sin decir de quién.** Falla A. Arrancar con la generación que hizo la imagen, y un aviso y un registro por cada ciclo.
5. **S27, L267 y L289-290: los ajustes básicos siguen en IFF y el Shell solo carga ficheros enteros.** Falla B justo en idioma, fuentes, pantalla y teclado. El texto tiene que mandar, y hace falta una orden para leer y poner un ajuste suelto.
