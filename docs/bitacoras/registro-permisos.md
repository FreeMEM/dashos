# Bitácora: el registro común y los permisos que se leen (registro-permisos)

Rama `registro-permisos`, sacada de `revision` (2344aad5). Reglas 2 y 3 de S36 ("un solo registro, legible" y "los
permisos se leen"), con S6 ("El registro: uno solo y legible"), S6b ("Dónde está lo concedido y cómo se mira") y S28
(la guardia que pregunta). Papel: implementa. No se leyó código ajeno a DashOS.

## 2026-11-16

### El registro común (`Estado:Registros/`)

- **Quién escribe**: solo `sesiones`. Es quien sabe quién es cada programa (la marca de su llave), así que pone él la
  hora, el programa y el usuario. Un programa manda qué pasó y el motivo (orden 81, `APUNTAR`); no elige ni el
  fichero, ni la clase (salvo los del sistema, entre `registro::CLASES_DEL_SISTEMA`), ni en nombre de quién escribe.
- **Tres ficheros**, para que lo que manda un programa no se coma lo de seguridad: `seguridad` (lo que decide
  `sesiones`), `sistema` (servicios que vuelven, topes de memoria, memoria baja, la guardia de la caja Amiga) y
  `programas`. Texto, una línea por hecho: `2026-11-16T08:41:30Z editor Ana permiso no se da "red" (usar la red): el
  usuario dijo que no`. La hora va en UTC (`arranque+12.345s` si aún no se sabe); la orden la enseña en la local.
- **Solo se añade**; a los 64 KB el fichero pasa a `.1` (el `.1` de antes se pierde): como mucho 128 KB por fichero.
- **Limpio**: sin letras de control ni secuencias de escape, UTF-8 bien formado, 256 bytes de texto como mucho (con
  "…" si se cortó; el motivo conserva al menos un tercio). Nombres sin espacios: nadie se cuela en otro campo.
- **Ritmo**: 20 líneas de golpe y luego 2 por segundo, por programa y usuario (no por proceso: lanzar copias no da
  más). Lo de más no se apunta; la siguiente línea dice cuántas se perdieron.
- **Quién lee** (orden 80, `REGISTRO_LEER`): cada uno, lo de su usuario y lo del sistema (como la lista de procesos de
  U6); todo, solo tras la acción de administrador "registro" (contraseña en `guardian`; vale mientras viva el programa
  que la pidió). `sesiones` filtra antes de dar nada y lo da en un objeto de memoria solo para leer.
- **El disco**: `sesiones` usa un cliente propio del servicio de archivos (marca aparte), con una raíz nueva en el
  alta: `HUECO_SUYO` (0x40). El cajón se crea si falta, suyo; si ya está y no es suyo, no se da. Se deja en 0700.
- **Lo que se apunta**: todo lo del registro de antes (negativas de matar, servicio, entrar, lanzar, administrar,
  bloquear, apagar; fallos), ahora con motivo; y además lo que se da y se quita (con quién lo pidió), lo que se cierra,
  entrar, volver y salir, los servicios que vuelven (relanzados por `inicio`), la memoria baja y su vuelta (lo manda el
  hilo del aviso al principal), lo que la guardia de la caja niega sin preguntar, y **quién topa con su tope de
  memoria**: el kernel cuenta en cada grupo las veces que un cobro no cupo por un tope (no el del raíz: eso es la
  máquina sin memoria) y lo da en `x3` de `grupo_uso` (llamada 20, sin número nuevo); `sesiones` lo mira cada 2 s y al
  irse cada programa.
- **El registro de antes** (el anillo binario de `Estado/Servicios/sesiones/registro`) sigue: lo lee el centro de
  control. Quitarlo es cosa de cuando Ajustes enseñe el común.
- **Lo que no hay**: vuelta atrás del arranque (no existen aún las generaciones, S8/S14); un controlador o la GPU
  reiniciados (no hay quien lo diga a `sesiones` todavía); "se deja tras N relanzados" de `inicio` (no se lo cuenta a
  `sesiones`).

### La orden `Registro`

`Registro [PROGRAMA x] [CLASE x] [DESDE fecha] [HASTA fecha] [ULTIMAS n] [DE fichero] [TODO] [SEGUIR]`: junta los tres
ficheros (y sus `.1`) por la hora; las fechas, por el principio y en hora local. `SEGUIR` se queda enseñando lo nuevo
(Ctrl-C). `Registro APUNTA texto [MOTIVO x] [VECES n]` apunta (como el programa que la lanza: el Shell). En
`SYS:C/Ordenes` y suelta.

### Los permisos en `Home:Prefs/Permisos/`

- `Home:Prefs/Permisos/concedidos`: cabecera y una línea `programa permiso`. Cajón (0755) y fichero (0644) de
  `sesiones`: el dueño los lee con `Type`; nadie más escribe. Se lee al entrar la persona y se escribe al cambiar algo;
  si su volumen está cerrado, cuando entre. Lo de los programas del sistema, en `Estado:Prefs/Permisos/concedidos`
  (con el usuario del sistema). Lo de antes (`Estado/Servicios/sesiones/permisos`) se pasa al arrancar y se borra.
- Un cajón o un fichero que no es de `sesiones` no se lee ni se escribe, y queda en el registro. Probado: la persona
  puede renombrar el cajón y poner otro (es su `Prefs`), pero el servicio no se lo da a `sesiones` (`HUECO_SUYO`).
- **Orden `Permiso`**: lista lo propio (libre; orden 82). `Permiso DAR|QUITAR programa permiso` (orden 83) lo
  confirma el usuario en `guardian`: "¿Cambiar un permiso?", con el teclado; un guion no puede contestar. Solo desde la
  sesión de delante. Quitar revoca en el acto las llaves de ese servicio de los programas de ese usuario.
- **`guardian`** enseña la frase en llano (S6b) en las preguntas de permisos ("Quiere usar la red."), y el nombre
  interno solo con la D ("Detalles"). El diálogo de un permiso pasa a ser el grande, como el de la caja.

### Lo siguiente (no hecho, como pide el encargo)

- **Modo de desarrollo (regla 4)**: lo aprendido aquí sirve. La marca de una aplicación en modo de desarrollo puede ir
  en el mismo cajón (`Home:Prefs/Permisos/desarrollo`), de `sesiones`, con el mismo `HUECO_SUYO` y la misma
  confirmación de `guardian` que `Permiso DAR` (otro marcador, "+desarrollo"); activarlo y quitarlo, al registro de
  seguridad; y la marca fija en pantalla la tiene que pintar el compositor (un aviso de `sesiones`, como
  `AVISO_DETRAS`). Lo de sustituir piezas de `SYS:` necesita antes la firma de `SYS:` (S14).
- **Tanda de administrador**: `ADMINISTRAR` acepta hoy una acción con un objetivo. Una tanda necesita una lista en
  la pregunta (no cabe en los 168 bytes de `PREGUNTA`: tendría que ir en un objeto de memoria, como las líneas del
  registro) y que el servicio que la hace compruebe cada cosa contra la lista enseñada.
- El registro en Ajustes (la misma lista), y los avisos en pantalla cuando algo se repite.
