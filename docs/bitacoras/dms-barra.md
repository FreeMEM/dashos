# Bitácora: la barra configurable de DMS (rama `dms-barra`)

Rama `dms-barra`, sacada de `revision` (e60553a6). Encargo del usuario (2026-11-16): "vamos a crear un sistema de
escritorio configurable como el de DMS de Niri. De hecho si te lo puedes fusilar, estaría bien". Y después: "si puedes
darle el mismo aspecto que mi escritorio en Shiva ya sería la rehostia". Diseño de partida: `docs/diseño/S27-escritorio-dms.md`
(sección 0: DMS es MIT desde el 12/11/2025 y se porta su código) y `docs/bitacoras/escritorio-dms.md` (oleadas 1 y 2).

Código de DMS leído: v2.1.2 (`~/recuperar/geosian-7d/dms-fuente/`, fuera de git): `Common/settings/SettingsSpec.js`
(`barConfigs`), `Common/settings/BarWidgetDefaults.js`, `Common/BarWidgetCatalog.qml`, `Modules/DBar/*` (medidas y
radios), `Common/Theme.qml` y `DCommon/Common/Shape.js` (fórmulas de grosor, letra, iconos y radios).

## Licencia

El texto exacto del `LICENSE` de DMS (lo único que se bajó del repositorio oficial, `AvengeMedia/DankMaterialShell`,
ramas `master` y `main`, iguales) está en `docs/licencias/DankMaterialShell-MIT.txt`. Lo portado lleva en su cabecera:
"Portado de DankMaterialShell, MIT, Copyright (c) 2025 Avenge Media LLC". En `dms-fuente` no venía el `LICENSE` (es la
carpeta `quickshell` del repositorio; su README lo enlaza en `../LICENSE`).

## Coordinación con `dms-avisos`, `dms-dock` e `iconos`

- **Mío**: `ENV:Escritorio/barra` (el modelo de `barConfigs`, el registro de widgets, el importador de `settings.json`),
  la barra (`usuario/src/bin/barra.rs`), Ajustes > Barra en el centro y el perfil "shiva" de la barra.
- **La paleta de shiva** (matugen `scheme-monochrome`, oscuro: primario blanco, superficie #131313): por encargo del
  coordinador (dato de `dms-avisos`), la hago yo, en `kit/src/tema.rs` y solo añadiendo: `MONOCROMO` (su "color" en el
  protocolo del tema es el negro puro) y `monocromo(oscuro)`, con los trece colores de matugen tal cual; `del_sistema`
  y `esquemas` lo devuelven para ese color. En Ajustes > Apariencia es una fila más ("Color fijo monocromo") y en la
  orden, `Ajuste acento monocromo`. `dms-avisos` y `dms-dock`: no hace falta nada, lo toman del tema.
- **Radios**: DMS tiene un `radiusStrength` global (shiva: 38) y, con niri, el radio de las ventanas (shiva: 3), que es
  también el de las esquinas de la barra. Mientras no haya un sitio común en `ENV:Escritorio/apariencia`, van como
  claves globales de `ENV:Escritorio/barra` (`radios`, `radio_ventanas`). Si otro agente los pone en la apariencia, que
  lo diga aquí y la barra los lee de allí.
- **Iconos**: la barra dibuja sus iconos con el lienzo (los de antes). Cuando `iconos` deje los Material Symbols en el
  kit, el registro de widgets ya lleva el nombre de Material de cada uno (el de DMS) para cambiarlos.

## Diseño en tres frases

Las barras son el modelo de DMS (`barConfigs`: hasta cuatro, cada una con posición, monitores, grosor, opacidad,
radios... y sus widgets por zonas, cada uno con su `id`, encendido y opciones) llevado a `ENV:Escritorio/barra` en
`clave = valor`, con un importador de `settings.json`. La barra (`usuario/src/bin/barra.rs`) pone una capa por barra y
monitor y dibuja los widgets con las medidas, radios, colores por papel y animaciones de DMS (`barra_widgets.rs`); el
centro le da el fichero (ella no ve `ENV:`) por una memoria suya y su timbre, así que cualquier cambio se ve al
momento. Ajustes > Barra es la página "Widgets" de DMS (zonas con filas que se arrastran, interruptor, opciones,
catálogo) sobre ese mismo fichero, con Guardar, Usar y Cancelar.

## Lo hecho

### El modelo (`usuario/src/barra_config.rs`, con `json_mini.rs`)

- Fichero `ENV:Escritorio/barra` (y `ENVARC:`), formato de `idioma::texto`. Claves globales (`barras`, `radios`,
  `radio_ventanas`, `estilo`, `formato_fecha`) y por barra con su nombre delante (`principal-posicion`...). Lo que falta
  vale lo de serie, así que una línea basta. Cada zona: `lanzador, espacios(numeros cuantos=5), -reloj`; `-`, apagado;
  entre paréntesis, opciones.
- Registro de widgets (`REGISTRO`), del catálogo de DMS: 31 (los de DMS menos `layout`, de MangoWC) más `menus` (los
  menús del programa activo, de DashOS). Cada uno con su nombre de DMS, su icono de Material, su zona y sus opciones
  (con su nombre en DMS y sus valores). Los que no tienen dato aún (`falta`: tiempo, música, portapapeles, temperatura,
  bandeja, batería, VPN, mayúsculas, actualizaciones, apagado...) se guardan y se ven en Ajustes, apagados, con el porqué;
  en la barra no salen.
- Las fórmulas de DMS (`Theme.qml`, `Shape.js`, `BarMetrics.qml`): grosor de la barra y de los widgets según el relleno,
  zona exclusiva, radios escalados por `radiusStrength`, radio de la píldora (y el de pulsada), letra e icono según el
  grosor.
- Lo que no vale se dice y no rompe: widgets o opciones desconocidos se guardan sin usarse; `menus` solo una vez;
  zonas de más de 24 widgets o que no caben en una línea, recortadas; barras repetidas, de más o con nombre malo.
- Importador `importar_dms`: `barConfigs` (posición, también las centradas de DMS, `screenPreferences`, espaciado,
  relleno, opacidad con "seguir el estilo", bordes, contornos, escalas, autoocultar, sombra), widgets como texto u
  objeto, y lo global que toca a la barra (`radiusStrength`, `niriLayoutRadiusOverride`, `interfaceStyle`,
  `clockDateFormat`, `barElevationEnabled`, `popupTransparency`). Dice en llano todo lo que deja fuera.
- `formato_fecha`: los códigos de Qt de `clockDateFormat` (d, dd, ddd, dddd, M... yyyy), con los nombres en castellano.

### La barra

- Una capa por barra y monitor (`wl_output` de cada uno, y los que llegan o se van después). Arriba, abajo, a la
  izquierda o a la derecha; las de lado no pisan las de arriba y abajo del mismo monitor (márgenes). Lo que cambia de
  forma se rehace; lo demás solo se repinta: todo en caliente.
- Los widgets (`usuario/src/barra_widgets.rs`): lanzador, espacios (estilos píldoras, puntos, líneas y tarjetas, con
  números, solo ocupados, borde del que se ve), ventana con el teclado, menús del programa, reloj (con el formato de
  fecha; de lado, apilado), CPU, memoria (en % o GB), disco (INFO de archivos), centro de control (red, sonido, % del
  volumen), avisos, espaciador y separador. Colores por papel: la tira en la superficie, las píldoras en el contenedor
  (`cardSurface`), lo de encima mezclado con el primario al 10 %, los espacios con el texto al 30 % y al 6 %.
- Animaciones de DMS, con su curva `expressiveFastSpatial` (0.42, 1.67, 0.21, 0.9; 350 ms): la píldora pulsada se
  deforma hacia el radio pequeño y vuelve con rebote; el indicador del espacio que se ve se alarga y el de antes se
  encoge.
- Los desplegables (avisos, procesos) salen junto a la barra de la que se pulsan y en su monitor.
- La configuración llega del centro: la barra crea una memoria, se la da al centro con una copia de la puerta de su
  timbre ("barra suscribir", solo a la barra por su nombre de `inicio`) y lee el texto con un cerrojo de vez (impar
  mientras se escribe).

### Ajustes > Barra (`usuario/src/barra_editor.rs`, en el centro)

- La lista de barras arriba (la elegida se edita) y debajo, como en DMS: chips de la barra (posición, monitores,
  activa, grosor, opacidad, esquinas, radios, nueva, quitar) y las tres zonas con sus filas: asa para arrastrar
  (ordenar y pasar de zona, con una raya de dónde cae), nombre y explicación (o por qué no tiene dato), interruptor,
  engranaje (opciones: sí o no, uno de varios, números con − y +) y quitar. "+ Añadir widget" enseña el catálogo.
- Cada cambio va a `ENV:` y a la barra al momento; Guardar lo pasa a `ENVARC:`; Usar lo deja; Cancelar vuelve a como
  estaba al abrir; cerrar con cambios guarda (como la Apariencia).

### Importar y el perfil de shiva

- `Ajuste DMS <settings.json> [GUARDAR]` (la plantilla de `Ajuste` gana `DMS/K`): importa a `ENV:Escritorio/barra` (y a
  `ENVARC:`) sobre lo que hubiera, y dice lo que quedó fuera.
- `SYS:Prefs/Presets/DMS/shiva.json`: los `barConfigs` del usuario en shiva (sin nada que no sea de la barra);
  `usuario/pruebas/perfil-barra-shiva.txt` es lo que da importarlo (la prueba lo exige).

### Autoocultar

- `autoHide` de DMS: sin zona exclusiva; oculta, la capa es una tira transparente de 2 en su borde. Al llegar el ratón
  se enseña; al irse, se oculta tras su `retardo` (250 ms de serie, `autoHideDelay`).

## Decisiones (pendientes del usuario)

1. **Lo de serie es el de DMS** (lanzador, espacios, ventana | música, reloj, tiempo | bandeja, portapapeles, CPU,
   memoria, avisos, batería, centro de control), con los menús del programa tras la ventana. Los que no tienen dato no
   se ven: hoy se ve lo mismo que antes, salvo que CPU y memoria van en dos píldoras (como en DMS) y no en una.
2. **Relleno 0 de serie** (DMS: 2): la barra mide 36 + 4 de hueco, los 40 de siempre; con el 2 serían 42 y se moverían
   las ventanas (y las pruebas). Con el perfil de shiva (relleno 4) mide 40 + 4.
3. **Opacidad 88 % de serie** (DMS: 100 %): la que se decidió en la oleada 1 (decisión 3 de S27).
4. **El perfil de shiva no es lo de serie**: va en `SYS:Prefs/Presets/DMS/shiva.json` y se pone con
   `Ajuste DMS SYS:Prefs/Presets/DMS/shiva.json GUARDAR`. Si el usuario lo quiere de serie, basta copiar
   `usuario/pruebas/perfil-barra-shiva.txt` a `SYS:Prefs/Env-Archive/Escritorio/barra` en el disco.
5. **Los radios** (`radios`, `radio_ventanas`) van en el fichero de la barra mientras no haya sitio común en la
   apariencia (ver "Coordinación").
6. **Clic al pulsar**, no al soltar (DMS: al soltar): así no cambian los tiempos de las pruebas; la animación de la
   píldora sí sigue al botón.
7. **El logo "sistema" del lanzador** (`launcherLogoMode: os`, el de shiva) se dibuja como el de aplicaciones: DashOS
   aún no tiene logo propio en el kit.
8. **Ajustes > Barra** va en la ventana de Ajustes de siempre (560 × 420): el editor cabe justo y se desplaza con la
   rueda. DMS abre los Ajustes más grandes; recomiendo agrandar la ventana de Ajustes o abrirla maximizada en esta
   categoría.

### El esquema monocromo (la paleta de shiva)

- `kit::tema::MONOCROMO` y `monocromo()`: el `scheme-monochrome` de matugen, los grises de `dms-colors.json` tal cual
  (sin croma no depende del fondo). Prueba: sin tono, lo mismo por `del_sistema` y por `esquemas`, y que se lee (4,5:1).
- El centro lo ofrece tras los cuatro acentos; `Ajuste acento monocromo` lo pone. `Ajuste DMS` pone también el color de
  DMS: dinámico con `scheme-monochrome`, el monocromo; dinámico con otro esquema, la paleta del fondo; un tema de DMS
  con nombre, nada (y lo dice). El `shiva.json` de prueba lleva `currentThemeName` y `matugenScheme`.

## Pruebas

- PC (`make prueba`): `json_mini.rs` (2, con hostiles: hondura, tamaño, muchos valores, 20 000 entradas al azar) y
  `barra_config.rs` (14: lo de serie y sus medidas, a mano, hostiles, monitores, el importador de shiva y hostil, los
  pedidos del editor, fechas). `cd kit && cargo test` (72, con el monocromo). `cd shell && cargo test` (71).
- `make prueba`: BIEN.
- `make arranque-virt`: dos vueltas, y las dos solo con la familia del fallo conocido de `revision` (teclas y clics del
  guion perdidos): `FALTA: [dash-ventana] pulsado: Pulsado 1 veces`. Lo vi con una captura: la ventana de ReAction de
  la caja Amiga sale encima de la lista de Permisos justo cuando el guion pulsa, el clic se lo lleva ella y el de Quitar
  deja el foco en el botón, así que los Supr de después no quitan nada y el guion se queda en ese paso. Mis líneas
  nuevas que salen antes de ese punto salen bien (suscripción, barras en pantalla, el hostil). Las del guion del Shell
  (`Ajuste` de la barra, `Ajuste DMS` con ficheros que no son JSON) van después de ese punto: en estas vueltas no se
  llegó. Las mismas cosas se prueban en `arranque-virt-shell`.
- `make arranque-virt-shell`: BIEN, con lo nuevo: un `Ajuste` de la barra se ve en marcha (ENV:, el centro, la barra) y
  `Ajuste DMS SYS:Prefs/Presets/DMS/shiva.json` cambia la barra (y se rehace, por el grosor).
- `make arranque-virt-2`: las líneas BIEN (una barra en cada monitor). Con la barra en el segundo monitor, Ajustes
  maximizada allí mide 1024x728 (no 768) y la comprobación de las capturas mira el fondo bajo esa barra: cambiados
  `arranque_virt_2.txt`, `guion_multimonitor.py` y `captura_multimonitor.py` (comprobada con las capturas de la vuelta).
- `make arranque-virt-escritorio` y `make arranque-virt-menus`: BIEN (los menús del programa en la barra, con la barra
  nueva).

## Medido

- La barra: 450 912 → 557 408 bytes (+106 KB: los widgets, el modelo y el lector de JSON, que también va en la barra).
  El centro: 614 752 → 709 040 (+94 KB: el editor y el modelo). Las órdenes juntas (`SYS:C/Ordenes`) llevan el
  importador: 307 624 bytes ahora (antes, sin medir). El disco de pruebas pasa la prueba con todo (A15).

## Capturas (fuera de git, `~/recuperar/geosian-7d/dms-barra-capturas/`)

- `antes/`: revision a 1920 × 1080 (`barra-recorte.png`, la barra de antes).
- `despues/`: `800-barra-de-serie.png` (la de serie a 800 × 600), `800-shell-shiva-importada.png` (tras `Ajuste DMS`),
  `shiva-barra-recorte.png` y `centro*.png` (1920 × 1080 con el perfil de shiva, su fondo y modo oscuro con la paleta
  del fondo), `ajustes-barra-violeta.png` (Ajustes > Barra), y con la paleta monocroma de shiva:
  `shiva-mono-escritorio.png`, `shiva-mono-barra-recorte.png`, `ajustes-barra-mono.png` y `mono.png`. El fondo es el
  `Fuji-Dark.png` del usuario reducido a 1920 × 1080 (`fondo/fuji.png`, nunca en git).
- En `arranque-virt-shell`, `Ajuste DMS` escribe `acento = monocromo`, pero el tema no cambia: la apariencia de Ana de
  la prueba trae a propósito `modo = morado` y el centro no la aplica entera (ya era así).

## Pendiente

- La sombra de la barra: la pone el compositor bajo toda capa "barra"; shiva no tiene elevación (`barElevationEnabled`
  false) y el modelo lo guarda (`sombra = 0`), pero quitarla es del compositor (no lo toco: zona de `x86-gpu`).
- Los iconos: los dibujados de antes; cuando `iconos` deje Material Symbols en el kit, el registro ya trae el nombre de
  cada uno.
- Los widgets sin dato (música, tiempo, bandeja, portapapeles, batería, temperatura...) hasta que haya servicios.
- El editor no se maneja con el teclado (solo el ratón y la rueda); la ventana de Ajustes (560 × 420) es pequeña para
  esta página y para la barra lateral, que con nueve categorías ya no cabe entera sin maximizar.
- Los espacios son de todo el escritorio, no por monitor (S27 sección 3): todas las barras enseñan los mismos.
- Los menús del programa solo van en la primera barra de arriba del primer monitor.
- El widget de espacios de DMS con iconos de los programas (`showWorkspaceApps`) y nombres: DashOS no tiene esos datos.
