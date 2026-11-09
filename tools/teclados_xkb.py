#!/usr/bin/env python3
"""Compara los mapas de teclado del kit (`kit/src/teclado.rs`) con los de xkb de este equipo (`xkbcli compile-keymap`).

Para cada mapa y cada tecla de la tabla, nivel a nivel (normal, mayúsculas, AltGr): lo que da el kit debe ser lo que da
xkb, y una tecla muerta del kit debe ser muerta en xkb. Donde el kit no da nada y xkb sí, solo se cuenta (son signos
que el kit no tiene, no errores).
"""

import re
import subprocess
import sys

CODIGO_A_XKB = {41: "TLDE", 43: "BKSL", 86: "LSGT", 57: "SPCE"}
CODIGO_A_XKB.update({2 + i: f"AE{i + 1:02d}" for i in range(12)})
CODIGO_A_XKB.update({16 + i: f"AD{i + 1:02d}" for i in range(12)})
CODIGO_A_XKB.update({30 + i: f"AC{i + 1:02d}" for i in range(11)})
CODIGO_A_XKB.update({44 + i: f"AB{i + 1:02d}" for i in range(10)})

MUERTAS = {"dead_acute": "´", "dead_grave": "`", "dead_circumflex": "^", "dead_diaeresis": "¨", "dead_tilde": "~"}


def nombres():
    t = {}
    for l in open("/usr/include/X11/keysymdef.h", encoding="latin-1"):
        m = re.match(r"#define XK_(\w+)\s+0x([0-9a-fA-F]+)(.*)", l)
        if not m:
            continue
        u = re.search(r"U\+([0-9A-Fa-f]{4,6})", m.group(3))
        v = int(m.group(2), 16)
        # Con U+ en el comentario, ese; si no, los de Latin-1 valen su propio número, y 0x01xxxxxx es Unicode.
        if u:
            t.setdefault(m.group(1), chr(int(u.group(1), 16)))
        elif v < 0x100:
            t.setdefault(m.group(1), chr(v))
        elif v >> 24 == 1:
            t.setdefault(m.group(1), chr(v & 0xFFFFFF))
    return t


def xkb(disposicion):
    salida = subprocess.run(["xkbcli", "compile-keymap", "--layout", disposicion], capture_output=True, text=True, check=True).stdout
    teclas = {}
    for m in re.finditer(r"key <(\w+)>\s*\{(.*?)\};", salida, re.S):
        # Los símbolos son la primera lista entre corchetes que no es el índice de "symbols[1]".
        cuerpo = re.sub(r"symbols\[\d+\]", "", m.group(2))
        lista = re.search(r"\[([^\]]*)\]", cuerpo)
        if lista:
            teclas.setdefault(m.group(1), [x.strip() for x in lista.group(1).split(",")])
    return teclas


def caracter(s):
    s = s[1:-1]
    return {"\\\\": "\\", "\\'": "'", "\\0": ""}.get(s, s)


def kit(ruta):
    fuente = open(ruta, encoding="utf-8").read()
    mapas = {}
    qwerty = re.search(r"const QWERTY: \[Entrada; \d+\] = \[(.*?)\];", fuente, re.S).group(1)
    for m in re.finditer(r'pub static (\w+): Mapa = Mapa \{\s*nombre: "(\w+)",\s*propias: &\[(.*?)\],\s*\};', fuente, re.S):
        tabla = {}
        for cuerpo in (m.group(3), qwerty):
            for e in re.finditer(r"\((\d+), \[('(?:\\.|[^'])*'|N), ('(?:\\.|[^'])*'|N), ('(?:\\.|[^'])*'|N)\], (0b\d+|0)\)", cuerpo):
                codigo = int(e.group(1))
                if codigo in tabla:
                    continue
                niveles = ["" if x == "N" else caracter(x) for x in e.group(2, 3, 4)]
                tabla[codigo] = (niveles, int(e.group(5), 0))
        mapas[m.group(2)] = tabla
    return mapas


def main():
    ruta = sys.argv[1] if len(sys.argv) > 1 else "kit/src/teclado.rs"
    ks = nombres()
    fallos, faltan = 0, 0
    for nombre, tabla in kit(ruta).items():
        x = xkb(nombre)
        for codigo, (niveles, muertas) in sorted(tabla.items()):
            simbolos = x.get(CODIGO_A_XKB[codigo], [])
            # xkb: [normal, mayúsculas, AltGr, AltGr+mayúsculas]; el espacio solo tiene uno.
            for nivel in range(3):
                s = simbolos[nivel] if nivel < len(simbolos) else (simbolos[0] if simbolos and codigo == 57 else "")
                muerta = s in MUERTAS
                esperado = MUERTAS.get(s) or {"space": " "}.get(s) or ks.get(s, s if len(s) == 1 else "")
                if s == "NoSymbol":
                    esperado = ""
                dado = niveles[nivel]
                if dado == "":
                    faltan += esperado != ""
                    continue
                if dado != esperado or bool(muertas & (1 << nivel)) != muerta:
                    fallos += 1
                    print(f"MAL {nombre} tecla {codigo} ({CODIGO_A_XKB[codigo]}) nivel {nivel}: el kit da {dado!r}"
                          f"{' muerta' if muertas & (1 << nivel) else ''}, xkb {s} ({esperado!r}{' muerta' if muerta else ''})")
    print(f"{'BIEN' if not fallos else 'MAL'}: {fallos} diferencias; {faltan} signos de xkb que el kit no da")
    sys.exit(1 if fallos else 0)


if __name__ == "__main__":
    main()
