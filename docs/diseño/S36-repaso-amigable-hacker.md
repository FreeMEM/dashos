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

## Orden

Primero se corrigen los documentos con las reglas 1 a 10 (rama `repaso-s0`, revisada antes de unir). Luego, al hacer
cada pieza, su prueba incluye las dos preguntas: que exista la orden de Shell y el fichero de texto, además de la
ventana.
