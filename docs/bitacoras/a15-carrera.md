# A15: la carrera entre las pruebas que llenan el disco y la que lo comprueba

Rama `a15-carrera`, desde `revision` f27af229. Encargo: que "[comprobador] A15 la foto: bien" y "[ficheros] A15
sujetar sin soltar: bien" dejen de fallar a ratos en `make arranque-virt` según la rama y la carga, de raíz y sin
debilitar lo que comprueban. Leídas antes `paq-formato`, `union-dms`, `admin-desarrollo` y `archivos-memoria`.

## La causa, medida

Con una traza temporal en `archivos` (bloques libres al sujetar y al soltar, y guardados mientras tanto), en la base:

- El comprobador sujeta el disco muy pronto (línea 105 de la salida, antes de que `ficheros` escriba nada) y no lo
  suelta hasta que acaba sus 50 vueltas, cuando `ficheros` ya ha hecho todas sus pruebas de escribir: FS4 (200 KB),
  A7 (dos ficheros de 600 KB), A11 (300 KB), A14 (lectura anticipada, con 16 guardados, y descarte, 256 KB) y FS8.
  Con el disco sujeto, nada de lo que se borra vuelve a quedar libre.
- Disco de pruebas: 1792 bloques; el servicio suelta lo sujetado con menos de 1/16, 112. Al sujetar había 387
  libres; al soltar, **114**: dos bloques de sobra. Por eso unos KB más en un programa del disco (paq-formato: `C:Paq`
  4 KB más grande) o un guardado más lo tiraban, y por eso dependía de la carga: cuánto de lo de `ficheros` cae dentro
  de la foto depende de cuándo empieza y cuánto dura (3 s sin carga, 29-37 s con carga).
- Lo que se veía como "la que llena corre antes de que acabe el comprobador" es la consecuencia: el servicio suelta al
  comprobador por falta de sitio, `ficheros` puede sujetar en seguida y su prueba hostil llena el disco mientras el
  comprobador sigue recorriendo una foto que ya no está sujeta ("la foto: error 5").
- Y un fallo del servicio que lo hacía más confuso (apuntado en `union-dms`): quién fue soltado por falta de sitio se
  guardaba en un solo sitio (`SOLTADA_POR_SITIO`), así que, si después se soltaba también a `ficheros`, el `SOLTAR` del
  comprobador daba `NO_EXISTE` (-2) en vez de `LLENO`, y la prueba decía "MAL" en vez de "interrumpido".
- Lo que gastan los demás programas con el disco sujeto va con el tiempo: cada guardado reescribe unos bloques de
  cajones, y los viejos se retienen. Medido con `ficheros` ya parado: 46 bloques en 3 s sin carga; 104 en 29 s y 111
  en 37 s con carga.

## El arreglo

- **`archivos`**: el aviso de "se te soltó por falta de sitio" va con cada cliente (`Cliente::soltado_por_sitio`, que
  `tras_guardar` apunta tras cada orden y cada guardado). Cada uno recibe su `LLENO` una vez; ya no se pisan.
- **`archivos`, `INFO`**: en `asa`, 1 si alguien tiene el disco sujeto (documentado en `protocolo/archivos.rs`). Nadie
  más usaba ese campo en `INFO`.
- **`ficheros`, espera explícita y margen medido**: antes de cada `ESCRIBIR` (si no es él quien sujeta), mira `INFO`:
  si otro tiene el disco sujeto y lo que va a escribir no cabe dejando el 1/16 del servicio más `RESERVA_AJENA`
  (1 MB, el doble y algo de lo medido para los demás en 37 s), espera a que lo suelte (hasta 120 s; si no, lo dice y
  escribe). En este disco eso quiere decir que mientras el comprobador recorre su foto, `ficheros` no escribe; en uno
  con más sitio escribiría a la vez hasta ese margen. La foto sigue teniendo al servicio escribiendo a la vez: los
  guardados de los demás programas (9 a 29 por foto, medidos).
- **`ficheros`, "A15 sujetar"**: lo que escribe en cada una de sus tres vueltas se mide con el disco ya sujeto: hasta
  200 KB, lo que quepa dejando el 1/16 y `RESERVA` (256 KB); con menos de 64 KB por vuelta, falla diciendo cuántos KB
  libres había. Hoy salen los 200 KB de antes (gasta 64 bloques de los ~360 libres).
- **`ficheros`, "A15 sujetar sin soltar"**: además, un segundo `SOLTAR` tras el `LLENO` da `NO_EXISTE` (el aviso se da
  una vez y es de ese cliente).
- **Esperas por tiempo y no por vueltas**: el comprobador esperaba 10 s a que otro soltara (la hostil de `ficheros`, con
  carga, puede tardar más); `ficheros`, 30 s, y la foto del comprobador ha llegado a 37 s. Las dos, 120 s.

Con la traza, después del arreglo y con carga (28 bucles ocupados además de lo de los otros agentes): la foto duró
37 s; `ficheros` se paró en su primera escritura ("quedan 1428 KB libres: se espera"); el comprobador soltó con 276
libres (164 de sobra, antes 2).

## Pruebas

Commits: 4592f445 (`archivos`: el `LLENO` de cada cliente e `INFO` con la sujeción) y 14e04f95 (`ficheros` y
`comprobador`). Sin cambios en las líneas esperadas ni en el guion. "Con carga": 28 bucles ocupados en los 32 núcleos
hasta que sale A15, además de los QEMU de otros agentes.

| Vuelta | A15 (foto, sujetar, sin soltar) | Lo que falló | Notas |
|---|---|---|---|
| base, sin carga | bien | "[dash-ventana] pulsado" | |
| base, con carga (hasta A15) | bien | (parada tras A15) | la carga apenas alargó la foto (3 s) |
| base con traza, sin carga (hasta A15) | bien | (parada tras A15) | **2 bloques de sobra** al soltar (114 de 112) |
| arreglo con traza, sin carga (256 KB de reserva) | bien | (parada tras A15) | 30 de sobra |
| arreglo con traza, con carga (256 KB) | bien | (parada tras A15) | foto de 29 s; 20 de sobra: pocos, de ahí 1 MB |
| arreglo con traza, con carga (1 MB) | bien | (parada tras A15) | foto de 37 s; 164 de sobra |
| arreglo final, sin carga | bien | "[ficheros] ... ocupa sitio" | FS4 medía lo libre antes de esperar; arreglado |
| arreglo final, sin carga | bien | "[explorador] no abre SYS:DeCarla: -12" | la tecla pegada (búsqueda llena de "a") |
| arreglo final, con carga | bien | "[dash-ventana] pulsado" | foto de 29 s |
| arreglo final, con carga | bien | "[dash-ventana] pulsado" | foto de 31 s |
| arreglo final, sin carga | bien | "[dash-ventana] pulsado" | |

- `make prueba`: bien.
- `make arranque-virt-2`: bien (BIEN las dos cabezas); A15 bien en su salida.
- `make arranque-virt-linux`: bien (en esa salida no salen las líneas de A15).
- "[dash-ventana] pulsado" falta en cuatro de las cinco vueltas enteras, también en la de la base sin el arreglo: es de
  los clics del guion, no de esto. Ninguna vuelta entera de `make arranque-virt` pasó del todo por eso y por la tecla
  pegada.

## Pendiente del usuario

- Mientras el comprobador recorre su foto al arrancar, `ficheros` ya no escribe a la vez en el disco de pruebas (no
  cabe con margen). Lo que escribe durante la foto son los demás programas. Antes, cuánto de `ficheros` caía dentro
  dependía de la carga, y era justo lo que la tiraba. Si se quiere una escritura grande y segura durante la foto, haría
  falta un disco de pruebas más grande (el de la caja Linux de Arch fija la partición de DashOS en 7 MB:
  `tools/caja_linux_invitado.py`, `TAM_DASHOS`), y no se ha tocado.

## Lo que queda

- Lo que gastan los demás con el disco sujeto crece con lo que dura la foto. Con lo que queda libre (unos 270 bloques
  sobre el 1/16) aguanta unos 90 s al ritmo medido con carga (3 bloques por segundo); un PC mucho más cargado podría
  volver a interrumpirla, y ahora lo diría como "interrumpido", no como "MAL".
- La prueba de que dos clientes soltados por falta de sitio reciben cada uno su `LLENO` no está hecha (haría falta
  forzar que se suelte primero al comprobador): se ve por el código y por la línea nueva de la hostil (un segundo
  `SOLTAR` da `NO_EXISTE`).

## Lo que no se ha comprobado

- No he visto fallar la base en mis vueltas (tres, y una con carga): la causa sale de la traza (2 bloques de sobra) y
  de las vueltas que fallaron en `paq-formato` y `union-dms`, no de verla fallar aquí.
- En la Pi, y con un disco de pruebas de otro tamaño.
