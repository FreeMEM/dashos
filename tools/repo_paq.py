#!/usr/bin/env python3
"""El repositorio de pruebas de Paq (S8, rama `paq`): paquetes de DashOS hechos aquí con programas propios de prueba
(guiones del Shell y textos; nada ajeno), su índice firmado y lo que hace falta para confiar en él.

Cada paquete es un `tar` ustar comprimido con gzip, con su manifiesto (`<nombre>.readme`, formato 2) en la raíz y su
cajón al lado. El índice (`PKG-INDEX 2`), en `<repo>/aarch64-dashos/index.gz`, firmado con Ed25519 sobre la etiqueta
`PKG-INDEX-2` (`index.gz.sig`).

La clave es **de pruebas**: su semilla está aquí, a la vista, y no vale para nada más. Su pública va en
`Estado/Paq/Claves/prueba`, y el repositorio en `Estado/Paq/Repositorios`.

Lo que deja en DESTINO (la raíz del disco de pruebas):

- `Work/Repos/prueba/aarch64-dashos/`: el índice de serie 1 (saludo 1.0, letras y los paquetes hostiles) y los
  paquetes. En `Work:` porque la prueba del Shell lo cambia (copia encima el de la serie 2, como si el repositorio
  publicara una versión nueva) y `SYS:` no se escribe; Paq lo lee como `SYS:Work/Repos/prueba`.
- `Work/Repos/prueba-2/`: el índice de serie 2 (sale saludo 1.1).
- `Work/Repos/firmamal/aarch64-dashos/`: un repositorio cuyo índice está firmado con otra clave.
- `Estado/Paq/Repositorios` y `Estado/Paq/Claves/prueba` (y `firmamal`, con la clave buena: el índice no lo está).

Lo hostil: `fuera` (una ruta con `..`), `absoluto` (una ruta que empieza por `/`), `enlace` (un enlace simbólico),
`cambiado` (el paquete cambiado después de hacer el índice), `vecino` (escribe en el cajón de otro paquete),
`imposible` (pide un paquete que no existe), `circulo-a` y `circulo-b` (se piden el uno al otro) y `ajeno` (en el
repositorio mal firmado).

Uso: repo_paq.py DESTINO
"""
import gzip
import hashlib
import io
import os
import sys
import tarfile

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization

ARQ = "aarch64-dashos"
# Las semillas de las claves de pruebas (no valen para nada más).
SEMILLA = bytes(range(1, 33))
OTRA = bytes(range(101, 133))
FECHA = 1763294400  # 2025-11-16: las fechas de dentro del tar, fijas (así el disco sale igual cada vez)


def publica(k):
    return k.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)


def firma(k, etiqueta, datos):
    pub = publica(k)
    return f"ed25519 {hashlib.sha256(pub).hexdigest()[:16]} {k.sign(etiqueta + b'\0' + datos).hex()}\n".encode()


def readme(nombre, version, cajon, campos):
    return (f"Format: 2\nName: {nombre}\nVersion: {version}\nArchitecture: {ARQ}\nInstall: drawer {cajon}\n"
            f"Short: {nombre} {version}, de las pruebas de Paq\n{campos}\nUn paquete de las pruebas de Paq.\n").encode()


def tar(entradas):
    """entradas: (ruta, bytes o None si es cajón, modo, tipo de tarfile o None)."""
    b = io.BytesIO()
    with tarfile.open(fileobj=b, mode="w", format=tarfile.USTAR_FORMAT) as t:
        for ruta, datos, modo, tipo in entradas:
            i = tarfile.TarInfo(ruta)
            i.mtime, i.mode, i.uid, i.gid, i.uname, i.gname = FECHA, modo, 0, 0, "", ""
            if tipo is not None:
                i.type, i.linkname = tipo, "../../SYS/S/Startup-Sequence"
                t.addfile(i)
            elif datos is None:
                i.type = tarfile.DIRTYPE
                t.addfile(i)
            else:
                i.size = len(datos)
                t.addfile(i, io.BytesIO(datos))
    return gzip.compress(b.getvalue(), mtime=0)


def paquete(nombre, version, cajon, campos="", ficheros=None, extra=()):
    """Un paquete normal: su cajón con un guion del Shell (`<cajón>/<cajón>`) y un LEEME, y lo de `extra` además."""
    ficheros = ficheros if ficheros is not None else [
        (f"{cajon}/{cajon}", f'Echo "{cajon} {version}, instalado con Paq"\n'.encode(), 0o755),
        (f"{cajon}/LEEME", f"{nombre} {version}\n".encode(), 0o644),
    ]
    entradas = [(cajon, None, 0o755, None)] + [(r, d, m, None) for r, d, m in ficheros] + list(extra)
    entradas.append((f"{nombre}.readme", readme(nombre, version, cajon, campos), 0o644, None))
    return {"nombre": nombre, "version": version, "cajon": cajon, "campos": campos, "datos": tar(entradas)}


def indice(repo, serie, paquetes, ids):
    t = f"PKG-INDEX 2\nRepository: {repo}\nArchitecture: {ARQ}\nSerial: {serie}\nExpires: 2030-01-01T00:00:00Z\n"
    for p in paquetes:
        fichero = f"{p['nombre']}-{p['version']}-1.tar.gz"
        de_mas = "".join(l + "\n" for l in p["campos"].splitlines() if l.split(":")[0] in ("Requires", "Conflicts", "Provides", "Assigns"))
        d = p.get("en_indice", p["datos"])
        t += (f"\nName: {p['nombre']}\nVersion: {p['version']}\nRelease: 1\nId: {ids[(p['nombre'], p['version'])]}\nUrl: {fichero}\n"
              f"Size: {len(d)}\nSha256: {hashlib.sha256(d).hexdigest()}\nInstall: drawer {p['cajon']}\n"
              f"Short: {p['nombre']} {p['version']}, de las pruebas de Paq\n{de_mas}")
    return gzip.compress(t.encode(), mtime=0)


def escribir(ruta, datos):
    os.makedirs(os.path.dirname(ruta), exist_ok=True)
    with open(ruta, "wb") as f:
        f.write(datos)


def repositorio(dir_, repo, serie, paquetes, ids, clave):
    arq = os.path.join(dir_, ARQ)
    ix = indice(repo, serie, paquetes, ids)
    escribir(os.path.join(arq, "index.gz"), ix)
    escribir(os.path.join(arq, "index.gz.sig"), firma(clave, b"PKG-INDEX-2", ix))
    for p in paquetes:
        escribir(os.path.join(arq, f"{p['nombre']}-{p['version']}-1.tar.gz"), p["datos"])


def hacer(destino):
    clave, otra = Ed25519PrivateKey.from_private_bytes(SEMILLA), Ed25519PrivateKey.from_private_bytes(OTRA)
    saludo = "Requires: pkg:letras >= 1.0\nAssigns: SALUDO: Saludo"
    normales = [
        paquete("saludo", "1.0", "Saludo", saludo),
        paquete("letras", "1.0", "Letras", "", [("Letras/abc", b"abc\n", 0o644)]),
        paquete("circulo-a", "1.0", "CirculoA", "Requires: pkg:circulo-b"),
        paquete("circulo-b", "1.0", "CirculoB", "Requires: pkg:circulo-a"),
        paquete("imposible", "1.0", "Imposible", "Requires: pkg:noexiste"),
    ]
    hostiles = [
        paquete("fuera", "1.0", "Fuera", extra=[("../Fuera.txt", b"fuera\n", 0o644, None)]),
        paquete("absoluto", "1.0", "Absoluto", extra=[("/C/Absoluto", b"absoluto\n", 0o755, None)]),
        paquete("enlace", "1.0", "Enlace", extra=[("Enlace/arranque", None, 0o644, tarfile.SYMTYPE)]),
        paquete("vecino", "1.0", "Vecino", extra=[("Saludo/LEEME", b"te piso\n", 0o644, None)]),
    ]
    # `cambiado`: el índice dice la huella del bueno; en el repositorio está otro (cambiado después de firmar).
    cambiado = paquete("cambiado", "1.0", "Cambiado")
    cambiado["en_indice"] = cambiado["datos"]
    cambiado["datos"] = paquete("cambiado", "1.0", "Cambiado", ficheros=[("Cambiado/Cambiado", b"Echo \"te he cambiado\"\n", 0o755)])["datos"]
    todos = normales + hostiles + [cambiado]
    ids = {(p["nombre"], p["version"]): i + 1 for i, p in enumerate(todos)}
    nuevo = paquete("saludo", "1.1", "Saludo", saludo)
    ids[("saludo", "1.1")] = len(ids) + 1
    repos = os.path.join(destino, "Work", "Repos")
    repositorio(os.path.join(repos, "prueba"), "prueba", 1, todos, ids, clave)
    # La serie 2: lo mismo y saludo 1.1. Solo el índice (y el paquete nuevo): lo demás ya está en `prueba`.
    ix2 = indice("prueba", 2, todos + [nuevo], ids)
    escribir(os.path.join(repos, "prueba-2", "index.gz"), ix2)
    escribir(os.path.join(repos, "prueba-2", "index.gz.sig"), firma(clave, b"PKG-INDEX-2", ix2))
    escribir(os.path.join(repos, "prueba", ARQ, "saludo-1.1-1.tar.gz"), nuevo["datos"])
    # Un repositorio firmado con una clave que no es la suya.
    ajeno = paquete("ajeno", "1.0", "Ajeno")
    repositorio(os.path.join(repos, "firmamal"), "firmamal", 1, [ajeno], {("ajeno", "1.0"): 1}, otra)
    paq = os.path.join(destino, "Estado", "Paq")
    escribir(os.path.join(paq, "Repositorios"), (
        "# Los repositorios de Paq (S8). Añadir o quitar uno es acción de administrador.\n"
        "Repository: prueba\nUrl: SYS:Work/Repos/prueba\nPriority: 20\nProtected: no\n\n"
        "Repository: firmamal\nUrl: SYS:Work/Repos/firmamal\nPriority: 10\nProtected: no\n").encode())
    for repo in ("prueba", "firmamal"):
        escribir(os.path.join(paq, "Claves", repo), (
            f"# La clave de pruebas de DashOS para el repositorio {repo}: solo vale para las pruebas.\n"
            f"Repository: {repo}\nUse: index\nKey: {publica(clave).hex()}\n").encode())


if __name__ == "__main__":
    hacer(sys.argv[1])
