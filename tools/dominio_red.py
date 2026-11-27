#!/usr/bin/env python3
"""El dominio de red de S35, paso 2 (docs/bitacoras/dominio-red.md y dominio-usb.md): su kernel, su /init y la medida
en QEMU solo.

Dos variantes, cada una en su carpeta (fuera de git, como todo lo que se compila aquí):

  usb   (la de `make arranque-virt-dominio`): el aparato es USB, lo lleva el xHCI de DashOS y el dominio lo engancha por
        USB/IP con vhci-hcd. Sin PCI. Configuración usb.config. Carpeta $DASHOS_DOMINIO_USB (por defecto
        ~/recuperar/geosian-7d/dominio-usb).
  pci   (el paso intermedio, retirado de DashOS): la e1000 de QEMU por PCI. Configuración paso.config. Carpeta
        $DASHOS_DOMINIO_RED (por defecto ~/recuperar/geosian-7d/dominio-red). Sirve para medir en QEMU solo.

El código de Linux es el del paso 1 ($DASHOS_MEDIR_LINUX/linux-<versión>, de kernel.org, comprobado allí).

    tools/dominio_red.py preparar [usb|pci]  el kernel (tinyconfig + comun, arm64 y la de la variante) y el initramfs
                                             con caja-linux/dominios/dominio_red.c como /init: Image e initramfs.cpio
    tools/dominio_red.py init [usb|pci]      solo el /init y el initramfs (el kernel, el que haya)
    tools/dominio_red.py qemu [MB] [N]       (pci) N arranques en qemu-system-aarch64 -M virt (1 CPU, Cortex-A72
                                             emulado), con la e1000 y el mismo árbol de zonas reservadas que tenía en
                                             DashOS, con "solo=1": el tiempo hasta "listo" y la memoria usada (medianas)
    tools/dominio_red.py minimo              (pci) la menor memoria (MB) con la que llega a "listo" en QEMU solo

Para `make arranque-virt-dominio`: DOMINIO_IMAGE y DOMINIO_INITRD (por defecto, los de la variante usb).
"""

import os
import re
import select
import statistics
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from medir_dominios import FUENTE, TOOLCHAIN_ARM64, cpio_newc, leer_config, pedido, run  # noqa: E402

RAIZ = Path(__file__).resolve().parent.parent
DOMINIOS = RAIZ / "caja-linux" / "dominios"
VARIANTES = {
    "usb": ("usb.config", Path(os.environ.get("DASHOS_DOMINIO_USB", Path.home() / "recuperar" / "geosian-7d" / "dominio-usb"))),
    "pci": ("paso.config", Path(os.environ.get("DASHOS_DOMINIO_RED", Path.home() / "recuperar" / "geosian-7d" / "dominio-red"))),
}
# Lo de QEMU solo (`qemu`, `minimo`, `ver`) es de la variante pci: la e1000 en QEMU, sin DashOS.
TRABAJO = VARIANTES["pci"][1]
IMAGEN = TRABAJO / "Image"
INITRD = TRABAJO / "initramfs.cpio"
NUCLEOS = os.cpu_count() or 8
CC = f"{TOOLCHAIN_ARM64}gcc"


def preparar(variante="usb", kernel=True):
    fragmento, trabajo = VARIANTES[variante]
    obj = trabajo / "obj" / "kernel"
    obj.mkdir(parents=True, exist_ok=True)
    mk = ["make", "-C", FUENTE, f"O={obj}", "ARCH=arm64", f"-j{NUCLEOS}", f"CROSS_COMPILE={TOOLCHAIN_ARM64}"]
    imagen, initrd = trabajo / "Image", trabajo / "initramfs.cpio"
    if kernel:
        run(*mk, "tinyconfig", stdout=subprocess.DEVNULL)
        texto = "\n".join((DOMINIOS / f).read_text() for f in ("comun.config", "arm64.config", fragmento))
        (obj / "dominio.config").write_text(texto)
        run(FUENTE / "scripts/kconfig/merge_config.sh", "-m", "-O", obj, obj / ".config", obj / "dominio.config",
            cwd=FUENTE, env={**os.environ, "ARCH": "arm64"}, stdout=subprocess.DEVNULL)
        run(*mk, "olddefconfig", stdout=subprocess.DEVNULL)
        hay = leer_config(obj / ".config")
        malas = [f"{k}={v} (queda {hay.get(k, 'n')})" for k, v in pedido(texto).items()
                 if hay.get(k, "n") != v and not (v == "n" and k not in hay)]
        if malas:
            print("AVISO, opciones que no quedaron como se pidió:", *malas, sep="\n  ", flush=True)
        run(*mk, "Image", stdout=subprocess.DEVNULL)
        imagen.write_bytes((obj / "arch/arm64/boot/Image").read_bytes())
    binario = trabajo / "obj" / "dominio_red"
    run(CC, "-Os", "-static", "-nostdlib", "-ffreestanding", "-fno-stack-protector", "-fno-builtin", "-Wall",
        "-Wl,--gc-sections", "-s", "-o", binario, DOMINIOS / "dominio_red.c")
    entradas = [(d, 0o040755, b"", (0, 0)) for d in ("dev", "proc", "sys")]
    entradas.append(("dev/console", 0o020600, b"", (5, 1)))
    entradas.append(("init", 0o100755, binario.read_bytes(), (0, 0)))
    initrd.write_bytes(cpio_newc(entradas))
    print(f"kernel: {imagen} ({imagen.stat().st_size} bytes); initramfs: {initrd} ({initrd.stat().st_size} bytes); "
          f"/init: {binario.stat().st_size} bytes", flush=True)


def arbol(mb):
    """El árbol de QEMU virt con lo que el dominio tiene en DashOS: la zona de DMA (restricted-dma-pool, 2 MB) y la
    ventana (1 MB) reservadas al final de su RAM, y la zona como memory-region del PCIe. Sin el canal (solo=1)."""
    dtb = TRABAJO / "obj" / f"virt-{mb}.dtb"
    base = ["qemu-system-aarch64", "-M", "virt,gic-version=2", "-cpu", "cortex-a72", "-m", f"{mb}M", "-nographic",
            "-netdev", "hubport,id=n0,hubid=0", "-device", "e1000,netdev=n0,romfile="]
    subprocess.run(base + ["-M", f"virt,gic-version=2,dumpdtb={dtb}"], check=True, stderr=subprocess.DEVNULL)
    dts = subprocess.run(["dtc", "-I", "dtb", "-O", "dts", dtb], check=True, capture_output=True, text=True).stdout
    fin = 0x4000_0000 + (mb << 20)
    dma, ventana = fin - (2 << 20), fin - (3 << 20)
    dts = re.sub(r"(pcie@10000000 \{)", r"\1\n\t\tmemory-region = <0x9001>;", dts, count=1)
    extra = f"""
\treserved-memory {{
\t\t#address-cells = <0x02>;
\t\t#size-cells = <0x02>;
\t\tranges;
\t\tdma@{dma:x} {{ compatible = "restricted-dma-pool"; reg = <0x00 {dma:#x} 0x00 0x200000>; phandle = <0x9001>; }};
\t\tventana@{ventana:x} {{ reg = <0x00 {ventana:#x} 0x00 0x100000>; }};
\t}};
"""
    i = dts.rindex("};")
    (TRABAJO / "obj" / "virt.dts").write_text(dts[:i] + extra + dts[i:])
    subprocess.run(["dtc", "-I", "dts", "-O", "dtb", "-o", dtb, TRABAJO / "obj" / "virt.dts"], check=True,
                   stderr=subprocess.DEVNULL)
    return base, dtb


MALO = re.compile(r"Kernel panic|Out of memory|oom-kill|fallo:|Unable to handle|Oops")


def arrancar(mb, espera=40, ver=False):
    base, dtb = arbol(mb)
    orden = base + ["-no-reboot", "-kernel", IMAGEN, "-initrd", INITRD, "-dtb", dtb, "-append",
                    "console=ttyAMA0 solo=1"]
    t0 = time.monotonic()
    p = subprocess.Popen([str(x) for x in orden], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT)
    texto, t_listo = b"", None
    try:
        while time.monotonic() - t0 < espera:
            r, _, _ = select.select([p.stdout], [], [], 0.05)
            if r:
                trozo = os.read(p.stdout.fileno(), 65536)
                if not trozo:
                    break
                texto += trozo
                if t_listo is None and re.search(rb"listo \(solo\) .*total_kb=\d+\r?\n", texto):
                    t_listo = time.monotonic() - t0
                if t_listo or MALO.search(texto.decode(errors="replace")):
                    time.sleep(0.3)
                    texto += os.read(p.stdout.fileno(), 65536) if select.select([p.stdout], [], [], 0)[0] else b""
                    break
    finally:
        if p.poll() is None:
            p.kill()  # nuestro propio hijo, por su PID
        p.wait()
    s = texto.decode(errors="replace")
    if ver:
        print(s)
    m = re.search(r"listo \(solo\) reloj_ms=(\d+) usada_kb=(\d+) total_kb=(\d+)", s)
    if not m or MALO.search(s):
        return dict(ok=False, mb=mb, cola=s[-800:])
    return dict(ok=True, mb=mb, fuera_s=t_listo, reloj_ms=int(m.group(1)), usada_kb=int(m.group(2)),
                total_kb=int(m.group(3)))


def qemu(mb=32, n=5):
    rs = [arrancar(mb) for _ in range(n)]
    buenos = [r for r in rs if r["ok"]]
    for r in rs:
        print(r, flush=True)
    if buenos:
        med = {k: statistics.median(r[k] for r in buenos) for k in ("fuera_s", "reloj_ms", "usada_kb")}
        print(f"QEMU solo, {mb} MB, {len(buenos)}/{n} bien: listo desde fuera {med['fuera_s']:.2f} s, reloj del "
              f"dominio {med['reloj_ms']:.0f} ms, usada {med['usada_kb'] / 1024:.1f} MB "
              f"(de {buenos[0]['total_kb'] / 1024:.1f} MB que ve)", flush=True)


def minimo():
    bajo, alto = 8, 64
    if not arrancar(alto)["ok"]:
        raise SystemExit(f"no arranca ni con {alto} MB")
    while alto - bajo > 1:
        medio = (alto + bajo) // 2
        ok = arrancar(medio)["ok"]
        print(f"  {medio} MB: {'bien' if ok else 'mal'}", flush=True)
        alto, bajo = (medio, bajo) if ok else (alto, medio)
    while not all(arrancar(alto)["ok"] for _ in range(2)):
        alto += 1
    print(f"mínimo en QEMU solo: {alto} MB", flush=True)


def main():
    a = sys.argv[1:]
    if not a:
        print(__doc__)
    elif a[0] in ("preparar", "init"):
        variante = a[1] if len(a) > 1 else "usb"
        if variante not in VARIANTES:
            raise SystemExit(f"variante desconocida: {variante} (usb o pci)")
        preparar(variante, kernel=a[0] == "preparar")
    elif a[0] == "qemu":
        qemu(int(a[1]) if len(a) > 1 else 32, int(a[2]) if len(a) > 2 else 5)
    elif a[0] == "ver":
        print(arrancar(int(a[1]) if len(a) > 1 else 32, ver=True))
    elif a[0] == "minimo":
        minimo()
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
