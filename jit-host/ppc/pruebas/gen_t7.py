#!/usr/bin/env python3
"""DashOS, fase F1h de la caja PowerPC: genera t7.s, tw, twi y sc.

El trap lleva al vector 0x700 del banco (../host_ppc.cpp) y a SIGTRAP en el oráculo; los dos hacen lo mismo: r20 += 1,
r21 = la dirección del trap, y siguen en la instrucción siguiente. sc lleva al vector 0xc00 del banco, que responde
como Linux en qemu-ppc a una llamada que no existe (r0 = 0x7777): r3 = 38 (ENOSYS) y SO de CR0 a 1.

Cada caso deja 12 bytes en la tabla de 0x00100000:
- tw/twi: r20 (traps), r21 (dónde) y r25, que se cambia justo antes del trap en el mismo bloque (si la salida por
  la excepción dejara mal el asignador de registros, r25 saldría con el valor viejo);
- sc: r3, CR y r20 (+1 tras el sc: se sigue una sola vez en la siguiente).

    python3 gen_t7.py > t7.s
"""
from pathlib import Path

from gen_t1 import li32

PARES = [(0, 0), (1, 2), (2, 1), (5, 5), (0xffffffff, 1), (1, 0xffffffff), (0x80000000, 0x7fffffff),
         (0x7fffffff, 0x80000000)]
VALORES = [0, 1, 0xffffffff, 5, 0x80000000, 0x7fffffff, 0x1000]
INMEDIATOS = [0, 1, -1, 5, 0x7fff, -0x8000, 0x1000]


def main():
    out = ["        .section .text.prueba,\"ax\"", "        .globl prueba", "prueba:"]
    mapa = []

    def emit(*ls):
        out.extend("        " + l for l in ls)

    emit(*li32("31", 0x00100000))
    k = 0

    def caso(desc, insn, a, b=None):
        nonlocal k
        k += 1
        emit("li 20,0", "li 21,0", f"li 25,{k & 0x7fff}", *li32("3", a))
        if b is not None:
            emit(*li32("4", b))
        emit("addi 25,25,1", insn, "stw 20,0(31)", "stw 21,4(31)", "stw 25,8(31)", "addi 31,31,12")
        mapa.append(desc)

    for to in range(32):
        for a, b in PARES:
            caso(f"tw {to},3,4 r3={a:08x} r4={b:08x}", f"tw {to},3,4", a, b)
    for to in range(32):
        for a in VALORES:
            for i in INMEDIATOS:
                caso(f"twi {to},3,{i} r3={a:08x}", f"twi {to},3,{i}", a)
    # sc
    for cr in (0, 0xffffffff, 0x10000000, 0xe0000000, 0x12345678):
        emit(*li32("0", 0x7777), *li32("3", 0x1234), *li32("4", cr), "mtcrf 0xff,4", "li 20,0")
        emit("sc", "addi 20,20,1", "mfcr 5", "stw 3,0(31)", "stw 5,4(31)", "stw 20,8(31)", "addi 31,31,12")
        mapa.append(f"sc (r0=7777, CR={cr:08x}): r3, CR, r20")
    emit("li 0,0", "mtcrf 0xff,0", "blr")
    (Path(__file__).parent / "t7.map").write_text("\n".join(mapa) + "\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
