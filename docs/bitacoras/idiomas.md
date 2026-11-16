# Idiomas y catálogos en lo nativo (paso 4)

Rama `idiomas`, desde `revision` (41e5b81). Encargo: el paso 4 de `docs/REVISION_LOOP.md`. Que DashOS nativo tenga
idiomas y catálogos como AmigaOS: hasta ahora los textos de los programas y del kit iban en castellano dentro del
código. Diseño de partida: S4b (`LOCALE:`, Prefs), S27 (sección 7) y S11. La caja Amiga no se toca: su
`locale.library` es otro paso.

## Diseño

### Las piezas

- **Crate `idioma`** (`idioma/`, `no_std` con `alloc`, sin dependencias). Entiende los ficheros y decide cuáles leer,
  pero no abre ninguno: trabaja con bytes y con un `Lector` que se los da. Se prueba entero en el PC (`cargo test`).
  - `prefs`: `locale.prefs`, el IFF `PREF`/`LCLE` de AmigaOS (el `LocalePrefs` de 860 bytes): país, hasta diez
    idiomas por orden de preferencia, minutos al oeste de Greenwich y los datos del país. Lee y escribe.
  - `catalogo`: IFF `CTLG` (`FVER`, `LANG`, `CSET`, `STRS`). UTF-8 con `CSET` 106 (como AmigaOS 4) en los nativos;
    los del Amiga (sin `CSET`, 0, 3 o 4: Latin-1; 111: Latin-9) se convierten al leer.
  - `pais`: el `CountryPrefs` de 504 bytes, suelto en un `.country` (IFF `PREF`/`CTRY`) o dentro de `locale.prefs`.
    Números con sus grupos, decimales y dinero (sin coma flotante: el dinero se da en céntimos).
  - `fecha`: las plantillas de `FormatDate` (`%d/%m/%Y`, `%A`, `%e`…) y los 50 textos de un idioma (días, meses,
    sí, no…) con los números de `GetLocaleStr`.
  - `sesion`: `Idiomas::cargar` (qué se lee y en qué orden), `catalogo` (como `OpenCatalog`) y `ha_cambiado`.
  - `puesto` y `formato`: el catálogo en uso de un programa y los textos con huecos.
- **El kit** depende de `idioma` y lo reexporta (`kit::idioma`). `kit::textos` tiene el catálogo del propio kit
  (`kit.catalog`) y `arrancar`, que lo hace todo de una vez.
- **`usuario/src/idiomas.rs`**: el `Lector` de verdad, sobre el servicio de archivos y la tabla de nombres del proceso
  (`dashos::asignaciones`), con `LEER_NOMBRE` (un viaje por fichero). Lo incluye cada programa con `#[path]`, como
  `ventana_cliente.rs`: usa el montón, y la biblioteca `dashos` la enlazan también programas sin montón.
- **`tools/catalogo.py`** y la carpeta **`locale/`**: de dónde salen los ficheros de `LOCALE:` (abajo).

### Qué se lee, y en qué orden

1. `ENV:Sys/locale.prefs`. Si no está, `ENVARC:Sys/locale.prefs` (hoy `ENV:` no se copia de `ENVARC:` al entrar). Si
   no hay ninguno, castellano y España. El que hay manda aunque esté roto: uno roto en `ENV:` no deja paso al
   guardado, da lo de serie.
2. Si el `locale.prefs` no trae los datos del país (uno hecho a mano), `LOCALE:Countries/<país>.country`, por su
   nombre tal cual y en minúsculas. Si no, España.
3. Los textos del idioma: `LOCALE:Languages/<idioma>.language`, del primer idioma preferido que lo tenga.
4. El catálogo de un programa: por cada idioma preferido, en orden y **hasta llegar al del propio programa** (el
   castellano: con ese no se busca, como `OC_BuiltInLanguage`), `PROGDIR:Catalogs/<idioma>/<programa>.catalog` y
   luego `LOCALE:Catalogs/<idioma>/<programa>.catalog`. El primero que valga.

Sin `locale.prefs`, un programa hace dos preguntas al servicio de archivos al arrancar (las dos, "no existe") y
ninguna más.

### Cuándo vale un catálogo

Vale entero o no vale: si no vale, el programa sigue con sus textos. No vale si no es un IFF `CTLG`, si un largo dice
más de lo que hay (cortado), si pasa de 1 MB, si su `CSET` es uno que no sabemos leer, si su `$VER:` trae el nombre de
otro programa, o si no es de la versión que pide el programa (un catálogo de otra versión tiene otros números; sin
`$VER:` es la versión 0). Dentro de uno que vale:

- Un texto que no es UTF-8 válido se salta: ese sale con el de siempre.
- De un número repetido vale el primero.
- Un texto con huecos cuya traducción pide un valor que el original no da (`{}` de más) no se usa: sale el original.
- El número 0 es "sin número": siempre el texto de siempre.

Los nombres de idioma, de país y de programa que llegan de un fichero van a formar parte de un camino: no valen con
barras, dos puntos, bytes de control, punto delante o más de 29 letras.

### Cómo lo usa un programa

```rust
#[path = "../idiomas.rs"]
mod idiomas;
const CATALOGO: (&str, u32) = ("explorador", 1);       // nombre y versión: tools/catalogo.py lo lee de aquí
static TEXTOS: idiomas::Puesto = idiomas::Puesto::nuevo();
fn tr(numero: u32, de_siempre: &'static str) -> &'static str { TEXTOS.texto(numero, de_siempre) }
fn trf(numero: u32, de_siempre: &'static str, valores: &[&dyn core::fmt::Display]) -> String { TEXTOS.con(numero, de_siempre, valores) }

let idioma = idiomas::arrancar(&mut fs, &tabla, &TEXTOS, CATALOGO);   // al empezar
boton(tr(12, "Abrir"));
linea(&trf(13, "{} elementos, {} libres", &[&n, &libre]));
idioma.local.fecha_corta(segundos);                                    // 03/11/2026
```

El texto de siempre va en el código, como en `GetCatalogStr`. `tr` da un `&'static str`: el catálogo puesto no se
libera nunca (cambiar de idioma cuesta lo que mide un catálogo, unos KB).

### De dónde salen los ficheros: `locale/` y `tools/catalogo.py`

```
locale/programas.txt           de qué fuentes sale el catálogo de cada programa
locale/<programa>.cd           sus textos (lo hace la herramienta desde los tr() del código; no se edita)
locale/english/<programa>.ct   la traducción al inglés (se edita: cada MSG_n, su traducción y el original debajo)
locale/idiomas/idioma.cd       los 50 textos de un idioma, en castellano
locale/idiomas/english.ct      los del inglés: de ahí sale LOCALE:Languages/english.language
locale/paises/*.pais           los países, en texto: de ahí sale LOCALE:Countries/*.country
```

Los formatos `.cd` y `.ct` son los de `tools/catalogo.py` de Dash (los de FlexCat), que se tomó de referencia.

- `numerar FUENTES`: quien escribe un texto nuevo pone `tr(0, "texto")` y esto le da número (el del mismo texto si
  ya lo tiene, o el siguiente libre).
- `cd`, `ct` (`--update` guarda las traducciones de uno anterior), `catalog` (`--latin1` para el Amiga), `pais`,
  `prefs` (escribe un `locale.prefs`) y `ver` (enseña lo que trae un fichero).
- `todo -o build/Locale`: rehace los `.cd` y deja el árbol de `LOCALE:` en `build/Locale`. `make -C kernel catalogos`
  lo llama con `--comprobar` y **falla** si un `.cd` no está al día con el código, si a un `.ct` le sobra un texto,
  si su original cambió desde que se tradujo, o si una traducción pide valores que el original no da. Lo que falta
  por traducir es un aviso.

`tools/disco_archivos.py` copia `build/Locale` al `Locale/` del disco de pruebas.

### El aviso de cambio

El servicio de archivos **no tiene avisos de cambio** (no hay nada como `StartNotify`: se miró el protocolo, 36
órdenes y ninguna vigila). Así que hoy un programa ve el idioma nuevo **al abrirse**. Para quien quiera enterarse
antes, `Idiomas::ha_cambiado` compara la fecha y el tamaño de `locale.prefs` con los que leyó (una pregunta al
servicio); si cambió, `arrancar` otra vez. Ningún programa lo llama todavía por su cuenta. Pendiente: una orden de
vigilar en `archivos`, que es lo que pide S4b ("los servicios vigilan `ENV:`").

## Decisiones pendientes del usuario

1. **Sin idiomas en `locale.prefs`, castellano.** En AmigaOS una lista vacía es inglés. Aquí el idioma de serie es el
   castellano (lo pide el encargo), así que un `locale.prefs` guardado por el editor del Amiga sin ningún idioma se
   lee distinto en la caja (inglés) y en lo nativo (castellano). Quien quiera inglés en los dos pone `english`.
2. **Los `.language` nativos son catálogos**, no código. En AmigaOS un `.language` es una biblioteca 68k; lo nativo no
   puede cargarla. El nuestro es un IFF `CTLG` con los 50 textos del idioma. Un `.language` de AmigaOS en esa carpeta
   se rechaza sin más (no es un IFF) y se sigue en castellano.
3. **El texto de `locale.prefs` y de los `.country` se lee como Latin-9** (ISO 8859-15), no Latin-1: es lo mismo
   salvo ocho signos, y así cabe el `€` (0xA4) en un fichero que también lee el Amiga. Un Amiga con fuentes Latin-1
   verá `¤`.
4. **Un catálogo con el nombre de otro programa en su `$VER:` no vale.** AmigaOS no lo mira. Aquí sí, para que un
   fichero mal copiado no ponga textos que no tocan.
5. **`PROGDIR:Catalogs` antes que `LOCALE:Catalogs`**, como AmigaOS. Un programa puede traer sus catálogos en su cajón.
6. **La hora de España de serie es UTC+1 todo el año** (`gmt` -60): no hay zonas horarias ni cambio de hora. El
   `locale.prefs` trae los minutos de diferencia, como en el Amiga.
7. **Los nombres de fichero que crea el explorador se traducen**: "nota (copia).txt", "(restaurado)" y "Carpeta sin
   nombre" salen en inglés como "nota (copy).txt" y "Untitled Folder", como en otros sistemas. La otra opción es
   dejarlos fijos en castellano.
8. **El centro tiene dos raíces más para leer** (`inicio`: el disco y el `RAM:` de Ana, solo lectura), porque nace de
   `inicio` solo con `C`. Sin ellas no puede leer `LOCALE:` ni `ENV:`. Es un programa del escritorio, de confianza,
   pero es más alcance del que tenía.

## Bitácora

### Paso 1: el crate, la herramienta, el kit y la prueba en QEMU

- `idioma/`: lo del diseño. 27 pruebas de PC (23 del crate y 4 que leen lo que escribe `tools/catalogo.py`: el
  `.country` de España sale byte a byte igual que el país de serie del código).
- El formato se cotejó con ficheros de verdad de un AmigaOS del usuario (solo cabeceras y tamaños, sin copiar nada):
  `locale.prefs` mide 894 bytes con un `LCLE` de 860; un `.country`, 538 con un `CTRY` de 504; un `.catalog` trae
  `FVER`, `LANG`, `CSET` de 32 bytes y `STRS`. El país, los idiomas y la diferencia horaria están donde los lee el
  crate. Los valores de España, Reino Unido y Estados Unidos (`locale/paises/*.pais`) son nuestros.
- El kit: "sí"/"no", los errores de PNG y los nombres de los acentos (`kit::tema::nombre_acento`).
- `cambio` (lo lanza `guardian` como Ana y como Carla) dice lo suyo en el idioma de cada una. En el disco de pruebas,
  Carla tiene `Estado/RAM/Carla/Env/Sys/locale.prefs` (inglés, Reino Unido); Ana no tiene.

Prueba en QEMU (`make arranque-virt`), las líneas tal cual:

```
[cambio] Ana: idioma español (España), de serie: "Hola, soy la prueba del cambio de usuario", "2 ventanas de Ana", el kit dice "sí"; martes, 3 de noviembre de 2026; 03/11/2026 18:05; 1.234.567; -1.234,56 €
[cambio] Carla: idioma english (great_britain), de ENV:Sys/locale.prefs: "Hello, I am the user switching test", "2 windows of Carla", el kit dice "yes"; Tuesday 3 November 2026; 03/11/2026 17:05; 1,234,567; -£1,234.56
[cambio] Carla: catálogos malos (están: true): cortado false, de otro programa false, de otra versión false, fuera del cajón false; el idioma ha cambiado false: bien
```

Lo hostil en QEMU: en el disco hay `rota.catalog` (medio catálogo) y `ajena.catalog` (el del kit con otro nombre); ni
esos, ni el del kit pedido en la versión 99, ni un nombre que sube por el árbol (`../../../Prefs/fuente`) valen.

`make prueba`, `arranque-virt`, `arranque-virt-2` y `arranque-virt-linux`: bien a la primera.

### Paso 2: el centro, el explorador y el Workbench

Cada programa lleva su `CATALOGO`, su `Puesto` y sus `tr`/`trf`, y llama a `idiomas::arrancar` antes de pintar nada.

| Programa | Textos | Dónde arranca el idioma |
|---|---|---|
| kit (con la rejilla de permisos) | 25 | lo pone cada programa |
| centro | 96 | tras aplicar el tema; y otra vez al abrir su ventana, si `locale.prefs` cambió |
| explorador | 177 | al recibir su tabla de nombres, antes de la primera ventana |
| Workbench | 35 | antes de construir su interfaz |
| cambio (prueba) | 2 | al empezar |

Todos traducidos al inglés (`locale/english/*.ct`).

- **Lo de la consola sigue en castellano fijo.** Las líneas que comprueban las pruebas no pasan por catálogo. Donde
  una línea de consola reutilizaba un texto que también se pinta, se separaron (el explorador lleva textos gemelos
  "fijo" y "visible"). En el centro, algunas líneas de consola imprimen las mismas filas que se ven: con inglés
  saldrían en inglés (sin `locale.prefs`, idénticas).
- **`usuario/src/permisos_kit.rs`** (la rejilla de permisos, del Workbench y del explorador) va en el catálogo del kit.
- **El centro** nace de `inicio` solo con su cajón `C`: ahora `inicio` le da además el disco (raíz 1) y el `RAM:` de
  Ana (raíz 3), los dos solo para leer, y `sesiones` le pone `SYS:` y `RAM:` en su tabla (`LEEN_IDIOMA`). Cuando
  `ENV:` pase a memoria, esa raíz 3 fija (`Estado/RAM/Ana` en `RAICES_DE_MAS`) hay que cambiarla con lo demás.
- **Sin pasar a los formatos del país**: las fechas del explorador ("13 nov", "10:04") y del Workbench y el centro
  (en UTC), y los tamaños ("7,0 MB": la coma la pone el catálogo, no el país). Usar `Local` cambiaría lo que se ve
  hoy en castellano (otra forma de fecha, la hora de España en vez de UTC), y eso no se tocó.

`make prueba` (con las 24 pruebas de PC del explorador y las 27 de `idioma`), `arranque-virt`, `arranque-virt-2` y
`arranque-virt-linux`: bien a la primera, con los tres programas pasando por catálogo (en castellano, como Ana).
Rendimiento: no se midió; cada programa hace al arrancar dos preguntas más al servicio de archivos (las dos, "no
existe") y una a `sesiones` por su tabla.

## Lo que queda

- **Una orden de vigilar en `archivos`**, para que el idioma cambie al momento (S4b). Hoy: al abrir el programa (y
  el centro, al reabrir su ventana).
- **El editor de idioma** (la sección "Idioma y región" de Ajustes, y `SYS:Prefs/Idioma`): `Prefs::escribir` ya
  escribe el `locale.prefs`, pero no hay ventana que lo use. Hoy el idioma se cambia poniendo el fichero
  (`tools/catalogo.py prefs`).
- **`ENV:` copiado de `ENVARC:` al entrar** (otro paso lo está pasando a memoria): entonces el `locale.prefs` de
  Carla del disco de pruebas tiene que salir de su `ENVARC:` o sembrarse de otra forma; el lector ya mira
  `ENVARC:Sys/locale.prefs` si no hay nada en `ENV:`.
- **Los demás programas**: `guardian`, `barra`, `panel`, `fondo`, `selector`, `lanzador`, `avisos`, `ajustes` (la
  prueba del kit) y kit-c siguen en castellano fijo. La fecha de la barra (`usuario/src/reloj.rs`) no usa el país.
- **Fechas y tamaños según el país** en el explorador, el Workbench y el centro (arriba).
- **Los `.info` y nombres de cajones según el idioma** ("Documentos"…, S4b): nada.
- **`LANG` para los programas de Linux** desde `locale.prefs` (S4b): nada.
- **En la caja Amiga**, `locale.library` dando estos catálogos en Latin-1 a los 68k: es otro paso. `tools/catalogo.py
  catalog --latin1` ya los compila así.
- **El editor de catálogos en Dash** (`SYS:Utilities`): es de la sesión de Dash.
- Zonas horarias y cambio de hora.

## Lo que no se ha comprobado

- Ningún programa de verdad (centro, explorador, Workbench) **se ha visto en inglés en pantalla**: en las pruebas
  corren como Ana, en castellano. En inglés solo se ha visto `cambio` (sus textos, los del kit y los formatos). Los
  catálogos ingleses de los tres se compilan y se comprueban contra sus `.cd` (huecos, números), y nada más: puede
  haber textos que no quepan en su sitio en inglés.
- Las plantillas de fecha contra el `FormatDate` de un AmigaOS de verdad (no se pasó por el oráculo): `%e` rellena
  con un espacio y `%q`/`%Q` van sin cero, según la documentación.
- Un `locale.prefs` escrito por el editor de Locale de AmigaOS dentro de la caja, leído por lo nativo: el formato se
  cotejó con ficheros de verdad, pero no se hizo la prueba de punta a punta.
- El número de país (`cp_CountryCode`): ponemos las letras ISO ("ES"); no se sabe qué pone AmigaOS en cada uno.
