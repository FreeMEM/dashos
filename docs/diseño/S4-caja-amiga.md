# S4. La caja Amiga

> **Nota del 2026-11-03 por la tarde.** El usuario decidió que lo nativo sea little-endian y solo la caja Amiga
> big-endian. Donde este documento dice big-endian para lo nativo, manda `PLAN.md`.

## El problema

La caja Amiga es donde viven los programas 68k. Es un proceso de DashOS (S1) con su propio exec. Por dentro tiene que
parecer un Amiga con AmigaOS 3.1, y por fuera tiene que ser un proceso moderno más: con su memoria protegida, que se
puede matar, y cuyas ventanas son ventanas del escritorio.

Lo que ya está decidido:

- Big-endian y punteros de 32 bits dentro de la caja; los primeros 4 GB de su espacio son el "Amiga" (S3).
- Prioridad estricta de exec dentro de la caja (usuario, S2).
- El exec de la caja apunta el dueño de cada recurso, para poder matar tareas 68k sueltas (usuario, S2).
- Enforcer integrado, siempre activo (usuario, S3).
- La organización del sistema es la de AmigaOS: volúmenes, asignaciones, `LIBS:`, `DEVS:` (usuario; S4b).
- El JIT es el de Emu68, y ya funciona separado (`jit-host/`).

Este documento resuelve seis preguntas: cómo se llama del 68k a lo nativo, cómo funcionan `Forbid()` y `Disable()`,
cuántos núcleos usa la caja, cómo se ven sus ventanas, cómo ve los ficheros, y qué programas quedan fuera.

## Lo que se comprobó

- **Emu68 no tiene ningún mecanismo para llamar a código nativo.** Hay que añadirlo.
- **La línea A del 68k está libre.** Las instrucciones que empiezan por `$A` no existen en el 68k. Emu68 las traduce
  en una sola función, `EMIT_lineA` de `src/M68k_Translator.c`, que hoy solo lanza la excepción. Es el sitio natural
  para engancharse. Es un cambio pequeño en un fichero de Emu68, que sigue siendo MPL.
- **MorphOS hace lo mismo con su caja (ABox).** Sus bibliotecas nativas mantienen la interfaz del 68k: los argumentos
  van en registros 68k virtuales. Cuando código nativo llama a código nativo a través de esa interfaz, no arranca la
  emulación. Solo la arranca si un programa 68k ha parcheado la entrada de la biblioteca.

## 1. Del 68k a lo nativo

### Cómo llama un programa Amiga

Un programa 68k llama a una función de biblioteca con `JSR -desplazamiento(A6)`. A6 tiene la base de la biblioteca.
Justo antes de la base está la tabla de saltos: una entrada de 6 bytes por función. En AmigaOS cada entrada es un
`JMP` a la función.

### Qué cambia en DashOS

En las bibliotecas nativas, cada entrada de la tabla lleva una instrucción de línea A reservada, seguida de un número
de 32 bits que identifica la función nativa. Por ejemplo, `$A0FF` y el número.

El JIT la reconoce al traducir y emite una llamada directa a la función nativa:

- Los registros del 68k ya viven en registros ARM (S1, prueba del JIT). La función nativa los recibe sin copiar nada.
- Los registros x19 a x28 son de "conservar" en la convención de llamada de ARM. La función nativa los respeta sola,
  y ahí viven D0 a D7, A5 y A6.
- A0 a A4 viven en x13 a x17, que la función nativa puede machacar. El JIT los guarda antes y los repone después.
- La función devuelve el resultado en D0, como en el Amiga.

### Llamadas al revés: de lo nativo al 68k

Hay sitios donde el sistema llama a código del programa: ganchos (hooks) de MUI e Intuition, servidores de
interrupción, funciones de comparación. Para eso hay una función `llamar_68k(dirección, registros)` que entra en el
JIT y vuelve cuando el código 68k hace `RTS`. MorphOS la llama `EmulCallDirect68k`.

### Parches con `SetFunction`

Muchos programas Amiga parchean funciones del sistema con `SetFunction`, que cambia la entrada de la tabla. Se
respeta igual que en MorphOS:

- Las llamadas desde 68k pasan por la tabla, así que ven el parche solas.
- Las llamadas entre bibliotecas nativas miran la entrada de la tabla. Si sigue siendo la marca nativa, llaman
  directo. Si alguien la ha cambiado, entran en el JIT y ejecutan el parche.

### El código nativo de la caja

Las bibliotecas nativas de la caja trabajan con estructuras del Amiga: big-endian y con punteros de 32 bits. El
código de 64 bits tiene punteros de 64 bits, así que no puede usar esas estructuras tal cual.

- **En Dash**, el backend ARM de dashc puede compilar con punteros de 32 bits (PLAN, Dash). Es la razón de fondo para
  escribir las bibliotecas en Dash.
- **En C**, GCC ya no ofrece punteros de 32 bits en AArch64. Hay que usar un tipo propio para los punteros de las
  estructuras y convertir al leerlos. Es incómodo, pero sirve para las primeras bibliotecas, mientras llega el
  backend de Dash.

## 2. `Forbid()`, `Disable()` y los núcleos

### La regla del Amiga

En el Amiga solo corre una tarea en cada momento. Muchos programas lo dan por hecho: miran un dato y actúan, sin
cerrojo, porque saben que nadie más corre a la vez. `Forbid()` solo evita que otra tarea les quite el turno.

### Recomendación: una caja, un núcleo a la vez

- **Cada tarea 68k es un hilo del kernel** (lo pide matar tareas, S2). Pero la caja tiene un testigo: solo el hilo
  que lo tiene puede correr código de la caja. Es como el cerrojo global de Python.
- **El planificador de la caja** decide quién tiene el testigo, con la prioridad estricta de exec y turnos entre
  iguales. Le quita el testigo a una tarea cuando acaba su turno, salvo que esté dentro de `Forbid()`.
- **`Forbid()`**: la tarea se queda el testigo hasta `Permit()`.
- **`Disable()`**: además, se aplazan las interrupciones de la caja (las de la sección siguiente).
- **El resto del sistema no se entera.** `Forbid()` no para el escritorio ni a otros procesos: solo a la caja.

Esto corrige lo que dejaba abierto S2: "más adelante la caja podría repartir tareas entre núcleos". **No es seguro**
con programas Amiga normales, por la regla de arriba. Los varios núcleos se aprovechan de otras formas:

- El código nativo de las bibliotecas suelta el testigo cuando hace trabajo largo que no toca la caja, como
  descomprimir una imagen o esperar al disco. Otra tarea 68k puede correr mientras tanto.
- Puede haber varias cajas, cada una en su núcleo.
- El escritorio, el sistema de archivos y lo demás corren en los otros núcleos.

### Interrupciones de la caja

Los programas Amiga instalan servidores de interrupción, por ejemplo uno que se ejecuta en cada barrido vertical
(`INTB_VERTB`). La caja los simula:

- Un temporizador a 50 o 60 Hz, según la preferencia de vídeo, dispara el barrido vertical.
- `Cause()` y las interrupciones por software se ejecutan con prioridad sobre las tareas, como en el Amiga.
- Corren con el testigo de la caja y fuera de cualquier tarea.

## 3. Ventanas: la idea de Newbench

Newbench quería que cada programa Amiga fuera una ventana del escritorio, pero con un emulador entero por programa.
En DashOS sale de forma natural, porque Intuition es nativa:

- **La pantalla de Workbench es el escritorio de DashOS.** Cada ventana que abre un programa en ella es una ventana
  del compositor, con su borde, su sitio en el mosaico o flotante, y su escritorio virtual (S10).
- **Los adornos de Intuition** (barra de título, gadgets de cerrar y de profundidad) los puede dibujar el programa
  como en el Amiga o el compositor como en DashOS. Recomiendo el estilo Amiga por dentro y el borde de DashOS por
  fuera.
- **Las pantallas propias** (`OpenScreen`) se ven como una ventana con toda la pantalla dentro, o a pantalla completa
  si el usuario lo pide.
- **Dibujar**: graphics.library nativa dibuja en un objeto de memoria compartido con el compositor (S3). Las
  pantallas de 256 colores o menos usan paleta, y el compositor la traduce al mostrarla.
- **Mapas de bits planares.** Algunos programas escriben directamente en los planos de bits (`BitMap->Planes`). Para
  ellos, la caja ofrece mapas planares como el chipset AGA, y los convierte a píxeles normales en cada cuadro. Los
  programas modernos usan mapas de píxeles normales, como con Picasso96 o CyberGraphX.

## 4. Ficheros

- **dos.library de la caja habla con el sistema de archivos de DashOS** por su canal (S1). No hay discos Amiga
  falsos.
- **Los volúmenes y asignaciones son los de DashOS**, que ya sigue la organización de AmigaOS (S4b). Un programa 68k
  que abre `LIBS:mui.library` o `ENV:Language` encuentra lo que espera.
- **Bibliotecas 68k en `LIBS:`**: si la biblioteca tiene versión nativa, se usa la nativa. Si no, se carga la 68k y
  corre en el JIT. Así funcionan MUI 3.8 y otras bibliotecas de terceros desde el primer día.
- **Los permisos de Linux** (usuario) se aplican a la caja entera: la caja corre como el usuario que la lanzó y ve lo
  que ese usuario puede ver.

## 5. Varias cajas

- **Una caja principal**, la del Workbench, que arranca con la sesión del usuario.
- **Cajas aparte** para programas dudosos o que fallan mucho. Si una cae, no se lleva a las demás.
- **Entre cajas** se comparten los ficheros, el portapapeles y los puertos ARexx con nombre, a través de un servicio
  que hace de puente.
- Lanzar una caja es crear un proceso y cargar las bibliotecas nativas, que ya están en memoria. La meta de S0
  (primer programa 68k en menos de 1 s) parece alcanzable, pero no está medida.

## 6. Bibliotecas que aún no existan: los módulos del Kickstart del usuario

Reescribir todas las bibliotecas de AmigaOS 3.1 lleva tiempo. Mientras tanto hay un atajo:

- Si el usuario tiene una ROM de Kickstart propia, la caja puede cargar de ella las bibliotecas que DashOS aún no
  tenga en nativo, y ejecutarlas en el JIT sobre el exec de DashOS. Por ejemplo utility, mathieee o icon.
- Solo sirven las que no tocan el chipset. graphics, layers e intuition de la ROM no valen, porque hablan con el
  hardware del Amiga.
- DashOS no lleva ninguna ROM. La pone el usuario, como en Amiberry o en WinUAE.
- Es el mismo fichero que el oráculo de las pruebas (`AGENTS.md`).

## 7. Lo que queda fuera

- **Programas que tocan el chipset**: juegos y demos que escriben en el Copper, el Blitter, Paula o las CIA, y los
  cargadores de pista. Para ellos, la solución es un emulador completo, Amiberry o FS-UAE, empaquetado como una
  aplicación más, con sus ventanas en el escritorio como quería Newbench.
- **Programas que tocan la MMU o el modo supervisor del 68k**: Enforcer original, VMM y similares. DashOS ya trae su
  Enforcer.
- **Programas que cuentan ciclos de CPU** para medir el tiempo. Con el JIT van demasiado rápido.

## Recomendación, resumida

1. Las llamadas a bibliotecas nativas van por una instrucción de línea A en la tabla de saltos, que el JIT convierte
   en una llamada directa. Se respeta `SetFunction` como en MorphOS.
2. Una caja usa un núcleo a la vez, con un testigo. `Forbid()` y `Disable()` solo afectan a la caja.
3. Las ventanas Amiga son ventanas del escritorio, y las pantallas propias son ventanas o pantalla completa.
4. dos.library usa el sistema de archivos de DashOS, con la organización de AmigaOS.
5. Varias cajas, con un puente para ARexx y el portapapeles.
6. Mientras falten bibliotecas nativas, se cargan las del Kickstart del usuario, si las tiene.
7. Lo que toca el chipset va a un emulador completo empaquetado.

## Lo que queda abierto

1. ¿Adornos de ventana al estilo Amiga por dentro y borde de DashOS por fuera? ¿O todo al estilo Amiga?
2. ¿Cargar bibliotecas de la ROM de Kickstart del usuario mientras falten las nativas? Recomendado: sí, como atajo.
3. ¿Empaquetar Amiberry como aplicación para los programas que tocan el chipset? Recomendado: sí. Se puede apoyar en
   lo que ya hay en AmiCachy y Newbench.

## Cómo encaja

- Con S1: la caja es un proceso del micronúcleo.
- Con S2: corrige la idea de repartir una caja entre núcleos; la caja usa un testigo. Las tareas son hilos, y se
  pueden matar.
- Con S3: usa el hueco de 4 GB, la página 0 de solo lectura y el Enforcer.
- S4b define los volúmenes y asignaciones que ve la caja.
- S9 y S10 reciben las ventanas Amiga como ventanas normales.
- S11 decide la API nativa fuera de la caja.
- El backend ARM de Dash con punteros de 32 bits es lo que hace cómodo escribir las bibliotecas de la caja.

## Fuentes

- Interfaz de emulación de MorphOS: [emulinterface.h del SDK](https://morphos-team.net/sdk/includes/emul/emulinterface.html).
- Bibliotecas nativas con interfaz 68k en MorphOS: [ejemplos de Ilkka Lehtoranta](https://morphos.aminet.net/dev/misc/library_examples.readme)
  y [un hilo sobre AmiSSL en MorphOS](https://forums.hollywood-mal.com/viewtopic.php?p=12533).
- Estructura `EmulHandle`: [documentación de Free Pascal](https://docs.freepascal.org/daily/packages/morphunits/exec/temulhandle.html).
- Emu68: `src/M68k_Translator.c` (`EMIT_lineA`), en el clon de `third_party/Emu68`.

Lo que no se comprobó: la llamada nativa por línea A no está hecha todavía; es el paso pendiente de la fase 1 del
plan. Tampoco el tiempo de arranque de una caja.
