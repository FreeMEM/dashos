# S8. Paquetes y Hangar

## El problema

El encargo pide paquetes que se actualicen como con pacman, y que Hangar forme parte. Y las tareas anteriores fijan
el terreno:

- **`SYS:` es de solo lectura** y solo lo cambian las actualizaciones firmadas (S4b, S6).
- **Lo instalado va a `Paquetes:`**, un cajón por paquete, y cada paquete declara sus asignaciones en vez de tocar
  `S:User-Startup` (S4b).
- **Cada usuario puede instalar en su `Home:`** sin ser administrador (S4b).
- **Las instantáneas** del sistema de archivos hacen posible que todo sea atómico y reversible (S5).
- **Firmas Ed25519** para paquetes y repositorios (S6).
- **Cada paquete declara los permisos que necesita** (S6).
- **Hay tres mundos de programas**: nativos de DashOS, Amiga 68k y Linux (S4, S7).

## Qué es Hangar hoy

Hangar es una tienda de software para AmigaOS 3.x, en desarrollo y privada (`~/Proyectos/hangar`). Tiene dos
piezas: un cliente para el Amiga, escrito en Dash con MUI, y un servicio en internet, en Rust con PostgreSQL. El
servicio recoge el catálogo de Aminet y otras fuentes, con fichas e imágenes. El cliente busca, instala con recibo,
actualiza, desinstala y ejecuta, y permite comprar programas de pago. Los programas gratis se bajan de su sitio
original; los de pago, del servicio, solo para quien los compró.

## Lo que aprendieron otros

- **Fedora Silverblue (OSTree y rpm-ostree)** separa el sistema base, que es una imagen versionada y de solo
  lectura, de los paquetes que el usuario añade encima. Las actualizaciones se preparan en segundo plano y entran al
  reiniciar, y se puede volver a la versión anterior sin tocar los datos del usuario. Fedora recomienda Flatpak para
  las aplicaciones gráficas y deja las capas de paquetes para lo del sistema.
- **casync** parte una imagen en trozos por contenido. Al actualizar, solo se bajan los trozos nuevos.
- **pacman** es sencillo y rápido: una base de datos de paquetes por repositorio, dependencias, ganchos tras
  instalar y una orden para ponerlo todo al día (`pacman -Syu`).
- **amipkg**, el gestor de paquetes para AmigaOS 3.x que Hangar estudió, guarda un recibo con cada fichero instalado
  y firma su catálogo con Ed25519.

## Recomendación

### Dos niveles, como Silverblue

1. **El sistema base (`SYS:`) es una imagen**, no una colección de paquetes sueltos. Se construye en el servidor a
   partir de paquetes, se firma entera, y llega como una instantánea nueva de `SYS:` (S5). Se aplica al reiniciar. Si
   el arranque falla, se vuelve a la anterior (S14).
2. **Lo demás son paquetes**, en `Paquetes:` (de todo el sistema) o en `Home:` (de un usuario). Cada instalación,
   actualización o desinstalación es una transacción: se hace en una instantánea nueva de `Paquetes:`, y solo si todo
   sale bien se cambia a ella. Si algo falla, no ha pasado nada.

Para el usuario hay una sola orden, como en pacman, que pone al día las dos cosas.

### El gestor: `Paq`

Nombre provisional. Es una orden en `C:` y un servicio del sistema:

- `Paq Instalar <nombre>`, `Paq Quitar <nombre>`, `Paq Buscar <texto>`, `Paq Actualizar` (todo al día) y
  `Paq Volver` (deshacer la última transacción). Con alias cortos en inglés para quien venga de Linux.
- **Repositorios** con un índice firmado. Claves de confianza: la de DashOS, la de Hangar y las que añada un
  administrador (S6).
- **Dependencias** por nombre y versión, y por lo que un paquete ofrece, por ejemplo `mui.library 20`.
- **Sin choques de ficheros**: cada paquete vive en su cajón, y las asignaciones en capas (S4b) deciden quién gana.
  Si dos paquetes ofrecen la misma biblioteca, el gestor pregunta o sigue la prioridad que diga el repositorio.
- **Actualizaciones por trozos**, como casync, para la imagen de `SYS:` y para los paquetes grandes.
- **Recetas**: cada paquete nativo se construye desde una receta en un repositorio, al estilo de los `PKGBUILD` de
  Arch, en un servidor de construcción.

### Qué lleva un paquete

- Su cajón: `C`, `Libs`, `Devs`, `Fonts`, `Locale`, `Prefs`, iconos y el programa.
- Sus datos: nombre, versión, arquitectura, dependencias, asignaciones que necesita (S4b) y permisos que pide (S6).
- Su firma.

### Los tres mundos

| Clase | De dónde sale | Dónde se instala | Quién lo gestiona |
|---|---|---|---|
| Nativo de DashOS | Repositorio de DashOS y Hangar | `Paquetes:` o `Home:` | `Paq` |
| Amiga 68k | Hangar (Aminet, GitHub, autores, de pago) | `Paquetes:Amiga/<programa>` o donde el usuario elija | `Paq`, con el recibo de Hangar |
| Linux | La distribución de la caja Linux (S7) | Dentro de la caja Linux | El gestor de la distribución, por ejemplo pacman de Arch Linux ARM |

- **Programas Amiga**: muchos traen un guion de `Installer`. Se ejecuta dentro de la caja, con el `Installer` del
  Amiga, y `Paq` apunta lo que escribió, como el recibo de Hangar. Si no hay guion, se copia el cajón entero.
- **Programas de Linux**: los instala la distribución de dentro, con su propio gestor. DashOS solo pone sus iconos en
  el lanzador (S7).

### Hangar en DashOS

**Hangar es la tienda y `Paq` es el motor.** Hangar no instala por su cuenta: le pide a `Paq` que lo haga.

- **El escaparate**: fichas con imágenes, búsqueda, novedades, valoraciones y compras. Lo que Hangar ya hace.
- **Un catálogo, tres mundos**: nativos de DashOS, programas Amiga y, si se quiere, los de la caja Linux.
- **De pago**: el servicio de Hangar entrega el paquete a quien lo compró, firmado con la clave de Hangar.
- **El cliente de Hangar que hoy es para el Amiga ya funcionaría en DashOS el primer día**, dentro de la caja Amiga:
  es un programa 68k con MUI. Cuando el backend ARM de Dash exista, el mismo código se compila en nativo.

Hangar tendría que ganar una arquitectura nueva en su catálogo, la de DashOS, al lado de la del 68k. Es una decisión
del proyecto Hangar, que tiene abierta la pregunta de qué sistemas cubre. Este bucle no toca Hangar.

### Actualizaciones

- `Paq` mira si hay novedades una vez al día, y baja en segundo plano.
- Los paquetes se actualizan al momento. Si el programa está abierto, se avisa para reiniciarlo.
- `SYS:` se actualiza al reiniciar. Se avisa, nunca se fuerza.
- Las actualizaciones de seguridad se marcan como tales en el índice.

## Lo que queda abierto

1. ¿Nombre de la orden: `Paq`, u otro?
2. ¿Sistema base como imagen firmada, y paquetes solo por encima, como Silverblue? Recomendado: sí. La otra opción es
   que todo sean paquetes, como Arch, que es más flexible pero menos seguro de actualizar.
3. ¿Hangar como tienda y catálogo encima de `Paq`, con una arquitectura nueva de DashOS en su catálogo? Recomendado:
   sí. Toca decidirlo en el proyecto Hangar.

## Cómo encaja

- Con S0: principio 3 (todo o nada) y principio 4 (un solo sistema).
- Con S4: los programas Amiga se instalan con su `Installer`, dentro de la caja.
- Con S4b: `Paquetes:`, capas de asignaciones, y nada de `S:User-Startup`.
- Con S5: transacciones e imagen de `SYS:` como instantáneas.
- Con S6: firmas Ed25519 y permisos declarados por paquete.
- Con S7: lo de Linux lo gestiona la distribución de la caja Linux.
- S14: arranque con la instantánea nueva de `SYS:` y vuelta atrás si falla.

## Fuentes

- Silverblue y rpm-ostree: [documentación de rpm-ostree](https://coreos.github.io/rpm-ostree/),
  [repositorio](https://github.com/jlebon/rpm-ostree) y
  [experiencias en Fedora](https://discussion.fedoraproject.org/t/experiences-with-package-layering/1186).
- casync: [paquete de Fedora](https://packages.fedoraproject.org/pkgs/casync/casync) y
  [artículo sobre troceado por contenido](https://ranger.uta.edu/~jiang/publication/Conferences/2020/MSST20-DSync(WenXia).pdf).
- Hangar y amipkg: el plan del proyecto Hangar (privado), solo leído.
- pacman: funcionamiento conocido.

No se comprobó nada en código: es diseño.
