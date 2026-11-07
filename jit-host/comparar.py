#!/usr/bin/env python3
"""U7: la prueba del JIT en little-endian. Para cada programa de pruebas68k/*.bin:

1. Lo ejecuta con el JIT big-endian (build/) y con el little-endian (build-le/) y compara lo que sale desde
   "[HOST] Programa": registros, FPU y la suma de toda la memoria del 68k. Deben ser iguales.
2. Compara el código ARM que genera el JIT big-endian con el de referencia (pruebas68k/ref/*.vol, sacado antes de
   tocar el JIT para U7). En big-endian, los cambios de U7 no deben cambiar nada. Solo se admiten diferencias en
   los inmediatos de MOVZ/MOVK/MOVN (direcciones de funciones de C, que se mueven al cambiar el programa).

    python3 comparar.py               # las dos cosas (hacen falta build/jit-host y build-le/jit-host)
    python3 comparar.py --guardar-ref # guarda la referencia con build-ref/jit-host: Emu68 sin los parches de U7
                                      # (0001-0003 y 0008), compilado con
                                      # make EMU68=<clon sin U7> B=build-ref
    python3 comparar.py --solo-be     # solo el punto 2
"""
import re
import resource
import struct
import subprocess
import sys
from pathlib import Path

AQUI = Path(__file__).resolve().parent
PRUEBAS = AQUI / "pruebas68k"
REF = PRUEBAS / "ref"
TMP = AQUI / "build" / "comparar"


def ejecutar(orden, prog, volcado, memoria=None):
    exe, qemu = (AQUI / "build" / "jit-host", "qemu-aarch64_be") if orden == "be" else \
        (AQUI / "build-ref" / "jit-host", "qemu-aarch64_be") if orden == "ref" else \
        (AQUI / "build-le" / "jit-host", "qemu-aarch64")
    r = subprocess.run([qemu, str(exe), str(prog), str(volcado)] + ([str(memoria)] if memoria else []), capture_output=True, text=True, timeout=120,
                       preexec_fn=lambda: resource.setrlimit(resource.RLIMIT_CORE, (0, 0)))
    salida = r.stdout
    i = salida.find("[HOST] Programa")
    resto = salida[i:] if i >= 0 else ""
    # los avisos de Emu68 de "instrucción vista por primera vez" (FMOVEM...) no cuentan
    lineas = [sin_ruta(l) for l in resto.splitlines() if l.startswith("[")]
    return r.returncode, lineas


def sin_ruta(linea):
    """La línea "[HOST] Programa ruta" sin la ruta (la referencia se sacó en otro directorio)"""
    return re.sub(r"^(\[HOST\] Programa )\S*/", r"\1", linea)


def bloques(f, cab=">"):
    """Los bloques de un volcado: la cabecera (dirección 68k y bytes) va en el orden del banco que lo hizo; el
    código ARM siempre en little-endian (las instrucciones de AArch64 lo son también en big-endian)."""
    d = Path(f).read_bytes()
    p, out = 0, []
    while p + 8 <= len(d):
        a, n = struct.unpack_from(cab + "II", d, p)
        p += 8
        out.append((a, d[p:p + n]))
        p += n
    return out


def es_mov_inmediato(w):
    # MOVN (00), MOVZ (10), MOVK (11): sf opc 100101 hw imm16 Rd
    return (w >> 23) & 0x3F == 0b100101 and ((w >> 29) & 3) != 1


def comparar_codigo(ref, nuevo):
    """(bloques iguales, bloques con solo inmediatos distintos, lista de problemas)"""
    a, b = bloques(ref), bloques(nuevo)
    if len(a) != len(b):
        return 0, 0, [f"{len(a)} bloques en la referencia y {len(b)} ahora"]
    iguales = solo_inm = 0
    problemas = []
    for (pa, ca), (pb, cb) in zip(a, b):
        if pa != pb or len(ca) != len(cb):
            problemas.append(f"bloque ${pa:x}: {len(ca)} bytes en la referencia, ${pb:x} con {len(cb)} ahora")
            continue
        if ca == cb:
            iguales += 1
            continue
        wa = struct.unpack(f"<{len(ca) // 4}I", ca)
        wb = struct.unpack(f"<{len(cb) // 4}I", cb)
        malas = [i for i, (x, y) in enumerate(zip(wa, wb))
                 if x != y and not (es_mov_inmediato(x) and es_mov_inmediato(y) and (x ^ y) & ~(0xFFFF << 5) == 0)]
        if malas:
            i = malas[0]
            problemas.append(f"bloque ${pa:x}: {len(malas)} instrucciones distintas (la {i}: {wa[i]:08x} -> {wb[i]:08x})")
        else:
            solo_inm += 1
    return iguales, solo_inm, problemas


def main():
    guardar = "--guardar-ref" in sys.argv
    solo_be = "--solo-be" in sys.argv or guardar
    TMP.mkdir(parents=True, exist_ok=True)
    REF.mkdir(exist_ok=True)
    fallos = 0
    for prog in sorted(PRUEBAS.glob("*.bin")):
        nombre = prog.stem
        if guardar:
            rc_be, be = ejecutar("ref", prog, TMP / f"{nombre}.be.vol")
            (REF / f"{nombre}.vol").write_bytes((TMP / f"{nombre}.be.vol").read_bytes())
            (REF / f"{nombre}.txt").write_text("\n".join(be) + "\n")
            print(f"{nombre}: referencia guardada (rc {rc_be})")
            continue
        rc_be, be = ejecutar("be", prog, TMP / f"{nombre}.be.vol")
        iguales, solo_inm, problemas = comparar_codigo(REF / f"{nombre}.vol", TMP / f"{nombre}.be.vol")
        ref_txt = [sin_ruta(l) for l in (REF / f"{nombre}.txt").read_text().splitlines()]
        if be != ref_txt:
            problemas.append("la salida en big-endian no es la de la referencia")
        estado = "bien" if not problemas and rc_be == 0 else "MAL"
        print(f"{nombre}: big-endian {estado}: {iguales} bloques iguales, {solo_inm} con solo direcciones distintas")
        for p in problemas[:5]:
            print(f"    {p}")
        fallos += estado != "bien"
        if solo_be:
            continue
        rc_le, le = ejecutar("le", prog, TMP / f"{nombre}.le.vol", TMP / f"{nombre}.le.mem")
        if rc_le == 0 and le == be:
            print(f"{nombre}: little-endian bien: la misma salida")
        else:
            fallos += 1
            dif = next((f"{x}  |  {y}" for x, y in zip(be, le) if x != y), "salida más corta")
            print(f"{nombre}: little-endian MAL (rc {rc_le}): {dif}")
            ejecutar("be", prog, TMP / f"{nombre}.be.vol", TMP / f"{nombre}.be.mem")
            mb, ml = (TMP / f"{nombre}.be.mem").read_bytes(), (TMP / f"{nombre}.le.mem").read_bytes() if (TMP / f"{nombre}.le.mem").exists() else b""
            difs = [i for i in range(min(len(mb), len(ml))) if mb[i] != ml[i]]
            if difs:
                print(f"    memoria distinta en {len(difs)} bytes; los primeros: " +
                      " ".join(f"${0x140000 + i:x}" for i in difs[:8]))
    sys.exit(1 if fallos else 0)


if __name__ == "__main__":
    main()
