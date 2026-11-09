#!/usr/bin/env python3
"""Banco de medidas de la multitarea de DashOS (M0, docs/MULTITAREA_LOOP.md).

    tools/medir.py paquete SALIDA            junta los hijos de prueba (Rust, C y Dash) en el paquete del kernel
    tools/medir.py correr IMAGEN JSON [-n N] arranca la imagen de `make medir` N veces en QEMU raspi4b y guarda las
                                             medianas de cada medida en JSON (con todas las vueltas)
    tools/medir.py comparar BASE NUEVO       compara dos JSON

El hijo en C se compila con clang y ld.lld; el de Dash, con `./dash --dashos` de ~/Projects/FreeMEMLang (o $DASH).
"""
import json
import os
import re
import statistics
import struct
import subprocess
import sys
import tempfile

RAIZ = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
USUARIO = os.path.join(RAIZ, "usuario")
RUST = os.path.join(USUARIO, "target/aarch64-unknown-none-softfloat/release")
DASH = os.environ.get("DASH", os.path.expanduser("~/Projects/FreeMEMLang/dash"))


def paquete(salida):
    tmp = tempfile.mkdtemp(prefix="medir-")
    c_o, c_elf, d_elf = (os.path.join(tmp, n) for n in ("hijo.o", "hijo-c", "hijo-dash"))
    subprocess.run(["clang", "--target=aarch64-none-elf", "-O2", "-ffreestanding", "-nostdlib", "-fno-pic",
                    "-mgeneral-regs-only", "-c", os.path.join(USUARIO, "medir/hijo.c"), "-o", c_o], check=True)
    subprocess.run(["ld.lld", "-static", "-e", "_start", "-Ttext=0x400000", "-z", "noexecstack", "-o", c_elf, c_o],
                   check=True)
    subprocess.run([DASH, "--dashos", os.path.join(USUARIO, "medir/hijo.dash"), "-o", d_elf], check=True,
                   stdout=subprocess.DEVNULL)
    hijos = [("hijo-rust", os.path.join(RUST, "medir-hijo")), ("hijo-c", c_elf), ("hijo-dash", d_elf)]
    datos = [open(f, "rb").read() for _, f in hijos]
    cab = 8 + 24 * len(hijos)
    out = bytearray(b"MEDP" + struct.pack("<I", len(hijos)))
    pos = (cab + 7) & ~7
    trozos = []
    for (nombre, _), d in zip(hijos, datos):
        out += nombre.encode().ljust(16, b"\0") + struct.pack("<II", pos, len(d))
        trozos.append((pos, d))
        pos = (pos + len(d) + 7) & ~7
    for p, d in trozos:
        out += b"\0" * (p - len(out)) + d
    with open(salida, "wb") as f:
        f.write(out)
    print(f"paquete: {len(out) // 1024} KB, " + ", ".join(f"{n} {len(d) // 1024} KB" for (n, _), d in zip(hijos, datos)))


LINEA = re.compile(r"\[medir\] (\S+)(.*)")


def clave(prueba, campos):
    """Lo que identifica una medida (sin los valores)."""
    ident = {"ida-vuelta": ("lenguaje", "juntos", "carga"), "reparto": ("lenguaje", "n"),
             "temporizador": ("us", "carga"), "dormilon": ("dormir_ms",), "ociosos": ()}.get(prueba, ())
    return prueba + "".join(f" {k}={campos[k]}" for k in ident if k in campos)


def leer_salida(texto):
    medidas = {}
    for m in LINEA.finditer(texto):
        prueba, resto = m.group(1), m.group(2)
        campos = dict(kv.split("=", 1) for kv in resto.split() if "=" in kv)
        medidas[clave(prueba, campos)] = campos
    return medidas


def dtb():
    for raiz in (RAIZ, os.path.expanduser("~/Projects/dashos")):
        f = os.path.join(raiz, "build/firmware/bcm2711-rpi-4-b-qemu.dtb")
        if os.path.exists(f):
            return f
    sys.exit("falta build/firmware/bcm2711-rpi-4-b-qemu.dtb: se crea con una vuelta de tools/prueba_qemu.sh")


def una_vuelta(imagen, segundos=300):
    """Arranca la imagen y devuelve la salida del puerto serie hasta "[medir] fin" (o hasta el tope)."""
    import selectors
    import time
    q = subprocess.Popen(["qemu-system-aarch64", "-M", "raspi4b", "-kernel", imagen, "-dtb", dtb(), "-display", "none",
                          "-serial", "stdio", "-serial", "null", "-monitor", "none"],
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL)
    sel = selectors.DefaultSelector()
    sel.register(q.stdout, selectors.EVENT_READ)
    datos = bytearray()
    fin = time.time() + segundos
    try:
        while time.time() < fin and b"[medir] fin" not in datos:
            if sel.select(timeout=1):
                trozo = os.read(q.stdout.fileno(), 65536)
                if not trozo:
                    break
                datos += trozo
    finally:
        q.kill()
        q.wait()
    return datos.decode("utf-8", errors="replace").replace("\r", "")


def correr(imagen, salida_json, vueltas):
    todas = []
    for v in range(vueltas):
        texto = una_vuelta(imagen)
        m = leer_salida(texto)
        bien = "[medir] fin" in texto
        print(f"vuelta {v + 1}: {len(m)} medidas" + ("" if bien else " (NO ACABÓ)"))
        if not bien:
            print(texto[-3000:])
        todas.append(m)
    medianas = {}
    for k in sorted({k for m in todas for k in m}):
        campos = {}
        for c in {c for m in todas if k in m for c in m[k]}:
            vals = [m[k][c] for m in todas if k in m and c in m[k]]
            try:
                campos[c] = statistics.median(float(x) for x in vals)
            except ValueError:
                campos[c] = vals[0]
        medianas[k] = campos
    with open(salida_json, "w") as f:
        json.dump({"medianas": medianas, "vueltas": todas}, f, indent=1, sort_keys=True)
    for k, c in medianas.items():
        print(k, " ".join(f"{a}={b:g}" if isinstance(b, float) else f"{a}={b}" for a, b in sorted(c.items())))


def comparar(base, nuevo):
    a = json.load(open(base))["medianas"]
    b = json.load(open(nuevo))["medianas"]
    for k in sorted(set(a) | set(b)):
        if k not in a or k not in b:
            print(f"{k}: solo en {'nuevo' if k in b else 'base'}")
            continue
        partes = []
        for c in sorted(set(a[k]) & set(b[k])):
            x, y = a[k][c], b[k][c]
            if isinstance(x, float) and isinstance(y, float) and x != y:
                cambio = f" ({(y - x) / x * 100:+.0f} %)" if x else ""
                partes.append(f"{c} {x:g} -> {y:g}{cambio}")
        print(f"{k}: " + ("; ".join(partes) if partes else "igual"))


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "paquete":
        paquete(sys.argv[2])
    elif len(sys.argv) >= 4 and sys.argv[1] == "correr":
        n = int(sys.argv[sys.argv.index("-n") + 1]) if "-n" in sys.argv else 1
        correr(sys.argv[2], sys.argv[3], n)
    elif len(sys.argv) == 4 and sys.argv[1] == "comparar":
        comparar(sys.argv[2], sys.argv[3])
    else:
        print(__doc__)
        sys.exit(2)
