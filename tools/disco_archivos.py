#!/usr/bin/env python3
"""Disco de prueba del NVMe (U6d, FS1): 8 MB.

- Primer MB: lo de la prueba del servicio de disco (U6f): un texto y la firma 0x55AA en el sector 0, otro texto en
  el 1. El cliente de U6f escribe el sector 2.
- Desde el MB 1: un RedoxFS hecho con `redoxfs-ar` a partir de una carpeta con LEEME.txt, C/grande.txt (las líneas
  "línea 0", "línea 1"..., unos 300 KB, para que pase por varios registros de 128 KB) y Prefs/fuente. Y un volumen
  cifrado, "Ana", con la contraseña "clave de Ana" (FS10), hecho con `redoxfs-volume`, que está al lado de
  `redoxfs-ar`; con Argon2 barato, para que abrirlo en QEMU no tarde. Y `Imagenes/disquete.adf`, el ADF de la
  prueba de S20 (`kernel/pruebas/disquete.adf.gz`, hecho con `tools/adf_oraculo.py --disquete`).

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

if sys.argv[1] == "--comprobar":
    disco, ver = sys.argv[2], sys.argv[3]
    desde = str(1 << 20)
    arbol = subprocess.run([ver, disco, desde], check=True, capture_output=True, text=True).stdout.splitlines()
    esperado = [
        "C/ cajón",
        "C/grande.txt 313890",
        "Escrito/ cajón",
        "Escrito/hola.txt 19 bits=0x4 : Saludo de DashOS, ñandú",
        "Imagenes/ cajón",
        "Imagenes/disquete.adf 901120",
        "LEEME.txt 19",
        "Prefs/ cajón",
        "Prefs/fuente 8",
    ]
    hola = subprocess.run([ver, disco, desde, "Escrito/hola.txt"], check=True, capture_output=True).stdout
    grande = subprocess.run([ver, disco, desde, "C/grande.txt"], check=True, capture_output=True).stdout
    # FS8: y nada roto, ni un bloque perdido.
    comprobado = subprocess.run([ver, disco, desde, "--comprobar"], capture_output=True, text=True).stdout
    fallos = []
    # FS10: el nombre y el contenido de lo que DashOS escribió en "Ana" no están en claro.
    crudo = open(disco, "rb").read()
    for secreto in (b"secreto.txt", b"solo para Ana"):
        if secreto in crudo:
            fallos.append(f"{secreto!r} en claro en el disco")
    if comprobado != "sin problemas\n":
        fallos.append(f"comprobación: {comprobado.strip()}")
    if arbol != esperado:
        fallos.append(f"árbol: {arbol}")
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
    with open(os.path.join(raiz, "Prefs", "fuente"), "w") as f:
        f.write("Topaz 8\n")
    fs = os.path.join(t, "fs.img")
    with open(fs, "wb") as f:
        f.truncate(7 << 20)
    subprocess.run([ar, fs, raiz], check=True, stdout=subprocess.DEVNULL)
    volumen = os.path.join(os.path.dirname(ar), "redoxfs-volume")
    subprocess.run([volumen, fs, "create-encrypted", "Ana", "--test-cost"], input=b"clave de Ana\n", check=True)
    d = bytearray(1 << 20)
    s0 = b"DashOS: sector 0 del disco NVMe"
    d[: len(s0)] = s0
    d[510:512] = b"\x55\xaa"
    s1 = b"DashOS: sector 1"
    d[512 : 512 + len(s1)] = s1
    with open(salida, "wb") as f:
        f.write(d)
        f.write(open(fs, "rb").read())
