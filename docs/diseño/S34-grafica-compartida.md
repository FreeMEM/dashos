# S34. La gráfica compartida en PC: un dominio de gráficos

Del 2026-11-15. Lo pidió el usuario: los programas de DashOS y los de la caja Linux tienen que usar la gráfica
dedicada (la RTX 3050 de pegaso) **a la vez**, sin que el usuario note dónde corre cada cosa, y con el máximo
rendimiento "aunque cueste mucho". Amplía y corrige S33 (códecs, sección de gráficas, en `revision`). Es diseño: no
hay código.

## Repaso de S33: lo que no llega

1. **"La dedicada se pasa a la caja Linux."** Así la gráfica es solo de la caja. Un programa nativo de DashOS no
   puede usarla. No cumple el encargo.
2. **La dueña sería la caja del usuario.** Es donde el usuario instala, rompe y reinicia cosas. Si cae o se
   apaga, el escritorio entero se queda sin gráfica.
3. **"DashOS recibe los cuadros como superficies"** no dice cómo, ni quién presenta. Con copias de por medio se
   pierde rendimiento y latencia en 4K y en juegos.
4. **Con una sola gráfica AMD o Nvidia: el controlador de Linux "como proceso"** con una capa que imita el kernel
   (regla 7, estilo DDE). Para Wi-Fi vale. Para una gráfica no: amdgpu, i915, xe y el de Nvidia son cientos de miles
   de líneas atadas al núcleo de DRM, a la memoria y a la energía de Linux; el de Nvidia es cerrado y solo carga en un
   Linux de verdad. Imitar eso cuesta más que una máquina virtual y sale más frágil.
5. **Cuatro mecanismos distintos** según la máquina (pase entero, SR-IOV, proceso aislado, virtio en la Pi). Cuatro
   caminos que probar y cuatro comportamientos.
6. **shiva: "la Arc para DashOS, la Radeon a la caja"** reparte las gráficas por dueño. Lo que quiere el usuario es
   que todo use cualquiera.
7. Falta decir qué pasa con CUDA, con la energía, al suspender, y si el dominio cae.

Lo que S33 acierta y se conserva: IOMMU primero; al arrancar, la pantalla del firmware; por CPU siempre lo libre;
sala limpia (los controladores de Linux, dentro de Linux); la Pi, con controladores propios.

## La propuesta: un solo mecanismo

**En PC, todas las gráficas con controlador de Linux las lleva un "dominio de gráficos"**: una máquina virtual
pequeña con un Linux mínimo y los controladores del fabricante (el oficial de Nvidia, amdgpu, i915 o xe). DashOS le
pasa las gráficas enteras con IOMMU. El dominio no es la caja del usuario: es un servicio del sistema, como un
controlador. Todos los demás son clientes suyos, por el mismo camino:

```
programas DashOS ─┐                        ┌─ RTX 3050  (controlador oficial)
compositor DashOS ─┼─ canal + memoria ──►  dominio de gráficos (Linux mínimo)
caja Linux ────────┘   compartida          └─ UHD 630 / Arc / Radeon (i915, xe, amdgpu)
```

- **Programas nativos de DashOS**: Vulkan y OpenGL normales, con Mesa compilado para DashOS (ya previsto: "Mesa sobre
  un servidor de GPU"). El controlador de Mesa no habla con un kernel: manda sus órdenes al dominio.
- **El compositor de DashOS**: un cliente más. Compone con la gráfica y presenta por ella.
- **La caja Linux del usuario**: ver "CUDA" abajo.
- Una gráfica, dos o tres: es lo mismo. Cada programa elige (o el sistema elige por él) con qué gráfica pinta, como
  `DRI_PRIME` o el "rendimiento alto" de un portátil. En pegaso, RTX para lo pesado y UHD 630 para lo ligero y para
  ahorrar. En shiva, Arc y Radeon igual.

### Cómo viajan las órdenes (rendimiento)

Dos protocolos, los dos abiertos y en Mesa, elegidos por fabricante:

- **Contexto nativo** (AMD e Intel): el programa lleva el controlador de usuario de verdad (radv, anv, iris) y solo
  se reenvía la interfaz con el kernel. El coste es casi el del nativo. Es lo que usa ChromeOS con las Intel y AMD.
- **Venus** (Nvidia): el controlador de usuario de Nvidia es cerrado y solo existe para Linux, así que se serializa
  Vulkan y lo ejecuta el dominio con el controlador oficial. OpenGL va encima con Zink. Rinde casi como nativo cuando
  manda la gráfica; pierde algo en programas con muchísimas llamadas pequeñas.

Para que el puente no cueste:

- **Anillos en memoria compartida, sin salidas de la máquina virtual** en el camino caliente: un hilo del dominio
  espera en el anillo (sondeo con un núcleo cuando hay carga, aviso cuando no).
- **Búferes compartidos sin copia**: las texturas, los cuadros y los búferes de vídeo son objetos de memoria de
  DashOS proyectados en el dominio, y memoria de la gráfica proyectada en el cliente. Nunca pasan por un canal.
- **Presentación sin copia**: el monitor lo enciende la gráfica a la que está enchufado. Si el compositor pinta con
  esa misma gráfica, el cuadro va directo a pantalla. Un programa a pantalla completa (un juego, un vídeo) se
  presenta directo, sin pasar por la composición. La copia entre gráficas solo existe cuando se pinta en una y el
  monitor está en otra (como en un portátil), por DMA entre las dos dentro del dominio.
- **El hipervisor no estorba**: los registros y la memoria de la gráfica van proyectados directos al dominio,
  interrupciones entregadas sin salir (posted interrupts), páginas grandes, núcleos fijos para el dominio.
- **Sincronía y latencia**: VRR, y los tiempos de presentación del compositor llegan hasta el dominio.

### CUDA y la caja Linux (corregido el 2026-11-15 tras la objeción de geosian-7d)

La primera versión proponía la caja Linux del usuario como contenedor dentro del dominio. **Se retira como opción de
serie**: el dominio presenta la pantalla de DashOS, y por él pasan los búferes del compositor y de todos los
programas, también las contraseñas que pinta `guardian`. Con el software sin vetar del usuario en el mismo kernel,
escapar del contenedor daría la pantalla entera y el DMA de las gráficas. Además obligaría a rehacer lo ya hecho en
la caja como máquina virtual (globo de memoria, cupos, cortafuegos, partición propia, puente Wayland, S28 y S29) y a
tener dos cajas distintas, la de la Pi y la del PC.

Lo que queda:

- **La caja Linux sigue siendo una máquina virtual aparte**, en PC y en la Pi, aislada como hoy. Es un cliente más
  del dominio: Vulkan y OpenGL por contexto nativo o Venus, vídeo por el dominio.
- **CUDA nativo sin compartir kernel: reenviar la interfaz del controlador de Nvidia.** Las bibliotecas de CUDA
  corren en la caja; sus llamadas al controlador (`/dev/nvidia*`, `ioctl` y memoria proyectada) cruzan al dominio
  por un intermediario que solo deja pasar una lista de órdenes conocidas. Es lo que hace WSL2 con su gráfica
  paravirtual (CUDA casi a velocidad nativa) y lo que hace gVisor con `nvproxy` para contenedores. Los módulos del
  kernel de Nvidia son abiertos (MIT y GPL) y definen esa interfaz. Cuesta: hay que seguir cada versión del
  controlador. Es la meta.
- **Dos dominios por confianza, donde hay dos gráficas** (pegaso, shiva): el **dominio de pantalla** lleva la
  integrada, el monitor, el compositor y las ventanas delicadas; el **dominio de cómputo** lleva la dedicada y sirve
  a la caja (CUDA) y a los programas de DashOS que la pidan (Venus). Si el de cómputo cae en manos de la caja, ve
  solo los búferes de los programas que eligieron esa gráfica, no la pantalla. Lo pintado en la dedicada llega al
  monitor con una copia entre gráficas, como en un portátil (pocos milisegundos; se medirá).
- **Mientras tanto** (antes del reenvío): la RTX entera a una máquina virtual de CUDA cuando se usa, como decía S33,
  y el escritorio en la integrada por el dominio de pantalla.
- **Para quien lo pida a sabiendas**: la caja dentro del dominio de cómputo (nunca en el de pantalla), con aviso.
- CUDA en programas nativos de DashOS: no. Lo nativo calcula con Vulkan.

**Medir antes de decidir**: el coste real de Venus y del contexto nativo frente al nativo, y el de la copia entre
gráficas, en pegaso.

### Vídeo

El dominio decodifica y codifica con el hardware (NVDEC, Quick Sync, VCN) y entrega los cuadros como búferes
compartidos, que el compositor presenta como plano sin copia. Las clases de DataTypes de S33 piden "decodifica
esto" al dominio; si no hay, por CPU.

## Lo que el usuario ve

- **Nada de esto.** Lanza un programa, de DashOS o de Linux, y usa la gráfica. Las ventanas son iguales.
- **Arranque**: la pantalla del firmware hasta que el dominio está listo (objetivo: menos de 2 segundos, con un
  kernel a medida, sin servicios); el compositor pasa entonces a la gráfica sin parpadeo.
- **Si el dominio cae**: el compositor vuelve a pintar por CPU sobre la pantalla del firmware o la última imagen, el
  dominio se relanza, y los programas reciben "dispositivo perdido" (lo que ya prevé Vulkan) y recrean lo suyo.
  DashOS no cae.
- **Energía**: la gráfica dedicada se apaga cuando nadie la usa. Suspender: el dominio suspende sus gráficas.
- **Permisos**: usar la gráfica es una capacidad (S6). Cada programa tiene su contexto y su cupo de memoria de vídeo.

## Lo que no entra

- La Raspberry Pi y las placas ARM con controlador propio: siguen como en S9 y S33.
- PC de menos de 8 GB: sin dominio (pantalla del firmware y CPU), o con un dominio solo para la integrada.
- Un controlador nativo de Intel sigue siendo posible más adelante; con este diseño ya no hace falta.

## Lo que cuesta

| Pieza | Dónde | Tamaño |
|---|---|---|
| IOMMU (X5b) | kernel x86 | Medio |
| Hipervisor de x86: VT-x con EPT, AMD-V con NPT (X10) | kernel x86 | Grande |
| Pasar un aparato PCIe a una máquina virtual, con sus interrupciones (X11) | kernel y monitor | Medio |
| Dominio de gráficos: Linux a medida, servidor (virglrenderer y su lado de contexto nativo), arranque rápido (X12) | zona GPL | Grande |
| Canal y memoria compartida sin copia entre procesos de DashOS y el dominio (X13) | kernel y monitor | Medio |
| Mesa en DashOS con Venus y contexto nativo sobre ese canal (X14) | usuario | Grande (pide la capa POSIX) |
| Compositor: presentar por el dominio, varias gráficas, pantalla completa directa, caída y vuelta (X15) | compositor | Grande |
| CUDA en la caja por reenvío de la interfaz de Nvidia, y el dominio de cómputo (X16) | zona GPL y caja Linux | Grande |
| Vídeo por hardware (X17) | dominio y DataTypes | Medio |

Orden: X5b, X10, X11, X12 con una prueba de CUDA en la caja (X16), X13, X15 (el escritorio acelerado), X14 (los
programas nativos), X17.

## Para decidir el usuario

1. ¿CUDA por reenvío de la interfaz de Nvidia, con la caja Linux como máquina virtual aparte? Recomendado: sí. La
   caja dentro del dominio queda solo como opción con aviso.
2. ¿Dos dominios (pantalla y cómputo) donde hay dos gráficas? Recomendado: sí.
3. ¿Monitor a la integrada (más seguro, una copia) o a la dedicada (sin copia, pero el dominio de cómputo ve la
   pantalla)? Recomendado: a la integrada, y medir la copia.
4. En un PC con una sola gráfica, ¿el dominio va siempre encendido para pintar el escritorio (un Linux mínimo, unos
   cientos de MB)? Recomendado: sí desde 8 GB; con menos, pantalla del firmware y CPU.

## Cómo encaja

S33 (su sección de gráficas en PC se sustituye por esta), S7 y S25 (caja Linux y su GPU), S9 (compositor y Mesa),
S30 (x86: IOMMU e hipervisor), S6 (capacidades), regla 7 de `AGENTS.md` (zona GPL: el dominio vive ahí, entero).
