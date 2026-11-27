#!/usr/bin/env python3
"""Recorta Material Symbols Rounded a lo que usa DashOS y rehace `kit/src/iconos/simbolos_datos.rs`.

Uso: python3 tools/iconos_material.py [--cache DIR] [--sin-red]

Lee la lista de `kit/iconos/simbolos.lista` (nombre y variantes: contorno, lleno) y los emblemas de la colección de
DashOS (`kit/iconos/dashos.iconos`, "emblema=", siempre llenos). Baja cada SVG del repositorio oficial de Google
(github.com/google/material-design-icons, Apache 2.0), fijado en `COMMIT`, a una caché fuera de git
(`~/.cache/dashos-iconos/` si no se dice otra). De cada SVG solo se queda el trazado (el atributo `d` de su único
`<path>`, en la caja 0 -960 960 960): eso es lo que pinta el kit.

Comprueba lo que baja: un solo `<path>`, la caja de siempre, y en el trazado solo órdenes y números. Si algo no cuadra,
para y lo dice (no escribe nada a medias). Con --sin-red, solo usa la caché.
"""
import argparse
import os
import re
import sys
import urllib.error
import urllib.request

COMMIT = "10ecc14790612ca8fd164e3f4ffe9c16fc50d5b7"  # master del 2026-10-23
REPO = "https://raw.githubusercontent.com/google/material-design-icons"
RAIZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LISTA = os.path.join(RAIZ, "kit/iconos/simbolos.lista")
COLECCION = os.path.join(RAIZ, "kit/iconos/dashos.iconos")
SALIDA = os.path.join(RAIZ, "kit/src/iconos/simbolos_datos.rs")
LICENCIA = os.path.join(RAIZ, "kit/iconos/LICENCIA-material-symbols.txt")
TRAZADO_BUENO = re.compile(r"^[MmLlHhVvCcSsQqTtAaZz0-9.,\- eE]+$")


def leer_lista():
    pedidos = {}
    for n, linea in enumerate(open(LISTA, encoding="utf-8"), 1):
        linea = linea.split("#", 1)[0].strip()
        if not linea:
            continue
        partes = linea.split()
        nombre, variantes = partes[0], partes[1:] or ["contorno"]
        if not re.fullmatch(r"[a-z0-9_]+", nombre):
            sys.exit(f"{LISTA}:{n}: nombre raro: {nombre!r}")
        for v in variantes:
            if v not in ("contorno", "lleno"):
                sys.exit(f"{LISTA}:{n}: variante desconocida: {v!r}")
            pedidos.setdefault(nombre, set()).add(v == "lleno")
    for linea in open(COLECCION, encoding="utf-8"):
        for m in re.finditer(r"\bemblema=([a-z0-9_]+)", linea.split("#", 1)[0]):
            pedidos.setdefault(m.group(1), set()).add(True)
    return pedidos


def url(nombre, lleno):
    return f"{REPO}/{COMMIT}/symbols/web/{nombre}/materialsymbolsrounded/{nombre}{'_fill1' if lleno else ''}_24px.svg"


def bajar(direccion, destino, sin_red, puede_faltar=False):
    if os.path.exists(destino + ".falta"):
        return None
    if os.path.exists(destino):
        return open(destino, "rb").read()
    if sin_red:
        sys.exit(f"no está en la caché y no hay red: {destino}")
    try:
        with urllib.request.urlopen(direccion, timeout=30) as r:
            datos = r.read()
    except urllib.error.HTTPError as e:
        if e.code == 404 and puede_faltar:
            os.makedirs(os.path.dirname(destino), exist_ok=True)
            open(destino + ".falta", "w").close()
            return None
        sys.exit(f"{direccion}: {e.code} (¿está bien escrito el nombre?)")
    if len(datos) > 64 * 1024:
        sys.exit(f"demasiado grande: {direccion}")
    os.makedirs(os.path.dirname(destino), exist_ok=True)
    with open(destino + ".nuevo", "wb") as f:
        f.write(datos)
    os.replace(destino + ".nuevo", destino)
    return datos


def trazado(svg, de_donde):
    texto = svg.decode("utf-8")
    if 'viewBox="0 -960 960 960"' not in texto:
        sys.exit(f"{de_donde}: no tiene la caja de siempre")
    caminos = re.findall(r'<path\b[^>]*\bd="([^"]*)"', texto)
    if len(caminos) != 1 or texto.count("<path") != 1:
        sys.exit(f"{de_donde}: se esperaba un solo <path>")
    d = caminos[0]
    if not TRAZADO_BUENO.match(d):
        sys.exit(f"{de_donde}: el trazado trae algo más que órdenes y números")
    return d


def ident(nombre, lleno):
    base = nombre.upper()
    if base[0].isdigit():
        base = "S_" + base
    return base + ("_LLENO" if lleno else "")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--cache", default=os.path.expanduser("~/.cache/dashos-iconos"))
    ap.add_argument("--sin-red", action="store_true")
    a = ap.parse_args()
    cache = os.path.join(a.cache, COMMIT)
    pedidos = leer_lista()
    filas = []
    for nombre in sorted(pedidos):
        for lleno in sorted(pedidos[nombre]):
            u = url(nombre, lleno)
            svg = bajar(u, os.path.join(cache, f"{nombre}{'_fill1' if lleno else ''}_24px.svg"), a.sin_red, lleno)
            if svg is None:
                # Los que no cambian al llenarlos no tienen variante llena: la de contorno es la misma.
                u = url(nombre, False)
                svg = bajar(u, os.path.join(cache, f"{nombre}_24px.svg"), a.sin_red)
            filas.append((nombre, lleno, trazado(svg, u)))
    licencia = bajar(f"{REPO}/{COMMIT}/LICENSE", os.path.join(cache, "LICENSE"), a.sin_red)
    if b"Apache License" not in licencia or b"Version 2.0" not in licencia:
        sys.exit("la licencia del repositorio no es la Apache 2.0 de siempre")
    with open(LICENCIA, "wb") as f:
        f.write(licencia)
    bytes_trazados = sum(len(d) for _, _, d in filas)
    out = []
    out.append("// Generado por `tools/iconos_material.py` desde `kit/iconos/simbolos.lista` y los emblemas de\n")
    out.append("// `kit/iconos/dashos.iconos`: no se toca a mano. Material Symbols Rounded (Apache 2.0, `kit/iconos/NOTICE`),\n")
    out.append(f"// github.com/google/material-design-icons en {COMMIT}, peso 400, grado 0, tamaño óptico 24.\n")
    out.append(f"// {len(filas)} símbolos, {bytes_trazados} bytes de trazados.\n\n")
    # Cada símbolo es una constante con su trazado: un programa solo lleva los que nombra (el enlazador tira los
    # demás). `TODOS` (para buscar por nombre) los lleva todos, y solo entra en quien busca por nombre.
    for nombre, lleno, d in filas:
        out.append(f"pub const {ident(nombre, lleno)}: Simbolo = Simbolo {{ nombre: \"{nombre}\", lleno: {'true' if lleno else 'false'}, d: \"{d}\" }};\n")
    out.append(f"\n/// Todos, por nombre y variante (de contorno antes que lleno), para buscar.\n")
    out.append(f"pub(crate) static TODOS: [Simbolo; {len(filas)}] = [\n")
    for nombre, lleno, _ in filas:
        out.append(f"    {ident(nombre, lleno)},\n")
    out.append("];\n")
    with open(SALIDA + ".nuevo", "w", encoding="utf-8") as f:
        f.write("".join(out))
    os.replace(SALIDA + ".nuevo", SALIDA)
    print(f"{len(filas)} símbolos ({bytes_trazados} bytes de trazados) en {os.path.relpath(SALIDA, RAIZ)}; commit {COMMIT}")


if __name__ == "__main__":
    main()
