#!/usr/bin/env python3
"""DashOS, fase F1h de la caja PowerPC: genera t5.s, mullwo, divwo y divwuo (con y sin punto).

Como t1 (gen_t1.py): cada par de valores de borde, también la división entre 0 y 0x80000000 / -1, que PPC deja sin
definir (rD) pero que con OE ponen OV; ahí rD sale como en qemu-ppc. XER entra con SO a 0 y a 1 (SO es pegajoso) y
con CA a 1 (no se toca). Tras cada una, rD, CR y XER a la tabla de 0x00100000.

    python3 gen_t5.py > t5.s
"""
from pathlib import Path

from gen_t1 import VALORES, Gen, li32

VALORES5 = VALORES + [0xffff8000, 0x00010000, 0x00008000, 0x40000000, 0xc0000000]
XER_IN = [0, 0x80000000, 0x20000000]


def main():
    g = Gen()
    g.emit("        .section .text.prueba,\"ax\"", "        .globl prueba", "prueba:")
    g.emit("mflr 0", "stw 0,4(1)", "stwu 1,-16(1)")
    g.emit(*li32("31", 0x00100000))
    for op in ("mullwo", "divwo", "divwuo"):
        for dot in ("", "."):
            for a in VALORES5:
                for b in VALORES5:
                    for xer in XER_IN:
                        g.ahora = f"{op + dot:8} r3={a:08x} r4={b:08x} XER={xer:08x}"
                        g.emit(*li32("3", a), *li32("4", b), *li32("5", 0x5a5a5a5a))
                        g.emit(*li32("29", xer), "mtxer 29", "li 0,0", "mtcrf 0xff,0")
                        g.emit(f"{op}{dot} 5,3,4")
                        g.guarda()
    # rD igual a una fuente
    for a, b in ((7, 0), (0x80000000, 0xffffffff), (100, 7), (0x10000, 0x10000)):
        for op in ("mullwo.", "divwo.", "divwuo."):
            for dest in ("3", "4"):
                g.ahora = f"{op:8} {dest},3,4 r3={a:08x} r4={b:08x}"
                g.emit(*li32("3", a), *li32("4", b), "li 0,0", "mtxer 0", "mtcrf 0xff,0")
                g.emit(f"{op} {dest},3,4", f"mr 5,{dest}")
                g.guarda()
    g.emit("addi 1,1,16", "lwz 0,4(1)", "mtlr 0", "li 0,0", "mtxer 0", "mtcrf 0xff,0", "blr")
    (Path(__file__).parent / "t5.map").write_text("\n".join(g.mapa) + "\n")
    print("\n".join(("        " + l) if not l.startswith((" ", "prueba:")) else l for l in g.out))


if __name__ == "__main__":
    main()
