#!/usr/bin/env python3
"""DashOS, fase F1 de la caja PowerPC: genera t3.s, la FPU con valores de borde.

Los operandos salen de una tabla (0, -0, 1, -1, 1.5, 0.1, 1e300, -1e-300, el menor denormal, el mayor doble,
infinitos, un NaN silencioso y otro con señal...). Cada operación deja frD en la tabla de resultados (0x00100000,
r2) con CR: registros de 12 bytes (frD, 8 bytes, y CR). Las de precisión simple, fsel, fneg, fabs, fnabs, frsp,
fctiw, fctiwz y fcmpu, y también fctiw, frsp y fadd con los cuatro modos de redondeo (mtfsfi). FPSCR no se compara:
el JIT no lleva sus bits de excepción.

    python3 gen_t3.py > t3.s
"""
import struct
from pathlib import Path

VALORES = [0.0, -0.0, 1.0, -1.0, 1.5, 0.1, 3.0, -2.75, 1e300, -1e-300, 5e-324, 1.7976931348623157e308,
           float("inf"), float("-inf")]
EXTRA = [0x7ff8000000000000, 0x7ff0000000000001, 0xfff8000000000123]   # qNaN, sNaN, -qNaN con datos
CORTOS = [0.0, -1.0, 1.5, 0.1, 1e300, float("inf"), 0x7ff8000000000000]
REDONDEO = [0.5, 1.5, 2.5, -0.5, -1.5, 0.1, 1e10, -3.7, 2147483647.5, -2147483648.6, 1e-310]


def bits(v):
    return v if isinstance(v, int) else struct.unpack(">Q", struct.pack(">d", v))[0]


def desc(v):
    return f"{v:#018x}" if isinstance(v, int) else repr(v)


class Gen:
    def __init__(self):
        self.out, self.mapa, self.tabla = [], [], []

    def emit(self, *l):
        self.out.extend(l)

    def idx(self, v):
        b = bits(v)
        if b not in self.tabla:
            self.tabla.append(b)
        return 8 * self.tabla.index(b)

    def carga(self, f, v):
        self.emit(f"lfd {f},{self.idx(v)}(30)")

    def guarda(self, d):
        self.mapa.append(d)
        self.emit("stfd 5,0(2)", "mfcr 0", "stw 0,8(2)", "addi 2,2,12")


def main():
    g = Gen()
    g.emit("        .section .text.prueba,\"ax\"", "        .globl prueba", "prueba:")
    g.emit("mflr 0", "stw 0,4(1)", "stwu 1,-16(1)")
    g.emit("lis 2,0x10", "lis 30,tabla@ha", "addi 30,30,tabla@l")
    todos = VALORES + EXTRA

    def prueba(insn, vals, d):
        g.emit("li 0,0", "mtcrf 0xff,0")
        g.carga(5, 0x5a5a5a5a5a5a5a5a)
        for f, v in zip((1, 2, 3), vals):
            g.carga(f, v)
        g.emit(insn)
        g.guarda(f"{insn:22} " + " ".join(desc(v) for v in vals) + d)

    for op in ("fadd", "fsub", "fmul", "fdiv"):
        for s in ("", "s"):
            for a in todos:
                for b in todos:
                    prueba(f"{op}{s} 5,1,2", (a, b), "")
    for op in ("fmadd", "fmsub", "fnmadd", "fnmsub"):
        for s in ("", "s"):
            for a in CORTOS:
                for b in CORTOS:
                    for c in CORTOS:
                        prueba(f"{op}{s} 5,1,3,2", (a, b, c), "")
    for a in todos:
        for b in CORTOS:
            prueba("fsel 5,1,2,3", (a, b, -7.0), "")
        for op in ("fneg", "fabs", "fnabs", "frsp", "fctiw", "fctiwz", "fmr"):
            prueba(f"{op} 5,1", (a,), "")
        for b in todos:
            prueba("fcmpu 3,1,2", (a, b), "")
            prueba("fcmpo 6,1,2", (a, b), "")
    for rn in (1, 2, 3, 0):
        g.emit(f"mtfsfi 7,{rn}")
        for a in REDONDEO:
            for op in ("fctiw", "frsp"):
                prueba(f"{op} 5,1", (a,), f" (RN={rn})")
            prueba("fadd 5,1,2", (a, 1e-17), f" (RN={rn})")
            prueba("fadds 5,1,2", (a, 3e-9), f" (RN={rn})")
        # mffs: solo los bits de control (RN, NI, habilitaciones)
        g.emit("mffs 5", "stfd 5,-8(1)", "lwz 0,-4(1)", "andi. 0,0,0xff", "stw 0,-4(1)", "lfd 5,-8(1)")
        g.guarda(f"mffs & 0xff (RN={rn})")
    for f in (1, 2, 3, 5):
        g.carga(f, 0x7fffffffffffffff)
    g.emit("addi 1,1,16", "lwz 0,4(1)", "mtlr 0", "li 0,0", "mtcrf 0xff,0", "li 30,0", "li 2,0", "blr")
    lineas = [("        " + l) if not l.startswith((" ", "prueba:")) else l for l in g.out]
    lineas += ["        .section .rodata", "        .align 3", "tabla:"]
    lineas += [f"        .quad {b:#x}" for b in g.tabla]
    print("\n".join(lineas))
    (Path(__file__).parent / "t3.map").write_text("\n".join(g.mapa) + "\n")


if __name__ == "__main__":
    main()
