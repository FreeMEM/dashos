# S11. Interfaz de programación

## El problema

Hay que decidir cómo se escriben las aplicaciones nuevas de DashOS. `PLAN.md` dejaba abierta la pregunta: ¿usan la
API Amiga dentro de la caja, una API moderna fuera de ella, o las dos? La recomendación provisional era "las dos, la
moderna como principal".

Las tareas anteriores fijan el terreno:

- Dentro de la caja Amiga, la API de AmigaOS 3.1 con estructuras de 32 bits, big-endian y memoria compartida (S4).
  Sirve para programas 68k y para bibliotecas nativas de la caja, pero no da protección entre programas.
- Fuera de la caja, procesos protegidos de 64 bits, con capacidades (S1, S6), objetos de memoria (S3), Wayland para
  las ventanas (S9) y POSIX con relibc para portar programas (S7).
- La organización es la del Amiga: asignaciones, `LIBS:`, catálogos de `LOCALE:`, Prefs, iconos (S4b).
- Dash es el lenguaje de las aplicaciones (PLAN). Hoy tiene 609 builtins de Amiga, 110 de ellos de MUI según
  `docs/BUILTINS.md` de Dash, y un editor visual de ventanas (`.dashui`).

## Lo que hicieron otros

- **AmigaOS 4** reorganizó las bibliotecas en "interfaces": se abre la biblioteca y luego se pide una interfaz con
  `GetInterface`. Una biblioteca puede tener varias (exec tiene la principal, la de MMU y la de depuración). En lugar
  de la base de la biblioteca, cada llamada recibe el puntero a la interfaz. Sirve para versionar, pero añade un paso
  que muchos programadores ven como ceremonia.
- **Fuchsia** no deja que los programas llamen al kernel directamente. Todo pasa por una biblioteca que el kernel
  proyecta en cada proceso (el vDSO), con funciones de C normales. El kernel comprueba que nadie se salta esa
  biblioteca. Así el contrato con el kernel puede cambiar sin romper programas.
- **AROS** reimplementó MUI con el nombre Zune, para tener un kit de interfaz compatible sin depender del de su autor.

## Recomendación

### Una API nativa con el aire del Amiga, sobre la base moderna

**Las dos, como estaba provisionalmente, pero la nativa no es POSIX a secas: es una API "Amiga moderna".** Se queda
con los conceptos buenos del Amiga y los pone sobre capacidades, de 64 bits y con memoria protegida:

| Concepto Amiga | En la API nativa de DashOS |
|---|---|
| Bibliotecas que se abren por nombre y versión (`OpenLibrary("x.library", 40)`) | Igual, buscando en las capas de `LIBS:` (S4b) |
| Puertos de mensajes y señales | Canales del micronúcleo, con capacidades en los mensajes (S1) |
| `dos.library`, cerrojos y asignaciones | Ficheros por el canal del sistema de archivos, rutas del Amiga (S4b) |
| Intuition y MUI | Un kit de interfaz nativo con el modelo de MUI, sobre Wayland (ver abajo) |
| Catálogos de `LOCALE:` | Igual, en UTF-8 (S4b) |
| IFF y DataTypes | Igual, con clases nativas (S4b) |
| Puertos ARexx | Igual, sobre canales, con permiso para hablar con otro programa (S6) |
| Iconos y tooltypes | Igual |

Lo que no se trae: memoria compartida por defecto, `Forbid()` y `Disable()`, y tocar las estructuras del sistema a
mano. Eso solo vive en la caja.

Para portar programas de Linux y Unix está POSIX con relibc (S7). Un programa nativo puede usar las dos cosas a la
vez.

### Al kernel, solo a través de una biblioteca

Como en Fuchsia: los programas no hacen llamadas al kernel a mano. Las hacen a través de una biblioteca que el kernel
proyecta en cada proceso. El contrato entre esa biblioteca y el kernel es privado. Así el kernel puede cambiar sin
que haya que recompilar los programas.

### Sin el paso de "interfaces" de AmigaOS 4

Las bibliotecas nativas se versionan por número, como en el Amiga, y se abren en un solo paso. Si una biblioteca
necesita varias caras, publica varias bibliotecas. Menos ceremonia, la misma idea.

### Formato de ejecutables y bibliotecas

- **ELF de 64 bits, big-endian**, para programas y bibliotecas nativos. Es el formato que generan Rust, GCC y, cuando
  tenga el backend ARM, Dash.
- **Una `.library` nativa es un objeto ELF compartido** con una tabla de funciones y un número de versión. Se abre
  por nombre y versión, como en el Amiga.
- **Las bibliotecas de C portadas** (zlib, FreeType...) son objetos `.so` normales, con su enlazador dinámico de
  relibc.
- **Los hunks del Amiga** solo viven en la caja (S4).

### El kit de interfaz: el modelo de MUI, nativo

**Recomendación: un kit de interfaz nativo que siga el modelo de MUI**: clases, atributos, avisos entre objetos,
grupos que se colocan solos y ventanas que se adaptan a la fuente. Como Zune en AROS, escrito desde cero con la
documentación pública de MUI.

- Dash ya tiene 110 builtins de MUI y el editor `.dashui`. **El mismo `.dashui` sirve en el Amiga y en DashOS.**
- Se dibuja sobre Wayland (S9), con escalado, tema claro y oscuro, y accesibilidad.
- Los programas 68k con MUI en la caja siguen usando MUI 68k, el que tenga instalado el usuario. Más adelante, las
  clases nativas pueden servir también a la caja.

Licencia: MUI es de sus autores. Reimplementar su modelo desde la documentación pública es lo que hizo Zune.

### Dash, el mismo programa en dos sistemas

Esta es la consecuencia más importante. **Un programa en Dash debería compilar para AmigaOS 3.x (hunks 68k) y para
DashOS (ELF nativo) sin cambios**, porque sus builtins son la capa común: ventanas, ficheros, red, MUI, catálogos.
Tachyon, Forja, Inferencia y el cliente de Hangar pasarían a ser programas de los dos sistemas.

Para eso, Dash necesita lo que ya dice `PLAN.md` (backend ARM y modo sin sistema) y, además, que cada builtin tenga
su versión para DashOS. Ese trabajo es de las sesiones de Dash (AGENTS.md, regla 4).

### Otros lenguajes

- **Rust**: una caja `dashos` con la API nativa, para servicios y aplicaciones.
- **C y C++**: cabeceras de la API nativa, y relibc para POSIX.
- **Lo demás**: por la caja Linux (S7).

### SDK

Cabeceras, la caja de Rust, los builtins de Dash, documentación en castellano e inglés, y compilación cruzada desde
Linux (shiva). Más adelante, compilar dentro de DashOS.

## Lo que queda abierto

1. ¿API nativa "Amiga moderna" (conceptos del Amiga sobre capacidades), más POSIX para portar? Recomendado: sí.
   Cierra la pregunta que estaba en `PLAN.md`.
2. ¿Kit de interfaz nativo con el modelo de MUI, para que `.dashui` valga en los dos sistemas? Recomendado: sí.
3. ¿Objetivo de Dash: el mismo programa compila para AmigaOS 3.x y para DashOS? Recomendado: sí. Es trabajo de las
   sesiones de Dash.

## Cómo encaja

- Con S1: canales y capacidades por debajo de puertos y mensajes; llamadas al kernel por una biblioteca.
- Con S3: objetos de memoria para compartir datos grandes entre programas.
- Con S4: la API Amiga de 32 bits se queda en la caja.
- Con S4b: bibliotecas por capas de `LIBS:`, catálogos en UTF-8.
- Con S6: ARexx con permiso; selector de ficheros del sistema.
- Con S7: relibc para POSIX.
- Con S9: el kit dibuja sobre Wayland.
- Con S10: los menús del kit van a la barra de la pantalla.

## Fuentes

- Interfaces de AmigaOS 4: [Exec en el wiki de AmigaOS](https://wiki.amigaos.net/wiki/Programming_AmigaOS_4:_Exec_-_The_Kernel),
  [crear una biblioteca de AmigaOS 4](https://wiki.amigaos.net/wiki/How_to_create_an_AmigaOS_4_library),
  [bibliotecas y dispositivos](https://wiki.amigaos.net/wiki/Libraries_and_Devices) y
  [hilo del foro de Hyperion](https://forum.hyperion-entertainment.com/viewtopic.php?p=20399).
- vDSO de Fuchsia: [fuchsia.dev](https://fuchsia.dev/zircon/docs/vdso.md).
- Builtins de Dash: `~/Projects/FreeMEMLang/docs/BUILTINS.md` (recuento de líneas con `mui_`).
- Zune: conocido; no se consultó en esta vuelta.

No se comprobó nada en código: es diseño.
