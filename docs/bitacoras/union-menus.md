# Unión: la prueba de los menús tras unir cuentas-2, archivos-memoria, dominio-usb y paq

Rama `union-menus`, copia de `fusion-fondo` (bf49ac2d: `revision` más esas cuatro ramas). Encargo: `make -C kernel
arranque-virt-menus` fallaba dos de dos con "FALTA: [centro] menús en marcha: en la barra, de ENV:Escritorio/menus; en
el compositor: sí"; en `revision` pasaba.

## La causa

De la rama `cuentas-2`, en `guardian`: para saber en qué imagen está miraba si tenía la llave 4 ("uso", la imagen de
usar) y la llave 5 ("codigos", la de `make arranque-virt-cuentas`). Pero `guardian` es el primero que nombra "uso", así
que lo lee, e `inicio` le da además la copia para escribirse en él, "yo:uso", que en la imagen de usar es la llave 5
("guardian lanzado en su grupo, con 5 llaves"). Así toda imagen de usar se tomaba por la de cuentas:

- Al entrar, `guardian` pedía al centro "abrir cuentas" ("[guardian] (prueba) pide al centro abrir Cuentas: 0"). La
  ventana del centro tiene su propio bucle y, mientras está abierta, el centro no lee su canal: los avisos de cambio de
  `ENV:` se quedan en cola. `Ajuste menus barra` escribía bien `ENV:Escritorio/menus` (acaba con 0 y sin decir nada,
  como debe), pero el centro no lo aplicaba hasta cerrar Ajustes, y el guion no la cierra.
- En la imagen de usar de verdad (`make virt-uso`), los códigos de recuperación se escribían también en la consola, y
  Ajustes se abría en Cuentas en cada entrada.

No eran `archivos-memoria` (la escritura en `ENV:` y los avisos están bien: el centro vigila y `Ajuste` escribe), ni
`paq` ni `dominio-usb`.

## El arreglo

`guardian` mira su tabla de llaves por nombre (`dashos::llaves::Tabla::buscar("uso")` y `"codigos"`), no cuántas trae.
Y lo dice al empezar: "[guardian] modo de uso: pantalla de entrada; los códigos de recuperación, solo en su pantalla"
(o "también en la consola (prueba)" en la de cuentas).

## Pruebas

- `usuario/src/llaves.rs`, `la_imagen_por_el_nombre_no_por_cuantas`: las tablas reales de `guardian` en cada imagen; en
  la de usar, 5 llaves y ninguna "codigos"; una "yo:codigos" no vale por "codigos".
- Líneas esperadas: `arranque_virt_menus.txt` y `arranque_virt_shell.txt` piden "solo en su pantalla";
  `arranque_virt_cuentas.txt`, "también en la consola (prueba)".

## Pendiente

- El centro no atiende los avisos de cambio de `ENV:` mientras su ventana está abierta (ya pasaba en `revision`): un
  `Ajuste menus barra` con Ajustes abierto se aplica al cerrarla. Habría que leer su canal también desde el bucle de la
  ventana. No se ha tocado.
