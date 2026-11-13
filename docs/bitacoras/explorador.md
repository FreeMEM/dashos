# El explorador de archivos ("Archivos")

Rama `explorador`, desde 130bfcb. Encargo del usuario: "nos hace falta un explorador tipo Nautilus". Ideas de
Nautilus 47 (GNOME Archivos) en lo que hace y en cómo se usa; ni código de Nautilus ni de GTK.

## Lo hecho

Un programa nuevo, `explorador` (`usuario/src/bin/explorador.rs`), servicio del escritorio como el Workbench. El
Workbench le pasa el doble clic en un volumen (o en un cajón de su ventana de reserva) por un canal nuevo,
"explorador": "abrir", la raíz y los nombres. Si el canal falla, el Workbench abre su ventana de cajón de siempre.
El explorador solo atiende ese pedido del Workbench (por el nombre que da `inicio` a la marca).

Las piezas:

| Fichero | Qué |
|---|---|
| `usuario/src/bin/explorador.rs` | El programa: servicio de archivos, ventanas, pestañas, acciones, diálogos, Propiedades |
| `usuario/src/explorador/entrada.rs` | El bucle: compositor, canal del Workbench, ratón, teclado; los trabajos y miniaturas entre medias |
| `usuario/src/explorador/pintar.rs` | El dibujo, a mano con el lienzo del kit (Material 3, los colores del tema del fondo) |
| `usuario/src/explorador/iconos.rs` | Qué icono lleva cada cosa (`icono_de`) y cómo se pinta: el único sitio que cambiará con los iconos nuevos |
| `usuario/src/explorador/logica.rs` | Sin dibujo ni servicio: nombres válidos, orden natural, selección, cursor, camino escrito, nombres de copia, papelera, textos |
| `usuario/src/explorador/ops.rs` | Listar, abrir paso a paso y los trabajos largos (copiar, mover, papelera, restaurar, borrar, buscar), a trozos y cancelables |
| `usuario/src/explorador/pruebas.rs` | 24 pruebas en el PC contra un servicio de archivos falso en memoria |
| `usuario/src/permisos_kit.rs` | La rejilla de permisos de la Información, sacada del Workbench: la usan los dos |

Lo que hace:

1. **Ventana**: barra lateral (Carpeta personal, Papelera, Sistema, Amiga con la pastilla 68K; abajo, lo libre del
   disco), barra de arriba con Atrás, Adelante, el camino con migas que se pulsan (la raíz como icono, "…" si no
   caben), Buscar, Vista, Menú, Maximizar y Cerrar (decoración del cliente, como Nautilus: arrastrar la barra mueve
   la ventana, doble clic la maximiza, los bordes cambian el tamaño). Rejilla (celdas de 116 × 128, iconos de 64,
   nombre en dos líneas) o lista (Nombre, Tamaño, Tipo, Modificado; pulsar la columna ordena, otra vez al revés).
   Las carpetas siempre primero, orden natural ("foto 2" antes que "foto 10"). Ocultos (punto delante o "~" detrás)
   con Ctrl+H.
2. **Selección y teclado**: clic, Ctrl+clic, Mayús+clic, rectángulo (con Ctrl, se suma), Ctrl+A; flechas, Inicio,
   Fin, RePág, AvPág (con Mayús, se extiende; con Ctrl, solo el cursor; Ctrl+espacio elige), Intro abre, Retroceso
   y Alt+Izquierda atrás, Alt+Derecha adelante, Alt+Arriba arriba, F2 cambiar el nombre (marcado sin la
   extensión), Supr a la papelera (Mayús+Supr, para siempre), Ctrl+L el camino, escribir busca, Esc. Ctrl+C, X, V,
   Z (deshacer), Ctrl+Mayús+N carpeta nueva, Ctrl+N ventana, Ctrl+T y Ctrl+W pestañas, Ctrl+Tab, Ctrl+I o
   Alt+Intro Propiedades, Ctrl+1 y Ctrl+2 la vista, F5 volver a leer. Menú del botón derecho sobre algo y sobre el
   fondo.
3. **Operaciones**: carpeta nueva, cambiar el nombre (con Deshacer), copiar, cortar y pegar (una copia sobre sí misma
   se llama "nombre (copia).ext", "(copia 2)"...), papelera con Deshacer, restaurar y vaciar, borrar de verdad con
   pregunta (también donde no hay papelera: Amiga), arrastrar y soltar en su misma ventana o entre sus ventanas
   (sobre una carpeta, un lugar, una miga o el fondo; a otra raíz o con Ctrl, copia; a la Papelera, tira). Lo largo
   va en `ops::Trabajo`: avanza 12 peticiones por vuelta del bucle, con una tarjeta de progreso (bytes o elementos)
   y su botón de cancelar; lo copiado a medias se borra al cancelar o si el disco se llena.
4. **Abrir**: doble clic en un programa de `Sistema/Programas` lo lanza `sesiones` como la persona de delante
   (`explorador` está en `LANZADORES`). Un fichero que no es un programa: un aviso de que no hay programa para él.
   "Abrir con…" enseña los programas y lanza el elegido, pero **sin el fichero** (`sesiones` no tiene cómo pasárselo;
   el aviso lo dice). Propiedades: la Información del Workbench con el kit (comentario, permisos y bits del Amiga,
   dueño), en un diálogo. Pestañas. Búsqueda en la carpeta y en las de dentro mientras se escribe (`ops::Busqueda`,
   a trozos, hasta 500 resultados y 32 de hondo), con el lugar de cada resultado.
5. **Miniaturas** de los PNG de hasta 2 MB y 1,5 Mpx, decodificadas en el propio explorador (S6b, regla 5) con
   `kit::png`, reducidas y guardadas (64 como mucho).

La papelera es `Home/.Papelera` con `archivos` (lo tirado, con un nombre que no se repite: "nota (2).txt") e `info`
(una ficha por cada uno: "dashos-papelera 1", raíz, fecha, nombre y cajones). Se crea al empezar (0700). Restaurar lee
la ficha con todo validado (la raíz tiene que ser de las suyas, cada nombre válido) y vuelve a abrir el sitio paso a
paso desde su raíz: una ficha retocada no saca nada de la raíz. No es la papelera de la caja Amiga
(`Estado/Servicios/caja-amiga.papelera`, que el servicio rellena por su cuenta): esa no se enseña todavía.

## Seguridad

- Raíces cedidas a sí mismo, como el Workbench: 1 Sistema (solo leer), 2 Home (`C`, provisional), 3 Amiga
  (`Estado/Servicios/caja-amiga`). Todo va por asas y un nombre cada vez; los nombres raros ni se enseñan ni se
  abren ni se crean (`logica::nombre_valido`: vacíos, ".", "..", con '/', ':', '|', de control o de más de 200 bytes).
- Copiar es leer y escribir por la ventana del servicio con los permisos de Ana: no se añadió ninguna orden de copiar
  al servicio. Mover es `RENOMBRAR` (que ya no deja meter un cajón en sí mismo).
- **Arreglo en `archivos`**: `un_nombre` aceptaba "." y ".." al crear, guardar, borrar y renombrar. Ahora no.
- Hostiles en el arranque (`[explorador] lo ajeno`): DeCarla no se lista (-12) ni su carta se abre ni se copia; ".."
  desde Home no sube; por Sistema no se crea; ".." no vale como nombre ni en el explorador ni en el servicio (crear y
  renombrar, mandados a mano); Privado no se mueve dentro de sí ni por el explorador ni por asas al servicio.
- Hostiles en el PC (`pruebas.rs`): nombres raros fuera de la lista, abrir con "..", "a/b" o vacío, lo de Carla,
  copiar sobre sí mismo, carpeta dentro de sí misma (copiar y mover), **disco lleno** (para, borra lo copiado a medias
  y no sigue), cancelar a medias, Sistema de solo lectura, mover sin pisar, papelera (tirar, nombres repetidos,
  restaurar con otro nombre si ya hay uno, vaciar con carpetas llenas), fichas retocadas a mala idea (raíz ajena,
  "..", "DeCarla"), y 2000 entradas al azar contra la lógica.
- `#![deny(unsafe_code)]` en el explorador salvo un módulo de dos funciones (`memoria`), que ve como bytes la ventana
  del servicio y los búferes proyectados, como hacen todos los programas de usuario. **No es** "solo en
  `kernel/src/base/`": eso no se puede sin una envoltura segura en la biblioteca `dashos`, que no hay.

## Lo probado

- `make prueba`: BIEN (con las 24 pruebas del explorador en el PC).
- `make arranque-virt`: BIEN (las dos listas de esperados, con el reinicio, el disco leído en Linux, la captura, el
  sonido y U6). El guion nuevo (`tools/prueba_uefi.sh`, pasos `ex`) sustituye al del Workbench: doble clic en el icono
  de Sistema, DeCarla, Atrás, Programas, hola lanzado, Propiedades de hola, Carpeta personal, Privado como lista,
  ordenar por tamaño y por nombre, Propiedades de nota.txt (comentario y Archivado), buscar "nota", carpeta nueva
  "Viaje" y un nombre ".." que no vale, copiar dos ficheros con Ctrl+flechas y Ctrl+espacio, pegarlos, F2, Supr,
  Papelera y Restaurar con el menú, Fondos con miniaturas (Ctrl+L y Ctrl+1), una pestaña, Ctrl+N, Ctrl+L con
  "Home/../DeCarla" (no vale) y "Home/Viaje", mover la segunda ventana por su barra, arrastrar datos.bin a la primera
  (Privado) y cerrar las dos con Ctrl+W. `tools/disco_archivos.py` comprueba lo que queda en el disco.
- Arreglado de paso en el guion: Ajustes esperaba a que el compositor dijera dónde puso su ventana (con un programa
  más al arrancar, esa línea salía tarde y el clic caía en su campo).
- Captura en Full HD: ver abajo.

## Lo que no se probó o no está

- El fantasma de lo que se arrastra no salió en la captura de 800 × 600 (el soltar sí funcionó). Sin mirar a fondo.
- Disco lleno de verdad en QEMU: no (llenar el disco de pruebas puede dejarlo de solo lectura para todos); solo con el
  servicio falso.
- Carreras al renombrar: el servicio lo hace en una transacción; no hay prueba de dos clientes a la vez.
- La caja Linux: su home está en otra partición que lee `caja-linux`, no `archivos`; no se llega por las raíces y no
  sale en la barra lateral.
- "Abrir con" no pasa el fichero al programa (falta en `sesiones` lanzar con una asa cedida).
- Los ficheros que no son programas no se abren con nada (no hay visor ni editor en `Programas`).
- Ventanas de 4 como mucho; 32 MB de cupo (el Workbench baja a 16: ya no abre cajones; en `virt`, con más no cabía
  la caja Amiga).
- Los iconos son los dibujados (el plan de iconos aprobado llegará después, por `iconos.rs`).
- Arrastrar a otros programas (Wayland `start_drag`) no: el compositor aún lo cancela.
