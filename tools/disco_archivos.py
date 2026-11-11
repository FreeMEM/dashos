#!/usr/bin/env python3
"""Disco de prueba del NVMe (U6d, FS1): 8 MB.

- Primer MB: lo de la prueba del servicio de disco (U6f): un texto y la firma 0x55AA en el sector 0, otro texto en
  el 1. El cliente de U6f escribe el sector 2.
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
import subprocess
import sys
import tempfile

# U2: un programa en el disco, para que `sesiones` lo lance desde ahí (el mismo `hola` que se construye).
HOLA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "usuario", "target", "aarch64-unknown-none-softfloat", "release", "hola")
# Y `pide`: lanzado desde el disco, nace con su canal con `sesiones` y repite la prueba de U4 con su número nuevo.
PIDE = os.path.join(os.path.dirname(HOLA), "pide")

if sys.argv[1] == "--comprobar":
    disco, ver = sys.argv[2], sys.argv[3]
    desde = str(1 << 20)
    arbol = subprocess.run([ver, disco, desde], check=True, capture_output=True, text=True).stdout.splitlines()
    esperado = [
        "C/ cajón",
        "C/Privado/ cajón",
        "C/Privado/nota.txt 18",
        "C/datos.bin 614400",
        "C/grande.txt 313890",
        "Escrito/ cajón",
        "Escrito/hola.txt 19 bits=0x4 : Saludo de DashOS, ñandú",
        "Estado/ cajón",
        "Estado/Servicios/ cajón",
        "Estado/Servicios/sesiones/ cajón",
        # U4: lo que el usuario (`guardian`, en la prueba) concedió y luego retiró: el fichero queda vacío.
        "Estado/Servicios/sesiones/permisos 0",
        "Imagenes/ cajón",
        "Imagenes/disquete.adf 901120",
        "LEEME.txt 19",
        "Prefs/ cajón",
        "Prefs/fuente 8",
        "Programas/ cajón",
        f"Programas/hola {os.path.getsize(HOLA)}",
        f"Programas/pide {os.path.getsize(PIDE)}",
    ]
    hola = subprocess.run([ver, disco, desde, "Escrito/hola.txt"], check=True, capture_output=True).stdout
    grande = subprocess.run([ver, disco, desde, "C/grande.txt"], check=True, capture_output=True).stdout
    # FS8: y nada roto, ni un bloque perdido.
    comprobado = subprocess.run([ver, disco, desde, "--comprobar"], capture_output=True, text=True).stdout
    fallos = []
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
    for secreto in (b"secreto.txt", b"solo para Ana"):
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
    with open(os.path.join(raiz, "Prefs", "fuente"), "w") as f:
        f.write("Topaz 8\n")
    os.makedirs(os.path.join(raiz, "Programas"))
    # U6: el cajón de `sesiones` (S6b: `Estado:Servicios/<nombre>`), suyo (usuario 1) y de nadie más.
    os.makedirs(os.path.join(raiz, "Estado", "Servicios", "sesiones"))
    os.chmod(os.path.join(raiz, "Estado", "Servicios", "sesiones"), 0o700)
    with open(os.path.join(raiz, "Programas", "hola"), "wb") as f:
        f.write(open(HOLA, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "hola"), 0o755)
    with open(os.path.join(raiz, "Programas", "pide"), "wb") as f:
        f.write(open(PIDE, "rb").read())
    os.chmod(os.path.join(raiz, "Programas", "pide"), 0o755)
    privado = os.path.join(raiz, "C", "Privado")
    os.makedirs(privado)
    with open(os.path.join(privado, "nota.txt"), "w") as f:
        f.write("privado de prueba\n")
    os.chmod(os.path.join(privado, "nota.txt"), 0o600)
    os.chmod(privado, 0o700)
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
    s1 = b"DashOS: sector 1"
    d[512 : 512 + len(s1)] = s1
    with open(salida, "wb") as f:
        f.write(d)
        f.write(open(fs, "rb").read())
