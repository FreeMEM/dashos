#!/usr/bin/env python3
"""B4: las huellas de las capturas de FS-UAE de captura.sh, para tests/denise_oraculo.rs.

    python3 -I huellas.py DIR > huellas.txt

Toma las capturas "full" de DIR (752 x 572) en orden. Cada modo de denise.exe se queda quieto unos segundos: las
capturas seguidas que son iguales son un modo (las de en medio, con el mapa a medio dibujar, no se repiten). Los
primeros grupos de dos o más iguales con el borde de denise.exe, tantos como modos tiene caja/pruebas68k/denise_modos.h, son los modos en
orden. De cada uno, las líneas 26 a 311 (cada una sale dos veces en la captura: se comprueba que son iguales) y, de
cada línea, la huella FNV-1a de 32 bits de sus bytes R, G, B.

El oráculo es un A1200 (AGA): en EHB parte a la mitad la componente de 8 bits (un 5 a media luz es $2A), y OCS la
de 4 (un 5 es un 2, $22). En los modos EHB, cada componente que no es múltiplo de 17 (solo salen de esa media luz)
se pasa a la de OCS: c = (2w + 1) / 17 y (c / 2) * 17.
"""
import re
import sys
from pathlib import Path

from PIL import Image

aqui = Path(__file__).resolve().parent
modos = [l for l in (aqui / "../../../pruebas68k/denise_modos.h").read_text().splitlines() if l.startswith("MODO(")]
bplcon0 = [int(re.match(r"MODO\((\w+)", l).group(1), 0) for l in modos]
ehb = [(c >> 12 & 7) == 6 and not c & 0x0C00 for c in bplcon0]

capturas = sorted(Path(sys.argv[1]).glob("fs-uae-full-*.png"))
grupos = []
for f in capturas:
    datos = Image.open(f).convert("RGB").tobytes()
    if grupos and grupos[-1][0] == datos:
        grupos[-1][1].append(f.name)
    else:
        grupos.append((datos, [f.name]))
# Solo las de denise.exe: en la línea $30, a la izquierda, el borde de su paleta (COLOR00 = $0A5).
def de_denise(datos):
    i = (2 * (0x30 - 26) * 752 + 2) * 3
    return datos[i:i + 3] == bytes([0x00, 0xAA, 0x55])

estables = [g for g in grupos if len(g[1]) >= 2 and de_denise(g[0])][:len(modos)]
assert len(estables) == len(modos), f"{len(estables)} modos estables de {len(modos)}"

def ocs(w):
    return w if w % 17 == 0 else ((2 * w + 1) // 17 >> 1) * 17

ANCHO, ALTO = 752, 286
print("# B4: huellas de las capturas de FS-UAE (huellas.py): modo, línea, FNV-1a de 32 bits de la línea (R, G, B).")
for n, (datos, nombres) in enumerate(estables):
    print(f"# modo {n + 1}: {len(nombres)} capturas iguales ({nombres[0]}...)")
    assert len(datos) == ANCHO * 2 * ALTO * 3
    for y in range(ALTO):
        fila = datos[2 * y * ANCHO * 3:(2 * y + 1) * ANCHO * 3]
        assert fila == datos[(2 * y + 1) * ANCHO * 3:(2 * y + 2) * ANCHO * 3], f"modo {n + 1}, línea {y + 26} no doblada"
        if ehb[n]:
            fila = bytes(ocs(w) for w in fila)
        h = 0x811C9DC5
        for b in fila:
            h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
        print(f"{n + 1} {y + 26} {h:08x}")
