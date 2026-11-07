#!/usr/bin/env python3
"""DashOS, fase F1 de la caja PowerPC: genera t2.s, las cargas y los guardados.

Una zona de datos (0x00110000..0x0011ffff) se llena con un patrón al empezar; r3 apunta a su mitad. Cada carga y
cada guardado se prueba con desplazamientos de borde (también negativos más allá de -256, impares y fuera de los
rangos de las instrucciones de ARM), en las formas D, X, con actualización, con bytes al revés, y lmw/stmw,
lwarx/stwcx. y dcbz. La tabla de resultados va en 0x00100000 con r2 de puntero (r31 lo pisa lmw), en registros de
12 bytes: el valor cargado (o releído tras guardar), r3 después (las formas con actualización) y CR.

    python3 gen_t2.py > t2.s
"""
from pathlib import Path

DATOS = 0x00110000
MITAD = 0x00118000
DESPL = [0, 4, -4, 1, 3, -1, 255, -256, 256, -257, 0x3ffc, 0x4000, 0x7ffc, 0x7fff, -0x8000, -0x7ffd, 0x1001, -0x1234]
INDICES = [0, 8, -8, 0x1235, -0x4321, 0x7ff0, -0x7ff0]


def li32(r, v):
    v &= 0xffffffff
    return [f"lis {r},{v >> 16:#x}", f"ori {r},{r},{v & 0xffff:#x}"]


class Gen:
    def __init__(self):
        self.out, self.mapa = [], []

    def emit(self, *l):
        self.out.extend(l)

    def guarda(self, desc, reg="5"):
        self.mapa.append(desc)
        self.emit(f"stw {reg},0(2)", "stw 3,4(2)", "mfcr 0", "stw 0,8(2)", "addi 2,2,12")

    def base(self, off=0):
        self.emit(*li32("3", MITAD + off), *li32("5", 0x5a5a5a5a), "li 0,0", "mtcrf 0xff,0")


def main():
    g = Gen()
    g.emit("        .section .text.prueba,\"ax\"", "        .globl prueba", "prueba:")
    g.emit("mflr 0", "stw 0,4(1)", "stwu 1,-16(1)")
    g.emit(*li32("2", 0x00100000))
    # el patrón: palabra en p = p * 0x01000193 + 0x811c9dc5
    g.emit(*li32("6", DATOS), *li32("7", 0x01000193), *li32("8", 0x811c9dc5), *li32("9", DATOS + 0x10000),
           "1:", "mullw 10,6,7", "add 10,10,8", "stw 10,0(6)", "addi 6,6,4", "cmplw 6,9", "blt 1b")
    cargas = {"lbz": 1, "lhz": 2, "lha": 2, "lwz": 4}
    for op, tam in cargas.items():
        for d in DESPL:
            g.base()
            g.emit(f"{op} 5,{d}(3)")
            g.guarda(f"{op} 5,{d}(3)")
            if d != 0:
                g.base()
                g.emit(f"{op}u 5,{d}(3)")
                g.guarda(f"{op}u 5,{d}(3)")
        for i in INDICES:
            g.base()
            g.emit(*li32("4", i), f"{op}x 5,3,4")
            g.guarda(f"{op}x 5,3,4 (r4={i})")
            g.base()
            g.emit(*li32("4", i), f"{op}ux 5,3,4")
            g.guarda(f"{op}ux 5,3,4 (r4={i})")
        g.base()
        g.emit(*li32("4", MITAD + 0x123), f"{op}x 5,0,4")
        g.guarda(f"{op}x 5,0,4 (rA = 0)")
    for op in ("lhbrx", "lwbrx"):
        for i in INDICES + [1, 3]:
            g.base()
            g.emit(*li32("4", i), f"{op} 5,3,4")
            g.guarda(f"{op} 5,3,4 (r4={i})")
    guardados = {"stb": "lbz", "sth": "lhz", "stw": "lwz"}
    valor = 0x89abcdef
    for op, relee in guardados.items():
        for d in DESPL:
            for u in ("", "u"):
                if u and d == 0:
                    continue
                g.base()
                g.emit(*li32("6", valor), f"{op}{u} 6,{d}(3)", f"{relee} 5,{d if not u else 0}(3)")
                g.guarda(f"{op}{u} 6,{d}(3) y {relee}")
                valor = (valor * 0x9e3779b1 + 7) & 0xffffffff
        for i in INDICES:
            for u in ("", "u"):
                g.base()
                g.emit(*li32("4", i), *li32("6", valor), f"{op}{u}x 6,3,4",
                       f"{relee}x 5,3,4" if not u else f"{relee} 5,0(3)")
                g.guarda(f"{op}{u}x 6,3,4 (r4={i}) y {relee}")
                valor = (valor * 0x9e3779b1 + 7) & 0xffffffff
    for op, relee in (("sthbrx", "lhz"), ("stwbrx", "lwz")):
        for i in INDICES + [1, 3]:
            g.base()
            g.emit(*li32("4", i), *li32("6", valor), f"{op} 6,3,4", "add 7,3,4", f"{relee} 5,0(7)")
            g.guarda(f"{op} 6,3,4 (r4={i}) y {relee}")
            valor = (valor * 0x9e3779b1 + 7) & 0xffffffff
    # lmw y stmw
    for rd in (5, 14, 20, 29, 31):
        for d in (0, -4, 12, -0x1000, 0x2001):
            g.base()
            g.emit(f"lmw {rd},{d}(3)")
            for r in range(rd, 32):
                g.guarda(f"lmw {rd},{d}(3): r{r}", str(r))
            g.base()
            for r in range(rd, 32):
                g.emit(*li32(str(r), 0x10101010 * (r & 15) + r))
            g.emit(f"stmw {rd},{d}(3)")
            for k in range(32 - rd):
                g.emit(f"lwz 5,{d + 4 * k}(3)")
                g.guarda(f"stmw {rd},{d}(3): palabra {k}")
    # reserva
    for d in (0, 0x40):
        g.base(d)
        g.emit(*li32("6", 0x13572468), "lwarx 5,0,3", "stwcx. 6,0,3", "lwz 5,0(3)")
        g.guarda(f"lwarx; stwcx. (base +{d:#x})")
        g.base(d)
        g.emit(*li32("6", 0x2468ace0), "stwcx. 6,0,3", "lwz 5,0(3)")
        g.guarda(f"stwcx. sin reserva (base +{d:#x})")
    # dcbz: un bloque de 32 bytes (el 750) a cero
    for off in (0, 0x24, 0x100):
        g.base()
        g.emit(*li32("4", off), "dcbz 3,4")
        for k in range(-2, 10):
            g.emit(f"lwz 5,{(off & ~31) + 4 * k}(3)")
            g.guarda(f"dcbz 3,4 (r4={off:#x}): palabra {k}")
    g.emit("addi 1,1,16", "lwz 0,4(1)", "mtlr 0", "li 0,0", "mtxer 0", "mtcrf 0xff,0")
    for r in range(2, 32):
        if r != 3:
            g.emit(f"li {r},0")
    g.emit("blr")
    print("\n".join(("        " + l) if not l.startswith((" ", "prueba:", "1:")) else l for l in g.out))
    (Path(__file__).parent / "t2.map").write_text("\n".join(g.mapa) + "\n")


if __name__ == "__main__":
    main()
