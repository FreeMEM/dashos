#!/usr/bin/env python3
"""S20, fase 1: la lectura de ADF de `amigados` contra amitools, que hace las imágenes.

Hace con `xdftool` un ADF por cada variante de AmigaDOS (OFS y FFS; normal, internacional y con caché de
directorios; de 880 KB y de 1,76 MB), con un árbol conocido: ficheros de tamaños en los bordes de los bloques (488 de
OFS, 512 de FFS), uno que pasa de 72 bloques (bloques de extensión), cajones anidados, muchos ficheros en un cajón
(cadenas de hash), nombres con letras de Latin-1, comentarios y bits de protección. Después lo lee con
`amigados/examples/ver.rs` y compara el árbol, los contenidos y los bloques libres (con `xdftool info`).

Al final, daña la imagen a propósito muchas veces (bytes al azar en los bloques usados) y comprueba que leerla no
cae nunca: da errores, pero no un pánico.

amitools es GPL: aquí solo se ejecuta, en un entorno aparte (`XDFTOOL`, por defecto ~/.cache/amitools-venv).

Uso: adf_oraculo.py [--disquete SALIDA.adf.gz]

Con --disquete, hace además el ADF de la prueba de QEMU (`kernel/pruebas/disquete.adf.gz`), el que lee el cliente
`adf-prueba` por el servicio `imagen`.
"""
import gzip
import os
import random
import re
import subprocess
import sys
import tempfile

RAIZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
XDFTOOL = os.environ.get("XDFTOOL", os.path.expanduser("~/.cache/amitools-venv/bin/xdftool"))
VER = os.path.join(RAIZ, "amigados/target/debug/examples/ver")


def xdf(imagen, *ordenes):
    """Varias órdenes de xdftool seguidas, separadas por "+"."""
    args = [XDFTOOL, imagen]
    for i, o in enumerate(ordenes):
        if i:
            args.append("+")
        args.extend(o)
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def datos(n, semilla):
    """n bytes de prueba, distintos para cada fichero."""
    r = random.Random(semilla)
    return bytes(r.getrandbits(8) for _ in range(n))


def arbol_de_prueba(hd):
    """El árbol: camino → (contenido o None si es cajón, protección de xdftool o None, comentario o None)."""
    a = {}
    for n in (0, 1, 487, 488, 489, 511, 512, 513, 976, 1024):
        a[f"t{n}.bin"] = (datos(n, n), None, None)
    # Más de 72 bloques: la cabecera no basta y hacen falta extensiones (dos, en OFS).
    grande = 300_000 if hd else 100_000
    a["C/grande.bin"] = (datos(grande, 7), None, None)
    a["C"] = (None, None, None)
    a["Uno/Dos/Tres"] = (None, None, None)
    a["Uno"] = (None, None, None)
    a["Uno/Dos"] = (None, None, None)
    a["Uno/Dos/Tres/hondo.txt"] = (b"en el fondo\n", None, None)
    # Latin-1, con mayúsculas que solo el hash internacional junta.
    a["Ébano"] = (None, None, None)
    a["Ébano/ñandú.txt"] = ("ñandú en Latin-1\n".encode(), "rwd", "un comentario con eñe")
    a["Ébano/ÀÉÎÕÜ y ÿ"] = (b"letras\n", "re", None)
    a["Nombre_de_treinta_letras_ABCDE"] = (b"30\n", "rwed", None)
    # Muchos en un cajón: con 72 huecos, cadenas largas.
    a["Muchos"] = (None, None, None)
    for i in range(150):
        a[f"Muchos/f{i:03}"] = (f"fichero {i}\n".encode(), None, None)
    return a


def hacer(imagen, variante, hd, arbol, t):
    tam = ["size=1760k"] if hd else []
    xdf(imagen, ["create", *tam], ["format", "Prueba ñ", variante])
    ordenes = []
    for camino in sorted(arbol, key=lambda c: (c.count("/"), c)):
        contenido, prot, com = arbol[camino]
        if contenido is None:
            ordenes.append(["makedir", camino])
            continue
        f = os.path.join(t, "f" + str(len(ordenes)))
        with open(f, "wb") as fh:
            fh.write(contenido)
        ordenes.append(["write", f, camino])
        if prot:
            ordenes.append(["protect", camino, prot])
        if com:
            ordenes.append(["comment", camino, com])
    # De 60 en 60 órdenes, para no pasarse del largo de una línea de órdenes.
    for i in range(0, len(ordenes), 60):
        xdf(imagen, ["open"], *ordenes[i : i + 60])


def bits(prot):
    """Los bits del Amiga que pone xdftool con "rwed": cada letra que falta es un bit a 1 (prohibido)."""
    if prot is None:
        return 0
    b = 0
    for i, letra in enumerate("dewr"):
        if letra not in prot:
            b |= 1 << i
    return b


def comprobar(imagen, variante, arbol):
    fallos = []
    salida = subprocess.run([VER, imagen], capture_output=True, text=True)
    if salida.returncode != 0:
        return [f"ver: {salida.stdout}{salida.stderr}"]
    lineas = salida.stdout.splitlines()
    cabeza = lineas[0]
    ffs = "ffs" in variante
    intl = "intl" in variante or "dc" in variante
    esperada = f"volumen Prueba ñ {'FFS' if ffs else 'OFS'} {'intl' if intl else '-'}"
    if not cabeza.startswith(esperada):
        fallos.append(f"cabecera: {cabeza!r}, se esperaba {esperada!r}")
    libres = re.search(r"free:\s+(\d+)", xdf(imagen, ["info"])).group(1)
    if f"libres=Ok(Some({libres}))" not in cabeza:
        fallos.append(f"libres: {cabeza!r}, xdftool dice {libres}")
    leido = {}
    for l in lineas[1:]:
        m = re.match(r"(.*?)(/)? (cajón|enlace|\d+) p=([0-9a-f]{8}) f=(\d+)(?: : (.*))?$", l)
        if not m:
            fallos.append(f"línea rara: {l!r}")
            continue
        leido[m.group(1)] = (m.group(3), int(m.group(4), 16), m.group(6))
    for camino, (contenido, prot, com) in arbol.items():
        if camino not in leido:
            fallos.append(f"falta {camino}")
            continue
        tipo, p, c = leido.pop(camino)
        if contenido is None:
            if tipo != "cajón":
                fallos.append(f"{camino}: {tipo}, no un cajón")
            continue
        if tipo != str(len(contenido)):
            fallos.append(f"{camino}: tamaño {tipo}, no {len(contenido)}")
        if p & 0xF != bits(prot):
            fallos.append(f"{camino}: protección {p:x}, no {bits(prot):x}")
        if c != com:
            fallos.append(f"{camino}: comentario {c!r}, no {com!r}")
    for camino in leido:
        fallos.append(f"sobra {camino}")
    # El contenido, de unos cuantos (todos los de tamaño en el borde, el grande y los de Latin-1).
    for camino, (contenido, _, _) in arbol.items():
        if contenido is None or camino.startswith("Muchos/f") and not camino.endswith("7"):
            continue
        r = subprocess.run([VER, imagen, camino], capture_output=True)
        if r.returncode != 0 or r.stdout != contenido:
            fallos.append(f"{camino}: el contenido no es el mismo ({len(r.stdout)} bytes)")
    # Sin mirar mayúsculas: en las variantes internacionales también las de Latin-1; en las otras, solo a-z.
    r = subprocess.run([VER, imagen, "ébano/ÑANDÚ.TXT"], capture_output=True)
    if intl and r.stdout != arbol["Ébano/ñandú.txt"][0]:
        fallos.append("internacional: no encuentra ébano/ÑANDÚ.TXT")
    if not intl and r.returncode == 0:
        fallos.append("sin internacional: encuentra ébano/ÑANDÚ.TXT")
    r = subprocess.run([VER, imagen, "muchos/F077"], capture_output=True)
    if r.stdout != b"fichero 77\n":
        fallos.append("no encuentra muchos/F077")
    return fallos


def danar(imagen, veces):
    """Bytes al azar en los bloques usados; leer no debe caer nunca."""
    original = open(imagen, "rb").read()
    usados = [n for n in range(len(original) // 512) if any(original[n * 512 : n * 512 + 512])]
    r = random.Random(20)
    caidas = []
    with tempfile.NamedTemporaryFile(suffix=".adf") as t:
        for i in range(veces):
            d = bytearray(original)
            for _ in range(r.randint(1, 8)):
                n = r.choice(usados)
                d[n * 512 + r.randrange(512)] = r.getrandbits(8)
            t.seek(0)
            t.write(d)
            t.flush()
            p = subprocess.run([VER, t.name, "--todo"], capture_output=True, text=True)
            if p.returncode not in (0, 2):
                caidas.append(f"vez {i}: código {p.returncode}: {p.stderr.strip()[-200:]}")
    return caidas


def disquete(salida):
    """El ADF de la prueba de QEMU: FFS internacional de 880 KB, lo que espera `usuario/src/bin/adf_prueba.rs`."""
    with tempfile.TemporaryDirectory() as t:
        img = os.path.join(t, "d.adf")
        f = {}
        f["LEEME.txt"] = "Un disquete de AmigaDOS, leído en DashOS.\n".encode("latin-1")
        # 100 KB que pasan de 72 bloques; el cliente los rehace y compara.
        f["C/grande.txt"] = "".join(f"línea {i}\n" for i in range(9000)).encode("latin-1")
        f["Cajón/ñandú.txt"] = "ñandú\n".encode("latin-1")
        xdf(img, ["create"], ["format", "Disquete", "ffs+intl"])
        ordenes = [["makedir", "C"], ["makedir", "Cajón"]]
        for i, (camino, contenido) in enumerate(f.items()):
            p = os.path.join(t, str(i))
            open(p, "wb").write(contenido)
            ordenes.append(["write", p, camino])
        ordenes.append(["comment", "LEEME.txt", "léeme primero"])
        ordenes.append(["protect", "Cajón/ñandú.txt", "re"])
        xdf(img, ["open"], *ordenes)
        with gzip.GzipFile(salida, "wb", mtime=0) as g:
            g.write(open(img, "rb").read())
    print(f"hecho {salida}")


def main():
    subprocess.run(["cargo", "build", "-q", "--example", "ver"], cwd=os.path.join(RAIZ, "amigados"), check=True)
    if "--disquete" in sys.argv:
        disquete(sys.argv[sys.argv.index("--disquete") + 1])
    variantes = [(v, False) for v in ("ofs", "ffs", "ofs+intl", "ffs+intl", "ofs+dc", "ffs+dc")]
    variantes += [("ofs", True), ("ffs+intl", True)]
    mal = 0
    with tempfile.TemporaryDirectory() as t:
        for variante, hd in variantes:
            img = os.path.join(t, f"{variante}{'-hd' if hd else ''}.adf")
            arbol = arbol_de_prueba(hd)
            hacer(img, variante, hd, arbol, t)
            fallos = comprobar(img, variante, arbol)
            nombre = f"{variante} {'1,76 MB' if hd else '880 KB'}"
            if fallos:
                mal += 1
                print(f"MAL: {nombre}", *fallos[:20], sep="\n  ")
            else:
                print(f"bien: {nombre} ({len(arbol)} entradas)")
        img = os.path.join(t, "ffs+intl.adf")
        caidas = danar(img, int(os.environ.get("VECES", "300")))
        if caidas:
            mal += 1
            print("MAL: se cae con imágenes dañadas", *caidas[:10], sep="\n  ")
        else:
            print("bien: dañada a propósito, no se cae")
    sys.exit(1 if mal else 0)


if __name__ == "__main__":
    main()
