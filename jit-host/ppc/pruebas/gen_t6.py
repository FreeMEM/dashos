#!/usr/bin/env python3
"""DashOS, fase F1h de la caja PowerPC: genera t6.s, las instrucciones de cadenas (lswi, lswx, stswi, stswx).

Cada caso tiene un registro de 132 bytes en la tabla de 0x00100000 (11 de 12 bytes para comparar_ppc.py):
- cargas: todos los GPR (stmw) y XER tras la instrucción; antes, cada GPR con un valor centinela. rD con y sin la
  vuelta de r31 a r0, todas las longitudes de 1 a 32 bytes (lswi) o de 0 a 108 (lswx, desde XER), direcciones sin
  alinear; rA (y rB) y el puntero de la tabla, siempre fuera de los registros que se cargan (si no, es una forma no
  válida).
- guardados: la zona del registro llena de 0xee y la instrucción escribe en ella (desde un desplazamiento de 0 a 3),
  con cada GPR con bytes distintos.

    python3 gen_t6.py > t6.s
"""
from pathlib import Path

TABLA = 0x00100000
REG = 132


def li32(r, v):
    v &= 0xffffffff
    return [f"lis {r},{v >> 16:#x}", f"ori {r},{r},{v & 0xffff:#x}"]


def centinela(r):
    return 0x5a5a0000 | (r * 0x0101)


def patron(r):
    b = [(0x10 + 4 * r + i) & 0xff for i in range(4)]
    return b[0] << 24 | b[1] << 16 | b[2] << 8 | b[3]


def libres(usados, n):
    """n registros (no r0) fuera de usados"""
    out = []
    for r in (2, 12, 9, 30, 6, 17, 22, 27, 4, 11, *range(1, 32)):
        if r not in usados and r not in out:
            out.append(r)
        if len(out) == n:
            return out
    raise ValueError(usados)


def main():
    out = ["        .section .text.prueba,\"ax\"", "        .globl prueba", "prueba:"]
    mapa = []
    caso = 0

    def emit(*ls):
        out.extend("        " + l for l in ls)

    def registro(desc, carga):
        nonlocal caso
        if carga:
            mapa.extend(f"{desc} [r{3 * j}-r{3 * j + 2}]" if j < 10 else f"{desc} [r30 r31 XER]" for j in range(11))
        else:
            mapa.extend(f"{desc} [bytes {12 * j}-{12 * j + 11}]" for j in range(11))
        caso += 1

    def rec():
        return TABLA + REG * caso

    # cargas
    for forma in ("lswi", "lswx"):
        rds = (3, 13, 14, 25, 30) if forma == "lswi" else (3, 14, 30)
        ns = [1, 2, 3, 4, 5, 6, 7, 8, 11, 16, 21, 31, 32] if forma == "lswi" else \
             [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 13, 16, 31, 33, 64, 100, 108]
        for rd in rds:
            for n in ns:
                for off in ((0, 1, 3) if forma == "lswi" else (0, 3)):
                    for ra0 in ((False,) if forma == "lswi" else (False, True)):
                        if ra0 and off != 3:
                            continue
                        cargados = {(rd + k) & 31 for k in range((n + 3) // 4)}
                        ra, rb, rp = libres(cargados | {0}, 3)
                        if ra0:
                            ra = 0
                        for r in range(32):
                            emit(*li32(str(r), centinela(r)))
                        if forma == "lswi":
                            emit(f"lis {ra},(fuente+{off})@ha", f"addi {ra},{ra},(fuente+{off})@l")
                            insn = f"lswi {rd},{ra},{n & 31}"
                        else:
                            emit(*li32(str(rp), 0xa0000000 | n), f"mtxer {rp}")
                            if ra == 0:
                                emit(f"lis {rb},(fuente+{off})@ha", f"addi {rb},{rb},(fuente+{off})@l")
                            else:
                                emit(f"lis {ra},fuente@ha", f"addi {ra},{ra},fuente@l", f"li {rb},{off}")
                            insn = f"lswx {rd},{ra},{rb}"
                        emit(insn, *li32(str(rp), rec()), f"stmw 0,0({rp})", "mfxer 0", f"stw 0,128({rp})")
                        registro(f"{insn} n={n} off={off}", True)
    # guardados
    for forma in ("stswi", "stswx"):
        rds = (3, 13, 14, 25, 30) if forma == "stswi" else (3, 14, 30)
        ns = [1, 2, 3, 4, 5, 6, 7, 8, 11, 16, 21, 31, 32] if forma == "stswi" else \
             [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 13, 16, 31, 33, 64, 100, 113, 127]
        for rd in rds:
            for n in ns:
                for off in (0, 1, 3):
                    guardados = {(rd + k) & 31 for k in range((n + 3) // 4)}
                    try:
                        ra, rb, rp = libres(guardados | {0}, 3)
                    except ValueError:
                        ra, rb, rp = 2, 12, 9      # n = 127: todos; rA y rB se guardan con su valor
                    emit(*li32(str(rp), rec()), *li32(str(ra), 0xeeeeeeee))
                    emit(*(f"stw {ra},{4 * i}({rp})" for i in range(33)))
                    emit(*li32(str(ra), 0x20000000 | n), f"mtxer {ra}")
                    for r in range(32):
                        emit(*li32(str(r), patron(r)))
                    if forma == "stswi":
                        emit(*li32(str(ra), rec() + off))
                        insn = f"stswi {rd},{ra},{n & 31}"
                    else:
                        emit(*li32(str(ra), rec()), f"li {rb},{off}")
                        insn = f"stswx {rd},{ra},{rb}"
                    emit(insn)
                    registro(f"{insn} n={n} off={off}", False)
    emit("li 0,0", "mtxer 0", "blr")
    out.append("        .balign 4")
    out.append("fuente:")
    for i in range(0, 256, 16):
        out.append("        .byte " + ",".join(f"{(j * 37 + 11) & 0xff:#04x}" for j in range(i, i + 16)))
    assert TABLA + REG * caso < 0x00140000
    (Path(__file__).parent / "t6.map").write_text("\n".join(mapa) + "\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()
