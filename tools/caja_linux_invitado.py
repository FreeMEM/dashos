#!/usr/bin/env python3
"""El invitado de prueba de la caja Linux, en el PC.

Nada de lo que baja o compila va a git: se queda en ~/.cache/dashos-caja-linux/.

    tools/caja_linux_invitado.py preparar   # baja y compila Linux (arm64) y un initramfs con busybox estático
    tools/caja_linux_invitado.py probar     # arranca ese Linux en qemu-system-aarch64 -M virt con el DTB y la carga
                                            # que saca caja-linux/vmm (no los de QEMU)
    tools/caja_linux_invitado.py probar-vhost  # lo mismo, con el disco virtio-blk del crate (cola y peticiones)
                                               # servido a QEMU por vhost-user: lo usa el controlador de Linux

"probar" no usa -kernel ni -dtb de QEMU. El crate coloca la Image, el DTB y el initrd según el protocolo de arranque
de arm64 y escribe un trampolín de seis instrucciones que pone x0 = DTB, x1..x3 = 0 y salta al kernel. QEMU solo
carga esos ficheros en sus direcciones (-device loader) y empieza en el trampolín, en EL1 con la MMU apagada.
Así se prueban el DTB y la carga tal como los usará el hipervisor de DashOS.

Las direcciones de los aparatos son las de la placa virt de QEMU (GIC, PL011, transportes virtio-mmio), porque los
aparatos que ve el invitado en esta prueba son los de QEMU. Se sacaron de su DTB (-M virt,dumpdtb), sin mirar su
código.
"""

import argparse
import gzip
import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.request
from pathlib import Path

RAIZ = Path(__file__).resolve().parent.parent
CACHE = Path(os.environ.get("DASHOS_CAJA_LINUX_CACHE", Path.home() / ".cache" / "dashos-caja-linux"))
TOOLCHAIN = Path.home() / ".local/opt/toolchains/aarch64--musl--stable-2026.08-1/bin/aarch64-buildroot-linux-musl-"
BUSYBOX = "1.37.0"
NUCLEOS = os.cpu_count() or 8

# Lo que se añade al defconfig de arm64 (ya trae virtio-mmio, virtio-blk, virtio-console y el PL011): nada de
# módulos que hagan falta para arrancar.
FRAGMENTO = """
CONFIG_VIRTIO_MMIO=y
CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES=y
CONFIG_VIRTIO_BLK=y
CONFIG_VIRTIO_CONSOLE=y
CONFIG_VIRTIO_NET=y
CONFIG_HVC_DRIVER=y
CONFIG_SERIAL_AMBA_PL011=y
CONFIG_SERIAL_AMBA_PL011_CONSOLE=y
CONFIG_BLK_DEV_INITRD=y
CONFIG_RD_GZIP=y
CONFIG_DEVTMPFS=y
CONFIG_DEVTMPFS_MOUNT=y
CONFIG_EXT4_FS=y
CONFIG_MODULES=n
"""

# El kernel pequeño para la caja de DashOS (Image-caja): tinyconfig y solo lo que hace falta para llegar a busybox con
# los aparatos de la caja (GICv2, temporizador, PSCI, PL011, virtio-mmio: consola y disco). La Image del defconfig
# (74 MB) no cabe en el paquete de arranque de DashOS, que el cargador de UEFI pone en 0x40200000 debajo de lo que
# ocupa EDK2.
FRAGMENTO_CAJA = """
CONFIG_PRINTK=y
CONFIG_PRINTK_TIME=y
CONFIG_BUG=y
CONFIG_MULTIUSER=y
CONFIG_FUTEX=y
CONFIG_EPOLL=y
CONFIG_SIGNALFD=y
CONFIG_TIMERFD=y
CONFIG_EVENTFD=y
CONFIG_SHMEM=y
CONFIG_POSIX_TIMERS=y
CONFIG_HIGH_RES_TIMERS=y
CONFIG_NO_HZ_IDLE=y
CONFIG_KERNEL_MODE_NEON=y
CONFIG_BLK_DEV_INITRD=y
CONFIG_RD_GZIP=y
CONFIG_BINFMT_ELF=y
CONFIG_BINFMT_SCRIPT=y
CONFIG_PROC_FS=y
CONFIG_SYSFS=y
CONFIG_TMPFS=y
CONFIG_DEVTMPFS=y
CONFIG_DEVTMPFS_MOUNT=y
CONFIG_TTY=y
CONFIG_VT=n
CONFIG_UNIX98_PTYS=y
CONFIG_SERIAL_EARLYCON=y
CONFIG_ARM_AMBA=y
CONFIG_SERIAL_AMBA_PL011=y
CONFIG_SERIAL_AMBA_PL011_CONSOLE=y
CONFIG_HVC_DRIVER=y
CONFIG_VIRTIO_MENU=y
CONFIG_VIRTIO=y
CONFIG_VIRTIO_MMIO=y
CONFIG_VIRTIO_CONSOLE=y
CONFIG_BLOCK=y
CONFIG_BLK_DEV=y
CONFIG_VIRTIO_BLK=y
CONFIG_OF=y
CONFIG_ARM_GIC=y
CONFIG_ARM_ARCH_TIMER=y
CONFIG_ARM_PSCI_FW=y
CONFIG_POWER_RESET=y
CONFIG_NET=y
CONFIG_INET=y
CONFIG_UNIX=y
CONFIG_PACKET=y
CONFIG_MODULES=n
"""

INIT = r"""#!/bin/sh
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev 2>/dev/null
echo "caja-linux: init en marcha"
echo "caja-linux: $(busybox | head -1)"
echo "caja-linux: kernel $(uname -r) $(uname -m)"
echo "caja-linux: cpus $(grep -c ^processor /proc/cpuinfo)"
echo "caja-linux: memoria $(grep MemTotal /proc/meminfo)"
echo "caja-linux: virtio $(ls /sys/bus/virtio/devices | tr '\n' ' ')"
for d in /sys/bus/virtio/devices/*; do echo "caja-linux:   $(basename $d) id=$(cat $d/device) $(basename $(readlink $d/driver) 2>/dev/null)"; done
echo "caja-linux: consola $(cat /sys/class/tty/console/active)"
if [ -b /dev/vda ]; then echo "caja-linux: vda $(cat /sys/block/vda/size) sectores, primeros bytes: $(head -c 16 /dev/vda)"; fi
# En la caja de DashOS (caja.escribir en la línea de órdenes): escribe en el sector 8, y el servicio lo mira al acabar.
if grep -q caja.escribir /proc/cmdline && [ -b /dev/vda ]; then
  echo -n ESCRITO-POR-LINUX | dd of=/dev/vda bs=512 seek=8 conv=fsync 2>/dev/null && sync
  echo "caja-linux: escrito en vda: $(dd if=/dev/vda bs=512 skip=8 count=1 2>/dev/null | head -c 17)"
fi
# Con un disco de 8 MiB o más (el de probar-vhost): suma de todo, y copia del primer MiB a los 4 MiB.
if [ -b /dev/vda ] && [ "$(cat /sys/block/vda/size)" -ge 16384 ]; then
  echo "caja-linux: md5 vda $(md5sum /dev/vda | cut -d' ' -f1)"
  dd if=/dev/vda of=/dev/vda bs=65536 count=16 seek=64 conv=fsync 2>/dev/null && sync
  echo "caja-linux: copia hecha, md5 4M $(dd if=/dev/vda bs=65536 skip=64 count=16 2>/dev/null | md5sum | cut -d' ' -f1)"
fi
if [ -e /sys/class/net/eth0 ]; then
  ifconfig lo 127.0.0.1 up
  ifconfig eth0 10.0.2.15 netmask 255.255.255.0 up
  echo "caja-linux: eth0 $(cat /sys/class/net/eth0/address) mtu $(cat /sys/class/net/eth0/mtu)"
  echo "caja-linux: ping $(ping -c 3 -W 3 10.0.2.2 | grep packets)"
fi
grep -E 'GIC|virtio|uart|arch_timer' /proc/interrupts | sed 's/^/caja-linux: irq /'
echo "caja-linux: psci $(dmesg | grep -o 'PSCIv[0-9.]* detected in firmware' | head -1)"
echo "caja-linux: fin de la prueba"
poweroff -f
"""


def run(*cmd, **kw):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    subprocess.run([str(c) for c in cmd], check=True, **kw)


def bajar(url, destino):
    if destino.exists():
        return
    print("bajando", url, flush=True)
    tmp = destino.with_suffix(destino.suffix + ".part")
    with urllib.request.urlopen(url) as r, open(tmp, "wb") as f:
        shutil.copyfileobj(r, f)
    tmp.rename(destino)


def version_linux():
    with urllib.request.urlopen("https://www.kernel.org/releases.json") as r:
        datos = json.load(r)
    for rel in datos["releases"]:
        if rel["moniker"] == "longterm" and rel["version"].startswith("6.18."):
            return rel["version"]
    raise SystemExit("no encuentro la 6.18 en kernel.org")


def kernel():
    imagen = CACHE / "Image"
    if imagen.exists():
        print("ya está", imagen)
        return
    v = version_linux()
    tar = CACHE / f"linux-{v}.tar.xz"
    bajar(f"https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-{v}.tar.xz", tar)
    fuente = CACHE / f"linux-{v}"
    if not fuente.exists():
        run("tar", "-xf", tar, "-C", CACHE)
    salida = CACHE / "linux-build"
    salida.mkdir(exist_ok=True)
    mk = ["make", "-C", fuente, f"O={salida}", "ARCH=arm64", "LLVM=1", f"-j{NUCLEOS}"]
    run(*mk, "defconfig")
    (salida / "caja.config").write_text(FRAGMENTO)
    run(fuente / "scripts/kconfig/merge_config.sh", "-m", "-O", salida, salida / ".config", salida / "caja.config",
        cwd=fuente)
    run(*mk, "olddefconfig")
    run(*mk, "Image")
    shutil.copy(salida / "arch/arm64/boot/Image", imagen)


def kernel_caja():
    """Image-caja: la de tinyconfig con FRAGMENTO_CAJA, para el paquete de arranque de DashOS."""
    imagen = CACHE / "Image-caja"
    if imagen.exists():
        print("ya está", imagen)
        return
    v = version_linux()
    tar = CACHE / f"linux-{v}.tar.xz"
    bajar(f"https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-{v}.tar.xz", tar)
    fuente = CACHE / f"linux-{v}"
    if not fuente.exists():
        run("tar", "-xf", tar, "-C", CACHE)
    salida = CACHE / "linux-build-caja"
    salida.mkdir(exist_ok=True)
    mk = ["make", "-C", fuente, f"O={salida}", "ARCH=arm64", "LLVM=1", f"-j{NUCLEOS}"]
    run(*mk, "tinyconfig")
    (salida / "caja.config").write_text(FRAGMENTO_CAJA)
    run(fuente / "scripts/kconfig/merge_config.sh", "-m", "-O", salida, salida / ".config", salida / "caja.config",
        cwd=fuente)
    run(*mk, "olddefconfig")
    config = (salida / ".config").read_text()
    faltan = [l for l in FRAGMENTO_CAJA.split() if l.endswith("=y") and l not in config.splitlines()]
    if faltan:
        print("aviso: no quedaron en la configuración:", " ".join(faltan))
    run(*mk, "Image")
    shutil.copy(salida / "arch/arm64/boot/Image", imagen)


def busybox():
    binario = CACHE / "busybox"
    if binario.exists():
        print("ya está", binario)
        return
    tar = CACHE / f"busybox-{BUSYBOX}.tar.bz2"
    bajar(f"https://busybox.net/downloads/busybox-{BUSYBOX}.tar.bz2", tar)
    fuente = CACHE / f"busybox-{BUSYBOX}"
    if not fuente.exists():
        run("tar", "-xf", tar, "-C", CACHE)
    mk = ["make", "-C", fuente, f"CROSS_COMPILE={TOOLCHAIN}", f"-j{NUCLEOS}"]
    run(*mk, "defconfig")
    cfg = fuente / ".config"
    texto = cfg.read_text()
    # Estático; tc no compila con las cabeceras nuevas del kernel.
    texto = texto.replace("# CONFIG_STATIC is not set", "CONFIG_STATIC=y")
    texto = texto.replace("CONFIG_TC=y", "# CONFIG_TC is not set")
    # La aceleración de SHA de 1.37 solo compila en x86.
    texto = texto.replace("CONFIG_SHA1_HWACCEL=y", "# CONFIG_SHA1_HWACCEL is not set")
    texto = texto.replace("CONFIG_SHA256_HWACCEL=y", "# CONFIG_SHA256_HWACCEL is not set")
    cfg.write_text(texto)
    run(*mk, "oldconfig", input=b"\n" * 200)
    run(*mk, "busybox")
    shutil.copy(fuente / "busybox", binario)


def cpio_newc(entradas):
    """Un archivo cpio "newc" (el que lee el kernel para el initramfs). entradas: (nombre, modo, datos, rdev)."""
    out = io.BytesIO()
    ino = 1
    for nombre, modo, datos, rdev in entradas + [("TRAILER!!!", 0, b"", (0, 0))]:
        nb = nombre.encode() + b"\0"
        cab = "070701" + "".join(f"{x:08x}" for x in (
            ino, modo, 0, 0, 1, 0, len(datos), 0, 0, rdev[0], rdev[1], len(nb), 0))
        out.write(cab.encode() + nb)
        out.write(b"\0" * (-(110 + len(nb)) % 4))
        out.write(datos)
        out.write(b"\0" * (-len(datos) % 4))
        ino += 1
    return out.getvalue()


def initramfs():
    destino = CACHE / "initramfs.cpio.gz"
    bb = (CACHE / "busybox").read_bytes()
    entradas = []
    for d in ("bin", "sbin", "dev", "proc", "sys", "etc", "tmp", "usr", "usr/bin", "usr/sbin"):
        entradas.append((d, 0o040755, b"", (0, 0)))
    entradas.append(("dev/console", 0o020600, b"", (5, 1)))
    entradas.append(("bin/busybox", 0o100755, bb, (0, 0)))
    for applet in ("sh", "mount", "echo", "cat", "ls", "grep", "head", "tr", "uname", "basename", "readlink",
                   "sed", "dmesg", "poweroff", "md5sum", "dd", "sync", "cut", "ifconfig", "ping"):
        entradas.append((f"bin/{applet}", 0o120777, b"busybox", (0, 0)))
    entradas.append(("init", 0o100755, INIT.encode(), (0, 0)))
    destino.write_bytes(gzip.compress(cpio_newc(entradas)))
    print("initramfs:", destino, destino.stat().st_size, "bytes")


def disco():
    """Un disco de prueba de 1 MiB con un texto conocido al principio."""
    destino = CACHE / "disco.img"
    datos = bytearray(1 << 20)
    marca = b"DISCO-CAJA-LINUX"
    datos[: len(marca)] = marca
    destino.write_bytes(bytes(datos))


def preparar(_args):
    CACHE.mkdir(parents=True, exist_ok=True)
    busybox()
    initramfs()
    disco()
    kernel()
    kernel_caja()


# La placa virt de QEMU, sacada de su DTB (qemu-system-aarch64 -M virt,dumpdtb=x.dtb): GICv2 en 0x08000000 (CPU en
# 0x08010000), PL011 en 0x09000000 con la SPI 1, transportes virtio-mmio en 0x0a000000 + 0x200·i con la SPI 16+i.
# Con -device virtio-*-device QEMU ocupa primero el transporte de número más alto.
QEMU_VIRT = {
    "gic": "v2:0x08000000:0x08010000",
    "uart": "0x09000000:1",
    "virtio": ["0x0a003e00:47", "0x0a003c00:46", "0x0a003a00:45"],
}


def probar(args, vhost=False):
    for f in ("Image", "initramfs.cpio.gz", "disco.img"):
        if not (CACHE / f).exists():
            raise SystemExit(f"falta {CACHE / f}: ejecuta antes 'preparar'")
    run("cargo", "build", "--release", "--quiet", "--manifest-path", RAIZ / "caja-linux/vmm/Cargo.toml",
        "--bin", "caja-vmm")
    herramienta = RAIZ / "caja-linux/vmm/target/release/caja-vmm"
    sal = CACHE / "prueba"
    if sal.exists():
        shutil.rmtree(sal)
    sal.mkdir()
    # QEMU sin -kernel deja su propio DTB en el primer MiB de la RAM: al invitado se le dan los 2 MiB siguientes en
    # adelante (el DTB nuestro dice que la RAM empieza ahí).
    base = 0x4000_0000 + (2 << 20)
    memoria = args.memoria * (1 << 20) - (2 << 20)
    cmd = [herramienta, "preparar", "--kernel", CACHE / "Image", "--initrd", CACHE / "initramfs.cpio.gz",
           "--memoria", f"{base:#x}:{memoria:#x}", "--cpus", str(args.cpus), "--gic", QEMU_VIRT["gic"],
           "--uart", QEMU_VIRT["uart"], "--bootargs", args.bootargs, "--salida", sal]
    for v in QEMU_VIRT["virtio"]:
        cmd += ["--virtio", v]
    run(*cmd)
    plan = json.loads((sal / "plan.json").read_text())
    run("dtc", "-I", "dtb", "-O", "dts", "-o", sal / "invitado.dts", sal / "invitado.dtb")
    qemu = ["qemu-system-aarch64", "-M", "virt,gic-version=2", "-cpu", "cortex-a72", "-smp", str(args.cpus),
            "-m", f"{args.memoria}M", "-display", "none", "-monitor", "none", "-no-reboot",
            # virtio-mmio moderno (versión 2), el mismo transporte que da caja-linux/vmm.
            "-global", "virtio-mmio.force-legacy=false",
            "-serial", f"file:{sal / 'pl011.log'}",
            "-device", "virtio-serial-device", "-chardev", f"file,id=hvc,path={sal / 'hvc0.log'}",
            "-device", "virtconsole,chardev=hvc",
            ]
    vhost_proc = None
    if vhost:
        # Memoria compartida para que el proceso de vhost-user vea la del invitado.
        qemu[2] = "virt,gic-version=2,memory-backend=ram"
        qemu += ["-object", f"memory-backend-memfd,id=ram,size={args.memoria}M,share=on",
                 "-chardev", f"socket,id=vu,path={sal / 'vhost.sock'}",
                 "-device", "vhost-user-blk,chardev=vu,num-queues=1",
                 "-chardev", f"socket,id=vn,path={sal / 'red.sock'}",
                 "-netdev", "vhost-user,id=n0,chardev=vn",
                 "-device", "virtio-net-device,netdev=n0,mac=52:54:00:12:34:56"]
        run("cargo", "build", "--release", "--quiet", "--manifest-path",
            RAIZ / "caja-linux/vhost-prueba/Cargo.toml")
        imagen = sal / "disco-vhost.img"
        # 8 MiB de bytes conocidos (seudoaleatorios, repetibles).
        bloque = hashlib.sha256(b"caja-linux").digest() * (8 << 20 >> 5)
        imagen.write_bytes(bloque[: 8 << 20])
        md5_original = hashlib.md5(imagen.read_bytes()).hexdigest()
        md5_primer_mib = hashlib.md5(imagen.read_bytes()[: 1 << 20]).hexdigest()
        binario = str(RAIZ / "caja-linux/vhost-prueba/target/release/vhost-blk")
        vhost_proc = [subprocess.Popen([binario, "blk", str(sal / "vhost.sock"), str(imagen)]),
                      subprocess.Popen([binario, "red", str(sal / "red.sock")])]
        for _ in range(100):
            if (sal / "vhost.sock").exists() and (sal / "red.sock").exists():
                break
            time.sleep(0.05)
    else:
        qemu += ["-drive", f"if=none,id=d0,format=raw,file={CACHE / 'disco.img'},readonly=on",
                 "-device", "virtio-blk-device,drive=d0"]
    for trozo in plan["trozos"]:
        qemu += ["-device", f"loader,file={sal / trozo['fichero']},addr={trozo['direccion']:#x},force-raw=on"]
    qemu += ["-device", f"loader,addr={plan['pc']:#x},cpu-num=0"]
    print("+", " ".join(str(c) for c in qemu), flush=True)
    try:
        subprocess.run([str(c) for c in qemu], timeout=args.tiempo, check=False)
    except subprocess.TimeoutExpired:
        print("QEMU no acabó en", args.tiempo, "s")
    for p in vhost_proc or []:
        try:
            p.wait(timeout=10)
        except subprocess.TimeoutExpired:
            p.kill()
    for nombre in ("pl011.log", "hvc0.log"):
        f = sal / nombre
        print(f"===== {nombre} =====")
        if f.exists():
            print(f.read_text(errors="replace"))
    hvc = (sal / "hvc0.log").read_text(errors="replace") if (sal / "hvc0.log").exists() else ""
    if "caja-linux: fin de la prueba" not in hvc:
        raise SystemExit("PRUEBA MAL: no llegó al final de init")
    if not vhost:
        if "DISCO-CAJA-LINUX" not in hvc:
            raise SystemExit("PRUEBA MAL: no leyó el disco")
        print("PRUEBA BIEN: Linux arrancó con el DTB y la carga de caja-linux/vmm")
        return
    datos = imagen.read_bytes()
    bien = (f"md5 vda {md5_original}" in hvc and f"md5 4M {md5_primer_mib}" in hvc
            and datos[4 << 20: 5 << 20] == datos[: 1 << 20])
    if not bien:
        raise SystemExit(f"PRUEBA MAL: se esperaba md5 {md5_original} del disco y {md5_primer_mib} de la copia")
    if "3 packets received" not in hvc:
        raise SystemExit("PRUEBA MAL: el ping por la red del crate no volvió")
    print("PRUEBA BIEN: los controladores virtio-blk y virtio-net de Linux usaron el disco y la red de caja-linux/vmm")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="orden", required=True)
    sub.add_parser("preparar").set_defaults(f=preparar)
    pr = sub.add_parser("probar")
    pr.add_argument("--cpus", type=int, default=2)
    pr.add_argument("--memoria", type=int, default=512, help="MiB")
    pr.add_argument("--tiempo", type=int, default=120, help="segundos antes de cortar QEMU")
    pr.add_argument("--bootargs", default="console=hvc0 earlycon panic=-1")
    pr.set_defaults(f=probar)
    pv = sub.add_parser("probar-vhost")
    for a in pr._actions[1:]:
        pv._add_action(a)
    pv.set_defaults(f=lambda a: probar(a, vhost=True))
    a = p.parse_args()
    a.f(a)


if __name__ == "__main__":
    main()
