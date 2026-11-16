# S13. Red

> **2026-11-15, segunda parte (rama `red-dns`, `docs/bitacoras/red.md`).** UDP, el resolvedor de nombres dentro del
> servicio (con `hosts` y caché), escuchar con el permiso `red-escuchar` aparte y el cortafuegos de entrada cerrado de
> serie, IPv6 por SLAAC, y la hora por la red (`hora`, con el permiso del reloj) con zona horaria en la barra. TLS no:
> el estudio y la propuesta, en [S13b](S13b-tls-propuesta.md). Lo que se configura va, por ahora, en `Prefs/Red` del
> disco del sistema, que el servicio lee al arrancar.

> **2026-11-12, primera versión (rama `red`, `docs/bitacoras/red.md`).** smoltcp como pila, provisional (la pregunta
> abierta 1). Su CUBIC no compila sin `std` (usa `f64::powi` y `mul_add`): va con Reno hasta que se le dé una raíz y
> una potencia propias. La memoria de cada conexión la pone el servicio, no el programa: una memoria del programa
> podría ser revocable y quitársela al servicio con él dentro.

> **2026-11-10.** Los controladores de Linux con LKL, y el Wi-Fi y las mejoras de red, en [S12b](S12b-controladores-de-linux.md).
>
> **2026-11-16.** LKL se retira (decidido por el usuario, S36): el controlador de Wi-Fi de Linux va en un dominio de
> controladores (S35). Las mejoras de red de S12b siguen.

> **Nota del 2026-11-03 por la tarde.** El usuario decidió que lo nativo sea little-endian y solo la caja Amiga
> big-endian. Donde este documento dice big-endian para lo nativo, manda `PLAN.md`.

## El problema

DashOS necesita una pila TCP/IP, cortafuegos, resolución de nombres, TLS y Wi-Fi. Y tres clases de programas tienen
que usar la red:

- Los nativos, con la API de DashOS o con sockets POSIX de relibc (S7, S11).
- Los 68k de la caja, con `bsdsocket.library`, la interfaz de AmiTCP y Roadshow.
- Los de la caja Linux, que traen su propia pila dentro de la máquina virtual (S7).

Lo que ya está decidido o recomendado:

- La red es un permiso que cada aplicación pide (S6).
- La red es un proceso, como todo servicio en el micronúcleo (S1). Los controladores de red (GENET, Wi-Fi, USB en
  QEMU) son otros procesos (S12).
- Cortafuegos que cierra las entradas por defecto (S6).
- La Pi 4 no tiene AES por hardware (S5).

## Lo que se comprobó

- **smoltcp**, la pila TCP/IP en Rust más usada, va por la versión 0.14.0 (agosto de 2026). Su README dice que
  negocia el escalado de ventana y que tiene control de congestión opcional, con CUBIC y Reno. No tiene acuses
  selectivos (SACK) ni marcas de tiempo de TCP. Comprobado en el README de su repositorio con la API de GitHub.
- **La red habla big-endian.** Las cabeceras de IP, TCP y UDP van en el orden de bytes de la red, que es big-endian.
  En DashOS no hay que dar la vuelta a nada al leerlas. En una CPU little-endian, sí.
- **TLS en una CPU sin AES.** Go y Firefox ponen ChaCha20-Poly1305 por delante de AES-GCM cuando la CPU no tiene AES
  por hardware. rustls pone ChaCha20 primero por defecto, aunque hay quien pide cambiarlo para CPUs con AES. En la
  Pi 4, ChaCha20 primero es lo correcto. En la Pi 5, AES.

## Las opciones para la pila

### A. smoltcp

- Gana: Rust, pequeña, sin `unsafe` en su mayor parte, licencia permisiva, pensada para sistemas sin Linux. Tiene lo
  esencial y control de congestión.
- Cuesta: le falta SACK y las marcas de tiempo, que ayudan en redes con pérdidas y a alta velocidad. Su rendimiento
  real en una red de verdad está por medir.

### B. Portar lwIP

- Gana: muy probada en sistemas pequeños, licencia BSD, completa.
- Cuesta: está en C y pensada para microcontroladores. Encaja peor con Rust y con un servicio de red con varios
  hilos.

### C. Portar la pila de FreeBSD

- Gana: la de más calidad de las que se pueden portar con licencia BSD.
- Cuesta: está muy atada al kernel de FreeBSD. Es un trabajo enorme.

### D. Escribir una propia

- Gana: hecha a medida.
- Cuesta: TCP tiene treinta años de casos raros. No compensa.

## Recomendación

**A: smoltcp como pila del servicio de red**, con su control de congestión CUBIC activado. Si las medidas en la Pi 4
lo piden, se le añade SACK y las marcas de tiempo, que es mejor que cambiar de pila.

### El servicio de red

- Un proceso con la pila, que habla con los controladores de red por anillos en memoria compartida (S1).
- Cada aplicación con permiso de red recibe una capacidad para abrir conexiones. Sin ella, no hay red (S6).
- Los sockets de POSIX y la API nativa son dos caras del mismo canal.
- IPv4 e IPv6, DHCP, y descubrimiento local con mDNS (para ver impresoras y otros equipos).

### Cortafuegos

- Cerrado por defecto para lo que entra. Abierto para lo que sale, de las aplicaciones con permiso.
- Una aplicación que quiere escuchar en un puerto lo pide como permiso aparte (`red-escuchar`, S6b). La pregunta va
  en llano: "X quiere que otros equipos se conecten a este ordenador". El puerto, en el detalle.
- Se configura en Ajustes, categoría Red (S27; es el editor `SYS:Prefs/Red`). Ver "Ajustes, órdenes y registro".

### Ajustes, órdenes y registro (repaso S36, 2026-11-15)

- **Los ajustes de red son ficheros de texto** en los ajustes de la máquina (`Estado:Prefs`, la capa de la máquina
  de `ENVARC:`, S4b): reglas del cortafuegos, DNS y DNS cifrado, IP fija. Los leen todos. Cambiarlos es acción de
  administrador (S6b), desde Ajustes o con la orden `Ajuste`: los dos escriben el mismo fichero, y el servicio de red
  lo relee. El nombre y el formato, por diseñar al hacerlo.
- **Órdenes en `C:`**, además de las equivalentes a `ifconfig` y `ping` de abajo: una que liste las conexiones
  abiertas por programa, y la del Wi-Fi. Los nombres, por diseñar al hacerlo.
- **Rastro**: lo que corta el cortafuegos va a `Estado:Registros/` (S6), una línea con programa, destino, motivo y
  hora, y lo repetido se resume. Se lee con la orden `Registro` y en Ajustes. Aviso en pantalla, en llano, solo si el
  usuario lo nota (un programa que no conecta) o si se repite.

### Nombres

- Un servicio de nombres con caché, que todos los programas usan.
- DNS cifrado (sobre TLS o HTTPS) como opción en Ajustes, categoría Red: una línea del fichero de ajustes de red.

### TLS

- **rustls**, con ChaCha20-Poly1305 por delante en la Pi 4, y AES-GCM por delante en la Pi 5.
- Los certificados raíz vienen en `SYS:` y se actualizan con el sistema (S8).
- Para programas de C portados que esperan OpenSSL, una capa de compatibilidad sobre rustls, o un port de una
  biblioteca C. Lo decide el primer programa que lo necesite.

### Wi-Fi

- Un servicio que gestiona redes, claves y conexión, como `wpa_supplicant` o `iwd` en Linux. Habla con el
  controlador del CYW43455 (S12).
- Las claves de las redes se guardan cifradas en el cajón del administrador.
- La lista de redes conocidas es texto que se lee (nombre, si se conecta sola); solo la clave va al almacén cifrado.
- Una orden de Shell para ver las redes, conectar y olvidar una, que hace lo mismo que la ventana.

### La caja Amiga

- **`bsdsocket.library` nativa** dentro de la caja, con la interfaz de AmiTCP y Roadshow. Por debajo, habla con el
  servicio de red. Los programas 68k como Tachyon o Inferencia funcionan sin pila TCP/IP del Amiga.
- **AmiSSL**: los programas 68k que la usan siguen usándola, en el JIT. Es OpenSSL compilado para 68k: funciona, pero
  va lento. Más adelante, una `amissl.library` nativa sobre rustls haría el TLS mucho más rápido. Es una interfaz
  grande, así que va después.
- **Herramientas de red del Amiga** (`ifconfig`, `ping` de Roadshow): se ofrecen órdenes equivalentes en `C:`.

### La caja Linux

- Lleva su propia pila TCP/IP dentro, con la red de virtio. DashOS le da una red con traducción de direcciones, como
  hacen las máquinas virtuales de escritorio.
- Sus conexiones cuentan como de una sola aplicación para el permiso de red (S6).
- Para que no sea una caja negra: en el registro queda al menos el destino de cada conexión de la caja; y un
  servidor de dentro se alcanza publicando un puerto (S14, S7), con orden o con un botón en Ajustes. El cortafuegos
  propio de la caja, sus reglas y lo que corta, en S29.

## Lo que queda abierto

1. ¿smoltcp como pila de red, mejorándola si hace falta? Recomendado: sí.
2. ¿DNS cifrado activado de serie, o como opción? Recomendado: como opción, para no depender de un proveedor.
3. ¿Una `amissl.library` nativa sobre rustls para acelerar los programas 68k? Recomendado: sí, pero después de la
   primera versión.

## Cómo encaja

- Con S1: la red es un servicio con anillos hacia los controladores.
- Con S5: sin AES por hardware, ChaCha20 primero.
- Con S6: permiso de red, permiso para escuchar, cortafuegos cerrado, y el registro en `Estado:Registros/`.
- Con S4b y S27: los ajustes de red, texto en la capa de la máquina; la ventana es Ajustes.
- Con S4: `bsdsocket.library` nativa en la caja.
- Con S7: la caja Linux con su pila y red virtio.
- Con S11: sockets POSIX y API nativa sobre el mismo canal.
- Con S12: controladores GENET, Wi-Fi y, en QEMU, red USB.
- S14: el servicio de red arranca después de sus controladores.

## Fuentes

- smoltcp: README de la versión en `smoltcp-rs/smoltcp` (consultado con la API de GitHub, última versión v0.14.0 del
  2026-08-17), y [README de versiones anteriores](https://code.dragonos.org.cn/xref/smoltcp-0.9.1/README.md?fi=smoltcp).
- Orden de cifrados sin AES: [Go](https://go.googlesource.com/go/+/9f39a43e0d728721d5a9e2586ce47a57585591c5%5E%21/),
  [Firefox](https://source-hub.etos-pro.ru/mozilla/gecko-dev/commit/1d51c89cb53987cf86c166f5e5aa4d08390e6c54) y
  [rustls, issue 509](https://github.com/ctz/rustls/issues/509).

Lo que no se comprobó: el rendimiento de smoltcp en una red real, y el coste de ChaCha20 en la A72.
