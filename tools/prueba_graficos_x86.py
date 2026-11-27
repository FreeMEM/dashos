#!/usr/bin/env python3
"""X12 (G5 de docs/diseño/S38-gpu-escritorio.md): la prueba del dominio de gráficos de x86 (`make -C kernel
graficos-x86`). DashOS (la imagen `graficos`) en QEMU q35 con KVM y una IOMMU (VT-d o AMD-Vi, con remapeo de
interrupciones) arranca un Linux mínimo en su hipervisor y le pasa una virtio-gpu con virgl (`virtio-gpu-gl-pci`, con
`iommu_platform=on`: su DMA pasa por la IOMMU, como el de una gráfica de verdad); la pantalla de DashOS es la VGA de
q35 (el GOP). El dominio pinta con Mesa y saca los cuadros por KMS.

    tools/prueba_graficos_x86.py <dashos-uefi.efi> <carpeta del dominio> [--iommu intel|amd] [--salida carpeta]

La carpeta del dominio es la de `tools/dominio_graficos.py preparar` (bzImage e initramfs.cpio, fuera de git): van en
la ESP como EFI/DashOS/GRAFICOS.BZI y GRAFICOS.CPI, que el cargador lee. Se comprueba, en este orden, la salida serie
(`ESPERADOS`, cada una tras la anterior), que no salga ningún fallo de DMA ni interrupción bloqueada, y dos capturas de
la pantalla de la GPU del dominio (por VNC, `tools/vnc_captura.py`, con medio segundo entre ellas: no negras y
distintas). Lo medido (cuadros por segundo, CPU por cuadro del programa en el dominio y de DashOS entero) se escribe al
final."""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from vnc_captura import capturar  # noqa: E402

# Lo que tiene que salir por la serie, en orden (texto que contiene la línea).
ESPERADOS = [
    "DashOS: con el dominio de gráficos",
    "{iommu} en",
    "graficos: la virtio-gpu de",
    "graficos: el dominio de gráficos en memoria",
    "[dominio-graficos] GPU 1af4:1050 pasada al dominio: 3 interrupciones, remapeadas por la IOMMU",
    "linux: ",
    "virtio-pci 0000:00:01.0",
    "[drm] Initialized virtio_gpu",
    "dominio: DRM: virtio_gpu",
    "GL_RENDERER virgl",
    "dominio: pintando",
    "[dominio-graficos] arrancado y pintando en",
    "dominio: píxel",
    "dominio: cuadros ",
    "[dominio-graficos] medida:",
    "[dominio-graficos] dominio: fin graficos 0",
]
# X13, la ventana compartida: el programa de prueba y el dominio, cada uno en su orden (van mezclados).
COMPARTIDA = [
    ["[dominio-graficos] ventana compartida en 0xa0000000 del invitado", "[graficos-cliente] ventana de 640x480 en la ranura 0",
     "[graficos-cliente] 300 de 300 cuadros vistos por el dominio", "[dominio-graficos] ventana compartida: 1 búferes puestos, 300 cuadros listos"],
    ["dominio: compartida: ventana de 640x480 (formato 1)", "dominio: compartida: leer el búfer compartido",
     "dominio: compartida: 300 cuadros de 640x480", "dominio: fin graficos 0"],
]
MALOS = ("fallo de DMA", "interrupción bloqueada", "Kernel panic", "[dominio-graficos] MAL", "[dominio-graficos] aviso")
OVMF = "/usr/share/edk2/x64/OVMF_CODE.4m.fd"
OVMF_VARS = "/usr/share/edk2/x64/OVMF_VARS.4m.fd"


def mirar(a, b):
    from PIL import Image, ImageChops
    ia, ib = Image.open(a).convert("RGB"), Image.open(b).convert("RGB")
    negra = lambda i: max(x[1] for x in i.getextrema()) < 16  # noqa: E731
    return not negra(ia), not negra(ib), ImageChops.difference(ia, ib).getbbox() is not None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("efi")
    ap.add_argument("dominio")
    ap.add_argument("--iommu", default="intel", choices=("intel", "amd"))
    ap.add_argument("--salida", default="target/graficos")
    ap.add_argument("--segundos", type=int, default=240)
    a = ap.parse_args()
    dom = Path(a.dominio)
    for f in ("bzImage", "initramfs.cpio"):
        if not (dom / f).exists():
            sys.exit(f"Falta {dom / f}: tools/dominio_graficos.py preparar (fuera de git)")
    sal = Path(a.salida).resolve()
    sal.mkdir(parents=True, exist_ok=True)
    serie = sal / "serie.txt"
    vnc = sal / "vnc.sock"
    for f in (serie, vnc, sal / "captura-1.png", sal / "captura-2.png"):
        f.unlink(missing_ok=True)
    esp = Path(tempfile.mkdtemp(prefix="graficos-esp-", dir=sal))
    (esp / "EFI/BOOT").mkdir(parents=True)
    (esp / "EFI/DashOS").mkdir(parents=True)
    shutil.copy(a.efi, esp / "EFI/BOOT/BOOTX64.EFI")
    shutil.copy(dom / "bzImage", esp / "EFI/DashOS/GRAFICOS.BZI")
    shutil.copy(dom / "initramfs.cpio", esp / "EFI/DashOS/GRAFICOS.CPI")
    vars_ = sal / "vars.fd"
    shutil.copy(OVMF_VARS, vars_)
    iommu = ["-device", "intel-iommu,intremap=on"] if a.iommu == "intel" else ["-device", "amd-iommu,dma-remap=on,intremap=on"]
    orden = ["qemu-system-x86_64", "-M", "q35,kernel-irqchip=split", "-accel", "kvm", "-cpu", "host", "-smp", "4", "-m", "2G",
             "-drive", f"if=pflash,format=raw,readonly=on,file={OVMF}", "-drive", f"if=pflash,format=raw,file={vars_}",
             "-drive", f"file=fat:{esp},format=raw,if=virtio,readonly=on", *iommu,
             "-device", "virtio-gpu-gl-pci,id=gpu0,disable-legacy=on,iommu_platform=on,xres=1280,yres=720",
             "-display", "egl-headless", "-vnc", f"unix:{vnc},display=gpu0",
             "-serial", f"file:{serie}", "-monitor", "none", "-net", "none", *os.environ.get("QEMU_EXTRA", "").split()]
    print("+", " ".join(orden), flush=True)
    t0 = time.time()
    q = subprocess.Popen(orden, stdout=subprocess.DEVNULL, stderr=open(sal / "qemu.err", "w"))
    capturas = []
    try:
        while time.time() - t0 < a.segundos and q.poll() is None:
            texto = serie.read_text(errors="replace") if serie.exists() else ""
            if not capturas and "[dominio-graficos] arrancado y pintando" in texto:
                time.sleep(3)
                for k in (1, 2):
                    capturas.append(sal / f"captura-{k}.png")
                    capturar(vnc, capturas[-1])
                    time.sleep(0.5)
            # X13: una más con la ventana compartida en pantalla (para verla; no se compara).
            if len(capturas) == 2 and "dominio: compartida: ventana de" in texto:
                time.sleep(1)
                capturas.append(sal / "captura-compartida.png")
                capturar(vnc, capturas[-1])
            if "[dominio-graficos] dominio: fin " in texto or "[dominio-graficos] MAL" in texto or "Kernel panic" in texto:
                time.sleep(1)
                break
            time.sleep(0.5)
    finally:
        q.kill()
        q.wait()
        shutil.rmtree(esp)
    texto = serie.read_text(errors="replace") if serie.exists() else ""
    lineas = texto.splitlines()
    bien = True
    i = 0
    nombre = "IOMMU: VT-d" if a.iommu == "intel" else "IOMMU: AMD-Vi"
    for e in ESPERADOS:
        e = e.format(iommu=nombre)
        while i < len(lineas) and e not in lineas[i]:
            i += 1
        if i == len(lineas):
            print(f'MAL: falta (en orden) "{e}"')
            bien = False
            break
        i += 1
    for grupo in COMPARTIDA:
        i = 0
        for e in grupo:
            while i < len(lineas) and e not in lineas[i]:
                i += 1
            if i == len(lineas):
                print(f'MAL: falta (en orden) "{e}"')
                bien = False
                break
            i += 1
    for l in lineas:
        if "dominio: compartida: " in l and " mal;" in l and " 0 mal;" not in l:
            print("MAL:", l.strip())
            bien = False
    for m in MALOS:
        for l in lineas:
            if m in l:
                print("MAL:", l.strip())
                bien = False
                break
    if len(capturas) >= 2 and all(c.exists() for c in capturas[:2]):
        na, nb, distintas = mirar(*capturas[:2])
        print(f"capturas de la GPU del dominio: {capturas[0]} y {capturas[1]}: no negras {na} y {nb}; distintas {distintas}")
        bien &= na and nb and distintas
    else:
        print("MAL: sin las dos capturas de la GPU del dominio")
        bien = False
    for l in lineas:
        if "dominio: cuadros " in l and " en " in l or "[dominio-graficos] medida:" in l or "arrancado y pintando" in l \
                or "[dominio-graficos] GPU" in l or "GL_RENDERER" in l or "salidas del invitado;" in l \
                or "compartida" in l or "[graficos-cliente]" in l:
            print(" ", l.strip())
    print(f"graficos-x86 ({a.iommu}): {'BIEN' if bien else 'MAL'} en {time.time() - t0:.0f} s; la salida en {serie}")
    sys.exit(0 if bien else 1)


if __name__ == "__main__":
    main()
