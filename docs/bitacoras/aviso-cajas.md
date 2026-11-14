# Bitácora: el aviso de memoria baja llega a las cajas (aviso-cajas)

Rama `aviso-cajas`, sacada de `revision` (16940f0). Sigue a `cupos.md` (cupos como topes): el kernel ya avisaba de la
memoria baja (llamada 53) y solo `sesiones` lo oía, sin hacer nada más que apuntarlo.

Sala limpia: el globo, escrito desde la especificación de virtio 1.2 (§5.5, el globo tradicional), sin leer QEMU,
crosvm, Firecracker, Linux, KVM ni Xen. Los manejadores de memoria baja de la caja Amiga, desde los autodocs de exec
(`AddMemHandler`, `RemMemHandler`) y los include (`exec/memory.h`, `exec/interrupts.h`), sin leer UAE, WinUAE, Amiberry
ni AROS.

## 2026-11-14: el aviso, repartido; el globo de Linux; los manejadores de la Amiga

### Cómo va

1. El kernel avisa a `sesiones` (llamada 53), como antes.
2. **`sesiones` lo reparte** a las cajas que lo escuchan. Cada caja se apunta al arrancar (orden 27,
   `ESCUCHAR_MEMORIA`) con el lado de escribir de un canal que solo lee ella. Solo se acepta a `caja-amiga` y
   `caja-linux` por el nombre que da `inicio`; a otro, -2, y su canal se cierra sin usar. Así **solo `sesiones` puede
   pedir a una caja que suelte**: nadie más tiene la llave de ese canal. Antes de mandar el aviso, `sesiones` mide lo
   que tiene cobrado el grupo de cada caja; cuando la caja dice lo que soltó (orden 29, `SOLTADO`), lo mide otra vez y
   lo apunta ("suelta memoria con el aviso: bien" si bajó).
3. **Caja Linux: el globo.** El invitado tiene ahora un virtio-balloon (`caja-linux/vmm/src/virtio/globo.rs`, en el
   transporte siguiente a los demás). Con memoria baja, la caja le pide un globo de una cuarta parte de su RAM (hasta la
   mitad, si llegan más avisos). Las páginas que el invitado mete en él se devuelven a DashOS con la **llamada 55
   `maquina_globo`** (nueva). Con memoria normal se le devuelven, sin bajar del 15 % libre; como el kernel solo avisa al
   cambiar de nivel, con el globo lleno la caja mira cada segundo lo libre (llamada 29, contadores 15 y 16, nuevos).
   Se ofrecen MUST_TELL_HOST (el invitado no usa una página que saca del globo hasta que el aparato contesta: se repone
   antes) y DEFLATE_ON_OOM (si el invitado se queda sin memoria, saca páginas del globo él solo, avisando).
4. **Caja Amiga: manejadores y lo crecido.** `AddMemHandler` y `RemMemHandler` (exec, `caja/exec/src/memoria_baja.rs`).
   Con el aviso, en la siguiente llamada a exec del programa en marcha se llama a cada manejador por su prioridad, como
   si un `AllocMem` de toda la memoria no cupiera (A0 su `MemHandlerData`, A1 su `is_Data`); `MEM_TRY_AGAIN` lo repite
   con `MEMHF_RECYCLE` (hasta 4 veces). Después, la memoria del 68k que creció y está libre al final se devuelve a
   DashOS (`caja_encoger_ram` en `caja.c`: quita y cierra los objetos de arriba). Sin programa en marcha, solo eso,
   entre un programa y otro. Lo que tenía la caja como "caché" (bibliotecas de disco sin abrir) ya se echaba al cerrar
   la última: no queda nada más que soltar.

### La llamada 55, `maquina_globo`

- Con la llave de una vCPU: soltar (quita la página de la etapa 2 y de las proyecciones del servicio, vacía las TLB del
  VMID en todos los núcleos, la devuelve al sistema y sus bytes al grupo) o reponer (página nueva a cero y limpia hasta
  la RAM, cobrada al grupo).
- Seguridad: solo memoria que no tiene nadie más. El kernel cuenta las referencias del objeto (`Arc::strong_count`) y
  tienen que ser exactamente las de esa máquina, las proyecciones y las llaves de quien llama, con su tabla de llaves y
  su espacio tomados. Una copia en un mensaje sin leer, una vista, otra máquina o una llave revocable hacen que se
  salte. Una página soltada no entra en una vista nueva, no se proyecta y el kernel no copia por ella (`Memoria::fisica`
  da 0). En el servicio, un mapa de bits de las páginas del globo: ningún aparato las toca (un invitado hostil que
  ponga una cola ahí recibe `FueraDeMemoria`, no tumba el servicio).
- `Memoria` guarda sus páginas como `AtomicU64` (0 = soltada) y cuántas hay soltadas: al soltarse el objeto, su cargo
  no devuelve otra vez lo ya devuelto.
- `unsafe` solo en `kernel/src/base/` (la etapa 2, las TLB y las tablas del proceso).

### La prueba: `presion`

`usuario/src/bin/presion.rs`, solo en las imágenes de prueba, con tope de 1 TB y el canal "presion" (lo lee él; las
cajas escriben). Cuando una caja dice que tiene algo que soltar ("caja-amiga" tras glotona, con `memh.exe` en marcha;
"caja-linux" cuando el invitado acaba su /init), llena la memoria con objetos hasta el 4 % libre (el aviso salta por
debajo del 5 %), la deja en el 7 % (sigue siendo memoria baja), espera 10 s y lo suelta todo.

- `caja/pruebas68k/memh.c`: guarda 2 MB de caché con un manejador (AddMemHandler) y espera el aviso llamando a exec;
  el manejador la suelta.
- `make arranque-virt`: la caja Amiga (la Linux no tiene hipervisor ahí).
- `make arranque-virt-linux`: las dos. El guion de la terminal espera a que el globo vuelva a estar vacío antes de
  `poweroff -f`.
- Hostil (`hostil`): apuntarse sin ser una caja, -2 y el canal cerrado sin usar; decir que soltó, -2.
- `caja-linux` (`prueba_globo`, en `-vm` y `-linux`): suelta y repone (a cero), otra vez no hace nada, direcciones sin
  alinear o fuera, tablas imposibles, una llave que no es una vCPU, la memoria en un mensaje o en una vista (no suelta).
- `cargo test` en `caja-linux/vmm` (el globo: negociación, objetivo y aviso de configuración, meter y sacar, `actual`,
  reinicio; hostiles: largo raro, búfer de escritura, lista enorme, fuera de la RAM; y en `a_lo_loco`) y en `caja/exec`
  (`crecer.rs`: lo crecido libre se devuelve y lo usado no; los manejadores en su orden, con su D0 intacto, TRY_AGAIN
  con RECYCLE y tope).

### Kernel del invitado

`Image-caja` necesita `CONFIG_VIRTIO_BALLOON` (y `CONFIG_MEMORY_BALLOON`): añadidos a `FRAGMENTO_CAJA` de
`tools/caja_linux_invitado.py`. `preparar` la vuelve a compilar sola (cambia el fragmento). Para no pisar la de las
otras sesiones, la de esta rama se compiló aparte (`~/.cache/dashos-caja-linux/Image-caja-globo`) y se pasó con
`LINUX=`.

### Lo medido

En `make arranque-virt-linux` (QEMU virt de 3 GB, invitado de 256 MB):

- Caja Linux: globo de 64 MB; el kernel recuperó 64 MB; `sesiones` ve su grupo pasar de 267.366 KB a 201.856 KB. Al
  soltar `presion`, el globo vuelve a 0 y se reponen las páginas.
- Caja Amiga: con el manejador de `memh.exe` (suelta 2 MB) y lo crecido por glotona, la memoria del 68k baja de 48 a
  4 MB: devuelve 45.056 KB; su grupo pasa de 72.356 KB a 27.326 KB.

En `make arranque-virt` (2 GB, sin hipervisor): lo de la Amiga, igual (72.386 KB a 27.356 KB).

### Comprobado

- `make prueba` bien; `make arranque-virt-linux` bien (con `LINUX=~/.cache/dashos-caja-linux/Image-caja-globo`);
  `make arranque-virt` bien (ver abajo).
- `cargo test` de `caja-linux/vmm` y de `caja/exec`.

### Fallos de por el camino

- `presion` tomaba por avisos de las cajas los mensajes de `inicio` (sin marca: quién es cada cliente del canal) y
  provocaba el aviso al arrancar, dos veces. Ahora solo cuenta "caja-amiga" o "caja-linux" con marca.
- Lo que escribe un programa 68k sale en Latin-1: los acentos de `memh.c` salían mal y la línea esperada no casaba.
- El kernel solo avisa al cambiar de nivel: con lo que sueltan las cajas, lo libre pasa del 10 % mientras `presion`
  aún llena, y al soltar `presion` ya no llega otro aviso. Por eso la caja Linux mira lo libre cada segundo con el
  globo lleno.

### Pendiente

- Cerrar con aviso al que se pase (S3), si ni así basta.
- Los manejadores de la Amiga solo se llaman con el aviso de DashOS, no cuando un `AllocMem` no cabe (como en un Amiga):
  aquí la memoria crece sola hasta el tope del grupo. Llamarlos también al llegar al tope queda por hacer.
- Un programa 68k que use memoria ya liberada en lo alto de la RAM crecida, tras devolverla, tumba la caja (antes la
  usaba sin que se notara).
- Con el puente de Wayland (vistas de la RAM del invitado), el kernel no suelta las páginas de esos trozos: hay que
  soltar las vistas o usar el globo solo en los trozos sin vistas. Sin probar con Arch ni con wayland.
- Sin estadísticas del invitado (cola de estadísticas): el globo pide siempre una cuarta parte de la RAM, sin saber
  cuánta tiene libre (con DEFLATE_ON_OOM, si le falta, la saca él).
- `Image-caja` de la caché de serie sin rehacer: hay que pasar `tools/caja_linux_invitado.py preparar` (la rehace al ver
  el fragmento nuevo). Sin el globo en el kernel del invitado, la prueba de `-linux` falla en las líneas del globo.
- Sin probar en una Pi.
