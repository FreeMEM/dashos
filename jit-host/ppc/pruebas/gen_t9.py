#!/usr/bin/env python3
"""DashOS, fase F1e de la caja PowerPC: genera t9.s, FPSCR con el camino rápido de la FPU (PPC_FPSCR_EXCEPTIONS = 0).

Con el camino rápido, las operaciones sin punto dejan OX, UX, ZX y XX en el FPSR de ARM y no ponen FI, FPRF ni los
bits VX; FPSCR los recoge al leerse (mffs, mtfs*, mcrfs, una forma con punto) y al guardar el contexto. Esta prueba
solo usa secuencias en las que eso da lo mismo que qemu-ppc: operaciones sin punto que no son inválidas, seguidas de
una forma con punto (que pone FI y FPRF) o de una lectura de FPSCR con FI y FPRF ya puestos de antemano como los
deja la operación. Vale igual con PPC_FPSCR_EXCEPTIONS = 1. Registros de 16 bytes como t8: frD (f5), CR y FPSCR.

    python3 gen_t9.py > t9.s
"""
from pathlib import Path

from gen_t8 import Gen

BIG, TINY, ONE, ZERO, TENTH, THREE = 1e300, 1e-300, 1.0, 0.0, 0.1, 3.0


def main():
    g = Gen()
    g.out += ["        .section .text.prueba,\"ax\"", "        .globl prueba", "prueba:"]
    g.emit("lis 2,0x10", "lis 30,tabla@ha", "addi 30,30,tabla@l")
    for i, v in enumerate((BIG, TINY, ONE, ZERO, TENTH, THREE)):
        g.carga(10 + i, v)       # f10 grande, f11 pequeño, f12 1, f13 0, f14 0.1, f15 3

    def secuencia(ops, fpscr=0, d=""):
        g.fpscr(fpscr)
        g.emit("li 0,0", "mtcrf 0xff,0")
        g.carga(5, 0x5a5a5a5a5a5a5a5a)
        g.emit(*ops)
        g.guarda(" ; ".join(ops) + (f" FPSCR={fpscr:08x}" if fpscr else "") + d)

    sueltas = ["fmul 6,10,10", "fmul 6,11,11", "fdiv 6,12,13", "fadd 6,14,15", "fmuls 6,10,12", "frsp 6,11",
               "fadds 6,14,15", "fmadd 6,10,10,12", "fdivs 6,12,15", "fctiw 6,14"]
    finales = ["fadd. 5,12,12", "fmul. 5,14,15", "fsub. 5,10,10", "fdiv. 5,12,15", "fmadd. 5,12,12,12",
               "frsp. 5,14"]   # fctiwz. no: no pone FPRF, que se queda como lo dejó la anterior
    for f in (0, 0x02000000, 0x80000000, 0x9e000000, 0x00000003):
        for a in sueltas:
            for z in finales:
                secuencia((a, z), f)
        for a, b in (("fmul 6,10,10", "fdiv 6,12,13"), ("fmul 6,11,11", "fadd 6,14,15"), ("fdiv 6,12,13", "frsp 6,11")):
            secuencia((a, b, "fadd. 5,12,12"), f)
            secuencia((a, "mtfsb0 0", b, "fmul. 5,12,12"), f)
            secuencia((a, "mtfsfi 1,0", "fmul. 5,12,15"), f)
            secuencia((a, "mcrfs 2,0", "mcrfs 3,1", "fadd. 5,12,12"), f)
    # una lectura de FPSCR (sin forma con punto) con FI y FPRF puestos como los deja la operación
    for a, pre in (("fadd 6,14,15", 0x00024000), ("fmul 6,10,10", 0x00025000), ("fmul 6,11,11", 0x00022000),
                   ("fdiv 6,12,13", 0x00005000), ("fadd 6,12,12", 0x00004000)):
        secuencia((a, "mffs 5"), pre)
        secuencia((a, "mffs. 5"), pre)
        secuencia((a, "fmr. 5,14"), pre)
        secuencia((a, "mcrfs 4,0", "mcrfs 5,1", "mffs 5"), pre)
        secuencia((a, "mtfsb1 21", "mffs 5"), pre)
        # una llamada nativa en medio (el banco guarda y carga el contexto; el oráculo la hace en su SIGILL)
        secuencia((a, ".long 0x04000001", "mffs 5"), pre)
    # y la última, sin leer FPSCR: la recoge el banco al guardar el contexto al final
    g.fpscr(0x00024000)
    for f in (1, 2, 3, 4, 5, 6, 31):
        g.carga(f, 0x7fffffffffffffff)
    g.emit("fadd 7,14,15")
    for f in range(7, 16):
        g.carga(f, 0x7fffffffffffffff)
    g.emit("li 0,0", "mtcrf 0xff,0", "li 30,0", "li 2,0", "li 3,0", "blr")
    g.out += ["        .section .rodata", "        .align 3", "tabla:"]
    g.out += [f"        .quad {b:#x}" for b in g.tabla]
    print("\n".join(g.out))
    (Path(__file__).parent / "t9.map").write_text("\n".join(g.mapa) + "\n")


if __name__ == "__main__":
    main()
