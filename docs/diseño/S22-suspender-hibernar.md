# S22. Suspender e hibernar sin fallos

Del 2026-11-10. Lo pidió el usuario: un modo de suspender e hibernar que no falle como el de Linux. Es diseño: no hay
código.

## El problema

En Linux, suspender y volver falla a menudo: la máquina no despierta, despierta sin pantalla, sin red o sin sonido, o
se cuelga al dormirse. Las causas conocidas:

1. **Cada controlador guarda y repone el estado de su aparato.** Si uno lo hace mal, el aparato despierta roto; si se
   cuelga, se cuelga el kernel entero, porque los controladores viven dentro.
2. **Dormirse espera a todos.** Un controlador o un programa que no contesta deja la máquina a medio dormir.
3. **El firmware** (ACPI en los PC) tiene fallos propios en cada modelo.
4. **La imagen de hibernación** depende del kernel exacto y del disco; si algo cambió, o la imagen se corrompe, se
   pierde la sesión o no arranca.
5. **Los programas no se enteran**: despiertan con la hora saltada y las conexiones muertas.

DashOS tiene una ventaja de partida: los controladores son procesos (S1, S12), los servicios guardan el estado lógico
(qué ficheros hay abiertos, qué ventanas, qué flujos de sonido) y el kernel puede parar cualquier grupo de procesos sin
pedirle permiso (S2).

## Lo que ofrece cada placa

De memoria; se comprueba antes de la fase Z3.

- **Ninguna Pi tiene suspensión a RAM de verdad** como un portátil: no se puede apagar el chip y dejar la memoria
  viva. Lo que hay es un **reposo profundo**: núcleos apagados salvo uno, relojes bajos y aparatos apagados, con la
  memoria viva. Gasta poco y despierta en una fracción de segundo.
- **Apagar y encender con un botón**: la Pi 5 lo trae; en la Pi 4 y la Pi 3, un GPIO despierta la placa apagada. Eso
  es un arranque, así que despertar de la hibernación es arrancar y reponer la imagen.
- **QEMU** permite probarlo todo sin manos: parar y seguir la máquina, y apagarla y arrancarla con el mismo disco.

## La idea: los controladores no se duermen, se relanzan

1. **Al dormir, los controladores se paran y nada más.** No guardan el estado del aparato.
2. **Al despertar, cada controlador arranca de cero**, como en el arranque, y su servicio se vuelve a conectar a él
   (vigilancia de E6 y aviso de muerte de K7): el compositor pone otra vez el modo de pantalla, el sonido reabre su
   salida, archivos vuelve a pedir el disco. Inicializar un aparato desde cero está probado en cada arranque; reponer
   un estado guardado, en cambio, solo se prueba al suspender.
3. **Si un controlador falla al despertar, falla su aparato, no el sistema.** Se reintenta con espera creciente y se
   avisa en el escritorio. El resto despierta.
4. **Primero lo que se ve y se toca**: pantalla, teclado y ratón se relanzan antes que nada, y lo primero que se ve es
   la pantalla de bloqueo de `guardian` (S6b).

## Dormirse nunca se cuelga

Cada fase tiene plazo. Lo que no contesta a tiempo se para a la fuerza:

| Fase | Qué | Plazo | Si no contesta |
|---|---|---|---|
| 1. Aviso | Los programas reciben "vas a dormir": guardan, el sonido sale con fundido | 1 s | Se sigue sin ellos |
| 2. Congelar | El kernel deja de planificar los grupos de usuario y de la caja Amiga (sin pedirles nada) | inmediato | — |
| 3. Guardar | Archivos lleva al disco todo lo pendiente (A1 y A2) | 2 s | Se sigue; lo que falte se pierde como si se fuera la luz, y el disco queda entero |
| 4. Parar aparatos | Se matan los controladores y se apagan sus aparatos por el firmware | 0,5 s por aparato | Se corta su corriente |
| 5. Dormir | Núcleos apagados salvo uno, en espera con las fuentes de despertar (botón, teclado, alarma) | — | — |

Lo que se para a la fuerza no se pierde en silencio (S36, regla 10). Al despertar sale un aviso en llano que nombra
los programas que no contestaron a tiempo ("Editor no llegó a guardar antes de dormir"). Y cada ciclo deja sus líneas
en `Estado:Registros/` (S6): qué se paró a la fuerza, qué aparato falló y qué despertó la máquina. Se lee con la orden
`Registro`.

Un vigilante por hardware cubre el kernel: si en fases 4 y 5 algo se queda colgado dentro del kernel, la placa
reinicia, y la hibernación (si se pidió) sigue en el disco.

## Cómo se pide dormir

- **Con el ratón**: en el menú de apagado (S10b), "Suspender" e "Hibernar". No preguntan: dormir se deshace
  despertando.
- **Desde el Shell**: las órdenes `Dormir` e `Hibernar`, con `HASTA hora` para poner la alarma que despierta. Así un
  guion hace lo mismo que el menú. La sintaxis exacta, por diseñar al hacerlo.
- **Solo**: por tiempo sin uso, por la tapa o por el botón, según los ajustes de energía. Esos ajustes son un fichero
  de texto en `ENVARC:` (S4b, S27), que escriben Ajustes y la orden `Ajuste`. Qué claves lleva, por diseñar al hacerlo.

## Despertar

1. Despiertan los núcleos; el kernel corrige el reloj (salto de tiempo) y los temporizadores.
2. Se relanzan pantalla, entrada y `guardian`: se ve el bloqueo.
3. Se relanzan los demás controladores; los servicios se reconectan.
4. Se descongelan los grupos.
5. Los programas reciben "despertaste", con cuánto tiempo pasó: la red vuelve a mirar sus conexiones, los relojes se
   ponen al día.

## Hibernar

- **Imagen de la memoria en uso**, comprimida (LZ4, S3) y **cifrada** con una llave al azar (R10) envuelta con la
  contraseña del usuario, como los volúmenes (FS10). En la imagen están los secretos de la sesión: sin la contraseña
  no se lee.
- **Antes, lo mismo que al dormir** (fases 1 a 4): los controladores no van en la imagen con estado de aparato; al
  reponerla, se relanzan como al despertar.
- **Al arrancar**, el kernel mira si hay imagen. `guardian` pide la contraseña, se comprueba la firma de la imagen y
  que es del mismo kernel; si todo cuadra, se repone y se sigue como al despertar.
- **Una actualización no tira la sesión** (S36, regla 10). Si hay imagen, se arranca con el `SYS:` y la generación de
  paquetes que la hicieron (S8, S14), aunque haya una actualización esperando. La nueva entra en el siguiente arranque
  limpio. Así el usuario no pierde lo abierto por algo que no hizo.
- **Si algo no cuadra** (un kernel que ya no está, imagen corrupta, contraseña cancelada), arranque normal: no se
  pierde nada de disco, porque archivos lo guardó antes. Se pierde la sesión abierta, y se avisa en llano, con el
  motivo en el registro.
- **Las cajas van en la imagen sin hacer nada**: la máquina virtual de Linux y la caja Amiga son memoria de procesos.
  La caja Linux recibe su aviso por virtio para que su reloj y su red se pongan al día.
- **Dónde**: un área propia en el disco, del tamaño de la memoria en uso; no hace falta un intercambio. Ese sitio
  tiene que tener dueño: o lo reserva el instalador (S32) o es un volumen elástico de DashFS. Cuál, por diseñar al
  hacerlo. Si la imagen no cabe, se dice antes de empezar ("No hay sitio para hibernar") y se ofrece suspender.

## Cómo se prueba

- **QEMU, sin manos**: dormir y despertar mil veces seguidas con programas abriendo ventanas, sonando y escribiendo;
  después, que todo responde y el disco está entero.
- **Hostiles**: un controlador que no se deja parar (se duerme igual), uno que se cae al despertar (el sistema
  despierta, ese aparato se relanza), un programa que ignora el aviso (se congela igual).
- **Hibernar**: imagen corrupta a propósito, imagen de otro kernel y contraseña mala: arranque normal y aviso. Con
  una actualización esperando: se arranca con la generación de la imagen y la sesión vuelve.
- **En la placa**: consumo en reposo, tiempo de dormirse y de despertar, al panel de medidas.

## Fases

| Fase | Qué | Prueba |
|---|---|---|
| Z1 | Congelar y descongelar grupos en el kernel; avisos de "vas a dormir" y "despertaste" | Un grupo congelado no avanza; el resto sí |
| Z2 | Reposo en QEMU: controladores parados y relanzados, servicios que se reconectan, bloqueo al volver | Mil ciclos en QEMU `virt` con la prueba de arranque corriendo |
| Z3 | Reposo en la Pi: núcleos apagados, relojes bajos, HDMI, USB y red apagados; fuentes de despertar | Consumo y tiempo de despertar en la Pi 4 |
| Z4 | Hibernar en QEMU: imagen comprimida, cifrada, con firma y versión | Ciclos de hibernar; las tres pruebas de imagen mala |
| Z5 | Hibernar en la Pi, con el botón o el GPIO | Pi 4 y Pi 5 |

Dependen de: vigilancia de servicios (E6, R12), aviso de muerte (K7, R9), `guardian` (R17), azar (R10) y el guardado
diferido de archivos (A1 y A2).

## Lo que queda abierto

1. ¿Relanzar los controladores al despertar en vez de guardar y reponer su estado? Recomendado: sí. Es lo que hace que
   no falle.
2. ¿Pedir la contraseña al despertar siempre, también del reposo? Recomendado: sí, con la opción de no pedirla en
   reposos de menos de un minuto.

## Cómo encaja

- S1, S12: controladores como procesos que se pueden matar y relanzar.
- S2: el kernel congela grupos.
- S3: compresión LZ4 de la imagen.
- S5 y FS10: la imagen cifrada como los volúmenes.
- S6b: `guardian` pide la contraseña al volver. S6: el registro de cada ciclo, en `Estado:Registros/`.
- S8: con imagen de hibernación se arranca con la generación que la hizo.
- S7 y S4: las cajas van en la imagen.
- S14: el gestor de servicios hace de director de las fases.
