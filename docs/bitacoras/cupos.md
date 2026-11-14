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
