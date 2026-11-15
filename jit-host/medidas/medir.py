#!/usr/bin/env python3
"""U7: cuánto cuesta el modo little-endian del JIT (girar los bytes en cada acceso a la memoria del 68k).

Para cada programa b-*.bin, con el banco big-endian (../build, qemu-aarch64_be) y el little-endian (../build-le,
qemu-aarch64):

- instrucciones ARM ejecutadas en el código traducido (con el plugin build/cuenta.so, restando lo que gastan las
  pruebas fijas del banco sin programa): la medida que no depende de la máquina;
- tiempo en QEMU (el mejor de N veces), en instrucciones 68k por segundo: orientativo, porque QEMU no ejecuta las
  instrucciones como una CPU de verdad (un REV le cuesta lo mismo que un ADD);
- que los dos órdenes dejen los mismos registros y la misma memoria.

    python3 medir.py [N]
"""
import re
import subprocess
import sys
from pathlib import Path

AQUI = Path(__file__).resolve().parent
BANCO = {"be": ("qemu-aarch64_be", AQUI.parent / "build" / "jit-host"),
         "le": ("qemu-aarch64", AQUI.parent / "build-le" / "jit-host")}
PLUGIN = AQUI / "build" / "cuenta.so"
TMP = AQUI / "build"


def contar(orden, prog=None):
    qemu, exe = BANCO[orden]
    log = TMP / f"cuenta.{orden}.txt"
    args = [qemu, "-plugin", str(PLUGIN), "-d", "plugin", "-D", str(log), str(exe)] + ([str(prog)] if prog else [])
    r = subprocess.run(args, capture_output=True, text=True, timeout=600)
    m = re.search(r"jit (\d+) total (\d+)", log.read_text())
    return int(m.group(1)), int(m.group(2)), r.stdout


def tiempo(orden, prog, n):
    qemu, exe = BANCO[orden]
    mejor, salida, insn = None, None, 0
    for _ in range(n):
        r = subprocess.run([qemu, str(exe), str(prog)], capture_output=True, text=True, timeout=600)
        m = re.search(r"medida: (\d+) instrucciones 68k en (\d+) us", r.stdout)
        insn, us = int(m.group(1)), int(m.group(2))
        mejor = us if mejor is None else min(mejor, us)
        salida = [l for l in r.stdout[r.stdout.find("[HOST] Programa"):].splitlines() if l.startswith("[")]
    return insn, mejor, salida


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 3
    base = {o: contar(o)[:2] for o in BANCO}
    print(f"{'programa':<12} {'instr. 68k':>11} {'ARM/68k BE':>10} {'ARM/68k LE':>10} {'más ARM':>8} "
          f"{'MIPS BE':>8} {'MIPS LE':>8} {'más tiempo':>10}  iguales")
    for prog in sorted(AQUI.glob("b-*.bin")):
        fila = {}
        for o in BANCO:
            jit, _, _ = contar(o, prog)
            insn, us, salida = tiempo(o, prog, n)
            fila[o] = (jit - base[o][0], insn, us, salida)
        (jb, ib, tb, sb), (jl, il, tl, sl) = fila["be"], fila["le"]
        print(f"{prog.stem[2:]:<12} {ib:>11} {jb / ib:>10.2f} {jl / il:>10.2f} {100 * (jl / jb - 1):>7.1f}% "
              f"{ib / tb:>8.1f} {il / tl:>8.1f} {100 * (tl / tb - 1):>9.1f}%  {'sí' if sb == sl else 'NO'}")


if __name__ == "__main__":
    main()
