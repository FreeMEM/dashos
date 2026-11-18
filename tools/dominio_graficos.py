#!/usr/bin/env python3
"""El dominio de gráficos de x86-64 (X12, G5 de docs/diseño/S38-gpu-escritorio.md), lado invitado: un Linux mínimo con
PCI, DRM y el controlador de virtio-gpu, y un initramfs con Mesa (solo el controlador virgl de Gallium, sin LLVM), GBM,
libdrm y su /init (caja-linux/dominios/graficos_dominio.c), que pinta con GLES 2 y saca los cuadros por KMS.

Nada de lo que se baja o se compila va a git: se queda en $DASHOS_DOMINIO_GRAFICOS (por defecto
~/recuperar/dashos-97/dominio-graficos). Linux y Mesa se compilan y se ejecutan; no se leen para escribir código de
DashOS (sala limpia). Lo de fuera se baja de su sitio oficial y se comprueba su SHA-256 antes de usarlo:

  - el código de Linux, el de medir-linux (tools/medir_dominios.py; si no está, de kernel.org como en dominio_x86.py);
  - la toolchain de x86-64 con musl de Bootlin (la suma que publica junto al .tar.xz);
  - Mesa (archive.mesa3d.org) y libdrm (dri.freedesktop.org): las sumas de aquí, de los ficheros cuya firma GPG se
    comprobó al bajarlos (Mesa la de Eric Engestrom, 57551DE15B968F6341C248F68D8E31AFC32428A6; libdrm la de Simon
    Ser, 34FF9526CFEF0E97A340E2E40FDE7BE0E88F5E48; el 2026-11-17);
  - zlib (zlib.net, la suma que publica en su página).

    tools/dominio_graficos.py preparar        la toolchain, zlib, libdrm, Mesa, el /init, el kernel (tinyconfig +
                                              caja-linux/dominios/x86_graficos.config): bzImage e initramfs.cpio
    tools/dominio_graficos.py qemu [SEG]      el dominio en QEMU (q35 con KVM, sin DashOS) con la misma GPU que tendrá
                                              en DashOS (virtio-gpu-gl-pci con virgl, egl-headless): su salida, dos
                                              capturas de la pantalla de la GPU (por VNC: tools/vnc_captura.py) (no negras y distintas) y la CPU por
                                              cuadro. La referencia para el dominio dentro de DashOS.
"""

import hashlib
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from medir_dominios import FUENTE, cpio_newc, leer_config, pedido, run  # noqa: E402

RAIZ = Path(__file__).resolve().parent.parent
DOMINIOS = RAIZ / "caja-linux" / "dominios"
TRABAJO = Path(os.environ.get("DASHOS_DOMINIO_GRAFICOS", Path.home() / "recuperar" / "dashos-97" / "dominio-graficos"))
DESCARGA = TRABAJO / "descarga"
OBJ = TRABAJO / "obj"
SYSROOT = OBJ / "raiz"
BZIMAGE = TRABAJO / "bzImage"
INITRD = TRABAJO / "initramfs.cpio"
NUCLEOS = min(os.cpu_count() or 8, 16)  # la máquina es compartida

LINUX = ("linux-7.3.4.tar.xz", "https://cdn.kernel.org/pub/linux/kernel/v7.x/linux-7.3.4.tar.xz",
         "b11b6df8295e8e991ff040b71aaa4994a8b55caa23b6b092907f195b5840fd1f")
TOOLCHAIN = ("x86-64--musl--stable-2026.08-1.tar.xz",
             "https://toolchains.bootlin.com/downloads/releases/toolchains/x86-64/tarballs/"
             "x86-64--musl--stable-2026.08-1.tar.xz",
             "78d3a4683d6ac47b5ee73bd5bce210b55eb93dff1b137c61298af97eb0d2b5a6")
MESA = ("mesa-26.2.7.tar.xz", "https://archive.mesa3d.org/mesa-26.2.7.tar.xz",
        "04294f75d22e9f61cd1e050206bc48b3d39366e788f1803a3368e3d5a872ce18")
LIBDRM = ("libdrm-2.4.134.tar.xz", "https://dri.freedesktop.org/libdrm/libdrm-2.4.134.tar.xz",
          "ac5e74d157830eb8bee44c6a6bf3ad49774ef0dd2a72bdad74a8f20308b52a95")
ZLIB = ("zlib-1.4.tar.xz", "https://zlib.net/zlib-1.4.tar.xz",
        "4f11c512053d7bab8d9e9b89bde3575add3e4c50a383b42ef6cb837035fc5a7c")

TC = OBJ / "toolchain" / "x86-64--musl--stable-2026.08-1"
CRUZ = "x86_64-buildroot-linux-musl-"
CFLAGS = "-O2 -march=x86-64 -mtune=generic"

# La línea del kernel en la máquina de DashOS (como LINEA de dominio_x86.py, sin el disco): consola temprana sondeada
# en el 0x3F8, la frecuencia del TSC (no hay con qué medirla), el TSC fiable, /dev/kmsg sin recortar, y los segundos
# que pinta el /init. Sin ACPI el PCI va por el mecanismo 1 (0xCF8/0xCFC), que el kernel prueba solo.
LINEA = ("earlyprintk=serial,0x3f8,115200,keep tsc_early_khz={khz} tsc=reliable printk.devkmsg=on "
         "dashos.graficos={segundos}")


def bajar(nombre, url, suma):
    DESCARGA.mkdir(parents=True, exist_ok=True)
    f = DESCARGA / nombre
    if not f.exists():
        print("bajando", url, flush=True)
        with urllib.request.urlopen(url) as r:
            f.write_bytes(r.read())
    h = hashlib.sha256(f.read_bytes()).hexdigest()
    if h != suma:
        raise SystemExit(f"{f}: sha256 {h}, se esperaba {suma}")
    return f


def desempaquetar(desc, nombre, donde=None):
    donde = donde or OBJ / "fuente"
    destino = donde / nombre
    if not destino.exists():
        donde.mkdir(parents=True, exist_ok=True)
        run("tar", "-xf", bajar(*desc), "-C", donde)
    return destino


def toolchain():
    desempaquetar(TOOLCHAIN, TC.name, OBJ / "toolchain")
    return TC / "bin" / CRUZ


def entorno():
    cc = toolchain()
    return {**os.environ, "CC": f"{cc}gcc", "CXX": f"{cc}g++", "AR": f"{cc}ar", "RANLIB": f"{cc}ranlib",
            "CFLAGS": CFLAGS, "CXXFLAGS": CFLAGS}


def fichero_cruzado():
    cc = toolchain()
    f = OBJ / "cruzado.txt"
    f.write_text(f"""[binaries]
c = '{cc}gcc'
cpp = '{cc}g++'
ar = '{cc}ar'
strip = '{cc}strip'
pkg-config = 'pkg-config'

[properties]
sys_root = '{SYSROOT}'
pkg_config_libdir = '{SYSROOT}/usr/lib/pkgconfig:{SYSROOT}/usr/share/pkgconfig'

[built-in options]
c_args = {CFLAGS.split()!r}
cpp_args = {CFLAGS.split()!r}

[host_machine]
system = 'linux'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'
""")
    return f


def zlib():
    if (SYSROOT / "usr/lib/libz.so").exists():
        return
    fuente = desempaquetar(ZLIB, "zlib-1.4")
    run("./configure", "--prefix=/usr", cwd=fuente, env=entorno(), stdout=subprocess.DEVNULL)
    run("make", f"-j{NUCLEOS}", cwd=fuente, stdout=subprocess.DEVNULL)
    run("make", "install", f"DESTDIR={SYSROOT}", cwd=fuente, stdout=subprocess.DEVNULL)


def meson(fuente, nombre, *opciones):
    build = OBJ / f"build-{nombre}"
    if not (build / "build.ninja").exists():
        run("meson", "setup", build, fuente, "--cross-file", fichero_cruzado(), "--prefix=/usr", "--libdir=lib",
            "--buildtype=release", "-Db_ndebug=true", "--wrap-mode=nodownload", *opciones)
    run("ninja", "-C", build, f"-j{NUCLEOS}")
    run("meson", "install", "-C", build, "--destdir", SYSROOT, "--quiet")


def libdrm():
    if (SYSROOT / "usr/lib/libdrm.so").exists():
        return
    meson(desempaquetar(LIBDRM, "libdrm-2.4.134"), "libdrm", "-Dintel=disabled", "-Dradeon=disabled",
          "-Damdgpu=disabled", "-Dnouveau=disabled", "-Dvmwgfx=disabled", "-Dfreedreno=disabled",
          "-Dvc4=disabled", "-Detnaviv=disabled", "-Dexynos=disabled", "-Domap=disabled", "-Dtegra=disabled",
          "-Dcairo-tests=disabled", "-Dman-pages=disabled", "-Dvalgrind=disabled", "-Dtests=false",
          "-Dinstall-test-programs=false", "-Dudev=false")


def mesa():
    if (SYSROOT / "usr/lib/libgbm.so").exists():
        return
    meson(desempaquetar(MESA, "mesa-26.2.7"), "mesa", "-Dplatforms=", "-Dgallium-drivers=virgl",
          "-Dvulkan-drivers=", "-Dllvm=disabled", "-Dglx=disabled", "-Degl=enabled", "-Dgbm=enabled",
          "-Dgles1=disabled", "-Dgles2=enabled", "-Dopengl=true", "-Dglvnd=disabled", "-Dvalgrind=disabled",
          "-Dlibunwind=disabled", "-Dlmsensors=disabled", "-Dzstd=disabled", "-Dxmlconfig=disabled",
          "-Dexpat=disabled", "-Dshader-cache=disabled", "-Dvideo-codecs=", "-Dgallium-va=disabled",
          "-Dbuild-tests=false", "-Dmicrosoft-clc=disabled", "-Dspirv-tools=disabled", "-Ddisplay-info=disabled",
          "-Dandroid-libbacktrace=disabled", "-Dintel-rt=disabled", "-Dtools=[]")


def init():
    """El /init: graficos_dominio.c contra Mesa, GBM y libdrm, con musl (dinámico)."""
    binario = OBJ / "graficos_dominio"
    cc = toolchain()
    run(f"{cc}gcc", *CFLAGS.split(), "-Wall", "-Wextra", "-o", binario, DOMINIOS / "graficos_dominio.c",
        f"-I{SYSROOT}/usr/include", f"-I{SYSROOT}/usr/include/libdrm", f"-L{SYSROOT}/usr/lib",
        f"-Wl,-rpath-link,{SYSROOT}/usr/lib", "-lEGL", "-lGLESv2", "-lgbm", "-ldrm", "-lm")
    run(f"{cc}strip", binario)
    return binario


def kernel():
    salida = OBJ / "kernel"
    salida.mkdir(parents=True, exist_ok=True)
    mk = ["make", "-C", FUENTE, f"O={salida}", "ARCH=x86_64", f"-j{NUCLEOS}"]
    texto = (DOMINIOS / "x86_graficos.config").read_text()
    if not (salida / "dominio.config").exists() or (salida / "dominio.config").read_text() != texto:
        run(*mk, "tinyconfig", stdout=subprocess.DEVNULL)
        (salida / "dominio.config").write_text(texto)
        run(FUENTE / "scripts/kconfig/merge_config.sh", "-m", "-O", salida, salida / ".config",
            salida / "dominio.config", cwd=FUENTE, env={**os.environ, "ARCH": "x86_64"}, stdout=subprocess.DEVNULL)
        run(*mk, "olddefconfig", stdout=subprocess.DEVNULL)
    hay = leer_config(salida / ".config")
    malas = [f"{k}={v} (queda {hay.get(k, 'n')})" for k, v in pedido(texto).items()
             if hay.get(k, "n") != v and not (v == "n" and k not in hay)]
    if malas:
        print("AVISO, opciones que no quedaron como se pidió:", *malas, sep="\n  ", flush=True)
    run(*mk, "bzImage", stdout=subprocess.DEVNULL)
    BZIMAGE.write_bytes((salida / "arch/x86/boot/bzImage").read_bytes())


def bibliotecas():
    """Las bibliotecas compartidas que van en /lib del initramfs: las de musl y gcc de la toolchain y las de la raíz
    (zlib, libdrm, Mesa y sus módulos de dri y gbm), sin símbolos. Los enlaces simbólicos, como enlaces."""
    cc = toolchain()
    limpio = OBJ / "lib-limpia"
    entradas = []
    tcl = TC / "x86_64-buildroot-linux-musl/sysroot"
    fuentes = [(tcl / "lib" / n, f"lib/{n}") for n in ("libc.so", "libgcc_s.so.1")]
    fuentes.append((tcl / "lib/libc.so", "lib/ld-musl-x86_64.so.1"))
    for n in sorted(x for x in (tcl / "usr/lib").glob("libstdc++.so*") if not x.name.endswith(".py")):
        fuentes.append((n, f"lib/{n.name}"))
    for sub in ("", "dri", "gbm"):
        d = SYSROOT / "usr/lib" / sub
        if d.is_dir():
            for n in sorted(d.iterdir()):
                if ".so" in n.name and (n.is_file() or n.is_symlink()):
                    fuentes.append((n, f"lib/{sub + '/' if sub else ''}{n.name}"))
    vistos = set()
    for origen, destino in fuentes:
        if destino in vistos:
            continue
        vistos.add(destino)
        if origen.is_symlink() and destino != "lib/ld-musl-x86_64.so.1":
            entradas.append((destino, 0o120777, os.readlink(origen).encode(), (0, 0)))
            continue
        copia = limpio / destino
        copia.parent.mkdir(parents=True, exist_ok=True)
        run(f"{cc}strip", "--strip-unneeded", "-o", copia, origen.resolve(), stdout=subprocess.DEVNULL)
        entradas.append((destino, 0o100755, copia.read_bytes(), (0, 0)))
    return entradas


def preparar():
    OBJ.mkdir(parents=True, exist_ok=True)
    if not FUENTE.exists():
        FUENTE.parent.mkdir(parents=True, exist_ok=True)
        run("tar", "-xf", bajar(*LINUX), "-C", FUENTE.parent)
    zlib()
    libdrm()
    mesa()
    ini = init()
    kernel()
    entradas = [(d, 0o040755, b"", (0, 0)) for d in ("dev", "proc", "sys", "tmp", "lib", "lib/dri", "lib/gbm",
                                                     "usr", "usr/lib")]
    entradas.append(("dev/console", 0o020600, b"", (5, 1)))
    entradas += bibliotecas()
    # Mesa busca sus módulos en /usr/lib/dri y /usr/lib/gbm (--prefix=/usr): enlaces a /lib.
    entradas.append(("usr/lib/dri", 0o120777, b"../../lib/dri", (0, 0)))
    entradas.append(("usr/lib/gbm", 0o120777, b"../../lib/gbm", (0, 0)))
    entradas = [e for e in entradas if not (e[0] in ("usr/lib/dri", "usr/lib/gbm") and e[1] == 0o040755)]
    entradas.append(("init", 0o100755, ini.read_bytes(), (0, 0)))
    INITRD.write_bytes(cpio_newc(entradas))
    print(f"kernel: {BZIMAGE} ({BZIMAGE.stat().st_size} bytes); initramfs: {INITRD} ({INITRD.stat().st_size} bytes;"
          f" /init {ini.stat().st_size})", flush=True)


# ---- la prueba en QEMU, sin DashOS ----

def khz_del_pc():
    from dominio_x86 import khz_del_pc as k
    return k()


def qmp(sock, *ordenes):
    import json
    import socket
    s = socket.socket(socket.AF_UNIX)
    s.connect(str(sock))
    f = s.makefile("rw")
    f.readline()
    respuestas = []
    for o in ({"execute": "qmp_capabilities"}, *ordenes):
        f.write(json.dumps(o) + "\n")
        f.flush()
        while True:
            r = json.loads(f.readline())
            if "return" in r or "error" in r:
                respuestas.append(r)
                break
    s.close()
    return respuestas[1:]


def captura(vnc, destino):
    """La pantalla por el VNC de QEMU (el screendump de QMP no tiene superficie con egl-headless)."""
    from vnc_captura import capturar
    destino.unlink(missing_ok=True)
    capturar(vnc, destino)
    return destino


def mirar_capturas(a, b):
    """(no negra a, no negra b, distintas): con PIL si está; si no, por los bytes del PNG."""
    try:
        from PIL import Image, ImageChops
        ia, ib = Image.open(a).convert("RGB"), Image.open(b).convert("RGB")
        no_negra = lambda i: i.getextrema() != ((0, 0), (0, 0), (0, 0))  # noqa: E731
        return no_negra(ia), no_negra(ib), ImageChops.difference(ia, ib).getbbox() is not None, ia.size
    except ImportError:
        da, db = a.read_bytes(), b.read_bytes()
        return len(da) > 4000, len(db) > 4000, da != db, None


def qemu(segundos=10):
    """El dominio en QEMU q35 con KVM: virtio-gpu-gl-pci (virgl) y egl-headless, como lo tendrá DashOS."""
    sal = OBJ / "qemu"
    sal.mkdir(parents=True, exist_ok=True)
    (sal / "vnc.sock").unlink(missing_ok=True)
    linea = LINEA.format(khz=khz_del_pc(), segundos=segundos)
    orden = ["qemu-system-x86_64", "-M", "q35", "-accel", "kvm", "-cpu", "host,-x2apic", "-smp", "1", "-m", "512M",
             "-nodefaults", "-no-user-config", "-display", "egl-headless", "-vnc", f"unix:{sal / 'vnc.sock'}", "-serial", "stdio", "-no-reboot",
             "-device", "virtio-gpu-gl-pci,id=gpu0,disable-legacy=on,iommu_platform=on,xres=1280,yres=720",
             "-kernel", str(BZIMAGE), "-initrd", str(INITRD), "-append", linea]
    print("+", " ".join(orden), flush=True)
    t0 = time.time()
    p = subprocess.Popen(orden, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, errors="replace")
    capturas, resultado, fin = [], None, None
    try:
        for l in p.stdout:
            l = l.rstrip()
            print(l, flush=True)
            if "dominio: pintando" in l:
                time.sleep(2)
                capturas.append(captura(sal / "vnc.sock", sal / "captura-1.png"))
                time.sleep(0.5)
                capturas.append(captura(sal / "vnc.sock", sal / "captura-2.png"))
            if "dominio: cuadros " in l and " en " in l:
                resultado = l
            if "dominio: fin " in l:
                fin = l
                break
            if time.time() - t0 > segundos + 60:
                break
    finally:
        p.kill()
        p.wait()
    if len(capturas) == 2:
        na, nb, distintas, tam = mirar_capturas(*capturas)
        print(f"capturas: {capturas[0]} y {capturas[1]} ({tam}): no negras {na} y {nb}, distintas {distintas}")
    print("resultado:", resultado)
    print("fin:", fin)
    return fin is not None and fin.endswith("fin graficos 0") and len(capturas) == 2


def main():
    orden = sys.argv[1] if len(sys.argv) > 1 else ""
    if orden == "preparar":
        preparar()
    elif orden == "qemu":
        sys.exit(0 if qemu(int(sys.argv[2]) if len(sys.argv) > 2 else 10) else 1)
    else:
        print(__doc__)
        sys.exit(2)


if __name__ == "__main__":
    main()
