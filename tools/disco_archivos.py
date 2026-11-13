#!/usr/bin/env python3
"""Disco de prueba del NVMe (U6d, FS1): 8 MB.

- Primer MB: lo de la prueba del servicio de disco (U6f): un texto y la firma 0x55AA en el sector 0, otro texto en
  el 1. El cliente de U6f escribe el sector 2. En el sector 0, además, la tabla de particiones (MBR, A14): una sola
  partición, la de DashOS (tipo 0x7F), desde el MB 1 hasta el final.
- Desde el MB 1: un RedoxFS hecho con `redoxfs-ar` a partir de una carpeta con LEEME.txt, C/grande.txt (las líneas
  "línea 0", "línea 1"..., unos 300 KB, para que pase por varios registros de 128 KB) y Prefs/fuente. Y un volumen
  cifrado, "Ana", con la contraseña "clave de Ana" (FS10), hecho con `redoxfs-volume`, que está al lado de
  `redoxfs-ar`; con Argon2 barato, para que abrirlo en QEMU no tarde. Y `Imagenes/disquete.adf`, el ADF de la
  prueba de S20 (`kernel/pruebas/disquete.adf.gz`, hecho con `tools/adf_oraculo.py --disquete`). Y `C/Privado/nota.txt`,
  solo para su dueño (cajón 0700, fichero 0600), para la prueba de permisos de R6 (`docs/REVISION_LOOP.md`).
- Todo, raíces incluidas (la del disco y la de "Ana"), es del usuario 1000, grupo 1000 (`duenos`, en
  archivos/herramientas, y `REDOXFS_DUENO` para "Ana"): `redoxfs-ar` copiaría el dueño de Linux, que cambia de un
  PC a otro.

Uso: disco_archivos.py SALIDA REDOXFS_AR
     disco_archivos.py --comprobar DISCO VER

Con --comprobar, tras la prueba en QEMU, mira con `ver` (archivos/herramientas) que el disco tiene lo que escribió
el cliente `ficheros` (FS4) y nada más: lo escrito por DashOS está de verdad en el disco. Y que el comprobador (FS8)
no encuentra nada roto en él. Y que lo que DashOS escribió en el volumen cifrado no está en claro en el disco (FS10).
"""
import gzip
import os
import struct
import subprocess
import time
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fondos_prueba  # noqa: E402

# U2: un programa en el disco, para que `sesiones` lo lance desde ahí (el mismo `hola` que se construye).
HOLA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "usuario", "target", "aarch64-unknown-none-softfloat", "release", "hola")
# Y `pide`: lanzado desde el disco, nace con su canal con `sesiones` y repite la prueba de U4 con su número nuevo.
PIDE = os.path.join(os.path.dirname(HOLA), "pide")
# U6: `cambio`, la prueba del cambio rápido de usuario (lo lanza `guardian`, como Ana y como Carla).
CAMBIO = os.path.join(os.path.dirname(HOLA), "cambio")
# S10: un programa del disco que avisa (lo lanza la barra).
AVISA = os.path.join(os.path.dirname(HOLA), "avisa")
# S28: el cajón de la caja Amiga (su `Caja:`), con lo que atacan sus virus de laboratorio (`caja/pruebas68k/malware/`):
# un ejecutable de AmigaOS, su icono, un guion de arranque, una imagen de disco (una pista de un ADF) y un fichero de
# datos. Tras la prueba, todo igual salvo el fichero de datos (sus programas pueden borrar sus datos: van a la papelera
# de la caja), un ejecutable nuevo que deja el virus de enlace (crear sí puede) y `C/viejo`, que actualiza un programa
# honrado con permiso del usuario.
CAJA = "Estado/Servicios/caja-amiga"
SALUDO = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "caja", "pruebas68k", "saludo.exe")
CAJA_ICONO = bytes([0xE3, 0x10, 0, 1]) + bytes(74) + b"saludo\0"
CAJA_GUION = b"; el arranque de la caja Amiga\nsaludo\n"
CAJA_ADF = b"DOS\0" + bytes(11 * 512 - 4)
# S28: un ejecutable que actualiza `actualiza.exe` (un programa honrado; la guardia pregunta): una cabecera hunk vacía
# con la versión en los 4 últimos bytes. La prueba lo deja en la versión 4 (tres vueltas con permiso).
CAJA_VIEJO = struct.pack(">8I", 0x3F3, 0, 0, 0, 0, 0, 0, 1)
# S28: la papelera de la caja (la crea el servicio de archivos), con lo que borran el troyano (sus datos, dos ficheros
# con el mismo nombre y uno de nombre largo: `caja/pruebas68k/malware/troyano.c`) y el virus de guiones (`inocente`).
PAPELERA = CAJA + ".papelera"
LARGO = "largo_" + "x" * 84
DISQUETE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "kernel", "pruebas", "disquete.adf.gz")

if sys.argv[1] == "--comprobar":
    disco, ver = sys.argv[2], sys.argv[3]
    FONDOS = os.path.join(os.path.dirname(os.path.abspath(disco)), "fondos-prueba")
    # Por la tabla de particiones, con el mismo lector que el servicio de disco (A14).
    desde = "tabla"
    arbol = subprocess.run([ver, disco, desde], check=True, capture_output=True, text=True).stdout.splitlines()
    apariencia = subprocess.run([ver, disco, desde, "C/Prefs/apariencia"], check=True, capture_output=True).stdout
    fondo = subprocess.run([ver, disco, desde, "C/Prefs/fondo"], capture_output=True).stdout
    esperado = [
        "C/ cajón",
        # C3: los fondos de escritorio de prueba (`tools/fondos_prueba.py`).
        "C/Fondos/ cajón",
        *[f"C/Fondos/{n} {os.path.getsize(os.path.join(FONDOS, n))}" for n in sorted(fondos_prueba.FONDOS)],
        "C/Prefs/ cajón",
        f"C/Prefs/apariencia {len(apariencia)}",
        # C2: los mosaicos del panel rápido, como los dejó el panel (los de serie).
        # C3: el fondo que se quedó, el último que se puso desde el centro.
        "C/Prefs/fondo 14",
        "C/Prefs/panel 24",
        "C/Privado/ cajón",
        # S10: el comentario y el bit de archivado que puso el Workbench (Información, como Ana).
        "C/Privado/nota.txt 18 bits=0x1 : nota de Ana",
        "C/datos.bin 614400",
        "C/frio.bin 1048576",
        "C/grande.txt 313890",
        "C/raro|nombre.txt 14",
        # S10: el cajón de otra persona (Carla, 0700), que el Workbench de Ana no lista ni abre.
        "DeCarla/ cajón",
        "DeCarla/carta.txt 17",
        "Escrito/ cajón",
        "Escrito/hola.txt 19 bits=0x4 : Saludo de DashOS, ñandú",
        "Estado/ cajón",
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
        f"{CAJA}/S/ cajón",
        f"{CAJA}/S/Startup-Sequence {len(CAJA_GUION)}",
        f"{PAPELERA}/ cajón",
        f"{PAPELERA}/basura.txt 1",
        f"{PAPELERA}/basura.txt.1 2",
        f"{PAPELERA}/datos.txt 17",
        # El fichero que el virus de guiones crea, intenta renombrar a User-Startup y borra.
        f"{PAPELERA}/inocente 21",
        f"{PAPELERA}/{LARGO} 3",
        "Estado/Servicios/sesiones/ cajón",
        # U4: lo que el usuario (`guardian`, en la prueba) concedió y luego retiró: el fichero queda vacío.
        "Estado/Servicios/sesiones/permisos 0",
        "Imagenes/ cajón",
        "Imagenes/disquete.adf 901120",
        # S10: un nombre con un tabulador (RedoxFS no admite ":"), que el Workbench no enseña.
        "Imagenes/raro\tdos.txt 14",
        "LEEME.txt 19",
        "Prefs/ cajón",
        "Prefs/fuente 8",
        "Programas/ cajón",
        f"Programas/avisa {os.path.getsize(AVISA)}",
        f"Programas/cambio {os.path.getsize(CAMBIO)}",
        f"Programas/hola {os.path.getsize(HOLA)}",
        f"Programas/panel {os.path.getsize(CAMBIO)}",
        f"Programas/pide {os.path.getsize(PIDE)}",
    ]
    hola = subprocess.run([ver, disco, desde, "Escrito/hola.txt"], check=True, capture_output=True).stdout
    grande = subprocess.run([ver, disco, desde, "C/grande.txt"], check=True, capture_output=True).stdout
    panel = subprocess.run([ver, disco, desde, "C/Prefs/panel"], check=True, capture_output=True).stdout
    # FS8: y nada roto, ni un bloque perdido.
    comprobado = subprocess.run([ver, disco, desde, "--comprobar"], capture_output=True, text=True).stdout
    fallos = []
    # S10c: lo que guardó el centro de control al elegir el modo oscuro, con doble clic el azul y después el color del
    # fondo (C3), con el atardecer: su semilla y los 26 colores de sus esquemas (13 por modo), calculados una vez.
    partes = apariencia.decode(errors="replace").rstrip("\n").split(" ")
    if partes[:3] != ["oscuro", "fondo", "c44e68"] or len(partes) != 29 or not all(len(p) == 6 for p in partes[2:]):
        fallos.append(f"C/Prefs/apariencia: {apariencia!r}")
    if fondo != b"atardecer.png\n":
        fallos.append(f"C/Prefs/fondo: {fondo!r}")
    # C2: los mosaicos del panel. Rotos al empezar; el panel quitó Bloquear y lo volvió a poner en su sitio: los de serie.
    if panel != b"oscuro bloquear volumen\n":
        fallos.append(f"C/Prefs/panel: {panel!r}")
    # A14: lo que DashOS escribió lleva la hora del reloj de la placa (en QEMU, la de este PC, en UTC): de esta prueba,
    # no de 1970 ni del futuro.
    fecha = subprocess.run([ver, disco, desde, "--fecha", "Escrito/hola.txt"], capture_output=True, text=True).stdout.split()
    ahora = int(time.time())
    if len(fecha) != 2 or not all(ahora - 600 <= int(f) <= ahora for f in fecha):
        fallos.append(f"Escrito/hola.txt con fecha {fecha} (ahora {ahora})")
    # U6: el registro de seguridad de `sesiones`, en su cajón: la cuenta (8 bytes) y un anillo de entradas de 40 (tipo,
    # estado, quién y qué). Tiene que tener los tres intentos del hostil (matar, la llave de un servicio y entrar).
    REGISTRO = "Estado/Servicios/sesiones/registro"
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
    if arbol != esperado:
        fallos.append(f"árbol: {arbol}")
    permisos = subprocess.run([ver, disco, desde, "Estado/Servicios/sesiones/permisos"], capture_output=True).stdout
    if permisos != b"":
        fallos.append(f"permisos concedidos: {permisos!r}")
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
    os.makedirs(os.path.join(raiz, "Programas"))
    # U6: el cajón de `sesiones` (S6b: `Estado:Servicios/<nombre>`), suyo (usuario 1) y de nadie más.
    os.makedirs(os.path.join(raiz, "Estado", "Servicios", "sesiones"))
    os.chmod(os.path.join(raiz, "Estado", "Servicios", "sesiones"), 0o700)
    # S28: el cajón de la caja Amiga (de Ana, como todo; solo suyo).
    caja = os.path.join(raiz, *CAJA.split("/"))
    for sub, nombre, datos in [("C", "saludo", open(SALUDO, "rb").read()), ("C", "saludo.info", CAJA_ICONO),
                               ("C", "viejo", CAJA_VIEJO),
                               ("S", "Startup-Sequence", CAJA_GUION), ("Discos", "disco.adf", CAJA_ADF),
                               ("", "datos.txt", b"datos de la caja\n")]:
        os.makedirs(os.path.join(caja, sub), exist_ok=True)
        with open(os.path.join(caja, sub, nombre), "wb") as f:
            f.write(datos)
    os.chmod(caja, 0o700)
    with open(os.path.join(raiz, "Programas", "hola"), "wb") as f:
        f.write(open(HOLA, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "hola"), 0o755)
    with open(os.path.join(raiz, "Programas", "pide"), "wb") as f:
        f.write(open(PIDE, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "pide"), 0o755)
    with open(os.path.join(raiz, "Programas", "avisa"), "wb") as f:
        f.write(open(AVISA, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "avisa"), 0o755)
    with open(os.path.join(raiz, "Programas", "cambio"), "wb") as f:
        f.write(open(CAMBIO, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "cambio"), 0o755)
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
    # S10c: la apariencia guardada del centro de control, en basura (bytes que no son UTF-8 y un color que no existe): el
    # centro se queda con la de serie, y al elegir otra la reescribe. Cajón 0700 y fichero 0600: el hostil no la ve.
    prefs = os.path.join(raiz, "C", "Prefs")
    os.makedirs(prefs)
    with open(os.path.join(prefs, "apariencia"), "wb") as f:
        f.write(b"\xff\xfeoscuro magenta\x00")
    os.chmod(os.path.join(prefs, "apariencia"), 0o600)
    # C2: los mosaicos del panel rápido, rotos: uno repetido y uno que no existe (la basura, en las pruebas de PC). El centro y el
    # panel se quedan los de serie; al guardar, el centro lo reescribe.
    with open(os.path.join(prefs, "panel"), "wb") as f:
        f.write(b"volumen volumen nube\n")
    os.chmod(os.path.join(prefs, "panel"), 0o600)
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
    with open(os.path.join(prefs, "fondo"), "w") as f:
        f.write("enorme.png\n")
    os.chmod(os.path.join(prefs, "fondo"), 0o600)
    # S26 (`tools/aspecto_caja_linux.py`): con DASHOS_PREFS="fichero=contenido;...", otros Prefs del usuario (el fondo,
    # la apariencia, el aspecto de las cajas) para las capturas del aspecto. "\n" en el contenido es un salto de línea.
    for par in filter(None, os.environ.get("DASHOS_PREFS", "").split(";")):
        nombre, _, contenido = par.partition("=")
        if nombre not in ("apariencia", "fondo", "cajas", "panel"):
            raise SystemExit(f"DASHOS_PREFS: {nombre} no es un Prefs del centro")
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
    fs = os.path.join(t, "fs.img")
    with open(fs, "wb") as f:
        f.truncate(7 << 20)
    subprocess.run([ar, fs, raiz], check=True, stdout=subprocess.DEVNULL)
    volumen = os.path.join(os.path.dirname(ar), "redoxfs-volume")
    # La raíz de "Ana", del 1000 al crearla: cambiarla después la reescribiría con la llave del volumen.
    subprocess.run([volumen, fs, "create-encrypted", "Ana", "--test-cost"], input=b"clave de Ana\n", check=True,
                   env={**os.environ, "REDOXFS_DUENO": "1000:1000"})
    # U3: "Carla", el volumen de la usuaria 1001, que `sesiones` abre al entrar y cierra al salir.
    subprocess.run([volumen, fs, "create-encrypted", "Carla", "--test-cost"], input=b"clave de Carla\n", check=True,
                   env={**os.environ, "REDOXFS_DUENO": "1001:1001"})
    duenos = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "archivos", "herramientas", "target", "release", "duenos")
    subprocess.run([duenos, fs, "0", "1000:1000"], check=True)
    subprocess.run([duenos, fs, "0", "1:1", "camino=Estado/Servicios/sesiones"], check=True)
    subprocess.run([duenos, fs, "0", "1001:1001", "camino=DeCarla"], check=True)
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
