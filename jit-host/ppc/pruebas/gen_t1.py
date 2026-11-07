#!/usr/bin/env python3
"""DashOS, fase F1 de la caja PowerPC: genera t1.s, las instrucciones de enteros con valores de borde.

Cada instrucción se prueba con cada par de valores de VALORES (y, las que leen el acarreo, con CA a 0 y a 1). Tras
cada una se guardan en la tabla de resultados (desde 0x00100000, r31) el registro destino, CR y XER: el banco y
qemu-ppc tienen que dejar la misma tabla. XER se limpia antes de cada instrucción (SO es pegajoso), y CR también.

    python3 gen_t1.py a > t1a.s   # suma, resta, multiplicación (con y sin OE)
    python3 gen_t1.py b > t1b.s   # división, lógicas, inmediatos, comparaciones, rotaciones, CR
"""
import random
import sys

VALORES = [0, 1, 0xffffffff, 0x7fffffff, 0x80000000, 0x12345678, 0xfffffffe, 0x80000001, 0x0000ffff, 0xabcd0000]

# r3, r4: operandos; r5: destino; r29: XER de entrada; r31: tabla de resultados
XO = ["add", "addc", "adde", "subf", "subfc", "subfe", "mullw", "mulhw", "mulhwu"]
XO_UNO = ["addme", "addze", "subfme", "subfze", "neg"]
LEEN_CA = {"adde", "subfe", "addme", "addze", "subfme", "subfze"}
CON_OE = ["add", "addc", "adde", "subf", "subfc", "subfe", "addme", "addze", "subfme", "subfze", "neg"]
LOGICAS = ["and", "andc", "or", "orc", "xor", "nand", "nor", "eqv", "slw", "srw", "sraw"]
UNARIAS = ["cntlzw", "extsb", "extsh"]
INMEDIATOS = [0, 1, -1, 0x7fff, -0x8000, 0x1234, -2]


def li32(r, v):
    v &= 0xffffffff
    return [f"lis {r},{v >> 16:#x}", f"ori {r},{r},{v & 0xffff:#x}"]


class Gen:
    def __init__(self):
        self.out = []
        self.mapa = []      # qué es cada registro de 12 bytes de la tabla (para comparar_ppc.py)
        self.ahora = ""

    def emit(self, *lines):
        self.out.extend(lines)

    def guarda(self):
        """r5, CR y XER a la tabla"""
        self.mapa.append(self.ahora)
        self.emit("stw 5,0(31)", "mfcr 0", "stw 0,4(31)", "mfxer 0", "stw 0,8(31)", "addi 31,31,12")

    def prepara(self, a, b, ca=0):
        self.ahora = f"r3={a:08x} r4={b:08x} CA={ca}"
        self.emit(*li32("3", a), *li32("4", b), *li32("5", 0x5a5a5a5a))
        self.emit(*li32("29", 0x20000000 if ca else 0), "mtxer 29", "li 0,0", "mtcrf 0xff,0")

    def prueba(self, insn, a, b, ca=0):
        self.prepara(a, b, ca)
        self.ahora = f"{insn:24} " + self.ahora
        self.emit(insn)
        self.guarda()


def main(parte):
    random.seed(1107)
    g = Gen()
    g.emit("        .section .text.prueba,\"ax\"", "        .globl prueba", "prueba:")
    g.emit("mflr 0", "stw 0,4(1)", "stwu 1,-16(1)")
    g.emit(*li32("31", 0x00100000))
    pares = [(a, b) for a in VALORES for b in VALORES]
    a_ = parte == "a"
    for op in XO if a_ else []:
        for dot in ("", "."):
            for a, b in pares:
                for ca in ((0, 1) if op in LEEN_CA else (0,)):
                    g.prueba(f"{op}{dot} 5,3,4", a, b, ca)
    for op in XO_UNO if a_ else []:
        for dot in ("", "."):
            for a in VALORES:
                for ca in ((0, 1) if op in LEEN_CA else (0,)):
                    g.prueba(f"{op}{dot} 5,3", a, 0, ca)
    for op in CON_OE if a_ else []:
        for dot in ("", "."):
            for a, b in (pares if op not in XO_UNO else [(v, 0) for v in VALORES]):
                for ca in ((0, 1) if op in LEEN_CA else (0,)):
                    g.prueba(f"{op}o{dot} 5,3" + ("" if op in XO_UNO else ",4"), a, b, ca)
    # divisiones: sin las que PPC deja sin definir (entre 0, y 0x80000000 / -1 con signo)
    if a_:
        return fin(g)
    for op in ("divw", "divwu"):
        for dot in ("", "."):
            for a, b in pares:
                if b == 0 or (op == "divw" and a == 0x80000000 and b == 0xffffffff):
                    continue
                g.prueba(f"{op}{dot} 5,3,4", a, b)
    for op in LOGICAS:
        for dot in ("", "."):
            for a, b in pares + [(v, s) for v in VALORES for s in (31, 32, 33, 63, 64, 5)]:
                g.prueba(f"{op}{dot} 5,3,4", a, b)
    for op in UNARIAS:
        for dot in ("", "."):
            for a in VALORES:
                g.prueba(f"{op}{dot} 5,3", a, 0)
    for a in VALORES:
        for i in INMEDIATOS:
            for insn in (f"addi 5,3,{i}", f"addis 5,3,{i}", f"addic 5,3,{i}", f"addic. 5,3,{i}",
                         f"subfic 5,3,{i}", f"mulli 5,3,{i}"):
                g.prueba(insn, a, 0)
            u = i & 0xffff
            for insn in (f"andi. 5,3,{u}", f"andis. 5,3,{u}", f"ori 5,3,{u}", f"oris 5,3,{u}",
                         f"xori 5,3,{u}", f"xoris 5,3,{u}"):
                g.prueba(insn, a, 0)
            for cr in (0, 3, 7):
                g.prueba(f"cmpwi {cr},3,{i}", a, 0)
                g.prueba(f"cmplwi {cr},3,{u}", a, 0)
        for sh in (0, 1, 15, 31):
            for dot in ("", "."):
                g.prueba(f"srawi{dot} 5,3,{sh}", a, 0)
    for a, b in pares:
        for cr in (0, 5):
            g.prueba(f"cmpw {cr},3,4", a, b)
            g.prueba(f"cmplw {cr},3,4", a, b)
    # rotaciones con máscara: combinaciones al azar más las de borde
    combos = [(0, 0, 31), (0, 16, 15), (31, 0, 0), (8, 24, 7), (0, 24, 19), (4, 31, 0), (1, 0, 30)]
    combos += [(random.randrange(32), random.randrange(32), random.randrange(32)) for _ in range(40)]
    for sh, mb, me in combos:
        for dot in ("", "."):
            for a in VALORES:
                g.prueba(f"rlwinm{dot} 5,3,{sh},{mb},{me}", a, 0)
                g.prepara(a, 0x0f0f0f0f)
                g.ahora = f"rlwimi{dot} 5,3,{sh},{mb},{me} (r5=r4) " + g.ahora
                g.emit("mr 5,4", f"rlwimi{dot} 5,3,{sh},{mb},{me}")
                g.guarda()
                g.prepara(a, sh + 32 * (sh & 1))
                g.ahora = f"rlwnm{dot} 5,3,4,{mb},{me} " + g.ahora
                g.emit(f"rlwnm{dot} 5,3,4,{mb},{me}")
                g.guarda()
    # campos de CR
    for a, b in pares[::3]:
        g.prepara(a, b)
        g.emit("cmpw 1,3,4", "cmplw 2,4,3", "cmpwi 6,3,0")
        base = g.ahora
        for op in ("crand", "cror", "crxor", "crnand", "crnor", "creqv", "crandc", "crorc"):
            g.ahora = f"cmpw 1/cmplw 2/cmpwi 6; {op} 1,5,10 " + base
            g.emit(f"{op} 1,5,10", "mfcr 5")
            g.guarda()
        g.ahora = "mcrf 7,1 " + base
        g.emit("mcrf 7,1", "mfcr 5")
        g.guarda()
        g.ahora = "mtcrf 0x81,3 " + base
        g.emit("mtcrf 0x81,3", "mfcr 5")
        g.guarda()
        g.ahora = "mcrxr 4 (XER=e0000000) " + base
        g.emit(*li32("29", 0xe0000000), "mtxer 29", "mcrxr 4", "mfcr 5")
        g.guarda()
    fin(g)


def fin(g):
    g.emit("addi 1,1,16", "lwz 0,4(1)", "mtlr 0", "li 0,0", "mtxer 0", "mtcrf 0xff,0", "blr")
    from pathlib import Path
    (Path(__file__).parent / f"t1{PARTE}.map").write_text("\n".join(g.mapa) + "\n")
    print("\n".join(("        " + l) if not l.startswith((" ", "prueba:")) else l for l in g.out))


PARTE = sys.argv[1] if len(sys.argv) > 1 else "a"

if __name__ == "__main__":
    main(PARTE)
