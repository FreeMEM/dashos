#!/usr/bin/env python3
"""DashOS, fase F1e de la caja PowerPC: genera t8.s, los bits de excepción de FPSCR y las formas con punto de la FPU.

Cada caso parte de un FPSCR conocido (mtfsf 0xff; casi siempre 0), con CR a 0, hace una instrucción y deja en la
tabla de 0x00100000 un registro de 16 bytes: frD, CR (CR1 en las formas con punto) y FPSCR (mffs). Las aritméticas
(doble y simple, con y sin punto) con valores que dan cada excepción: inexacto, desbordamiento, subdesbordamiento,
división entre 0, y las inválidas (NaN con señal, inf - inf, inf / inf, 0 / 0, inf * 0, comparación con NaN,
conversión imposible); frsp, fctiw, fctiwz, fcmpu, fcmpo; las que no cambian FPSCR con punto; mtfsf, mtfsfi, mtfsb0,
mtfsb1 (FEX y VX no se ponen a mano; FX sí) y mcrfs; y secuencias (bits pegajosos, FI y FPRF de la última).

    python3 gen_t8.py > t8.s
"""
import struct
from pathlib import Path

INF, NINF = float("inf"), float("-inf")
QNAN, SNAN, NQNAN = 0x7ff8000000000000, 0x7ff0000000000001, 0xfff8000000000123
VAL = [0.0, -0.0, 1.0, -1.5, 0.1, 3.0, 1e300, -1e300, 1e-300, 5e-324, 2.2250738585072014e-308, 1.7976931348623157e308,
       INF, NINF, QNAN, SNAN]
CORTOS = [0.0, 0.1, 1e300, 1e-300, INF, NINF, QNAN, SNAN]
# sin 16777217.0 (2^24 + 1) junto a valores pequeños: el JIT calcula las de precisión simple en doble y redondea a
# simple después, y en ese caso el doble redondeo da otro resultado (pendiente, F1d)
SIMPLE = [1e38, 3e38, -3e38, 1e-38, 1e-45, 1e-30, 0.1, 1.0]
CONV = [0.0, -0.0, 1.5, -2.5, 0.1, 2147483647.0, 2147483648.0, -2147483648.0, -2147483649.0, 1e300, -1e300, 5e-324,
        INF, NINF, QNAN, SNAN, NQNAN]
FPSCR_INI = [0x00000000, 0x82000000, 0x9e000000, 0x80000000, 0x02000000, 0x00000003, 0x000000f8, 0x6ff80700,
             0xfffff007, 0x02020000]


def fex(f):
    """FEX: alguna excepción habilitada. qemu-ppc (linux-user) da SIGFPE en ese caso: esos casos no se prueban"""
    vx = 1 if f & 0x01f80700 else 0
    return bool(((f >> 25) & (f >> 3) & 0xf) or (vx and f & 0x80))


def bits(v):
    return v if isinstance(v, int) else struct.unpack(">Q", struct.pack(">d", v))[0]


def desc(v):
    return f"{v:#018x}" if isinstance(v, int) else repr(v)


class Gen:
    def __init__(self):
        self.out, self.mapa, self.tabla = [], ["#tam=16"], []

    def emit(self, *l):
        self.out.extend("        " + x for x in l)

    def idx(self, v):
        b = bits(v)
        if b not in self.tabla:
            self.tabla.append(b)
        return 8 * self.tabla.index(b)

    def carga(self, f, v):
        self.emit(f"lfd {f},{self.idx(v)}(30)")

    def fpscr(self, v):
        """FPSCR = v (mtfsf con todos los campos; FEX y VX salen de los demás)"""
        self.carga(31, v)
        self.emit("mtfsf 0xff,31")

    def guarda(self, d):
        self.mapa.append(d)
        self.emit("mffs 6", "stfd 5,0(2)", "stfd 6,8(2)", "mfcr 0", "stw 0,8(2)", "addi 2,2,16")

    def caso(self, insn, vals, fpscr=0, d=""):
        self.fpscr(fpscr)
        self.emit("li 0,0", "mtcrf 0xff,0")
        self.carga(5, 0x5a5a5a5a5a5a5a5a)
        for f, v in zip((1, 2, 3), vals):
            self.carga(f, v)
        self.emit(insn)
        self.guarda(f"{insn:20} " + " ".join(desc(v) for v in vals) + (f" FPSCR={fpscr:08x}" if fpscr else "") + d)


def main():
    g = Gen()
    g.out += ["        .section .text.prueba,\"ax\"", "        .globl prueba", "prueba:"]
    g.emit("lis 2,0x10", "lis 30,tabla@ha", "addi 30,30,tabla@l")
    for op in ("fadd", "fsub", "fmul", "fdiv"):
        for s in ("", "s"):
            for dot in ("", "."):
                for a in VAL:
                    for b in VAL:
                        g.caso(f"{op}{s}{dot} 5,1,2", (a, b))
            for a in SIMPLE:
                for b in SIMPLE:
                    g.caso(f"{op}{s}. 5,1,2", (a, b))
    for op in ("fmadd", "fmsub", "fnmadd", "fnmsub"):
        for s in ("", "s"):
            for a in CORTOS:
                for b in CORTOS:
                    for c in CORTOS:
                        g.caso(f"{op}{s}. 5,1,3,2", (a, b, c))
    for dot in ("", "."):
        for a in VAL + SIMPLE + CONV:
            g.caso(f"frsp{dot} 5,1", (a,))
        for a in CONV + VAL:
            for op in ("fctiw", "fctiwz"):
                g.caso(f"{op}{dot} 5,1", (a,))
    for a in CORTOS + [1.0]:
        for b in CORTOS + [1.0]:
            g.caso("fcmpu 1,1,2", (a, b))
            g.caso("fcmpo 6,1,2", (a, b))
    # sin excepciones, con punto: CR1 de FPSCR
    for f in FPSCR_INI:
        for a in (1.0, -0.0, QNAN, SNAN, INF):
            for op in ("fmr.", "fneg.", "fabs.", "fnabs."):
                g.caso(f"{op} 5,1", (a,), f)
            g.caso("fsel. 5,1,2,3", (a, 2.0, 3.0), f)
        g.caso("mffs. 5", (), f)
    # las de FPSCR
    for f in FPSCR_INI:
        for v in FPSCR_INI:
            for fm in (0xff, 0x80, 0x01, 0x7f, 0x81):
                mask = sum(0xf << (4 * i) for i in range(8) if fm & (1 << i))
                if fex((v & mask) | (f & ~mask)):
                    continue
                for dot in ("", "."):
                    g.caso(f"mtfsf{dot} {fm:#x},1", (v,), f, f" (v={v:08x})")
        for bf in range(8):
            for u in (0, 1, 9, 15):
                if not fex((f & ~(0xf << (28 - 4 * bf))) | (u << (28 - 4 * bf))):
                    g.caso(f"mtfsfi. {bf},{u}", (), f)
        for bt in (0, 1, 2, 3, 6, 7, 12, 13, 14, 15, 19, 21, 23, 24, 28, 31):
            g.caso(f"mtfsb0 {bt}", (), f)
            if not fex(f | (0x80000000 >> bt)):
                g.caso(f"mtfsb1. {bt}", (), f)
        for bf in range(8):
            g.caso(f"mcrfs 2,{bf}", (), f)
    # secuencias
    for f in (0, 0x02020000, 0x82020000, 0x20000000 | 0x01000000, 0x0007f000):
        for ops in (("fadd 5,1,2", "fmul 5,1,1"), ("fdiv 5,1,2", "fadd. 5,1,1"), ("fmul 5,2,2", "fsub. 5,1,1"),
                    ("mtfsb0 0", "fdiv. 5,1,2"), ("mtfsb0 6", "fadd. 5,2,2"), ("fadds 5,1,2", "frsp. 5,1"),
                    ("fdiv 5,1,4", "fcmpu 7,1,4", "fneg. 5,1"), ("fcmpu 7,1,2",), ("fcmpo 7,2,2",), ("fctiw 5,1",),
                    ("fctiw 5,4",), ("fctiwz 5,2",), ("fadd 5,2,2",), ("fadd 5,1,2",), ("fmr. 5,1",), ("fsel 5,1,2,4",),
                    ("fadd 1,1,4", "fmr 5,1"), ("fadd 4,1,4", "fmr 5,4"), ("fmadd 4,1,4,4", "fmr 5,4"),
                    ("fmul 1,4,1", "fmr 5,1"), ("fmsub 2,2,1,4", "fmr 5,2"),
                    # frD igual a una fuente, con un NaN silencioso en frA y uno con señal en otra: PPC da el de frA
                    ("fadd 3,3,4", "fmr 5,3"), ("fadd 4,3,4", "fmr 5,4"), ("fmul 4,3,4", "fmr 5,4"),
                    ("fdivs 4,3,4", "fmr 5,4"), ("fmadd 4,3,4,2", "fmr 5,4"), ("fnmsub. 2,3,4,2", "fmr 5,2"),
                    ("fsub. 3,4,3", "fmr 5,3")):
            g.fpscr(f)
            g.emit("li 0,0", "mtcrf 0xff,0")
            g.carga(1, 0.1)
            g.carga(2, 3.0)
            g.carga(3, NQNAN)
            g.carga(4, SNAN)
            g.emit(*ops)
            g.guarda(" ; ".join(ops) + f" (f1=0.1 f2=3.0 f3=-qNaN f4=sNaN) FPSCR={f:08x}")
    g.fpscr(0)
    for f in (1, 2, 3, 4, 5, 6, 31):
        g.carga(f, 0x7fffffffffffffff)
    g.emit("li 0,0", "mtcrf 0xff,0", "li 30,0", "li 2,0", "blr")
    g.out += ["        .section .rodata", "        .align 3", "tabla:"]
    g.out += [f"        .quad {b:#x}" for b in g.tabla]
    print("\n".join(g.out))
    (Path(__file__).parent / "t8.map").write_text("\n".join(g.mapa) + "\n")


if __name__ == "__main__":
    main()
