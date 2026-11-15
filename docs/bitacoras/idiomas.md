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
