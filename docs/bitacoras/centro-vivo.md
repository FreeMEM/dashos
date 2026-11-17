# Centro vivo: el canal con Ajustes abierta, el fondo del usuario al entrar y los menús de Dash

Rama `centro-vivo`, de `revision` (f27af229). Encargo: tres arreglos. Leídas antes `union-menus.md` y `union-dms.md`.

## 1. El centro atiende su canal con la ventana de Ajustes abierta

### La causa

La ventana de Ajustes tenía su propio bucle (`ventana_dentro`, que espera al compositor) y, mientras estaba abierta, el
centro no leía su canal: los avisos de cambio de `ENV:`, las peticiones de la barra y del panel y otro "abrir" se
quedaban en cola hasta cerrarla (`union-menus.md`, "Pendiente").

Había una segunda trampa debajo: el compositor lleva **una conexión por proceso** (conoce a cada uno por su marca). Lo
que el centro pone en el compositor fuera de la ventana (el tema, dónde van los menús) lo hacía con una conexión "de un
momento". Con la ventana abierta, esa segunda conexión se mezclaría con la de la ventana: su `TROZO_CONECTAR` se ignora,
sus peticiones caen en la conexión de la ventana y su adiós la cierra. Por eso no bastaba con leer el canal desde el
bucle de la ventana. (Ya pasaba al elegir el sitio de los menús en Ajustes > Escritorio con la ventana abierta:
`cambiar_sitio_menus` abría una conexión de un momento; ninguna prueba llegaba ahí.)

### El arreglo

- Un hilo aparte (`hilo_canal`, como el de `fondo`) lee el canal "centro" y deja cada mensaje en una cola. Sin ventana,
  el bucle principal espera en una palabra (`hilos::esperar`); con ella, el hilo toca la puerta de la conexión de la
  ventana (`Cliente::conectar_con_puerta`), y `evento_plazo` vuelve. Si la cola se llena (16), el hilo espera a que se
  saque alguno: lo de detrás se queda en la cola del canal del kernel y no se tira nada (el `fondo` sí tira lo que sobra;
  no se ha cambiado). La puerta se pone y se quita con el cerrojo de la cola tomado: el hilo nunca escribe por una llave
  ya cerrada.
- Lo que hacía el bucle principal con cada mensaje es ahora `atender`, y lo llaman los dos bucles. Lo que hay que poner
  en el compositor lleva un `ConVentana`: con la ventana abierta, el tema (`TEMA_OBJ`, ya enlazado) y los menús
  (`dashos_menus_v1`, que se enlaza la primera vez en el objeto 45) van por la conexión de la ventana, sin esperar
  (el compositor lo hace en su orden); sin ella, como antes.
- En el bucle de la ventana, la vuelta sin evento (el plazo de 200 ms o el timbre) ya no se salta el resto: se atiende el
  canal y el monitor del sistema se mide cada 2 s aunque no se mueva nada (antes, solo con eventos).
- Si el tema en uso cambia desde fuera (`ENV:` o el modo del panel), la ventana lo sigue. Lo que la ventana da a la barra
  y al panel (avisos, centro de control) queda apuntado como dado: el aviso de cambio de su propia escritura no lo vuelve
  a dar.
- Otro "abrir X" con la ventana abierta pasa a esa categoría; su llave se contesta (0) al cerrarla, con la del que la
  abrió. Antes se abría otra ventana al cerrar la primera.
- De paso: "cc vigilar" y "avisos vigilar" no cerraban la llave de la respuesta (una por petición).
- La cola es `dashos::hilos::Cola` (un `Anillo` fijo de `usuario/src/anillo.rs`, sin montón), común con `fondo`: el
  `unsafe` de la cola está una vez, en la biblioteca.

## 2. El fondo del usuario sale al entrar

`fondo` arranca antes que el servicio de archivos: lee `Prefs/fondo`, no puede ("sin servicio de archivos") y se queda
el color liso (lo vio `union-dms` en la captura de shiva, y tuvo que ponerlo a mano en Ajustes > Apariencia).

Ahora, cuando el centro ve salir `ENV:` (acaba de entrar alguien: la sesión y el disco ya están), pide a `fondo`
"guardado": lee `Prefs/fondo` (si no hay, el de serie) y lo pone, si no es ya el que se ve. Con la paleta del fondo,
el tema sale del que se acaba de poner (también sin tema guardado: el de serie es la paleta del fondo). El centro lo
dice: "[centro] al entrar, el fondo guardado: X en pantalla (...)". Se espera la respuesta (pintar un fondo de
1920 × 1080 en QEMU, unos 400 ms); mientras, el canal se sigue recogiendo.

## 3. Los menús de Dash: casilla, opciones y submenú (pedido de la sesión de Dash)

- `kit-c/pruebas/ventana.dash`: un menú "Ver" con una casilla (Rejilla), tres opciones (Pequeño, Mediano, Grande) y un
  submenú (Orden), con lo que se elige por la consola (`mui_get_value`).
- kit-c: un submenú dentro de otro daba 0 pero dejaba sus entradas en el abierto, y su `_sub_end` cerraba el de fuera.
  Ahora es un error, como en el Amiga (no hay segundo nivel): lo dice ("[dash-ui] submenú "Más" dentro de otro: no hay
  segundo nivel (como en el Amiga); lo suyo no se pone"), lo que va hasta su `_sub_end` no se pone en ningún sitio y lo
  que viene después sigue en el de fuera. Un `_sub_end` sin submenú abierto también se dice. La prueba hostil va en
  `ventana.dash`: "Más" dentro de "Orden", con "No sale"; tras su fin, "Por tamaño" sigue en "Orden".

## Pruebas

- PC: `usuario/src/anillo.rs` (3, en `make prueba`): en orden, llena devuelve lo que se mete sin pisar nada, diez mil
  vueltas a ritmos distintos sin perder el orden, y lo que lleva se suelta una vez.
- `make prueba`: BIEN.
- `make arranque-virt-menus` (`tools/guion_menus.py`, pasos 12 y 13; capturas 25 a 27 en
  `~/recuperar/geosian-7d/centro-vivo-capturas/`, fuera de git): BIEN.
  - 12: el menú "Ver" de la ventana de Dash, con el teclado: la casilla se desmarca ("Rejilla: 0"), "Grande" desmarca las
    otras, y en "Orden" la tercera entrada es "Por tamaño" (sin el arreglo sería "No sale").
  - 13: Ajustes > Barra con el botón derecho sobre la barra; un guion del Shell lanzado con `Run` hace `Ajuste menus
    barra` y `Ajuste menus dentro` con Ajustes delante: el centro los aplica sin cerrarla (el primero, 3,3 s después de
    abrirse Ajustes, con 5 s de `Wait` en el guion: lo que tarda en arrancar `Ajuste`). Esc la cierra.
  - 13, hostil: 16 `Ajuste menus` seguidos (`RAM:r4`, de fondo) mientras Ajustes se abre y se cierra ocho veces
    (botón derecho, clic, Esc): queda el último (dentro), sin errores de protocolo, ocho abiertas y ocho cerradas, y
    después se abre y se cierra como siempre. Los avisos de cambio se juntan: de 16 cambios el centro aplicó 2 (lee lo
    que hay al llegar el aviso; lo de en medio no se ve, y está bien así).
  - Las vueltas de antes de que pasara: en la primera, Esc no cerraba Ajustes porque el clic en el título del Shell
    (para teclear) la tapaba entera; en la segunda, Ajustes no se llevó el teclado al abrirse. El guion ya no depende de
    eso (las órdenes van de un guion de fondo y un clic da el teclado a Ajustes). No era del centro.
- `make arranque-virt-dms`: BIEN. Su disco trae `Prefs/fondo` = atardecer.png. Al arrancar sale el fallo de partida
  ("[fondo] ondas.png: sin servicio de archivos; se queda el color liso"); al entrar Ana, "[centro] al entrar, el fondo
  guardado: atardecer.png en pantalla (640 × 480, 116 ms)".
- `make arranque-virt-escritorio` y `make arranque-virt-shell`: BIEN.
- `make arranque-virt`: falla en "[explorador] ventana 1: por Tamano, de menos a más: nota.txt" (la búsqueda se llena
  de "a": la tecla pegada, que arregla otra rama). Todo lo de antes pasa, Permisos del centro incluido; al entrar Ana,
  "al entrar, el fondo guardado: atardecer.png en pantalla (… 0 ms)" (ya estaba: lo puso el guion antes de entrar).

## Pendiente del usuario

1. **Otro "abrir" con Ajustes abierta pasa a esa categoría** en la misma ventana (antes se abría otra al cerrar la
   primera). Pero no la trae delante: un cliente no puede traerse delante en este compositor (no hay `xdg_activation`).
   Si Ajustes está tapada, el clic en la barra cambia la categoría sin que se vea. Hacerlo bien es del compositor.
2. **Al entrar se espera a `fondo`** mientras pinta el fondo guardado (116 ms a 800 × 600; unos 400 ms a 1920 × 1080).
   El canal se sigue recogiendo, pero se atiende después.

## Lo que queda

- El aviso "Escritorio/centro-control: no se entiende (Repetido); se queda el que había" sale en cada aviso de cambio
  de `ENV:` (en el disco de pruebas, el de fábrica está roto a propósito). Ya pasaba; ahora se ve más porque el centro
  atiende los avisos con la ventana abierta. No se ha tocado.
- El `fondo` sigue tirando lo que no cabe en su cola (16); el centro ya no.

## Lo que no se ha comprobado

- Que el tema siga a la paleta del fondo nuevo al entrar: en los discos de prueba, la apariencia de Ana está rota a
  propósito ("modo = morado") y se queda el tema que hay; ninguna prueba pasa por ahí.
- El modo oscuro del panel con Ajustes abierta (va por la conexión de la ventana, como el tema de `ENV:`): no hay prueba
  que lo haga con la ventana abierta.
- Ajustes > Escritorio, cambiar el sitio de los menús desde la propia ventana (ahora por su conexión): ninguna prueba.
- `make arranque-virt-2` y `-linux`: no tocan nada de esto y no se han pasado. En la Pi, nada.

## union-tres: `make arranque-virt` sin `guarda` (rama `union-tres`, de fusion-fondo 0c68227c)

El encargo: unido con `tecla-pegada` y `paq-formato`, el guion se quedaba en la commodity y no lanzaba `guarda`. El paso
de `guarda` solo empieza cuando ya salió todo lo demás que se espera; lo que faltaba en esa salida eran las líneas de
`actualiza.exe` de la caja Amiga: su primera pregunta se contestó "no" a los 30 s (el plazo de `guardian`), porque el
guion no llegó a pulsar la "e" a tiempo. En mi rama sola faltaba `guarda` por lo mismo: allí la prueba se paraba antes,
en el explorador (la tecla pegada), y `guarda` esperaba esas líneas.

No sale del centro: con Ajustes cerrada el centro hace lo de antes, y en ese tramo no hay ventana del centro ni fondo al
entrar (eso pasa al entrar Ana, mucho después). En seis vueltas de union-tres no se repitió: la pregunta de
`actualiza` se contestó a los 7-10 s.

Lo que sí salió (la tercera vuelta): "FALTA: [dash-ventana] pulsado". Con las tandas del Shell hasta tarde (las de
`paq-formato` son más), el paso de la ventana de Dash y el del panel quedaban libres en la misma vuelta del guion:
Super+A sacaba el panel encima y se llevaba el clic de "Añadir". Arreglo en `tools/prueba_uefi.sh`: los dos pasos van de
uno en uno (el de Dash no empieza con el del panel a medias, y el del panel no empieza con el de Dash a medias).

Pruebas en union-tres: `make prueba` BIEN; `make arranque-virt` tres vueltas seguidas BIEN enteras (con el reinicio, el
disco, la captura y el sonido), más una cuarta BIEN de antes del arreglo; `-menus`, `-dms` y `-shell` BIEN.

Otra vuelta de antes del arreglo (la primera) pasó todo `arranque_virt.txt`, `guarda` incluido, y falló en el segundo
arranque: tras "se reinicia la máquina", EDK2 se quedó 5 min sin pasar de su primera línea, hasta el tope de 900 s. No se
repitió en las otras cuatro vueltas con reinicio; no he visto por qué (no es del centro ni del fondo: es antes de que
arranque DashOS).

Lo que no he podido ver: la vuelta del coordinador sin la "e" a tiempo, con su traza del guion (no la había). Si vuelve,
`GUION_TRAZA=fichero` y `ESTADO_GUION=fichero` dicen qué hacía el guion entonces.
