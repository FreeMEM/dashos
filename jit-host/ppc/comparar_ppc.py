#!/usr/bin/env python3
"""DashOS, fase F1 de la caja PowerPC (docs/PPC_LOOP.md): cada binario de pruebas/ (o los que se den) en el JIT PPC
de Emu68 (build/jit-ppc, con qemu-aarch64_be) y en qemu-ppc (pruebas/oraculo): los registros finales y la suma de
la memoria tienen que ser iguales.

    python3 comparar_ppc.py               # todas las pruebas
    python3 comparar_ppc.py x.bin y.bin   # solo esas
"""
import subprocess
import sys
from pathlib import Path

AQUI = Path(__file__).resolve().parent
PRUEBAS = AQUI / "pruebas"
TMP = AQUI / "build" / "comparar"
MAX = int(__import__("os").environ.get("MAX", "12"))


def salida(cmd):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired:
        return None, ["(no acaba en 60 s)"]
    return r.returncode, [l for l in r.stdout.splitlines() if l.startswith("[REG]")]


def main():
    progs = [Path(a) for a in sys.argv[1:]] or sorted(PRUEBAS.glob("*.bin"))
    fallos = 0
    TMP.mkdir(parents=True, exist_ok=True)
    for p in progs:
        vj, vr = TMP / f"{p.stem}.jit.mem", TMP / f"{p.stem}.qemu.mem"
        _, jit = salida(["qemu-aarch64_be", str(AQUI / "build" / "jit-ppc"), str(p), str(vj)])
        _, ref = salida(["qemu-ppc", "-cpu", "750", str(PRUEBAS / "oraculo"), str(p), str(vr)])
        if jit == ref and len(ref) > 1:
            print(f"{p.stem}: bien ({ref[-1].split(': ')[-1]})")
            continue
        fallos += 1
        print(f"{p.stem}: MAL")
        for a, b in zip(jit + ["-"] * len(ref), ref + ["-"] * len(jit)):
            if a != b:
                print(f"    JIT:  {a}\n    qemu: {b}")
        if vj.exists() and vr.exists():
            mj, mr = vj.read_bytes(), vr.read_bytes()
            mapa = p.with_suffix(".map")
            if mapa.exists():
                # las pruebas generadas (gen_t*.py): registros de 12 bytes desde 0x00100000 (t1: rD, CR, XER; t2: valor, r3, CR)
                desc = mapa.read_text().splitlines()
                malos = [i for i in range(len(desc)) if mj[12 * i:12 * i + 12] != mr[12 * i:12 * i + 12]]
                for i in malos[:MAX]:
                    w = lambda m, k: int.from_bytes(m[12 * i + 4 * k:12 * i + 4 * k + 4], "big")
                    print(f"    {desc[i]}\n        JIT  {w(mj, 0):08x} {w(mj, 1):08x} {w(mj, 2):08x}"
                          f"\n        qemu {w(mr, 0):08x} {w(mr, 1):08x} {w(mr, 2):08x}")
                if len(malos) > MAX:
                    print(f"    ... {len(malos)} de {len(desc)} distintos")
            else:
                difs = [i for i in range(min(len(mj), len(mr))) if mj[i] != mr[i]]
                for i in difs[:8]:
                    print(f"    memoria {0x100000 + i:08x}: JIT {mj[i]:02x}, qemu {mr[i]:02x}")
                if len(difs) > 8:
                    print(f"    ... {len(difs)} bytes distintos")
    sys.exit(1 if fallos else 0)


if __name__ == "__main__":
    main()
