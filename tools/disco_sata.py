#!/usr/bin/env python3
"""Los discos SATA de prueba de `make -C kernel arranque-x86` (X5c).

crear RUTA N: un disco de 4 MB con "DashOS: sector 0 del disco SATA N" y la firma 0x55AA en el sector 0 (un MBR sin
particiones: `disco-sata` ve el primer MB) y "DashOS: sector 1" en el 1.
comprobar RUTA N: lo que escribió `disco-sata` en el disco, leído en Linux: el sector 2 con su texto y el 5 con el del
anillo. Sale con error si no está.
"""

import sys

SECTOR = 512


def crear(ruta, n):
    d = bytearray(4 << 20)
    s0 = f"DashOS: sector 0 del disco SATA {n}".encode()
    d[: len(s0)] = s0
    d[510:512] = b"\x55\xaa"
    s1 = b"DashOS: sector 1"
    d[SECTOR : SECTOR + len(s1)] = s1
    with open(ruta, "wb") as f:
        f.write(d)


def comprobar(ruta, n):
    with open(ruta, "rb") as f:
        d = f.read()
    sector = lambda k: d[k * SECTOR : (k + 1) * SECTOR]
    esperado = {2: f"DashOS: sector 2, escrito en el disco SATA {n}".encode(), 5: b"DashOS: sector 5, por el anillo"}
    mal = [k for k, t in esperado.items() if not sector(k).startswith(t)]
    if mal:
        raise SystemExit(f"MAL: {ruta}: sectores {mal} sin lo que escribió disco-sata")
    print(f"BIEN: {ruta}: los sectores 2 y 5 tienen lo que escribió disco-sata")


if __name__ == "__main__":
    if len(sys.argv) != 4 or sys.argv[1] not in ("crear", "comprobar"):
        raise SystemExit(__doc__)
    (crear if sys.argv[1] == "crear" else comprobar)(sys.argv[2], int(sys.argv[3]))
