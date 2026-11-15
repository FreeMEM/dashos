# S32. El instalador y la herramienta de discos

Borrador del 2026-11-15, pedido del usuario: "un instalador, que además respete a otros sistemas operativos, una
herramienta en plan gparted". Lo que sigue es diseño; nada está hecho salvo lo que se dice como existente.

## Lo que hay hoy

- **Particiones, solo leer** (`usuario/src/particiones.rs`, A14): MBR y GPT, la partición de DashOS (MBR 0x7F, GPT
  d0a18dbf-…-0017) y la de la caja Linux (…-0018). Desconfía de todo y no adivina. No lee la copia de la GPT del final
  del disco ni escribe nada. Pruebas con tablas rotas en `archivos/herramientas`.
- **DashFS crece y encoge, fuera de uso** (`dashfs-resize`, heredado de RedoxFS): crecer, sí, hasta el final de la
  partición. Encoger solo quita el hueco libre del final: no mueve datos, y si un bloque del tramo está usado se cae
  con un `assert` (un TODO de RedoxFS). Con el volumen montado, nada.
- **Volúmenes dentro de DashFS** (FS10): varios volúmenes en un mismo sistema de archivos, que comparten el espacio,
  como los subvolúmenes de btrfs o los volúmenes de APFS. Son elásticos por diseño: no tienen tamaño fijo. Su tope
  (`quota`) se guarda pero no se aplica todavía.
- **Imágenes de tarjeta para la Pi 5** (`make tarjeta-pi5`) y de disco para QEMU: se hacen en el PC, no hay instalador.

## Lo que pide un instalador que respeta a los demás

1. **No tocar nada sin plan.** Igual que con los paquetes: se calcula el plan entero (qué particiones se crean,
   encogen o mueven, qué se formatea, qué arranque se toca), se enseña con un dibujo del disco antes y después, y se
   confirma. Hasta entonces no se escribe un byte.
2. **Reconocer a los demás antes que nada.** Leer GPT (las dos copias) y MBR, y reconocer por su tipo y su firma:
   la partición de sistema EFI (ESP), Windows (NTFS, BitLocker), Linux (ext4, btrfs, swap, LUKS), macOS (APFS),
   AmigaOS (RDB en discos de Amiga, PFS/SFS/FFS), FAT y exFAT. Lo que no se reconoce no se toca.
3. **Encoger a otro solo si es seguro**, y mejor que lo haga su propio sistema:
   - NTFS: solo si no está hibernado ni con BitLocker sin suspender; si no, se para y se dice por qué. Encoger NTFS
     desde fuera es lo más arriesgado; recomendación: pedir al usuario que lo encoja desde Windows y usar el hueco.
   - ext4: se puede encoger desmontado; btrfs, montado; LUKS y APFS, no desde DashOS.
   - Primera versión: **solo usar hueco libre o un disco entero**. Encoger particiones ajenas, después y con
     herramientas probadas.
4. **El arranque compartido.** En un PC, la ESP se comparte: DashOS añade su cargador en su propia carpeta
   (`EFI/DashOS/`) y su entrada en la NVRAM, y no toca la de nadie. Si el firmware no deja elegir, un menú propio
   que encadena a los demás (como rEFInd). En la Pi 5, la tarjeta o el SSD con su `config.txt`; otros sistemas en
   otros discos.
5. **Deshacer.** Antes de escribir, copia de las tablas de particiones (las dos GPT y el MBR) en un fichero fuera
   del disco (un USB) y en el propio instalador. Si algo se corta, volver a las tablas de antes. Lo que se formatea
   no tiene vuelta: se dice así.

## La herramienta de discos (como GParted)

Una app del escritorio (y su versión de órdenes) que hace lo mismo que el instalador, pero suelto:

- Ver discos y particiones con su uso; crear, borrar y cambiar la etiqueta; formatear DashFS, FAT y exFAT.
- **Volúmenes de DashFS**: crear, quitar, renombrar, poner tope (cuando `quota` se aplique), instantáneas.
- **Crecer DashFS** montado (es lo más útil: tras ampliar la partición). **Encoger** desmontado y moviendo datos.
- Lo de otros sistemas, solo leer, salvo borrar una partición entera con doble confirmación.
- Comprobar y reparar DashFS (`check.rs` y `repair.rs` ya existen).
- Por encima de todo, el mismo plan con dibujo y confirmación del instalador.

## Lo que le falta a DashFS para esto

1. **Encoger moviendo datos**: recorrer los bloques usados del tramo que se quita y copiarlos más abajo (con copia
   al escribir es natural: escribir la copia y cambiar los punteros en una transacción), y quitar el `assert` del
   TODO por un error.
2. **Crecer montado**: avisar al servicio `archivos` de que la partición es mayor y añadir los bloques al
   asignador en una transacción. Sin parar nada.
3. **Aplicar `quota`** en los volúmenes (FS10 lo dejó guardado).
4. **Pruebas hostiles**: cortar la corriente en mitad de crecer y de encoger (como la prueba de cortes de los
   paquetes), encoger con el disco casi lleno, encoger por debajo de lo usado, una partición más pequeña que lo que
   dice la cabecera.

## Lo que queda abierto

1. ¿Primera versión solo con hueco libre o disco entero (recomendado), o también encoger particiones ajenas?
2. ¿El instalador corre desde DashOS arrancado de un USB (recomendado: es DashOS de verdad, con su escritorio), o es
   un programa aparte?
3. ¿Particiones aparte para `SYS:`, `Usuarios:`, `Work:`? Recomendado: una sola partición de DashFS con volúmenes
   elásticos dentro (FS10), salvo `SYS:`, que es una imagen firmada (S8, S14) y puede ir en dos ranuras A/B.
4. Cifrado del disco entero o por volumen (DashFS ya cifra volúmenes, `adiantum.rs`).

## Cómo encaja

- S5 y FS10: DashFS y sus volúmenes. S14: arranque y la imagen firmada de `SYS:`. S8: el plan con confirmación, el
  mismo estilo que los paquetes. S24: las máquinas donde se instala. S6b: instalar el sistema es de administrador.
