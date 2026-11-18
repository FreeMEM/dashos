#!/usr/bin/env python3
"""Disco de prueba del NVMe (U6d, FS1): 8 MB.

- Primer MB: lo de la prueba del servicio de disco (U6f): un texto y la firma 0x55AA en el sector 0, otro texto en
  el 1. El cliente de U6f escribe el sector 2. En el sector 0, además, la tabla de particiones (MBR, A14): una sola
  partición, la de DashOS (tipo 0x7F), desde el MB 1 hasta el final.
- Desde el MB 1: un RedoxFS hecho con `dashfs-ar` a partir de una carpeta con LEEME.txt, C/grande.txt (las líneas
  "línea 0", "línea 1"..., unos 300 KB, para que pase por varios registros de 128 KB) y Prefs/fuente. Y un volumen
  cifrado, "Ana", con la contraseña "clave de Ana" (FS10), hecho con `dashfs-volume`, que está al lado de
  `dashfs-ar`; con Argon2 barato, para que abrirlo en QEMU no tarde. Y `Imagenes/disquete.adf`, el ADF de la
  prueba de S20 (`kernel/pruebas/disquete.adf.gz`, hecho con `tools/adf_oraculo.py --disquete`). Y `C/Privado/nota.txt`,
  solo para su dueño (cajón 0700, fichero 0600), para la prueba de permisos de R6 (`docs/REVISION_LOOP.md`).
- Todo, raíces incluidas (la del disco y la de "Ana"), es del usuario 1000, grupo 1000 (`duenos`, en
  archivos/herramientas, y `DASHFS_DUENO` para "Ana"): `dashfs-ar` copiaría el dueño de Linux, que cambia de un
  PC a otro.

Uso: disco_archivos.py SALIDA DASHFS_AR
     disco_archivos.py --sin-cuentas SALIDA DASHFS_AR
     disco_archivos.py --comprobar DISCO VER

Con --sin-cuentas (rama cuentas-2, `make arranque-virt-cuentas`), el mismo disco sin `Estado/Cuentas/usuarios` ni
`grupos` (el cajón, sí, de `sesiones`): un primer arranque, en el que nadie puede entrar hasta crear la primera cuenta.

Con --comprobar, tras la prueba en QEMU, mira con `ver` (archivos/herramientas) que el disco tiene lo que escribió
el cliente `ficheros` (FS4) y nada más: lo escrito por DashOS está de verdad en el disco. Y que el comprobador (FS8)
no encuentra nada roto en él. Y que lo que DashOS escribió en el volumen cifrado no está en claro en el disco (FS10).
"""
import gzip
import os
import re
import struct
import subprocess
import time
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fondos_prueba  # noqa: E402
import repo_paq  # noqa: E402
import iconos_amiga_prueba  # noqa: E402

# U2: un programa en el disco, para que `sesiones` lo lance desde ahí (el mismo `hola` que se construye).
HOLA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "usuario", "target", "aarch64-unknown-none-softfloat", "release", "hola")
# Y `pide`: lanzado desde el disco, nace con su canal con `sesiones` y repite la prueba de U4 con su número nuevo.
PIDE = os.path.join(os.path.dirname(HOLA), "pide")
# U6: `cambio`, la prueba del cambio rápido de usuario (lo lanza `guardian`, como Ana y como Carla).
CAMBIO = os.path.join(os.path.dirname(HOLA), "cambio")
# `RAM:` en memoria: `ram`, su prueba (la lanza `guardian`, como Ana y como Carla).
RAM = os.path.join(os.path.dirname(HOLA), "ram")
# S10: un programa del disco que avisa (lo lanza la barra).
AVISA = os.path.join(os.path.dirname(HOLA), "avisa")
# El selector de ficheros: un programa del disco que guarda con "Guardar como" (lo lanza la barra, al final).
GUARDA = os.path.join(os.path.dirname(HOLA), "guarda")
# Trasfondo: un programa que espera 90 s (lo lanza la barra; el monitor del sistema lo cierra). S6b, S36: abierto
# encerrado, la prueba del encierro y del modo de desarrollo; lanzado desde el Shell, la prueba hostil de la tanda.
ESPERA = os.path.join(os.path.dirname(HOLA), "espera")
# S28: el cajón de la caja Amiga (su `Caja:`), con lo que atacan sus virus de laboratorio (`caja/pruebas68k/malware/`):
# un ejecutable de AmigaOS, su icono, un guion de arranque, una imagen de disco (una pista de un ADF) y un fichero de
# datos. Tras la prueba, todo igual salvo el fichero de datos (sus programas pueden borrar sus datos: van a la papelera
# de la caja), un ejecutable nuevo que deja el virus de enlace (crear sí puede) y `C/viejo`, que actualiza un programa
# honrado con permiso del usuario.
CAJA = "Estado/Servicios/caja-amiga"
# S13: `Prefs/Red/hosts` y `Prefs/Red/hora` (el vecino de la prueba, `tools/red_vecino.py`, da la hora) y `Prefs/zona`.
RED_HOSTS = "# Nombres que se saben sin preguntar a nadie.\n10.0.2.53 hora.prueba vecino.prueba\nfec0::53 vecino.prueba\nesto no es una línea de hosts\n"
RED_HORA = "hora.prueba\n"
ZONA = "Europe/Madrid\n"
SALUDO = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "caja", "pruebas68k", "saludo.exe")
CAJA_ICONO = bytes([0xE3, 0x10, 0, 1]) + bytes(74) + b"saludo\0"
CAJA_GUION = b"; el arranque de la caja Amiga\nsaludo\n"
CAJA_ADF = b"DOS\0" + bytes(11 * 512 - 4)
# Plan de iconos, parte 3: los `.info` de prueba (`tools/iconos_amiga_prueba.py`, uno de cada formato y uno roto) que
# enseña el explorador. El volumen con su `Disk.info` y el cajón `Iconos` con el suyo (los dos se esconden: son de otra
# cosa); dentro, un `.info` suelto por formato (sueltos se ven, cada uno con su icono). Sin los ficheros de cada uno: el
# disco va justo (A15).
_ICONOS = iconos_amiga_prueba.iconos()
CAJA_ICONOS = [("", "Disk.info", _ICONOS["Disk.info"]), ("", "Iconos.info", _ICONOS["Cajon.info"])] + \
    [("Iconos", n + ".info", _ICONOS[n + ".info"]) for n in ("Brillo", "Cristal", "Estrella", "Programa", "Roto")]
# S28: un ejecutable que actualiza `actualiza.exe` (un programa honrado; la guardia pregunta): una cabecera hunk vacía
# con la versión en los 4 últimos bytes. La prueba lo deja en la versión 4 (tres vueltas con permiso).
CAJA_VIEJO = struct.pack(">8I", 0x3F3, 0, 0, 0, 0, 0, 0, 1)
# ramlib: bibliotecas de disco en `Caja:Libs` (su `LIBS:`): una buena, una con el RomTag falso, una que miente con su
# versión (`caja/pruebas68k/prueba_lib.c`) y una que no es un ejecutable.
PRUEBAS68K = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "caja", "pruebas68k")
CAJA_LIBS = [(n, open(os.path.join(PRUEBAS68K, n), "rb").read()) for n in ("prueba.library", "falsa.library", "miente.library")]
CAJA_LIBS.append(("rota.library", b"\x00\x00\x03\xf3 esto no es un ejecutable"))
# S28: la papelera de la caja (la crea el servicio de archivos), con lo que borran el troyano (sus datos, dos ficheros
# con el mismo nombre y uno de nombre largo: `caja/pruebas68k/malware/troyano.c`) y el virus de guiones (`inocente`).
PAPELERA = CAJA + ".papelera"
# S6, S36: el registro común (`Estado:Registros/`, de `sesiones`, 0700), con `programas` ya casi en su tope (64 KB) de
# líneas de "ayer": la primera que apunte un programa lo hace pasar a `programas.1` (rotar, de punta a punta). Lo de
# antes se sigue leyendo con la orden `Registro`.
REGISTROS = "Estado/Registros"
TOPE_REGISTRO = 64 * 1024


def registro_viejo():
    lineas, total, i = [], 0, 0
    while True:
        l = f"2026-01-01T00:{i // 60 % 60:02d}:{i % 60:02d}Z prueba sistema programa línea vieja {i} de la prueba de rotar\n".encode()
        if total + len(l) > TOPE_REGISTRO - 200:
            break
        lineas.append(l)
        total += len(l)
        i += 1
    # La última, rellena hasta 10 bytes del tope: cualquier línea nueva lo pasa.
    falta = TOPE_REGISTRO - 10 - total
    cab = f"2026-01-01T00:59:59Z prueba sistema programa la última de ayer ".encode()
    lineas.append(cab + b"z" * (falta - len(cab) - 1) + b"\n")
    return b"".join(lineas)


REGISTRO_VIEJO = registro_viejo()
# S6b, rama `cuentas`: las cuentas, en `Estado:Cuentas/` (de `sesiones`, usuario 1: el cajón 0755 y los ficheros 0644,
# que leen todos). Ana (administradora) y Carla, como antes en el código. Y, a propósito, líneas que no valen: un número
# repetido, una sin campos, un nombre con letras raras y la última cortada (sin salto de línea): `sesiones` las salta,
# lo dice, y Ana y Carla entran igual.
CUENTAS = "Estado/Cuentas"
CUENTAS_USUARIOS = ("# Cuentas de usuario: nombre:uid:gid:nombre completo:volumen:estado\n"
                    "Ana:1000:1000:Ana:Ana:activa\n"
                    "Carla:1001:1001:Carla:Carla:activa\n"
                    "Intrusa:1000:1000:con el número de Ana:Intrusa:activa\n"
                    "esto no es una cuenta\n"
                    "Pe\u00f1a:1003:1003:letras raras:Pena:activa\n"
                    "Cortada:1004:1004:Cort").encode()
CUENTAS_GRUPOS = b"# Grupos: nombre:gid:miembros\nadmin:10:Ana\n"
# Lo que queda tras la prueba del Shell: Berta, creada, borrada y devuelta; sin el grupo "fotos" (se quedó vacío).
CUENTAS_FIN = (b"# Cuentas de usuario: nombre:uid:gid:nombre completo:volumen:estado\n"
               b"# Lo escribe el sistema (sesiones). Cambiarlo a mano no vale: la orden Usuario del Shell.\n"
               b"Ana:1000:1000:Ana:Ana:activa\nCarla:1001:1001:Carla:Carla:activa\nBerta:1002:1002:Berta Gil:Berta:activa\n")
GRUPOS_FIN = (b"# Grupos: nombre:gid:miembros (separados por comas). admin, los administradores.\n"
              b"# Lo escribe el sistema (sesiones). Cambiarlo a mano no vale: la orden Usuario del Shell.\n"
              b"admin:10:Ana\n")
LARGO = "largo_" + "x" * 84
# Paso 2 de las asignaciones: los cajones de `SYS:` del árbol de S4b ("El árbol de `SYS:`, completo") y los guiones de
# `S`. Los .info (iconos) aún no: el formato de iconos con PNG está por hacer.
SYS_CAJONES = [
    "Classes/DataTypes", "Classes/Gadgets", "Classes/Images", "Classes/MUI",
    "Devs/DataTypes", "Devs/DOSDrivers", "Devs/Keymaps", "Devs/Monitors", "Devs/Printers",
    "Fonts", "L", "Libs",
    "Locale/Catalogs", "Locale/Countries", "Locale/Help", "Locale/Languages",
    "Prefs/Env-Archive/Sys", "Prefs/Presets",
    "Rexx", "S", "S/Shell-Complementos",
    "Storage/DOSDrivers", "Storage/Keymaps", "Storage/Monitors", "Storage/Printers",
    "System", "Tools/Commodities", "Utilities", "WBStartup",
]
# El Shell nativo (docs/bitacoras/shell-nativa.md): el programa, en `System/Shell`; sus órdenes, en `C/` (el `SYS:C` de
# S4b), cada una un programa (`usuario/src/bin/ordenes/`); y el guion de su prueba, `S/Prueba-Shell`, que `guardian`
# le hace ejecutar como Ana: ejercita las órdenes y lo hostil, y deja el disco como estaba.
ORDENES_BIN = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "ordenes", "target", "aarch64-unknown-none-softfloat", "release")
SHELL = os.path.join(ORDENES_BIN, "shell")
# Las órdenes de base van juntas en un programa, `C/Ordenes` (17 órdenes en unos 100 KB): sueltas, cada una lleva su
# copia de lo común y entre todas ocupan 880 KB, que en este disco de 7 MB dejan sin sitio a las pruebas de archivos.
# Con DASHOS_ORDENES_SUELTAS=1 va además cada una en su programa (`C/Dir`, `C/Copy`...), que el Shell prefiere.
ORDENES = os.path.join(ORDENES_BIN, "ordenes")
SUELTAS = ["Abrir", "Administrar", "Ask", "Assign", "Cd", "Copy", "Date", "Delete", "Dir", "Echo", "Filenote", "Info", "List", "MakeDir", "Permiso",
           "Protect", "Registro", "Rename", "Type", "Usuario", "Version", "Wait"] if os.environ.get("DASHOS_ORDENES_SUELTAS") else []


# S8 (rama `paq`): la orden `Paq`, siempre suelta (`C/Paq`): lleva las firmas y el gzip, que no van en `C/Ordenes`.
PAQ = os.path.join(ORDENES_BIN, "orden-paq")
# El cajón de los paquetes de todos (`Paquetes:`), del servicio `paq` (usuario 2): lo demás solo lo lee. Trae lo que
# deja una transacción cortada (un cajón a medio desempaquetar y el cerrojo), que el servicio barre al arrancar.
PAQUETES_UID = "2:2"
PAQUETES_CORTADO = "Paquetes/Paq/Almacen/letras/0-0.9-1.nuevo"


def orden(nombre):
    return os.path.join(ORDENES_BIN, "orden-" + nombre.lower())


PRUEBA_SHELL = "\n".join([
    "; La prueba del Shell nativo: la ejecuta el Shell que lanza guardian como Ana (make arranque-virt).",
    "FailAt 21",
    'Echo "Prueba del Shell nativo"',
    "Cd",
    "MakeDir Home:PruebaShell",
    "Cd Home:PruebaShell",
    "Cd",
    "; Redirecciones, Type, Copy, Rename, Filenote, Protect, List, Dir.",
    'Echo >nota.txt "hola desde el Shell"',
    'Echo >>nota.txt "segunda línea"',
    "Type nota.txt NUMBER",
    "Copy nota.txt copia.txt QUIET",
    "Rename copia.txt TO otra.txt",
    'Filenote otra.txt "una copia"',
    "Protect otra.txt rwd",
    "Protect otra.txt",
    "List >lista.txt otra.txt",
    "Type lista.txt | Type HEX >hex.txt",
    "List #?.txt QUICK NOHEAD",
    "; El resumen de List con un solo fichero (en singular).",
    "MakeDir uno",
    "Copy nota.txt uno QUIET",
    "List uno",
    "Delete uno ALL QUIET",
    "; La fecha, en la zona del sistema (SYS:Prefs/zona, la de la barra).",
    "Date",
    "MakeDir sub",
    "Copy #?.txt sub QUIET",
    "Dir",
    "; Asignaciones: poner, añadir una capa, ver, y lo que no se puede.",
    "Assign Prueba: Home:PruebaShell",
    "Type Prueba:nota.txt",
    "Assign Prueba: SYS:S ADD",
    "Assign Prueba:",
    "List Prueba:#?-Startup QUICK NOHEAD",
    "Assign SYS: Home:PruebaShell",
    "Assign Bucle: Bucle:",
    "Which Dir",
    "Which Set",
    "Which NoEstoy",
    'Echo "Which no lo encuentra: $RC"',
    "; Variables locales y de entorno.",
    "Set nombre Ana",
    'Echo "Hola, $nombre"',
    'SetEnv PruebaShell "de entorno"',
    'Echo "Variable $PruebaShell"',
    "UnsetEnv PruebaShell",
    "; If, Skip y Lab.",
    "If EXISTS nota.txt",
    '  Echo "nota.txt existe"',
    "Else",
    '  Echo "MAL: nota.txt no existe"',
    "EndIf",
    "Set vuelta 1",
    "Lab otra",
    'Echo "vuelta $vuelta"',
    "If $vuelta EQ 1",
    "  Set vuelta 2",
    "  Skip otra BACK",
    "EndIf",
    "; Tuberías y entrada desde un fichero.",
    "Type nota.txt | Type NUMBER",
    'Echo "s" | Ask "¿Sigo?"',
    "If WARN",
    '  Echo "Ask dijo sí"',
    "EndIf",
    'Ask <nota.txt "¿Y ahora?"',
    'Echo "Ask dijo no: $RC"',
    "; Un guion con argumentos (.KEY), llamado por su nombre (con el bit de guion) y con Execute.",
    'Echo >saluda ".KEY quien/A,veces/N"',
    'Echo >>saluda "Echo *"Saludos a <quien> (<veces$1>)*""',
    "Protect saluda s ADD",
    "saluda Carla 3",
    "Execute saluda Ana",
    "Execute saluda",
    'Echo "sin argumento: $RC"',
    "; Colores por tipo en una consola (el registro se queda con las letras del escape); con NOCOLOR, sin ellos.",
    "Dir #? DIRS NOCOLOR",
    "; Lo que cuesta pintar una salida larga: 25.000 líneas sin colores, y 16.384 con colores, fondos y subrayado (un",
    "; fichero de 1.024 líneas, 16 veces). Sin copiarla al registro del sistema, que se llenaría.",
    "Medidas CALLA",
    "Medidas >NIL:",
    "Type SYS:C/grande.txt",
    "Medidas HABLA",
    "Medidas",
    'Echo >a.txt "*e[31mrojo*e[0m y *e[1;44mfondo azul*e[0m y *e[4;38;5;208mnaranja subrayado*e[0m"',
    "Type >b.txt a.txt a.txt a.txt a.txt",
    "Type >a.txt b.txt b.txt b.txt b.txt",
    "Type >b.txt a.txt a.txt a.txt a.txt",
    "Type >a.txt b.txt b.txt b.txt b.txt",
    "Type >b.txt a.txt a.txt a.txt a.txt",
    "Medidas CALLA",
    "Medidas >NIL:",
    "Type b.txt b.txt b.txt b.txt b.txt b.txt b.txt b.txt b.txt b.txt b.txt b.txt b.txt b.txt b.txt b.txt",
    "Medidas HABLA",
    "Medidas",
    "Delete a.txt b.txt QUIET",
    "; Completar: una orden, una ruta, una palabra de la plantilla, un volumen, y con varias posibilidades.",
    "Completar Di",
    "Completar Type no",
    "Completar Copy nota.txt Home: A",
    "Completar Dir SY",
    "Completar Dir S",
    "Completar Type SYS:S/Sh",
    "Completar NoEstoy",
    "; Resaltar: orden que existe, ruta, palabra de la plantilla, redirección, tubería, orden que no existe, comillas abiertas.",
    'Resaltar "Dir Home: ALL >nota.txt | Nada *"con comillas*""',
    'Resaltar "Echo *"sin cerrar"',
    "; Teclear: el historial (que se guarda en ENVARC:Shell/Historial), la sugerencia, Tab con menú, buscar hacia atrás.",
    'Teclea "<CTRL-X>Echo uno del historial<INTRO>"',
    'Teclea "<CTRL-X>Ech"',
    'Teclea "<CTRL-X>Ech<DERECHA>"',
    'Teclea "<CTRL-X>Ec<CTRL-DERECHA>"',
    'Teclea "<CTRL-X>Type no<TAB>"',
    'Teclea "<CTRL-X>Dir S<TAB>"',
    'Teclea "<CTRL-X>Dir S<TAB><TAB><TAB>"',
    'Teclea "<CTRL-X><CTRL-R>hist"',
    'Teclea "<ESC><CTRL-X>Echo<ARRIBA>"',
    'Teclea "<CTRL-X>"',
    "Historial",
    "Type ENVARC:Shell/Historial",
    "; Un historial dañado (una imagen, que no es texto) no rompe el panel nuevo que lo lee. Paneles y pestañas.",
    "Copy SYS:C/Fondos/atardecer.png ENVARC:Shell/Historial QUIET",
    "Panel VERTICAL",
    "Panel PESTANA",
    "; El indicador: sus ajustes, con una línea que no se entiende.",
    'Echo >ENVARC:Shell/Indicador "tema sobrio"',
    'Echo >>ENVARC:Shell/Indicador "color cajon fucsia"',
    "Panel",
    "; Complementos: el de serie (SYS:S/Shell-Complementos/alias) ya está cargado en este Shell.",
    "Alias",
    "; Y en otro Shell: uno que pone un alias, uno que falla (se salta) y uno que da una plantilla.",
    "If NOT EXISTS Home:S",
    "  MakeDir Home:S",
    "  Set hice_s si",
    "EndIf",
    "MakeDir Home:S/Shell-Complementos",
    'Echo >Home:S/Shell-Complementos/10-alias "Alias saludo Echo hola desde un alias"',
    'Echo >Home:S/Shell-Complementos/20-roto "NoExisteEstaOrden"',
    'Echo >>Home:S/Shell-Complementos/20-roto "Echo esto no llega"',
    'Echo >Home:S/Shell-Complementos/30-completa "Completa saluda quien/A,veces/N,GRITANDO/S"',
    "SYS:System/Shell ORDEN saludo",
    "SYS:System/Shell ORDEN Completar saluda Ana G",
    "Delete Home:S/Shell-Complementos ALL QUIET",
    "If $hice_s EQ si",
    "  Delete Home:S QUIET",
    "EndIf",
    "Delete ENVARC:Shell ALL QUIET",
    "; Tuberías de verdad: las dos órdenes a la vez. 313 KB pasan por un tubo de 64 KB (salen 463.890 bytes numerados,",
    "; que se cuentan sin copiarlos al registro); y si la que lee acaba antes, la que escribe acaba también.",
    "Medidas CALLA",
    "Medidas >NIL:",
    "Type SYS:C/grande.txt | Type NUMBER",
    "Medidas HABLA",
    "Medidas",
    'Type SYS:C/grande.txt | Ask "¿La primera?"',
    'Echo "tubería cortada: $RC"',
    "; Sustitución de órdenes: lo que va entre comillas invertidas se cambia por lo que escribe.",
    'Echo "Dir está en `Which Dir` y estoy en `Cd`"',
    "Echo `sin cerrar",
    "; Lo hostil. Comillas sin cerrar, una orden que no existe, una línea larguísima.",
    'Echo "sin cerrar',
    "NoExiste x",
    "Echo " + "x" * 3000,
    'Echo "tras la línea larga: $RC"',
    "; Escribir en SYS:, que es de solo lectura: redirigir, copiar, crear y borrar con un patrón.",
    "Echo >SYS:prohibido hola",
    "Copy nota.txt SYS:S QUIET",
    "MakeDir SYS:S/Nuevo",
    "Delete SYS:S/#?-Startup ALL",
    "Delete SYS:Prefs ALL QUIET",
    'Echo "Delete en SYS: $RC"',
    "; Lo de otra persona: ni leerlo ni lanzarlo. Y lanzar lo que no es un programa.",
    "Type SYS:DeCarla/carta.txt",
    "SYS:DeCarla/carta.txt",
    "SYS:LEEME.txt",
    "Copy SYS:LEEME.txt falso QUIET",
    "Protect falso rwed",
    "falso",
    "; Un patrón que recorre todo SYS: (cientos de nombres), a un fichero.",
    "List >todo.txt SYS:#?/#? QUICK NOHEAD",
    'Echo "patrón por todo SYS: $RC"',
    "; Un guion con un bucle sin fin, de fondo, y Break. Y una orden que tarda.",
    'Echo >bucle "Lab otra"',
    'Echo >>bucle "Skip otra BACK"',
    "Run >salida.txt Execute bucle",
    "Wait 1",
    "Break 1",
    "Wait 2",
    "Run >salida.txt Wait 100",
    "Break 1",
    "Wait 1",
    "Status",
    "; Ajustes en texto (docs/bitacoras/ajustes-texto.md): la orden Ajuste escribe el fichero de texto de ENV: (con",
    "; GUARDAR, también el de ENVARC:), y el centro de control lo aplica en marcha; uno editado a mano, igual. Lo hostil:",
    "; valores fuera de rango, nombres que no valen, lo de la máquina, un fichero roto y uno enorme.",
    "Ajuste LISTA",
    'Ajuste idioma "english, español"',
    "Wait 1",
    "Ajuste idioma",
    "Type ENV:Sys/locale",
    "Ajuste pais atlantida",
    'Ajuste idioma "../x"',
    "Ajuste tema morado",
    "Ajuste zona Europe/London",
    "Ajuste zona",
    "Ajuste nada",
    "Copy LOCALE:Countries/españa.country ENV:roto QUIET",
    "Ajuste roto.x",
    "Ajuste roto.x 1",
    "Copy SYS:C/frio.bin ENV:enorme QUIET",
    "Ajuste enorme.x 1",
    "Delete ENV:roto ENV:enorme QUIET",
    "Ajuste Escritorio/barra.segundos sí GUARDAR",
    "Ajuste Escritorio/barra.segundos",
    "Ajuste tema oscuro",
    "Wait 1",
    'Echo >ENV:Escritorio/apariencia "modo = claro"',
    "Wait 1",
    "Ajuste tema oscuro",
    "Wait 1",
    "Ajuste idioma español",
    "Wait 1",
    "; S27, oleada 3: los avisos y el centro de control, a mano: el centro los aplica en marcha (a la barra y al panel).",
    "Ajuste Escritorio/avisos.posicion abajo-derecha",
    "Wait 1",
    'Ajuste Escritorio/centro-control.mosaicos "usuario 8, volumen 4, red 4, editar 1 pequeño"',
    "Wait 1",
    "Ajuste Escritorio/avisos.posicion arriba-centro",
    "Delete ENV:Escritorio/centro-control QUIET",
    "Wait 1",
    "; S10, los menús de los programas (docs/bitacoras/menus.md): Ajuste menus, que el centro aplica en marcha (el",
    "; compositor lo dice a todos). Lo hostil: un sitio que no existe, y cien cambios seguidos a mano: queda el último.",
    "Ajuste menus arriba",
    "Ajuste menus barra",
    "Wait 1",
    "Ajuste menus",
    *['Echo >ENV:Escritorio/menus "sitio = ' + ("barra" if k % 2 == 0 else "dentro") + '"' for k in range(100)],
    "Wait 2",
    "Ajuste menus",
    "; La barra de DMS (docs/bitacoras/dms-barra.md): un cambio que no mueve nada (el reloj, con la fecha delante), que la",
    "; barra aplica en marcha por el centro; y el importador de settings.json, con un fichero que no es JSON y otro que",
    "; no es de DMS (no se cambia nada). Al acabar, sin el fichero: la barra de serie otra vez.",
    'Ajuste Escritorio/barra.principal-centro "reloj(orden=fecha)"',
    "Wait 1",
    "Ajuste DMS SYS:C/frio.bin",
    "Ajuste DMS LOCALE:Countries/españa.country",
    "Delete ENV:Escritorio/barra QUIET",
    "Wait 1",
    "Delete ENVARC:Escritorio ALL QUIET",
    "; S10, Commodities: Exchange como orden (el alias de serie, a SYS:Tools/Commodities/Exchange). Ana no tiene ninguna",
    "; en marcha (la de la prueba es del sistema): la lista vacía, los ajustes, y una que no existe.",
    "Exchange LISTA",
    "Exchange QUITAR NoExiste",
    "; FKey sin Home:Prefs/FKey: su tecla de serie, sin ponerla (LISTA no pide el permiso).",
    "SYS:Tools/Commodities/FKey LISTA",
    "; S6, S36: el registro común (Estado:Registros/). Lo que apunta un guion lleva el programa que lo lanza (el Shell),",
    "; y la hora y el usuario los pone sesiones.",
    'Registro APUNTA "hola desde el guion del Shell" MOTIVO "una prueba"',
    "Registro PROGRAMA Shell CLASE programa ULTIMAS 1",
    "; Lo de antes de rotar (programas.1) se sigue leyendo; lo del hostil (usuario de Carla) Ana no lo ve.",
    "Registro DE programas PROGRAMA prueba ULTIMAS 1",
    "Registro PROGRAMA hostil",
    "; Lo hostil: escapes y saltos de línea, una línea larga, más deprisa de lo que se deja, y tocar los ficheros a mano.",
    'Registro APUNTA "*e[2J*e[31mrojo*Nfalsa" MOTIVO "*e]0;titulo"',
    "Registro PROGRAMA Shell ULTIMAS 1",
    'Registro APUNTA "' + "larga " * 60 + '"',
    "Registro APUNTA rafaga VECES 60",
    'Echo "deprisa: $RC"',
    "Wait 2",
    'Registro APUNTA "tras la ráfaga"',
    "Registro CLASE registro PROGRAMA Shell ULTIMAS 1",
    "Echo >>SYS:Estado/Registros/seguridad hola",
    "Delete SYS:Estado/Registros/seguridad",
    "Type SYS:Estado/Registros/seguridad",
    "Registro CLASE inventada",
    "; Leer el de todos pide la contraseña de un administrador (en la prueba la da guardian, como Ana).",
    "Registro TODO PROGRAMA hostil CLASE permiso ULTIMAS 5",
    "; Ajustes en texto: lo que se negó a `ram` (de Carla) al pedir guardar ENV: en ENVARC: (GUARDAR_ENV).",
    "Registro TODO PROGRAMA ram CLASE ajustes ULTIMAS 2",
    "; En vivo: lo que llega mientras mira.",
    "Run >seguir.txt Registro SEGUIR PROGRAMA Shell CLASE programa ULTIMAS 0",
    "Wait 2",
    'Registro APUNTA "una línea en vivo"',
    "Wait 3",
    "Break 1",
    "Wait 1",
    "Type seguir.txt",
    "; S6b, S36: los permisos, en Home:Prefs/Permisos/. Verlos es libre; dar y quitar los confirma el usuario en la",
    "; ventana del sistema (en la prueba, el guion de QEMU: a pide, que sí; a otro, que no).",
    "Permiso",
    "Permiso DAR pide eco",
    "Permiso",
    "Type Home:Prefs/Permisos/concedidos",
    "Permiso DAR hostil eco",
    'Echo "dar a hostil: $RC"',
    "Permiso DAR pide inventado",
    "; Lo hostil: escribir el fichero a mano, borrarlo, o poner otro cajón en su sitio: no se lee ni se escribe ahí.",
    'Echo >>Home:Prefs/Permisos/concedidos "hostil eco"',
    "Delete Home:Prefs/Permisos/concedidos",
    "Rename Home:Prefs/Permisos TO Home:Prefs/Permisos.viejo",
    "MakeDir Home:Prefs/Permisos",
    'Echo >Home:Prefs/Permisos/concedidos "hostil eco"',
    "Permiso",
    "Permiso DAR pide plazo",
    "Delete Home:Prefs/Permisos ALL QUIET",
    "Rename Home:Prefs/Permisos.viejo TO Home:Prefs/Permisos",
    "Permiso QUITAR pide plazo",
    "Permiso QUITAR pide eco",
    "Type Home:Prefs/Permisos/concedidos",
    "; S36, decisión 1: la tanda de acciones de administrador. Una tanda es una acción: la ventana del sistema enseña",
    "; la lista entera, en llano, y pide la contraseña una vez (en la prueba la teclea el guion de QEMU). Lo que no es de",
    "; administrador no se enseña ni se hace.",
    "Administrar bloquear registro",
    "Administrar comprobar registro",
    "; Lo hostil (SYS:Programas/espera, lanzado desde el Shell): listas malas y enormes, otra a la vez, cambiar la lista",
    "; tras mandarla, colar otra acción detrás, una que falla en medio y la contraseña mala.",
    "SYS:Programas/espera",
    "; S6b, S36: abrir encerrado, como con doble clic, y el modo de desarrollo de una aplicación: sin él no carga de",
    "; Home:Libs; con él, sí; y quitado, otra vez no.",
    "If NOT EXISTS Home:Libs",
    "  MakeDir Home:Libs",
    "  Set hice_libs si",
    "EndIf",
    "If NOT EXISTS Home:Datos",
    "  Set hice_datos si",
    "EndIf",
    'Echo >Home:Libs/prueba.library "biblioteca de prueba de Ana"',
    "Abrir espera ESPERAR",
    "Permiso DAR espera desarrollo",
    "Permiso",
    "Abrir Programas:espera ESPERAR",
    "; Lo hostil: el modo de desarrollo escrito a mano, y abrir lo que no es un programa instalado.",
    'Echo >>Home:Prefs/Permisos/concedidos "hostil desarrollo"',
    "Abrir SYS:System/Shell",
    "Abrir Programas:../System/Shell",
    "Abrir noexiste",
    "Permiso QUITAR espera desarrollo",
    "Abrir SYS:Programas/espera ESPERAR",
    "Type Home:Datos/espera/vueltas",
    "Delete Home:Datos/espera ALL QUIET",
    "If $hice_datos EQ si",
    "  Delete Home:Datos QUIET",
    "EndIf",
    "Delete Home:Libs/prueba.library QUIET",
    "If $hice_libs EQ si",
    "  Delete Home:Libs QUIET",
    "EndIf",
    "; S6b, rama cuentas: las cuentas de usuario, de Estado:Cuentas/. Verlas es libre; crear, borrar, devolver y los",
    "; grupos son una tanda de acciones de administrador: la ventana del sistema enseña la lista y pide la contraseña de",
    "; un administrador, y al crear, también la de la cuenta nueva (las teclea el guion), y enseña su código de",
    "; recuperación. Berta: se crea en el grupo fotos, se borra (se puede devolver), se devuelve y sale de fotos.",
    "Usuario",
    'Usuario CREAR Berta "Berta Gil" +fotos',
    "Usuario GRUPOS Berta",
    "Usuario BORRAR Berta",
    "Usuario LISTA",
    "Usuario DEVOLVER Berta",
    "Usuario GRUPOS Berta -fotos",
    "Type SYS:Estado/Cuentas/grupos",
    "; Lo hostil, que no llega a preguntar: un nombre repetido (con otras mayúsculas), con dos puntos, con un salto de",
    "; línea, largo o del sistema; un número ya usado; la cuenta propia; la última administradora; una que no existe; dos",
    "; cuentas nuevas en una tanda; y una tanda que cuenta con la de antes (Eva no existe aún: no se puede meter en admin).",
    "Usuario CREAR berta",
    'Usuario CREAR "a:b"',
    'Usuario CREAR "Eva*Nfalsa"',
    "Usuario CREAR " + "x" * 40,
    "Usuario CREAR SYS",
    "Usuario CREAR Dora UID 1001",
    "Usuario BORRAR Ana",
    "Usuario GRUPOS Ana -admin",
    "Usuario BORRAR Nadie",
    'Administrar "crear Eva 0" "crear Gil 0"',
    'Administrar "meter Eva admin" "crear Eva 0"',
    "; Escribir las cuentas a mano, borrarlas o dejar otra cosa en su cajón: no.",
    'Echo >>SYS:Estado/Cuentas/usuarios "Intrusa:1009:1009:x:Intrusa:activa"',
    "Delete SYS:Estado/Cuentas/usuarios",
    "Echo >SYS:Estado/Cuentas/otra hola",
    "; Con la contraseña mala de un administrador, con la buena de quien no lo es (Carla) y con una contraseña corta para",
    "; la cuenta nueva: no se crea Dora.",
    "Usuario CREAR Dora",
    "Usuario CREAR Dora",
    "Usuario CREAR Dora",
    "Usuario",
    "; Rama cuentas-2: cambiar la propia contraseña (la ventana del sistema pide la de ahora y la nueva dos veces; esta",
    "; orden no ve ninguna). La de ahora mal y luego bien; de vuelta a la de siempre; dos nuevas distintas y Esc: no se",
    "; cambia. La de otra cuenta no llega a preguntarse.",
    "Usuario CLAVE",
    "Usuario CLAVE Ana",
    "Usuario CLAVE",
    "Usuario CLAVE Carla",
    "; S8 (rama paq): Paq. Los repositorios y las claves (texto en SYS:Estado/Paq), buscar e instalar para todos: el",
    "; plan es una tanda de acciones de administrador (la ventana del sistema lo enseña entero y pide la contraseña una",
    "; vez; la teclea el guion) y lo hace el servicio paq. Actualizar (el repositorio publica saludo 1.1: se copia su",
    "; índice nuevo), volver a la generación de antes, quitar (se lleva letras, que ya no hace falta) y limpiar.",
    "Paq Repos",
    "Paq Claves",
    "Paq Buscar salu",
    "Paq Instalar saludo",
    "Execute Paquetes:Saludo/Saludo",
    "Paq Info saludo",
    "Copy SYS:Work/Repos/prueba-2/#? Work:Repos/prueba/aarch64-dashos QUIET",
    "Paq Actualizar",
    "Execute Paquetes:Saludo/Saludo",
    "Paq Volver",
    "Execute Paquetes:Saludo/Saludo",
    "Paq Quitar saludo",
    "If EXISTS Paquetes:Saludo",
    '  Echo "MAL: queda Paquetes:Saludo"',
    "Else",
    '  Echo "Paquetes:Saludo quitado"',
    "EndIf",
    "Paq Generaciones",
    "Paq Limpiar",
    "Paq Generaciones",
    "; Lo de cada uno (Home:Paquetes), sin administrador: lo hace la orden, con los permisos de Ana. Con alias en inglés.",
    "Paq install saludo HOME",
    "Execute Home:Paquetes/Saludo/Saludo",
    "Paq in circulo-a HOME",
    "Paq gen HOME",
    "Paq rm saludo circulo-a HOME",
    "; Lo hostil: rutas que se salen (.. y /), un enlace, escribir en el cajón de otro paquete, un paquete cambiado",
    "; tras firmar el índice, uno que pide lo que no hay, uno de un repositorio mal firmado; nada se instala.",
    "Paq Instalar fuera HOME",
    "Paq Instalar absoluto HOME",
    "Paq Instalar enlace HOME",
    "Paq Instalar vecino HOME",
    "Paq Instalar cambiado HOME",
    "Paq Instalar imposible HOME",
    "Paq Instalar ajeno HOME",
    "Paq Instalar noexiste",
    "Paq gen HOME",
    "Delete Home:Paquetes ALL QUIET",
    "; Un programa sin permiso (el Shell de Ana) no escribe en Paquetes: ni en lo de Paq; y con la contraseña mala, nada.",
    "Echo >Paquetes:intruso hola",
    "Delete Paquetes:Paq/Generaciones/4",
    "Paq Instalar letras",
    "Paq Generaciones",
    "; Se deja todo como estaba.",
    "Cd Home:",
    "Assign Prueba: REMOVE",
    "Delete Home:PruebaShell ALL QUIET",
    "If EXISTS Home:PruebaShell",
    '  Echo "MAL: queda Home:PruebaShell"',
    "Else",
    '  Echo "Home:PruebaShell borrado"',
    "EndIf",
    'Echo "Fin de la prueba del Shell"',
    "EndShell",
    "",
]).encode()

SYS_FICHEROS = [
    ("S/Startup-Sequence", b"; El arranque de la maquina (S14): lo hace el sistema antes de que entre nadie.\n"),
    ("S/Shell-Startup", b"; Lo que hace cada Shell al abrirse.\n"),
    ("S/User-Startup", b""),
    # `ENVARC:` de fábrica: lo que `sesiones` copia a `ENV:` (en el `RAM:` de la sesión) al entrar; encima va lo del
    # usuario (`Home:Prefs/Env-Archive`). Lo mira la prueba `ram`.
    ("Prefs/Env-Archive/Sys/idioma", b"es\n"),
    ("Prefs/Env-Archive/Sys/teclado", b"es\n"),
    # S27, oleada 3: el centro de control de fábrica, roto (un mosaico repetido y otro que no existe; la basura, en las
    # pruebas de PC): el centro se queda el de serie (el del escritorio de shiva) hasta que el panel o Ajustes guarden
    # uno en el `ENVARC:` de Ana.
    ("Prefs/Env-Archive/Escritorio/centro-control", b"# Roto a proposito (la prueba): se queda el de serie.\nmosaicos = volumen 4, volumen 4, nube 2\n"),
    # Un complemento de serie del Shell: unos alias. Los de cada usuario van en su `Home:S/Shell-Complementos`.
    ("S/Shell-Complementos/alias", "; Alias de serie del Shell. Los tuyos, en Home:S/Shell-Complementos.\nAlias ll List\nAlias la Dir ALL\nAlias md MakeDir\nAlias Exchange SYS:Tools/Commodities/Exchange\n".encode()),
    ("S/Prueba-Shell", PRUEBA_SHELL),
    # La barra de DMS (docs/bitacoras/dms-barra.md): la barra del escritorio del usuario en shiva (sus barConfigs de
    # DMS, sin nada que no sea de la barra), para `Ajuste DMS SYS:Prefs/Presets/DMS/shiva.json`.
    ("Prefs/Presets/DMS/shiva.json", open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "usuario", "pruebas", "dms-barra-shiva.json"), "rb").read()),
    # Rama `fondos`: los fondos del sistema (`SYS:Fondos`), hechos aquí: el atardecer sobre el mar con la paleta
    # animada ("color cycling", 8 KB). Los fondos animados que no son imágenes no ocupan disco (`animado:aurora`).
    ("Fondos/mar.png", fondos_prueba.paleta_mar()),
]


# Idiomas (paso 4 de docs/REVISION_LOOP.md): el árbol de `LOCALE:` que deja `make catalogos` en build/Locale (catálogos
# en inglés, los textos del inglés y los países), en `Locale/` del disco. Y dos catálogos malos para la prueba hostil
# de `cambio`: rota.catalog (el del kit, cortado por la mitad) y ajena.catalog (el del kit con otro nombre de fichero).
# Y el `locale.prefs` de Carla (inglés, Reino Unido), en su capa de `ENVARC:`, que está en su volumen cifrado
# (`Home:Prefs/Env-Archive/Sys`): de ahí lo copia `sesiones` a su `ENV:` (en su `RAM:`, en memoria) al entrar. Ana no
# tiene, y va en castellano.
import catalogo  # noqa: E402
LOCALE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "build", "Locale")
PREFS_CARLA = "Prefs/Env-Archive/Sys/locale.prefs"
APARIENCIA_ANA = "Prefs/Env-Archive/Escritorio/apariencia"


def locale_ficheros():
    """[(camino en el disco, bytes)] de lo de idiomas."""
    ficheros = []
    for carpeta, _, nombres in os.walk(LOCALE):
        for n in nombres:
            camino = os.path.join(carpeta, n)
            ficheros.append(("Locale/" + os.path.relpath(camino, LOCALE).replace(os.sep, "/"), open(camino, "rb").read()))
    kit = dict(ficheros).get("Locale/Catalogs/english/kit.catalog")
    if kit is None:
        sys.exit("disco de pruebas: falta build/Locale (make -C kernel catalogos)")
    ficheros.append(("Locale/Catalogs/english/rota.catalog", kit[: len(kit) // 2]))
    ficheros.append(("Locale/Catalogs/english/ajena.catalog", kit))
    return sorted(ficheros)


def prefs_carla():
    """El `locale.prefs` de Carla: inglés y Reino Unido, con el país que dejó `make catalogos`."""
    gb = dict(locale_ficheros())["Locale/Countries/great_britain.country"]
    return catalogo.hacer_prefs("great_britain", ["english"], 0, gb)


def commodities_ficheros():
    """S10, Commodities (`docs/bitacoras/commodities.md`): las clásicas de `SYS:Tools/Commodities` (Blanker, FKey,
    NoCapsLock) y Exchange, como programas (nombre en el disco y fichero). Se leen al construir: no hacen falta para
    importar este fichero."""
    usuario = os.path.dirname(HOLA)
    return [
        ("Tools/Commodities/Blanker", os.path.join(usuario, "blanker")),
        ("Tools/Commodities/NoCapsLock", os.path.join(usuario, "nocapslock")),
        ("Tools/Commodities/FKey", os.path.join(ORDENES_BIN, "fkey")),
        ("Tools/Commodities/Exchange", os.path.join(ORDENES_BIN, "exchange")),
    ]


def sys_esperado():
    """Las líneas del árbol de `SYS:` en el listado de `ver` (los cajones con sus padres, y los ficheros)."""
    lineas = set()
    for c in SYS_CAJONES:
        partes = c.split("/")
        for i in range(1, len(partes) + 1):
            lineas.add("/".join(partes[:i]) + "/ cajón")
    for n, f in commodities_ficheros():
        lineas.add(f"{n} {os.path.getsize(f)}")
    for n, d in SYS_FICHEROS + locale_ficheros():
        lineas.add(f"{n} {len(d)}")
        partes = n.split("/")[:-1]
        for i in range(1, len(partes) + 1):
            lineas.add("/".join(partes[:i]) + "/ cajón")
    return lineas


DISQUETE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "kernel", "pruebas", "disquete.adf.gz")
# kit-c: el catálogo (UTF-8) de la prueba de las ventanas de Dash, si lo dejó `make kit-c` (hace falta Dash).
CATALOGO_DASH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "build", "DashVentana.catalog")
# Su cajón: la raíz de su ficha en `inicio` (su LOCALE: es `Locale/` dentro).
DASH_VENTANA = "Estado/Servicios/dash-ventana"

if sys.argv[1] == "--comprobar":
    disco, ver = sys.argv[2], sys.argv[3]
    FONDOS = os.path.join(os.path.dirname(os.path.abspath(disco)), "fondos-prueba")
    # Por la tabla de particiones, con el mismo lector que el servicio de disco (A14).
    desde = "tabla"
    arbol = subprocess.run([ver, disco, desde], check=True, capture_output=True, text=True).stdout.splitlines()
    fondo = subprocess.run([ver, disco, desde, "C/Prefs/fondo"], capture_output=True).stdout
    esperado = [
        "C/ cajón",
        # El Shell nativo: sus órdenes, en SYS:C, y el programa, en SYS:System.
        f"C/Ordenes {os.path.getsize(ORDENES)}",
        f"C/Paq {os.path.getsize(PAQ)}",
        *[f"C/{n} {os.path.getsize(orden(n))}" for n in SUELTAS],
        f"System/Shell {os.path.getsize(SHELL)}",
        # El explorador de archivos: su papelera en Home (vacía al acabar: lo tirado se restauró), la carpeta que crea
        # (Viaje, con grande.txt copiado y renombrado), y datos.bin copiado ahí y arrastrado luego a Privado.
        "C/.Papelera/ cajón",
        "C/.Papelera/archivos/ cajón",
        "C/.Papelera/info/ cajón",
        # C3: los fondos de escritorio de prueba (`tools/fondos_prueba.py`).
        "C/Fondos/ cajón",
        *[f"C/Fondos/{n} {os.path.getsize(os.path.join(FONDOS, n))}" for n in sorted(fondos_prueba.FONDOS)],
        *([f"C/Fondos/{os.path.basename(os.environ['DASHOS_FONDO_EXTRA'])} {os.path.getsize(os.environ['DASHOS_FONDO_EXTRA'])}"] if os.environ.get("DASHOS_FONDO_EXTRA") else []),
        "C/Prefs/ cajón",
        # Ajustes en texto: la apariencia ya no va aquí, sino en ENV:/ENVARC:Escritorio/apariencia (un solo sitio): en
        # C/Prefs no tiene que estar.
        # La escala de las ventanas Amiga, que pone el guion con la fila de Apariencia: "general dashos" y
        # "escala amiga 1".
        "C/Prefs/cajas 48",
        # C3: el fondo que se quedó, el último que se puso desde el centro.
        "C/Prefs/fondo 14",
        "C/Privado/ cajón",
        # S10: el comentario y el bit de archivado que puso el Workbench (Información, como Ana).
        "C/Privado/datos.bin 614400",
        "C/Privado/nota.txt 18 bits=0x1 : nota de Ana",
        "C/Viaje/ cajón",
        "C/Viaje/texto grande.txt 313890",
        "C/datos.bin 614400",
        "C/frio.bin 1048576",
        "C/grande.txt 313890",
        # El selector de ficheros: lo que `guarda` guardó con "Guardar como" y sustituyó después (queda lo segundo).
        "C/informe.txt 17",
        "C/raro|nombre.txt 14",
        # S10: el cajón de otra persona (Carla, 0700), que el Workbench de Ana no lista ni abre.
        "DeCarla/ cajón",
        "DeCarla/carta.txt 17",
        "Escrito/ cajón",
        "Escrito/hola.txt 19 bits=0x4 : Saludo de DashOS, ñandú",
        "Estado/ cajón",
        # `RAM:` ya no está en el disco: es un volumen en memoria por sesión (nada de `Estado/RAM`).
        "Estado/Servicios/ cajón",
        f"{CAJA}/ cajón",
        f"{CAJA}/C/ cajón",
        f"{CAJA}/C/saludo {os.path.getsize(SALUDO)}",
        f"{CAJA}/C/saludo.info {len(CAJA_ICONO)}",
        # Lo que crea el virus de enlace al no poder pegarse a C/saludo: un ejecutable nuevo de 32 bytes.
        f"{CAJA}/C/saludo.nuevo 32",
        f"{CAJA}/C/viejo 32",
        f"{CAJA}/Discos/ cajón",
        f"{CAJA}/Discos/disco.adf {len(CAJA_ADF)}",
        f"{CAJA}/Iconos/ cajón",
        *[f"{CAJA}/{s + '/' if s else ''}{n} {len(d)}" for s, n, d in CAJA_ICONOS],
        # T: y ENV: de la caja (los crea al arrancar si faltan).
        f"{CAJA}/Env/ cajón",
        f"{CAJA}/Libs/ cajón",
        *[f"{CAJA}/Libs/{n} {len(d)}" for n, d in sorted(CAJA_LIBS)],
        f"{CAJA}/S/ cajón",
        f"{CAJA}/S/Startup-Sequence {len(CAJA_GUION)}",
        f"{CAJA}/T/ cajón",
        f"{PAPELERA}/ cajón",
        f"{PAPELERA}/basura.txt 1",
        f"{PAPELERA}/basura.txt.1 2",
        f"{PAPELERA}/datos.txt 17",
        # El fichero que el virus de guiones crea, intenta renombrar a User-Startup y borra.
        f"{PAPELERA}/inocente 21",
        f"{PAPELERA}/{LARGO} 3",
        # kit-c: el catálogo de la prueba de las ventanas de Dash, si se compiló (`make kit-c`, con Dash), en su cajón.
        *([] if not os.path.exists(CATALOGO_DASH) else [
            f"{DASH_VENTANA}/ cajón", f"{DASH_VENTANA}/Locale/ cajón", f"{DASH_VENTANA}/Locale/Catalogs/ cajón",
            f"{DASH_VENTANA}/Locale/Catalogs/español/ cajón",
            f"{DASH_VENTANA}/Locale/Catalogs/español/DashVentana.catalog {os.path.getsize(CATALOGO_DASH)}"]),
        "Estado/Servicios/sesiones/ cajón",
        # S6b, S36: lo concedido ya no va en el cajón de `sesiones`: el de los programas del sistema, en la capa de la
        # máquina (el de las personas, en su Home:Prefs/Permisos, cifrado: lo mira el guion del Shell). Su fichero, aparte.
        "Estado/Prefs/ cajón",
        "Estado/Prefs/Permisos/ cajón",
        "Imagenes/ cajón",
        "Imagenes/disquete.adf 901120",
        # S10: un nombre con un tabulador (RedoxFS no admite ":"), que el Workbench no enseña.
        "Imagenes/raro\tdos.txt 14",
        "LEEME.txt 19",
        "Prefs/ cajón",
        # S13: lo que se configura de la red (el servicio de red lo lee) y la zona horaria (la lee la barra).
        "Prefs/Red/ cajón",
        f"Prefs/Red/hora {len(RED_HORA)}",
        f"Prefs/Red/hosts {len(RED_HOSTS.encode())}",
        "Prefs/fuente 8",
        f"Prefs/zona {len(ZONA)}",
        "Programas/ cajón",
        f"Programas/avisa {os.path.getsize(AVISA)}",
        f"Programas/cambio {os.path.getsize(CAMBIO)}",
        f"Programas/espera {os.path.getsize(ESPERA)}",
        f"Programas/guarda {os.path.getsize(GUARDA)}",
        f"Programas/hola {os.path.getsize(HOLA)}",
        f"Programas/panel {os.path.getsize(CAMBIO)}",
        f"Programas/pide {os.path.getsize(PIDE)}",
        f"Programas/ram {os.path.getsize(RAM)}",
        # `Work:` (provisional, un cajón del disco).
        "Work/ cajón",
    ]
    hola = subprocess.run([ver, disco, desde, "Escrito/hola.txt"], check=True, capture_output=True).stdout
    grande = subprocess.run([ver, disco, desde, "C/grande.txt"], check=True, capture_output=True).stdout
    # FS8: y nada roto, ni un bloque perdido.
    comprobado = subprocess.run([ver, disco, desde, "--comprobar"], capture_output=True, text=True).stdout
    fallos = []
    if fondo != b"atardecer.png\n":
        fallos.append(f"C/Prefs/fondo: {fondo!r}")
    # A14: lo que DashOS escribió lleva la hora del reloj de la placa (en QEMU, la de este PC, en UTC): de esta prueba,
    # no de 1970 ni del futuro.
    fecha = subprocess.run([ver, disco, desde, "--fecha", "Escrito/hola.txt"], capture_output=True, text=True).stdout.split()
    ahora = int(time.time())
    # La prueba dura hasta 15 min y su reinicio (`TIEMPO_VIRT`, 900 s desde union-dms) y hola.txt se escribe al
    # principio: con 600 s, una pasada entera ya no cabía (tecla-pegada, la primera que llegó hasta aquí tras la unión).
    if len(fecha) != 2 or not all(ahora - 1800 <= int(f) <= ahora for f in fecha):
        fallos.append(f"Escrito/hola.txt con fecha {fecha} (ahora {ahora})")
    # U6: el registro de seguridad de `sesiones`, en su cajón: la cuenta (8 bytes) y un anillo de entradas de 40 (tipo,
    # estado, quién y qué). Tiene que tener los tres intentos del hostil (matar, la llave de un servicio y entrar).
    REGISTRO = "Estado/Servicios/sesiones/registro"
    # S6, S36: el registro común y lo concedido del sistema miden lo que haya pasado: se miran aparte, más abajo.
    arbol_entero = arbol
    arbol = [l for l in arbol if not l.startswith(REGISTROS + "/") and not l.startswith("Estado/Prefs/Permisos/concedidos ")
             and not l.startswith(CUENTAS + "/")]
    linea = [l for l in arbol if l.startswith(REGISTRO + " ")]
    arbol = [l for l in arbol if not l.startswith(REGISTRO + " ")]
    if not linea:
        fallos.append("sin registro de seguridad en el disco")
    else:
        reg = subprocess.run([ver, disco, desde, REGISTRO], check=True, capture_output=True).stdout
        total = int.from_bytes(reg[:8], "little")
        entradas = [reg[8 + 40 * i : 8 + 40 * (i + 1)] for i in range(min(total, 1024))]
        tipos = {int.from_bytes(e[:4], "little") for e in entradas if e[8:24].rstrip(b"\0") == b"hostil"}
        if not {1, 2, 3} <= tipos:
            fallos.append(f"registro: {total} entradas, las del hostil son de los tipos {sorted(tipos)}")
    # FS10: el nombre y el contenido de lo que DashOS escribió en "Ana" no están en claro.
    crudo = open(disco, "rb").read()
    # U6: y lo que escribió `cambio` como Ana en su volumen, delante y detrás (letras que calcula al correr, para que no
    # estén en claro en el programa, que está en el disco).
    testigo = [bytes(ord("a") + (7 * i + semilla) % 26 for i in range(48)) for semilla in (0, 3)]
    for secreto in (b"secreto.txt", b"solo para Ana", *testigo):
        if secreto in crudo:
            fallos.append(f"{secreto!r} en claro en el disco")
    if comprobado != "sin problemas\n":
        fallos.append(f"comprobación: {comprobado.strip()}")
    # S8 (rama `paq`): lo de Paq se mira aparte (abajo): `Paquetes/`, el repositorio de pruebas y `Estado/Paq`.
    arbol_paq = [l for l in arbol if l.startswith(("Paquetes/", "Work/Repos/", "Estado/Paq/"))]
    arbol = [l for l in arbol if l not in arbol_paq]
    # Paso 2 de la caja Amiga: el AmigaOS del usuario (con DASHOS_AMIGAOS) no es parte de lo que se compara.
    arbol = [l for l in arbol if not l.startswith("Amiga/") and not l.startswith(("C/Prefs/amigaos ", "C/Prefs/programas-amiga "))]
    # Paso 2 de las asignaciones: los cajones de SYS:, en su sitio del orden de `ver` (el de los nombres).
    esperado = sorted(set(esperado) | sys_esperado(), key=lambda l: [p for p in l.split(" ")[0].split("/")])
    if arbol != esperado:
        fallos.append(f"árbol: {arbol}")
    # S6b, S36: lo concedido a los programas del sistema (a `pide`, eco y plazo), retirado al final: solo la cabecera.
    if any(l.startswith("Estado/Servicios/sesiones/permisos") for l in arbol_entero):
        fallos.append("sigue el fichero de permisos en el cajón de sesiones")
    permisos = subprocess.run([ver, disco, desde, "Estado/Prefs/Permisos/concedidos"], capture_output=True).stdout
    if not permisos.startswith(b"# ") or [l for l in permisos.split(b"\n") if l and not l.startswith(b"#")]:
        fallos.append(f"permisos concedidos del sistema: {permisos!r}")
    # S6, S36: el registro común. `programas.1`, lo de "ayer" tal cual (rotó con la primera línea nueva); los tres de
    # ahora, en texto, una línea por hecho, sin letras de control; con lo que se espera de la prueba.
    regs = {l.split(" ")[0][len(REGISTROS) + 1:]: l for l in arbol_entero if l.startswith(REGISTROS + "/")}
    leer_reg = lambda n: subprocess.run([ver, disco, desde, f"{REGISTROS}/{n}"], capture_output=True).stdout
    # `dominio-red` (rama dominio-red, el cuarto fichero del registro común) sale si el dominio de red apuntó algo.
    if not {"", "seguridad", "sistema", "programas", "programas.1"} <= set(regs) <= {"", "seguridad", "sistema", "programas", "programas.1", "dominio-red"}:
        fallos.append(f"Estado/Registros: {sorted(regs)}")
    elif leer_reg("programas.1") != REGISTRO_VIEJO:
        fallos.append("Estado/Registros/programas.1 no es lo de antes de rotar")
    else:
        hora = re.compile(r"^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ|arranque\+\d+\.\d{3}s) \S+ \S+ \S+ .+$")
        texto = {n: leer_reg(n).decode("utf-8", errors="replace") for n in ("seguridad", "sistema", "programas", "dominio-red") if n in regs}
        for n, t in texto.items():
            # Cada línea, de las que da `registro::linea`: como mucho 384 bytes con su salto.
            malas = [l for l in t.split("\n")[:-1] if not hora.match(l) or any(ord(c) < 32 or 0x7f <= ord(c) < 0xa0 for c in l) or len(l.encode()) > 383]
            # (Vacío vale: `dominio-red` se crea con el registro aunque el dominio no apunte nada; los otros tienen
            # sus líneas pedidas más abajo.)
            if malas or (t and not t.endswith("\n")):
                fallos.append(f"Estado/Registros/{n}: líneas que no valen: {malas[:3]}")
        for n, quiere in [("seguridad", " hostil Carla permiso no se da \"eco\""), ("seguridad", " pide sistema permiso se da \"eco\""),
                          ("seguridad", " pide Ana permiso se quita \"eco\""), ("seguridad", "no escribe lo concedido de Ana"),
                          ("sistema", " servicio vuelve: "), ("sistema", " memoria topa con su tope de memoria"),
                          ("programas", " Shell Ana programa hola desde el guion del Shell: una prueba"),
                          ("programas", " Shell Ana programa ?[2J?[31mrojo"), ("programas", " Shell Ana registro "),
                          ("sistema", " caja-amiga Ana guardia niega "), ("sistema", " pide sistema memoria topa con su tope"),
                          ("programas", " Shell Ana programa larga larga "),
                          # S27: el dock solo escribe y guarda su fichero de ajustes; lo ajeno, negado y apuntado.
                          ("seguridad", " dock Ana ajustes no escribe ENV:Escritorio/menus (estado -2)"),
                          ("seguridad", " dock Ana ajustes no guarda ENV:Sys/locale en ENVARC: (estado -2)"),
                          ("seguridad", " dock Ana ajustes no escribe ENV:Prefs/Red/hora (estado -2)")]:
            if quiere not in texto[n]:
                fallos.append(f"Estado/Registros/{n}: falta {quiere!r}")

    # S6b, rama `cuentas`: las cuentas, como las dejó la prueba del Shell (reescritas por `sesiones`: sin las líneas
    # malas), y nada más en su cajón. Y el volumen de Berta abre con su contraseña y con su código de recuperación (el
    # que enseñó `guardian`; en las pruebas lo escribe en la consola), y no con otra: en una copia del disco, con
    # `duenos`, que es lo que abre volúmenes cifrados desde Linux.
    # Rama cuentas-2: `guardian` recupera después a Berta con ese código: su contraseña es "nueva de Berta" y su código,
    # el segundo que escribe; ni la de antes ni el primer código (gastado) la abren.
    cuentas = sorted(l.split(" ")[0] for l in arbol_entero if l.startswith(CUENTAS + "/"))
    if cuentas != [CUENTAS + "/", CUENTAS + "/grupos", CUENTAS + "/usuarios"]:
        fallos.append(f"{CUENTAS}: {cuentas}")
    for n, quiere in (("usuarios", CUENTAS_FIN), ("grupos", GRUPOS_FIN)):
        hay = subprocess.run([ver, disco, desde, f"{CUENTAS}/{n}"], capture_output=True).stdout
        if hay != quiere:
            fallos.append(f"{CUENTAS}/{n}: {hay!r}")
    salida = os.path.join(os.path.dirname(os.path.abspath(disco)), "salida_virt.txt")
    codigo = re.findall(r"el código de recuperación de Berta es ([A-Z0-9-]{29})", open(salida, errors="replace").read()) if os.path.exists(salida) else []
    if len(codigo) != 2 or codigo[0] == codigo[1]:
        fallos.append(f"los códigos de recuperación de Berta en la consola: {codigo}")
    else:
        with tempfile.TemporaryDirectory() as t:
            copia = os.path.join(t, "disco.img")
            with open(copia, "wb") as f:
                f.write(open(disco, "rb").read())
            duenos = os.path.join(os.path.dirname(ver), "duenos")
            for clave, abre in (("nueva de Berta", True), (codigo[1], True), ("clave de Berta", False), (codigo[0], False),
                                ("clave de Ana", False), (codigo[1].replace("-", ""), False)):
                r = subprocess.run([duenos, copia, str(1 << 20), "1002:1002", "Berta"], input=(clave + "\n").encode(), capture_output=True)
                if (r.returncode == 0) != abre:
                    fallos.append(f"el volumen de Berta con {clave!r}: {r.returncode}")

    # S8 (rama `paq`): Paquetes: como lo deja la prueba del Shell. Lo barrido al arrancar (el cajón a medias y el
    # cerrojo), fuera; ningún cajón a la vista (saludo se quitó, con letras); las generaciones 3 (volver: saludo 1.0 y
    # letras) y 4 (vacía), que es la que manda, tras Limpiar; en el almacén, solo lo de la 3 (saludo 1.1 se limpió).
    # Nada fuera de su sitio (el intruso del Shell no está). Y el índice del repositorio, el de la serie 2.
    nombres_paq = sorted(l.split(" ")[0] for l in arbol_paq if l.startswith("Paquetes/"))
    quiere_paq = sorted([
        "Paquetes/", "Paquetes/Paq/", "Paquetes/Paq/Almacen/", "Paquetes/Paq/Almacen/letras/", "Paquetes/Paq/Almacen/letras/0-1.0-1/",
        "Paquetes/Paq/Almacen/letras/0-1.0-1/abc", "Paquetes/Paq/Almacen/letras/0-1.0-1.pkg", "Paquetes/Paq/Almacen/letras/0-1.0-1.readme",
        "Paquetes/Paq/Almacen/saludo/", "Paquetes/Paq/Almacen/saludo/0-1.0-1/", "Paquetes/Paq/Almacen/saludo/0-1.0-1/LEEME",
        "Paquetes/Paq/Almacen/saludo/0-1.0-1/Saludo", "Paquetes/Paq/Almacen/saludo/0-1.0-1.pkg", "Paquetes/Paq/Almacen/saludo/0-1.0-1.readme",
        "Paquetes/Paq/Asignaciones", "Paquetes/Paq/Generaciones/", "Paquetes/Paq/Generaciones/3", "Paquetes/Paq/Generaciones/4",
        "Paquetes/Paq/Series",
    ])
    if nombres_paq != quiere_paq:
        fallos.append(f"Paquetes: {nombres_paq}")
    gen4 = subprocess.run([ver, disco, desde, "Paquetes/Paq/Generaciones/4"], capture_output=True).stdout.decode(errors="replace")
    if not gen4.startswith("PKG-GENERATION 1\nGeneration: 4\n") or "Package:" in gen4:
        fallos.append(f"Paquetes/Paq/Generaciones/4: {gen4!r}")
    gen3 = subprocess.run([ver, disco, desde, "Paquetes/Paq/Generaciones/3"], capture_output=True).stdout.decode(errors="replace")
    if "Package: saludo 1.0-1 prueba explicit\n" not in gen3 or "Package: letras 1.0-1 prueba dependency\n" not in gen3:
        fallos.append(f"Paquetes/Paq/Generaciones/3: {gen3!r}")
    ix = subprocess.run([ver, disco, desde, "Work/Repos/prueba/aarch64-dashos/index.gz"], capture_output=True).stdout
    ix2 = subprocess.run([ver, disco, desde, "Work/Repos/prueba-2/index.gz"], capture_output=True).stdout
    if not ix or ix != ix2:
        fallos.append("Work/Repos/prueba: el índice no es el de la serie 2")

    informe = subprocess.run([ver, disco, desde, "C/informe.txt"], capture_output=True).stdout
    if informe != "segunda versión\n".encode():
        fallos.append(f"C/informe.txt: {informe!r}")
    if hola != b"hola desde DashOS!\n":
        fallos.append(f"Escrito/hola.txt: {hola!r}")
    if grande != "".join(f"línea {i}\n" for i in range(25000)).encode():
        fallos.append("C/grande.txt cambió")
    # A14, TRIM: lo que `ficheros` escribió en "Escrito/descarte.bin", borró y guardó ya no está en el disco (QEMU
    # descarta con discard=unmap). Se buscan 64 bytes de en medio.
    x, trozo = 0x9E3779B97F4A7C15, bytearray()
    for _ in range(128 * 1024 + 64):
        x ^= (x << 13) & 0xFFFFFFFFFFFFFFFF
        x ^= x >> 7
        x ^= (x << 17) & 0xFFFFFFFFFFFFFFFF
        trozo.append(x & 0xFF)
    if bytes(trozo[128 * 1024:]) in crudo:
        fallos.append("Escrito/descarte.bin, borrado y guardado, sigue en el disco")
    # S28: lo que atacaron los virus de laboratorio de la caja Amiga, byte a byte como estaba.
    intactos = [
        (f"{CAJA}/C/saludo", open(SALUDO, "rb").read()),
        (f"{CAJA}/C/saludo.info", CAJA_ICONO),
        (f"{CAJA}/S/Startup-Sequence", CAJA_GUION),
        (f"{CAJA}/Discos/disco.adf", CAJA_ADF),
        ("Imagenes/disquete.adf", gzip.open(DISQUETE).read()),
        ("LEEME.txt", b"DashOS lee RedoxFS\n"),
        ("Programas/hola", open(HOLA, "rb").read()),
    ]
    # S28: lo borrado por la caja, en su papelera tal cual; y lo que actualizó `actualiza.exe` con permiso, tres veces.
    intactos += [(f"{PAPELERA}/datos.txt", b"datos de la caja\n"), (f"{PAPELERA}/basura.txt", b"1"),
                 (f"{PAPELERA}/basura.txt.1", b"12"), (f"{PAPELERA}/{LARGO}", b"123"),
                 (f"{CAJA}/C/viejo", struct.pack(">8I", 0x3F3, 0, 0, 0, 0, 0, 0, 4))]
    for camino, bien in intactos:
        hay = subprocess.run([ver, disco, desde, camino], capture_output=True).stdout
        if hay != bien:
            fallos.append(f"{camino} cambió ({len(hay)} bytes)")
    if fallos:
        print("MAL: el disco tras la prueba:", *fallos, sep="\n  ")
        sys.exit(1)
    print("BIEN: el disco tras la prueba, leído en Linux")
    sys.exit(0)

sin_cuentas = sys.argv[1] == "--sin-cuentas"
if sin_cuentas:
    del sys.argv[1]
salida, ar = sys.argv[1], sys.argv[2]
with tempfile.TemporaryDirectory() as t:
    raiz = os.path.join(t, "raiz")
    os.makedirs(os.path.join(raiz, "C"))
    os.makedirs(os.path.join(raiz, "Prefs"))
    os.makedirs(os.path.join(raiz, "Imagenes"))
    adf = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "kernel", "pruebas", "disquete.adf.gz")
    with open(os.path.join(raiz, "Imagenes", "disquete.adf"), "wb") as f:
        f.write(gzip.open(adf).read())
    with open(os.path.join(raiz, "LEEME.txt"), "w") as f:
        f.write("DashOS lee RedoxFS\n")
    with open(os.path.join(raiz, "C", "grande.txt"), "w") as f:
        f.write("".join(f"línea {i}\n" for i in range(25000)))
    # A7: 600 KB que todos leen, para que `hostil` pida páginas prestadas (dos trozos de 512 KB).
    with open(os.path.join(raiz, "C", "datos.bin"), "wb") as f:
        f.write(bytes((i * 7 + 3) % 251 for i in range(614400)))
    # A14: 1 MB que no se comprime (xorshift, como en `ficheros`), para leerlo en frío con la lectura anticipada.
    x, frio = 0x2545F4914F6CDD1D, bytearray()
    for _ in range(1 << 20):
        x ^= (x << 13) & 0xFFFFFFFFFFFFFFFF
        x ^= x >> 7
        x ^= (x << 17) & 0xFFFFFFFFFFFFFFFF
        frio.append(x & 0xFF)
    with open(os.path.join(raiz, "C", "frio.bin"), "wb") as f:
        f.write(frio)
    with open(os.path.join(raiz, "Prefs", "fuente"), "w") as f:
        f.write("Topaz 8\n")
    # S13: `Prefs/Red` (hosts y el servidor de hora, que es el vecino de la prueba) y la zona horaria.
    os.makedirs(os.path.join(raiz, "Prefs", "Red"))
    for nombre, texto in (("Red/hosts", RED_HOSTS), ("Red/hora", RED_HORA), ("zona", ZONA)):
        with open(os.path.join(raiz, "Prefs", *nombre.split("/")), "w") as f:
            f.write(texto)
    os.makedirs(os.path.join(raiz, "Programas"))
    if os.path.exists(CATALOGO_DASH):
        cat = os.path.join(raiz, *DASH_VENTANA.split("/"), "Locale", "Catalogs", "español")
        os.makedirs(cat)
        with open(os.path.join(cat, "DashVentana.catalog"), "wb") as f:
            f.write(open(CATALOGO_DASH, "rb").read())
    # U6: el cajón de `sesiones` (S6b: `Estado:Servicios/<nombre>`), suyo (usuario 1) y de nadie más.
    os.makedirs(os.path.join(raiz, "Estado", "Servicios", "sesiones"))
    os.chmod(os.path.join(raiz, "Estado", "Servicios", "sesiones"), 0o700)
    # S6, S36: el registro común, con `programas` casi lleno (de `sesiones`, 0700); y `Estado:Prefs`, la capa de la
    # máquina (S4b), donde `sesiones` crea `Permisos` para lo concedido a los programas del sistema.
    os.makedirs(os.path.join(raiz, *REGISTROS.split("/")))
    os.chmod(os.path.join(raiz, *REGISTROS.split("/")), 0o700)
    with open(os.path.join(raiz, *REGISTROS.split("/"), "programas"), "wb") as f:
        f.write(REGISTRO_VIEJO)
    os.makedirs(os.path.join(raiz, "Estado", "Prefs"))
    cuentas = os.path.join(raiz, *CUENTAS.split("/"))
    os.makedirs(cuentas)
    for nombre, datos in (("usuarios", CUENTAS_USUARIOS), ("grupos", CUENTAS_GRUPOS)):
        if sin_cuentas:
            continue
        with open(os.path.join(cuentas, nombre), "wb") as f:
            f.write(datos)
        os.chmod(os.path.join(cuentas, nombre), 0o644)
    os.chmod(cuentas, 0o755)
    # `Work:` (el cajón `Work`), provisional: no hay aún un segundo disco. `RAM:` no está en el disco: es un volumen en
    # memoria que `sesiones` crea al entrar cada usuario.
    os.makedirs(os.path.join(raiz, "Work"))
    # Paso 2 de las asignaciones: la raíz del disco es `SYS:`, con los cajones del árbol de S4b (vacíos donde aún no hay
    # nada; `C` ya existe y guarda, provisional, lo del Home del escritorio). `Prefs` ya existe (con `fuente`).
    for cajon in SYS_CAJONES:
        os.makedirs(os.path.join(raiz, *cajon.split("/")), exist_ok=True)
    for nombre, datos in SYS_FICHEROS + locale_ficheros():
        os.makedirs(os.path.dirname(os.path.join(raiz, *nombre.split("/"))), exist_ok=True)
        with open(os.path.join(raiz, *nombre.split("/")), "wb") as f:
            f.write(datos)
    for nombre, origen in commodities_ficheros():
        with open(os.path.join(raiz, *nombre.split("/")), "wb") as f:
            f.write(open(origen, "rb").read())
        os.chmod(os.path.join(raiz, *nombre.split("/")), 0o755)
    # S28: el cajón de la caja Amiga (de Ana, como todo; solo suyo).
    caja = os.path.join(raiz, *CAJA.split("/"))
    for sub, nombre, datos in [("C", "saludo", open(SALUDO, "rb").read()), ("C", "saludo.info", CAJA_ICONO),
                               ("C", "viejo", CAJA_VIEJO),
                               ("S", "Startup-Sequence", CAJA_GUION), ("Discos", "disco.adf", CAJA_ADF),
                               ("", "datos.txt", b"datos de la caja\n"),
                               *[("Libs", n, d) for n, d in CAJA_LIBS], *CAJA_ICONOS]:
        os.makedirs(os.path.join(caja, sub), exist_ok=True)
        with open(os.path.join(caja, sub, nombre), "wb") as f:
            f.write(datos)
    os.chmod(caja, 0o700)
    # S8 (rama `paq`): el repositorio de pruebas (`tools/repo_paq.py`, en `Work/Repos`), la lista y la clave
    # (`Estado/Paq`), y `Paquetes/` con lo que deja una transacción cortada.
    repo_paq.hacer(raiz)
    os.makedirs(os.path.join(raiz, *PAQUETES_CORTADO.split("/")))
    with open(os.path.join(raiz, *PAQUETES_CORTADO.split("/"), "abc"), "wb") as f:
        f.write(b"a medias\n")
    os.makedirs(os.path.join(raiz, "Paquetes", "Paq", "Generaciones"))
    with open(os.path.join(raiz, "Paquetes", "Paq", "Cerrojo"), "wb") as f:
        f.write(b"tanda de antes de apagar; arranque 0; desde 0\n")
    # El Shell nativo y sus órdenes.
    for destino, origen in [(("System", "Shell"), SHELL), (("C", "Ordenes"), ORDENES), (("C", "Paq"), PAQ)] + [(("C", n), orden(n)) for n in SUELTAS]:
        with open(os.path.join(raiz, *destino), "wb") as f:
            f.write(open(origen, "rb").read())
        os.chmod(os.path.join(raiz, *destino), 0o755)
    with open(os.path.join(raiz, "Programas", "hola"), "wb") as f:
        f.write(open(HOLA, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "hola"), 0o755)
    with open(os.path.join(raiz, "Programas", "pide"), "wb") as f:
        f.write(open(PIDE, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "pide"), 0o755)
    with open(os.path.join(raiz, "Programas", "guarda"), "wb") as f:
        f.write(open(GUARDA, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "guarda"), 0o755)
    with open(os.path.join(raiz, "Programas", "avisa"), "wb") as f:
        f.write(open(AVISA, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "avisa"), 0o755)
    with open(os.path.join(raiz, "Programas", "espera"), "wb") as f:
        f.write(open(ESPERA, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "espera"), 0o755)
    with open(os.path.join(raiz, "Programas", "cambio"), "wb") as f:
        f.write(open(CAMBIO, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "cambio"), 0o755)
    with open(os.path.join(raiz, "Programas", "ram"), "wb") as f:
        f.write(open(RAM, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "ram"), 0o755)
    # U6, paso 2, prueba hostil: una copia de `cambio` con el nombre del panel rápido; lanzada del disco no tiene sus poderes.
    with open(os.path.join(raiz, "Programas", "panel"), "wb") as f:
        f.write(open(CAMBIO, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "panel"), 0o755)
    privado = os.path.join(raiz, "C", "Privado")
    os.makedirs(privado)
    with open(os.path.join(privado, "nota.txt"), "w") as f:
        f.write("privado de prueba\n")
    os.chmod(os.path.join(privado, "nota.txt"), 0o600)
    os.chmod(privado, 0o700)
    # Lo que el centro de control guarda en `Prefs` (el panel y el fondo; la apariencia va en `ENVARC:`, abajo): cajón
    # 0700 y ficheros 0600, que el hostil no ve.
    prefs = os.path.join(raiz, "C", "Prefs")
    os.makedirs(prefs)
    os.chmod(prefs, 0o700)
    # C3: los fondos de escritorio de prueba, hechos aquí (`tools/fondos_prueba.py`, guardados junto al disco para no
    # rehacerlos): ondas.png y atardecer.png, que se ven; rota.png (un byte cambiado: su CRC no cuadra) y enorme.png
    # (40000 × 40000). El guardado es el enorme: el fondo no lo pone y se queda el color liso hasta que el centro pone
    # otro.
    hechos = os.path.join(os.path.dirname(os.path.abspath(salida)), "fondos-prueba")
    fondos_prueba.fondos(hechos)
    os.makedirs(os.path.join(raiz, "C", "Fondos"))
    for n in fondos_prueba.FONDOS:
        with open(os.path.join(raiz, "C", "Fondos", n), "wb") as f:
            f.write(open(os.path.join(hechos, n), "rb").read())
    # S27: con DASHOS_FONDO_EXTRA=imagen.png, un fondo más (para las capturas con el fondo del usuario: nunca en git).
    extra = os.environ.get("DASHOS_FONDO_EXTRA")
    if extra:
        with open(os.path.join(raiz, "C", "Fondos", os.path.basename(extra)), "wb") as f:
            f.write(open(extra, "rb").read())
    with open(os.path.join(prefs, "fondo"), "w") as f:
        f.write("enorme.png\n")
    os.chmod(os.path.join(prefs, "fondo"), 0o600)
    # S26 (`tools/aspecto_caja_linux.py`): con DASHOS_PREFS="fichero=contenido;...", otros Prefs del usuario (el fondo,
    # la apariencia, el aspecto de las cajas) para las capturas del aspecto. "\n" en el contenido es un salto de línea.
    for par in filter(None, os.environ.get("DASHOS_PREFS", "").split(";")):
        nombre, _, contenido = par.partition("=")
        if nombre == "barra":
            # La barra de DMS: un perfil de `usuario/pruebas` (perfil-barra-<nombre>.txt) como la de fábrica
            # (`SYS:Prefs/Env-Archive/Escritorio/barra`), para las capturas.
            escritorio = os.path.join(raiz, "Prefs", "Env-Archive", "Escritorio")
            os.makedirs(escritorio, exist_ok=True)
            perfil = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "usuario", "pruebas", f"perfil-barra-{contenido}.txt")
            with open(os.path.join(escritorio, "barra"), "wb") as f:
                f.write(open(perfil, "rb").read())
            continue
        if nombre == "fondo_de":
            # Un fondo de fuera (para las capturas; nunca en git): se copia a C/Fondos y se pone.
            os.makedirs(os.path.join(raiz, "C", "Fondos"), exist_ok=True)
            destino = os.path.basename(contenido)
            with open(os.path.join(raiz, "C", "Fondos", destino), "wb") as f:
                f.write(open(contenido, "rb").read())
            with open(os.path.join(prefs, "fondo"), "w") as f:
                f.write(destino + "\n")
            continue
        if nombre not in ("apariencia", "fondo", "cajas", "panel"):
            raise SystemExit(f"DASHOS_PREFS: {nombre} no es un Prefs del centro")
        if nombre == "apariencia":
            # Ajustes en texto: la apariencia de fábrica (`SYS:Prefs/Env-Archive/Escritorio/apariencia`), en texto, de
            # "claro fondo" o "oscuro violeta" (modo y acento).
            modo, _, acento = contenido.partition(" ")
            escritorio = os.path.join(raiz, "Prefs", "Env-Archive", "Escritorio")
            os.makedirs(escritorio, exist_ok=True)
            with open(os.path.join(escritorio, "apariencia"), "w") as f:
                f.write(f"modo = {modo}\nacento = {acento.split(' ')[0] or 'fondo'}\n")
            continue
        with open(os.path.join(prefs, nombre), "w") as f:
            f.write(contenido.replace("\\n", "\n") + "\n")
        os.chmod(os.path.join(prefs, nombre), 0o600)
    # S10: el cajón de otra persona, de Carla y solo suyo (el dueño se pone abajo, con `duenos`), y un nombre con un
    # tabulador en Imagenes (RedoxFS no admite ':'): el Workbench de Ana no abre lo primero ni enseña lo segundo. En la
    # raíz y en Imagenes, no en `C`: así no cambia lo que ven el selector y el hostil.
    carla = os.path.join(raiz, "DeCarla")
    os.makedirs(carla)
    with open(os.path.join(carla, "carta.txt"), "w") as f:
        f.write("solo para Carla.\n")
    os.chmod(os.path.join(carla, "carta.txt"), 0o600)
    os.chmod(carla, 0o700)
    with open(os.path.join(raiz, "Imagenes", "raro\tdos.txt"), "w") as f:
        f.write("no se enseña\n")
    # U5: un nombre con '|', que el selector no debe enseñar (separa las filas de su lista y correría las de después).
    with open(os.path.join(raiz, "C", "raro|nombre.txt"), "w") as f:
        f.write("no se enseña\n")
    # La caja Amiga, paso 2: con DASHOS_AMIGAOS=carpeta (la del AmigaOS del usuario, de su equipo: nunca va a git), una
    # copia en Amiga/AmigaOS y su ajuste en C/Prefs/amigaos, y el disco crece lo que haga falta.
    tamano = 7 << 20
    amigaos = os.environ.get("DASHOS_AMIGAOS")
    if amigaos:
        import shutil
        shutil.copytree(os.path.expanduser(amigaos), os.path.join(raiz, "Amiga", "AmigaOS"))
        with open(os.path.join(raiz, "C", "Prefs", "amigaos"), "w") as f:
            f.write("Amiga/AmigaOS\n")
    # Paso 3: con DASHOS_PROGRAMAS_AMIGA=fichero:fichero... (programas de AmigaOS del usuario: demos de MUI, Tachyon...,
    # tampoco van a git), una copia en Amiga/Programas y su lista en C/Prefs/programas-amiga (la caja los ejecuta). Un
    # cajón de la lista se copia también allí (lo que el programa lee de PROGDIR:), sin ejecutarlo.
    programas = [p for p in os.environ.get("DASHOS_PROGRAMAS_AMIGA", "").split(":") if p]
    if programas:
        import shutil
        os.makedirs(os.path.join(raiz, "Amiga", "Programas"), exist_ok=True)
        lista = []
        for p in programas:
            n = os.path.basename(p.rstrip("/"))
            # Un cajón (los iconos de Tachyon, que busca en PROGDIR:iconos/) se copia entero y no se ejecuta.
            if os.path.isdir(os.path.expanduser(p)):
                shutil.copytree(os.path.expanduser(p), os.path.join(raiz, "Amiga", "Programas", n))
                continue
            shutil.copy(os.path.expanduser(p), os.path.join(raiz, "Amiga", "Programas", n))
            lista.append(f"SYS:Amiga/Programas/{n}\n")
        with open(os.path.join(raiz, "C", "Prefs", "programas-amiga"), "w") as f:
            f.write("".join(lista))
    if amigaos or programas:
        ocupado = sum(os.path.getsize(os.path.join(d, n)) for d, _, ns in os.walk(os.path.join(raiz, "Amiga")) for n in ns)
        tamano += (ocupado * 3 // 2 + (4 << 20)) & ~((1 << 20) - 1)
    fs = os.path.join(t, "fs.img")
    with open(fs, "wb") as f:
        f.truncate(tamano)
    subprocess.run([ar, fs, raiz], check=True, stdout=subprocess.DEVNULL)
    volumen = os.path.join(os.path.dirname(ar), "dashfs-volume")
    # La raíz de "Ana", del 1000 al crearla: cambiarla después la reescribiría con la llave del volumen.
    subprocess.run([volumen, fs, "create-encrypted", "Ana", "--test-cost"], input=b"clave de Ana\n", check=True,
                   env={**os.environ, "DASHFS_DUENO": "1000:1000"})
    # U3: "Carla", el volumen de la usuaria 1001, que `sesiones` abre al entrar y cierra al salir.
    subprocess.run([volumen, fs, "create-encrypted", "Carla", "--test-cost"], input=b"clave de Carla\n", check=True,
                   env={**os.environ, "DASHFS_DUENO": "1001:1001"})
    duenos = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "archivos", "herramientas", "target", "release", "duenos")
    subprocess.run([duenos, fs, "0", "1000:1000"], check=True)
    # Idiomas: el `locale.prefs` de Carla, en su volumen (su capa de `ENVARC:`), suyo.
    prefs = os.path.join(t, "locale.prefs")
    with open(prefs, "wb") as f:
        f.write(prefs_carla())
    subprocess.run([os.path.join(os.path.dirname(duenos), "poner"), fs, "0", "Carla", "1001:1001", PREFS_CARLA, prefs],
                   input=b"clave de Carla\n", check=True)
    # Ajustes en texto, hostil: la apariencia guardada de Ana (su `ENVARC:`, en su volumen), puesta a mano y mal (un
    # modo que no existe). Al entrar ella, `sesiones` la copia a su `ENV:` y el centro dice que no se entiende y deja el
    # tema que hay; `Ajuste tema` la arregla (prueba del Shell).
    apariencia_ana = os.path.join(t, "apariencia")
    with open(apariencia_ana, "w") as f:
        f.write("# puesto a mano\nmodo = morado\n")
    subprocess.run([os.path.join(os.path.dirname(duenos), "poner"), fs, "0", "Ana", "1000:1000", APARIENCIA_ANA, apariencia_ana],
                   input=b"clave de Ana\n", check=True)
    subprocess.run([duenos, fs, "0", "1:1", "camino=Estado/Servicios/sesiones"], check=True)
    subprocess.run([duenos, fs, "0", "1:1", "camino=" + REGISTROS], check=True)
    subprocess.run([duenos, fs, "0", "1:1", "camino=" + CUENTAS], check=True)
    subprocess.run([duenos, fs, "0", "1001:1001", "camino=DeCarla"], check=True)
    subprocess.run([duenos, fs, "0", PAQUETES_UID, "camino=Paquetes"], check=True)
    # A10: un daño que la comprobación encuentra y la reparación arregla (los enlaces de LEEME.txt, 7 en vez de 1).
    # El servicio de archivos repara al arrancar; al final, la comprobación desde Linux tiene que salir limpia.
    estropear = os.path.join(os.path.dirname(duenos), "estropear")
    subprocess.run([estropear, fs, "0", "LEEME.txt", "7"], check=True)
    ver = os.path.join(os.path.dirname(duenos), "ver")
    roto = subprocess.run([ver, fs, "0", "--comprobar"], capture_output=True, text=True).stdout
    if roto == "sin problemas\n":
        sys.exit("disco de pruebas: el daño de A10 no se ve al comprobar")
    d = bytearray(1 << 20)
    s0 = b"DashOS: sector 0 del disco NVMe"
    d[: len(s0)] = s0
    d[510:512] = b"\x55\xaa"
    # A14: la entrada 1 de la tabla, la partición de DashOS: tipo 0x7F, desde el sector 2048, lo que ocupe el RedoxFS.
    d[446 + 4] = 0x7F
    d[446 + 8 : 446 + 16] = (2048).to_bytes(4, "little") + (os.path.getsize(fs) // 512).to_bytes(4, "little")
    s1 = b"DashOS: sector 1"
    d[512 : 512 + len(s1)] = s1
    with open(salida, "wb") as f:
        f.write(d)
        f.write(open(fs, "rb").read())
