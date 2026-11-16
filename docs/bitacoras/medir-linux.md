# Bitácora: medida de un dominio de controladores de Linux (S35, paso 1)

Rama `medir-linux`, sacada de `revision` (2344aad5). No toca `kernel/`, `usuario/` ni la caja: solo un guion del PC,
la configuración del kernel del dominio y su `/init` de prueba.

## 2026-11-16

### Qué hay

- `caja-linux/dominios/`: configuración propia del kernel (encima de `make tinyconfig`): `comun.config`,
  `arm64.config`, `x86_64.config`, `red.config` y `wifi.config`. Sacada de los Kconfig y de la documentación del
  kernel; sin copiar configuraciones de otros. Y `servidor.c`, el `/init` de la prueba, sin biblioteca C.
- `tools/medir_dominios.py`: compila los kernels, hace el initramfs, arranca en QEMU, busca la memoria mínima y saca
  la tabla (`todo`) y las variantes de configuración (`variantes`) a `resultados.json`.

Lo pesado (código de Linux 7.3.4, kernels, `resultados.json`, registros) vive en `~/recuperar/geosian-7d/medir-linux/`,
fuera de git. Sala limpia: no se leyó código C de Linux; solo Kconfig, `kernel/configs/tiny*.config` y el registro
del propio kernel al arrancar.

### Lo medido

Las cifras y el método, en S35, "Memoria", "Medido (2026-11-16)". En corto: el dominio de red y Wi-Fi arranca con
15 a 17 MB en aarch64 (1 a 4 procesadores) y usa 14 a 17 MB en reposo; listo en medio segundo. Objetivo de 48 MB:
se cumple. En x86-64 pide 27 MB para arrancar por la dirección de carga del kernel (16 MB); con
`CONFIG_PHYSICAL_START=0x200000`, 17 MB.

### Pruebas

- Cada arranque se da por bueno solo si llega a "listo" y a "memoria-fin" sin pánico, OOM ni "fallo:", y sigue vivo
  dos segundos más.
- Hostiles: la búsqueda del mínimo arranca a propósito con memoria de menos (salen OOM y pánicos, y se cuentan como
  fallo); y un dominio sin su aparato (kernel de red con `dominio=wifi`) dice "fallo: sin interfaz de Wi-Fi" y no
  dice "listo".
- No se pasó `make prueba` ni `make arranque-virt`: no se toca nada de DashOS.

### Pendiente

- Medir con un Wi-Fi de verdad (llave USB prestada al dominio, paso 2 de S35) y con `wpa_supplicant` o `iwd` dentro.
- Medir en la Pi (con KVM, o el hipervisor de DashOS) y no con el procesador emulado.
- Lo que añade el servidor de verdad (servir el aparato a DashOS por virtio).
- Si el kernel del dominio y el de la caja (Image-caja, 6.18) deben ser la misma versión: aquí se midió la 7.3.4.
