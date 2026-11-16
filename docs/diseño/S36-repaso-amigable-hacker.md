# S36. Repaso de los diseños contra "amigable de 2026, con fondo para informáticos"

Pedido por el usuario el 2026-11-15. Se leyeron 39 diseños contra las dos preguntas de S0 (¿lo usa sin ayuda quien no
es informático?, ¿puede un informático verlo y cambiarlo sin leer el código?). Salieron 162 hallazgos. El detalle,
documento a documento y con cita, está en `docs/revisiones/repaso-s0/` (tres ficheros). Aquí va lo que se repite, como
reglas comunes, y lo que tiene que decidir el usuario.

Límites del repaso: cada lector solo leyó su grupo, así que algún "no lo dice" puede estar resuelto en otro documento;
de S18 y de los diseños del kernel solo se leyó lo que toca al usuario; no se miró el código. Los diseños de la rama
`x86` (S30, S31, S34) no entraron.

## Reglas comunes que salen del repaso

Son consecuencia directa del principio de S0. Los diseños se corrigen para cumplirlas.

1. **Cada ajuste es un fichero de texto en `ENVARC:` que manda.** La ventana de Ajustes y una orden del Shell escriben
   ese mismo fichero; editarlo a mano vale igual. Si hace falta un formato binario por compatibilidad (IFF del Amiga),
   se genera del texto. Hay una orden para leer y poner un ajuste suelto. Protegido (solo con la ventana de confianza o
   con contraseña) queda solo lo que es de seguridad: permisos, cuentas y ajustes de la máquina.
   Afecta a: S10c, S27, S3 ("memoria amiga"), S13 (red), S7 y S18 (cajas), S26 (tema).
2. **Un solo registro, legible.** Texto en `Estado:Registros/`, una orden para leerlo y la misma lista en Ajustes. Todo
   lo que el sistema para, reinicia, niega o deshace deja una línea con programa, motivo y hora: un permiso negado, una
   vuelta atrás del arranque, un controlador o la GPU reiniciados, un programa que topa con su cupo, lo que mata Linux
   por memoria, cada ciclo de dormir. Aviso en pantalla, en llano, solo si el usuario lo nota o si se repite.
   Afecta a: S6, S6b, S14, S12, S25, S29, S35, S22, S3, hilos.
3. **Los permisos se leen.** Lo concedido está en un solo sitio, en texto que su dueño puede leer, con una orden
   `Permiso` (listar, dar, quitar). Ver los permisos propios es libre; cambiarlos sigue pidiendo confirmación de
   confianza. Afecta a: S6, S6b, S28 (hoy lo ponen en dos sitios distintos), S10c.
4. **Modo de desarrollo explícito.** Para sustituir una pieza del sistema, probar un servicio propio o una imagen de
   dominio propia: se activa a propósito, se ve siempre en pantalla, queda en el registro y vive en la capa del usuario
   o de la máquina, sin tocar `SYS:`. Hoy la única salida es el Shell, que quita todo el encierro.
   Afecta a: S6, S6b, S14, S29, S35.
5. **Las instantáneas tienen cara.** Cada una se ve como un cajón de solo lectura; "Versiones anteriores" en el menú de
   un icono saca un fichero suelto; hay una orden `Instantanea`. Volver entero sigue existiendo. Afecta a: S5, S28, S29.
6. **Cada dato, un sitio con nombre.** Nada del usuario vive donde no lo pueda encontrar: la papelera de la caja Amiga,
   `Home:Datos`, el cajón de cada caja (hoy en tres sitios según el documento). Lo interno no estorba, pero se ve con
   "Mostrar todos los ficheros". Afecta a: S4b, S28, S8 (ya corregido).
7. **Las cajas se dejan mirar y mandar.** El disco de la caja Linux se ve como un volumen con nombre (solo lectura
   desde fuera); una orden `Caja` da estado, arranca, para y ejecuta dentro. Los dominios de controladores se listan
   con su memoria y su tope, y su registro sale a un fichero. Afecta a: S7, S13, S35.
8. **Lo que falta se ofrece, no se explica.** Si algo no va de serie (un códec, un controlador), al necesitarlo sale un
   aviso en llano con un solo botón. Afecta a: S33, S32.
9. **Lo que existe se ve.** Los menús de un programa se enseñan también con el clic normal en la barra; el botón
   derecho queda como atajo del Amiga. Afecta a: S10.
10. **Dormir e hibernar no pierden sin avisar.** Si hay imagen de hibernación, se arranca con la generación que la
    hizo; al despertar se dice qué programas no guardaron. Afecta a: S22, S8.

## Contradicciones entre documentos

- **Controladores de Linux**: S12b dio por aceptado LKL (el kernel de Linux como biblioteca dentro de un proceso de
  DashOS) y `hilos-de-usuario.md` se diseñó para él; S35, escrito después, pone los controladores en máquinas virtuales
  y no nombra LKL. S35 se escribió sin cotejar S12b. **Por decidir** (abajo).
- **Quién cambia el tema**: S26 dice que solo el centro de control; S10 promete cambiarlo por guion y S10b tiene un
  servicio de apariencia. Con la regla 1 gana "fichero de texto, ventana y orden".
- **"Pantallas" o "espacios de trabajo"**: S10 y S23 usan un nombre y unas teclas; S27, otros. **Por decidir.**

## Por decidir el usuario

1. Tres hallazgos chocan con decisiones ya tomadas; no se tocan si no se dice: la contraseña en cada acción de
   administrador; que las aplicaciones encerradas no carguen de `Home:Libs`; la guardia contra malware de Amiga, que
   pregunta.
2. LKL o dominios de controladores (o cada uno para algo).
3. "Pantallas" o "espacios de trabajo", y sus teclas.
4. Juegos del Amiga recién instalado: hoy hay que poner una ROM y elegir modo de caja. La propuesta del repaso es
   llevar una ROM libre de serie y que la caja elija sola; llevar una ROM es una decisión de licencia y de la regla "sin
   ROMs en el repositorio".

## Estudio y recomendación de las cuatro decisiones (2026-11-16)

Pedido por el usuario. Son recomendaciones: no están decididas hasta que él lo diga.

### 1. Las tres decisiones ya tomadas: se quedan, con un ajuste cada una

- **Contraseña en cada acción de administrador.** Se queda sin "vale durante 5 minutos": esa ventana de tiempo es
  justo lo que aprovecha un programa malo para colar su acción detrás de la del usuario. Ajuste: **una tanda es una
  acción**. Instalar diez paquetes de un plan de `Paq`, o crear un usuario con sus grupos, es una sola ventana que
  enseña la lista entera y pide la contraseña una vez. Lo que no esté en la lista enseñada no se hace.
- **Las aplicaciones encerradas no cargan de `Home:Libs`.** Se queda: si no, cualquier programa del Shell deja una
  biblioteca que se cuela en todas las aplicaciones. Ajuste: **modo de desarrollo por aplicación** (regla 4). El
  usuario marca una aplicación y esa sí carga de su capa; marcarla pide la confirmación de `guardian` (si no, la
  marcaría el mismo programa malo), se ve en su ficha y queda en el registro. Y una orden para lanzar desde el Shell
  un programa encerrado, para probarlo como lo verá el usuario.
- **La guardia de la caja Amiga, que pregunta.** Se queda preguntar. Ajustes: con la sesión delante la pregunta
  **espera**, sin el plazo de 30 segundos; el plazo existe porque hoy la caja se queda parada mientras pregunta, y lo
  que hay que arreglar es eso (el programa 68k espera, la caja sigue atendiendo su ventana). Con la sesión bloqueada
  o sin nadie, es "no", con un aviso que se queda: "actualiza.exe no pudo cambiar viejo. Volver a intentar". Y la
  versión anterior del ejecutable se guarda siempre en la papelera, también cuando se dice que sí: así se puede
  deshacer.

### 2. LKL o máquinas virtuales: máquinas virtuales (S35), y LKL se retira

| | LKL (S12b) | Dominio en máquina virtual (S35) |
|---|---|---|
| Qué Linux corre | Una variante fuera del kernel oficial (`arch/lkl`), mantenida aparte | El kernel oficial, sin tocar |
| Lo que hay que escribir | La capa que pide `arch/lkl` y, además, cómo llegan a ese Linux el bus, las interrupciones y el DMA del aparato de verdad | Pasar el aparato a la máquina y servirlo por virtio |
| Lo que DashOS ya tiene | Hilos en los procesos | El hipervisor (la caja Linux) y los controladores virtio, probados |
| Seguir las versiones de Linux | Depende de que el proyecto LKL siga | Se cambia el kernel y ya |
| Frontera de licencia | Código GPL enlazado con nuestra capa, en la zona GPL | Un kernel aparte que solo habla virtio: la más limpia |
| Protección del propio Linux | La de un proceso | La de un proceso y, además, el código del kernel protegido desde fuera (capa 3) |
| Memoria | Sin medir | Sin medir (objetivo 48 MB) |

Recomendado: **máquinas virtuales**. La razón que pesa: LKL se pensó para usar la pila de red y los sistemas de
ficheros de Linux dentro de un programa, no para llevar aparatos de verdad; lo que S12b quería ahorrar (escribir la
unión con cada bus) habría que escribirlo igual, y sobre un Linux que no es el oficial. Comprobado: LKL sigue fuera
del kernel oficial (las propuestas para meterlo llegan hasta 2021). No comprobado: si hoy admite varios procesadores
o aparatos reales; lo que se recuerda es que no, pero no se ha verificado.

Lo que cambia con la decisión:
- S12b queda sustituido por S35 en lo de LKL; lo de red de S12b (SACK, `amissl.library` nativa) sigue.
- Los hilos de usuario no se pierden: hacen falta igual.
- **El Wi-Fi de la Pi** (SDIO, sin IOMMU) era lo primero de S12b. Con S35: controlador propio (el chip es "fullmac",
  casi todo lo hace su firmware) o dominio sin barrera de DMA. Sigue por decidir con la placa delante.
- **Corrección a S35**: "con 1 GB o menos, sin dominios" es demasiado tajante. La Pi 3 (1 GB) y el THEA1200 (512 MB)
  dependen del Wi-Fi. Mejor: el dominio de red entra si su tope medido cabe (48 MB en 1 GB cabe); los de gráficas, no.
- Si la medida del paso 1 de S35 sale mal (muy por encima de 48 MB), se vuelve a mirar. LKL no tendría por qué
  gastar menos: es el mismo kernel.

### 3. "Pantallas" o "espacios de trabajo": son tres cosas y cada una lleva su nombre

- **Monitor**: el aparato físico.
- **Espacio de trabajo**: lo de todos los días, del 1 al 9, cada uno con su disposición (S27, ya hecho en el
  compositor). Es la palabra que ve el usuario.
- **Pantalla**: solo la pantalla propia de un programa de Amiga (`OpenScreen`, con su resolución y su paleta, S23). Se
  ve como un espacio de trabajo más, que es lo que S27 ya dice.

Teclas: mandan las de S27, que son las que existen (Super+1..9, Super+arriba y abajo). Amiga+M y Amiga+N se quedan
como atajo del Amiga para lo mismo (pasar al siguiente y al anterior), y el gesto de arrastrar una pantalla hacia
abajo se conserva. S10 y S23 se corrigen para decirlo así. S27 usa a veces "pantalla" por "monitor": se corrige.

### 4. ROM para los juegos del Amiga: sí, la de AROS, en la imagen pero no en el repositorio

- Qué es: la ROM de AROS para 68k es libre (licencia APL) y se puede repartir. No es la de Commodore.
- No rompe la sala limpia: esa regla es no leer su código para escribir el nuestro. Repartir su binario no lo es.
- No rompe "sin ROMs en el repositorio": la imagen de `SYS:` la baja al construirse, con su huella comprobada, y
  dice de dónde sale el código fuente (lo pide su licencia). En git no entra.
- Cómo se usa: si el usuario tiene su Kickstart, manda el suyo (y sigue siendo la referencia de las pruebas). Si no,
  la caja arranca con la de AROS sin preguntar. La caja elige sola el modo ("máquina entera" al primer acceso a los
  chips) y la configuración de cada juego se guarda en texto junto a él.
- Lo que se dice claro: con la ROM de AROS no funcionarán todos los juegos. Cuántos, no se sabe: no está medido.
  Cuando uno falle, el aviso lo dice en llano y ofrece poner un Kickstart propio.
- Cuándo: no hay prisa. Los chips del Amiga (S18) están diseñados, no hechos; esto se hace en su fase 2. S18 ya lo
  traía como pregunta 8 ("probarlo en la fase 2 como extra"): la recomendación es que deje de ser un extra.

## Orden

Primero se corrigen los documentos con las reglas 1 a 10 (rama `repaso-s0`, revisada antes de unir). Luego, al hacer
cada pieza, su prueba incluye las dos preguntas: que exista la orden de Shell y el fichero de texto, además de la
ventana.
