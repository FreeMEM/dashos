#!/usr/bin/env python3
"""U7: pasa un ejecutable hunk del Amiga a un binario plano que empieza en una dirección fija (por defecto $100000):
los hunks de código, datos y bss uno detrás de otro (alineados a 8), con sus reubicaciones de 32 bits aplicadas. Se
empieza a ejecutar en el primer byte del primer hunk.

    hunk2bin.py programa programa.bin [base]
"""
import struct
import sys

HUNK_UNIT, HUNK_NAME, HUNK_CODE, HUNK_DATA, HUNK_BSS = 0x3E7, 0x3E8, 0x3E9, 0x3EA, 0x3EB
HUNK_RELOC32, HUNK_SYMBOL, HUNK_DEBUG, HUNK_END, HUNK_HEADER = 0x3EC, 0x3F0, 0x3F1, 0x3F2, 0x3F3
HUNK_RELOC32SHORT, HUNK_DREL32 = 0x3FC, 0x3F7


def main():
    d = open(sys.argv[1], "rb").read()
    base = int(sys.argv[3], 0) if len(sys.argv) > 3 else 0x100000
    p = 0

    def u32():
        nonlocal p
        v = struct.unpack_from(">I", d, p)[0]
        p += 4
        return v

    def u16():
        nonlocal p
        v = struct.unpack_from(">H", d, p)[0]
        p += 2
        return v

    assert u32() == HUNK_HEADER
    while u32():  # nombres de bibliotecas residentes (ninguno)
        pass
    n = u32()
    first, last = u32(), u32()
    sizes = [(u32() & 0x3FFFFFFF) * 4 for _ in range(last - first + 1)]
    addr, a = [], base
    for s in sizes:
        addr.append(a)
        a += (s + 7) & ~7
    img = bytearray(a - base)
    h = -1
    while p < len(d):
        t = u32() & 0x3FFFFFFF
        if t in (HUNK_CODE, HUNK_DATA):
            h += 1
            w = u32() * 4
            off = addr[h] - base
            img[off:off + w] = d[p:p + w]
            p += w
        elif t == HUNK_BSS:
            h += 1
            u32()
        elif t in (HUNK_RELOC32, HUNK_DREL32, HUNK_RELOC32SHORT):
            short = t != HUNK_RELOC32
            rd = u16 if short else u32
            while True:
                c = rd()
                if c == 0:
                    break
                to = rd()
                for _ in range(c):
                    o = addr[h] - base + rd()
                    v = struct.unpack_from(">I", img, o)[0] + addr[to]
                    struct.pack_into(">I", img, o, v & 0xFFFFFFFF)
            if short and p & 2:
                p += 2
        elif t == HUNK_SYMBOL:
            while True:
                l = u32()
                if l == 0:
                    break
                p += l * 4 + 4
        elif t == HUNK_DEBUG:
            p += u32() * 4
        elif t in (HUNK_END, HUNK_UNIT, HUNK_NAME):
            if t != HUNK_END:
                p += u32() * 4
        else:
            sys.exit(f"hunk {t:#x} desconocido en {p - 4:#x}")
    assert h + 1 == n, (h, n)
    open(sys.argv[2], "wb").write(img)


if __name__ == "__main__":
    main()
