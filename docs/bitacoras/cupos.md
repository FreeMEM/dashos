# Bitácora: cupos-topes (los cupos de los grupos, de reserva a tope)

Rama `cupos-topes`, sacada de `revision` en b8805be. Encargo aprobado por el usuario ("lo de cgroups es perfecto"):
el presupuesto de un grupo deja de ser una reserva y pasa a ser un tope, con la idea de `memory.max` de los cgroups de
Linux (solo la idea; no se ha leído código de Linux).

## 2026-11-14

### Lo hecho

- **Cobrar al usar** (`kernel/src/cupo.rs`). `grupo_crear` (18) ya no aparta nada del padre: solo le cobra los 512 bytes
  del grupo en sí. Cada cobro suma a su grupo y a todos los de encima, hasta el raíz, con sumas atómicas sin cerrojos
  (como `cpu_tiempo`); si alguno pasaría de su tope, se deshace lo sumado abajo y la llamada da `E_MEMORIA`. Soltar
  resta en toda la cadena. El tope del raíz sigue siendo la memoria libre para procesos al arrancar: lo usado por todos
  no pasa de ahí, y lo físico que falte da `E_MEMORIA` como siempre.
- **`grupo_uso` (20)**: `x0` el tope, `x1` lo usado con los subgrupos (lo de siempre, que ahora cuenta lo real de los
  hijos y no sus presupuestos) y `x2`, nuevo, lo usado sin ellos. En la biblioteca, `grupo_uso_propio`.
- **Aviso de memoria baja** (`kernel/src/presion.rs`, llamada 53 `memoria_aviso`). Por debajo del 5 % de lo que había
  libre para procesos al arrancar, el kernel manda un mensaje al canal del oyente; por encima del 10 %, otro. Se mira
  al acabar cada llamada (dos lecturas atómicas; el repartidor copia sin cerrojo las páginas libres en
  `memoria::libres_para_procesos`) y lo manda el repartidor de interrupciones. Escucha quien tenga el permiso de apagar:
  `sesiones`, que lo atiende en un hilo y lo apunta en el registro.
- **Topes nuevos** en `inicio`: la caja Amiga, 32 MB más 2048 (el mayor ajuste `memoria amiga`; la caja se para en el
  suyo, 256 MB de serie), sin la cuarta parte de lo libre ni los grupos de prueba para buscarla. `sesiones`, 512 MB
  (lo de sus lanzados cuenta en el suyo); cada programa que lanza, 64 MB (antes 8). La caja Linux sigue con su invitado
  más 66 MB, sin el bucle que partía a mitades (ya no puede fallar el `grupo_crear`).

### Pruebas

- `make prueba`, `prueba_topes` (kernel): un cobro sube por la cadena; 2 MB en un nieto que no caben en el abuelo de 4
  no se cobran y no dejan nada a medias; cien hermanos de 1 TB caben; la memoria llena de verdad (912 MB en raspi4b)
  falla limpio, avisa por el canal (nivel 1) y, soltada, avisa otra vez (nivel 0).
- `grupos` (usuario), pasos 6 y 28 a 35: veinte grupos de media RAM (diez veces la máquina); en uno, un hijo crea y suelta ocho objetos de 4 MB; un nieto
  con tope de 64 MB bajo un grupo de 2 MB se para en 1,6 MB; lo usado del grupo cuenta lo del nieto y lo propio no;
  matar el grupo devuelve todo; un grupo con tope de diez veces la RAM llena la memoria de verdad (912 MB), falla
  limpio y, al acabar, el grupo vuelve a 0 y caben otra vez 16 MB. Para eso el lanzador da a `grupos` 1 TB y el kernel
  al lanzador otro tanto (ya no se aparta nada).

- En `make arranque-virt` (QEMU virt de 2 GB): `sesiones` escucha el aviso; la caja Amiga tiene un tope de 2080 MB;
  `espia` se lanza; `glotona` llega a 46 MB, su tope de prueba de 48 (antes la paraba el cupo en 15).
- Pasan, cada una una vez: `make prueba`, `arranque-virt` (dos vueltas), `-vm`, `-2`, `-escritorio`, `-diag`, `-usb`
  y `arranque-virt-linux` (el invitado con sus 256 MB).
- No probado: Tachyon de verdad en la caja (no está en el disco de pruebas; `glotona` hace de glotón), ni una Pi.

### Lo medido: lo que cuesta cobrar

`prueba_coste_cobro` en `make prueba` (raspi4b en QEMU, lo mejor de tres vueltas en cada arranque; tres arranques):
10.000 objetos de memoria de 4 KB creados y soltados en un grupo a tres niveles bajo el raíz, y 100.000 cobros sueltos.

| | Objetos (µs) | Cobros sueltos (µs) |
|---|---|---|
| Antes (reserva, un grupo) | 14.999, 15.020, 15.585 | 5.435, 5.406, 5.403 |
| Después, con orden `AcqRel` | 16.454 | 15.016 |
| Después, con `Relaxed` | 15.555, 15.598 | 10.955, 10.930 |

Crear y soltar objetos: de 15,0 a 15,6 ms, un 4 % más (el tope era un 10 %). Con `AcqRel` en cada grupo de la cadena
salía un 10 % justo: las cuentas van sin orden de memoria, porque no publican nada más. El cobro solo, a cuatro niveles,
cuesta el doble (55 a 110 ns).

### Pendiente

- Repartir el aviso a las cajas para que suelten: el globo de la caja Linux y las cachés de la caja Amiga. Hoy
  `sesiones` solo lo apunta.
- Cerrar a alguien cuando ni así baste (diseño en S3).
- Que `inicio` lea el ajuste `memoria amiga` y lo ponga de tope del grupo: hoy el tope es el mayor posible y lo
  respeta la caja.

## 2026-11-14, segunda vuelta (decisiones del usuario)

- **Permiso propio del aviso:** llave de arranque del tipo 11 ("memoria", `Objeto::PermisoMemoria`), que `inicio` manda
  solo a `sesiones` (`PERMISO_MEMORIA`, 25), como la de apagar. La llamada 53 ya no acepta el permiso de apagar.
- **`grupo_tope` (54):** cambia el tope de un grupo con su llave y escribir. El propio grupo y los de encima, no
  (`E_DERECHOS`). Elegido: un tope por debajo de lo usado vale; no se quita nada y todo cobro falla hasta que lo usado
  baje.
- **El tope de la caja Amiga lo pone el kernel:** nace con 288 MB (256 de serie más 32); `sesiones` le pone el del ajuste
  (llamada 54, con la llave del grupo que le da `inicio` en el alta) al darla de alta y cuando el centro se lo dice
  (`CAJAS_CAMBIADAS`, 26, con los MB: al arrancar si no es el de serie y al cambiarlo; solo se le acepta al centro).
  `sesiones` no puede leer `Prefs/cajas`: es del usuario (cajón 0700); probado, le salía el de serie. Sin caja en
  marcha, no hay nada que poner: queda para su alta.
- **Pruebas hostiles en `grupos`:** 36, un proceso con la llave de su propio grupo no se sube el tope (`E_DERECHOS`);
  37 a 39, sin escribir no se cambia; bajado a 1 MB con 3 MB usados, no se quita nada y un proceso nuevo da
  `E_MEMORIA`; subido, se mata y vuelve a 0; 40, la llamada 53 con otra llave da `E_TIPO` y sin llave `E_LLAVE`. En el
  kernel, `es_o_cuelga_de` con la cadena.
- **`red-prueba`, 12 rechazos en vez de 14 en fusion-fondo (una vez de cuatro):** la ráfaga se mandaba sin mirar si la
  cola del servicio (16 mensajes) estaba llena; ahora reintenta con -8, como ya hacía con los mal formados. En esta
  rama salen siempre 14. No hay nada de los topes en ese camino (el servicio de red no tiene subgrupos ni se quedó sin
  memoria en ese registro), pero la causa no está comprobada: es lo más probable.
- **Tras b9f8f3e:** `make prueba` bien; `make arranque-virt` bien a la segunda (la primera cayó en "suelta en la ventana 1:
  mueve a Home/Privado" del guion del explorador, un fallo suelto ya apuntado en `caja-amiga.md`); `red-prueba` da 14
  rechazos en las cuatro vueltas de esta rama. `make arranque-virt-wayland` bien en esta rama (el invitado con 1024 MB,
  Firefox abre `Index of file:///usr/local/share/dashos/`). El fallo de wayland en fusion-fondo no se reproduce aquí; el
  `salida_virt_wayland.txt` de fusion-fondo es del 2026-11-13, de antes de unir esta rama, así que no dice nada de él.
  Los topes no tocan la caja Linux: no crea subgrupos, y su tope (invitado más 66 MB) es el mismo número que antes.
