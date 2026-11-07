#!/usr/bin/env python3
"""DashOS, fase F1 de la caja PowerPC: genera t4.s, los saltos condicionales.

Cada BO válido (con y sin restar CTR, con la condición a 1, a 0 o sin ella, y los bits de predicción y) con varios
BI, CR y CTR, en bc, bcl, bclr, bclrl, bcctr y bcctrl. Registros de 12 bytes en 0x00100000 (r2): si saltó (1) o
no (0), CTR y LR después.

    python3 gen_t4.py > t4.s
"""
from pathlib import Path

# BO: 0000y resta y salta si CTR != 0 y cond falsa; 0001y CTR == 0 y falsa; 001zy falsa; 0100y CTR != 0 y cierta;
# 0101y CTR == 0 y cierta; 011zy cierta; 1z00y CTR != 0; 1z01y CTR == 0; 1z1zz siempre
BOS = [0, 1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13, 16, 17, 18, 19, 20]   # los que admite el ensamblador (z a 0)
BIS = [0, 2, 5, 31]
CRS = [0x00000000, 0xa0000005, 0x24000001]
CTRS = [0, 1, 2, 0xffffffff]


class Gen:
    def __init__(self):
        self.out, self.mapa, self.n = [], [], 0

    def emit(self, *l):
        self.out.extend(l)

    def etiqueta(self):
        self.n += 1
        return f"e{self.n}"

    def guarda(self, d):
        self.mapa.append(d)
        self.emit("stw 5,0(2)", "mfctr 0", "stw 0,4(2)", "mflr 0", "stw 0,8(2)", "addi 2,2,12")


def main():
    g = Gen()
    g.emit("        .section .text.prueba,\"ax\"", "        .globl prueba", "prueba:")
    g.emit("mflr 0", "stw 0,4(1)", "stwu 1,-16(1)", "lis 2,0x10")
    for forma in ("bc", "bcl", "bclr", "bclrl", "bcctr", "bcctrl"):
        for bo in BOS:
            if forma.startswith("bcctr") and not bo & 4:
                continue        # bcctr que resta CTR: forma no válida
            for bi in BIS:
                for cr in CRS:
                    for ctr in (CTRS if not forma.startswith("bcctr") else [0]):
                        si, fin = g.etiqueta(), g.etiqueta()
                        g.emit(f"lis 6,{cr >> 16:#x}", f"ori 6,6,{cr & 0xffff:#x}", "mtcrf 0xff,6",
                               f"lis 6,{ctr >> 16:#x}", f"ori 6,6,{ctr & 0xffff:#x}", "mtctr 6",
                               "li 0,0", "mtlr 0")
                        if forma.startswith("bclr") or forma.startswith("bcctr"):
                            g.emit(f"lis 7,{si}@ha", f"addi 7,7,{si}@l",
                                   "mtlr 7" if forma.startswith("bclr") else "mtctr 7")
                        destino = f",{si}" if forma in ("bc", "bcl") else ""
                        g.emit(f"{forma} {bo},{bi}{destino}", "li 5,0", f"b {fin}", f"{si}:", "li 5,1", f"{fin}:")
                        g.guarda(f"{forma} {bo},{bi} CR={cr:08x} CTR={ctr:08x}")
    g.emit("addi 1,1,16", "lwz 0,4(1)", "mtlr 0", "li 0,0", "mtcrf 0xff,0", "mtctr 0")
    for r in (2, 5, 6, 7):
        g.emit(f"li {r},0")
    g.emit("blr")
    print("\n".join(("        " + l) if not (l.startswith((" ", "prueba:")) or l.endswith(":")) else l
                    for l in g.out))
    (Path(__file__).parent / "t4.map").write_text("\n".join(g.mapa) + "\n")


if __name__ == "__main__":
    main()
