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
import tempfile
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
CONFIG_NETDEVICES=y
CONFIG_NET_CORE=y
CONFIG_VIRTIO_NET=y
CONFIG_MODULES=n
"""

# El kernel para Arch Linux ARM en la caja (Image-arch): lo de Image-caja y lo que pide systemd (su README, "REQUIREMENTS":
# cgroups, devtmpfs, inotify, signalfd, timerfd, epoll, fhandle, espacios de nombres, seccomp, autofs, ACL de tmpfs) y
# lo que piden glibc y pacman (futex, flock, AIO, rseq, membarrier, SysV IPC; pacman 7 baja los paquetes en un sandbox
# con Landlock). El disco es un ext4 (la partición de la caja en el NVMe de DashOS, por virtio-blk) y la hora sale del
# PL031 que emula la caja. Sin módulos: los de Arch son de su propio kernel (linux-aarch64, una Image de 42 MB que
# necesita los suyos en un initramfs de 15 MB), que no se usa: con 57 MB entre los dos, sin probar, es dudoso que quepa en
# el paquete de arranque de DashOS (la del defconfig, de 74 MB, no cabía). La de aquí mide 7 MB.
FRAGMENTO_ARCH = FRAGMENTO_CAJA + """
CONFIG_LOCALVERSION="-caja"
CONFIG_SYSVIPC=y
CONFIG_POSIX_MQUEUE=y
CONFIG_FHANDLE=y
CONFIG_FILE_LOCKING=y
CONFIG_AIO=y
CONFIG_IO_URING=y
CONFIG_ADVISE_SYSCALLS=y
CONFIG_MEMBARRIER=y
CONFIG_RSEQ=y
CONFIG_KCMP=y
CONFIG_CROSS_MEMORY_ATTACH=y
CONFIG_SYSCTL=y
CONFIG_PROC_SYSCTL=y
CONFIG_INOTIFY_USER=y
CONFIG_FANOTIFY=y
CONFIG_AUTOFS_FS=y
CONFIG_TMPFS_POSIX_ACL=y
CONFIG_TMPFS_XATTR=y
CONFIG_EXT4_FS=y
CONFIG_EXT4_USE_FOR_EXT2=y
CONFIG_EXT4_FS_POSIX_ACL=y
CONFIG_EXT4_FS_SECURITY=y
CONFIG_FS_POSIX_ACL=y
CONFIG_UEVENT_HELPER=n
CONFIG_FW_LOADER_USER_HELPER=n
CONFIG_CGROUPS=y
CONFIG_MEMCG=y
CONFIG_BLK_CGROUP=y
CONFIG_CGROUP_SCHED=y
CONFIG_FAIR_GROUP_SCHED=y
CONFIG_CFS_BANDWIDTH=y
CONFIG_CGROUP_PIDS=y
CONFIG_CGROUP_FREEZER=y
CONFIG_CPUSETS=y
CONFIG_CGROUP_CPUACCT=y
CONFIG_CGROUP_BPF=y
CONFIG_BPF_SYSCALL=y
CONFIG_NAMESPACES=y
CONFIG_UTS_NS=y
CONFIG_IPC_NS=y
CONFIG_USER_NS=y
CONFIG_PID_NS=y
CONFIG_NET_NS=y
CONFIG_SECCOMP=y
CONFIG_SECCOMP_FILTER=y
CONFIG_PSI=y
CONFIG_KEYS=y
CONFIG_IPV6=y
CONFIG_UNIX_DIAG=y
CONFIG_INET_DIAG=y
CONFIG_RTC_CLASS=y
CONFIG_RTC_HCTOSYS=y
CONFIG_RTC_DRV_PL031=y
CONFIG_MAGIC_SYSRQ=y
CONFIG_SECURITY=y
CONFIG_SECURITY_LANDLOCK=y
CONFIG_LSM="landlock"
"""

INIT = r"""#!/bin/sh
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev 2>/dev/null
busybox --install -s /bin 2>/dev/null
hostname caja
echo "caja-linux: init en marcha"
echo "caja-linux: $(busybox | head -1)"
echo "caja-linux: kernel $(uname -r) $(uname -m)"
echo "caja-linux: cpus $(grep -c ^processor /proc/cpuinfo), nproc $(nproc), en línea $(cat /sys/devices/system/cpu/online)"
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
# La red: en QEMU, 10.0.2.15 es la de quien tenga la tarjeta; en la caja de DashOS, DashOS. El invitado, la 16.
if [ -e /sys/class/net/eth0 ]; then
  ifconfig lo 127.0.0.1 up
  ifconfig eth0 10.0.2.16 netmask 255.255.255.0 up
  route add default gw 10.0.2.2 2>/dev/null
  echo "caja-linux: eth0 $(cat /sys/class/net/eth0/address) mtu $(cat /sys/class/net/eth0/mtu)"
  echo "caja-linux: ping $(ping -c 3 -W 3 10.0.2.2 | grep packets)"
fi
# Varias vCPU (caja.paralelo en la línea de órdenes): el mismo trabajo de CPU tantas veces como CPU haya, seguidas y
# a la vez, con lo que tarda cada tanda (reloj del invitado) y el tiempo de usuario que se llevó cada CPU.
if grep -q caja.paralelo /proc/cmdline; then
  n=$(nproc)
  trabajo() { i=0; while [ $i -lt 20000 ]; do i=$((i+1)); done; }
  t0=$(cut -d' ' -f1 /proc/uptime)
  k=0; while [ $k -lt $n ]; do trabajo; k=$((k+1)); done
  t1=$(cut -d' ' -f1 /proc/uptime)
  k=0; while [ $k -lt $n ]; do trabajo & k=$((k+1)); done; wait
  t2=$(cut -d' ' -f1 /proc/uptime)
  echo "caja-linux: paralelo $n trabajos: seguidos $(awk "BEGIN{print $t1-$t0}") s, a la vez $(awk "BEGIN{print $t2-$t1}") s"
  echo "caja-linux: usuario por cpu (tics): $(grep '^cpu[0-9]' /proc/stat | awk '{printf "%s %s ", $1, $2}')"
fi
grep -E 'GIC|virtio|uart|arch_timer|IPI[01]:' /proc/interrupts | sed 's/^/caja-linux: irq /'
echo "caja-linux: psci $(dmesg | grep -o 'PSCIv[0-9.]* detected in firmware' | head -1)"
echo "caja-linux: fin de la prueba"
# Con una ventana de terminal en DashOS (caja.terminal): una shell interactiva en hvc0, con su terminal de control,
# hasta que se sale de ella (exit o poweroff -f).
if grep -q caja.terminal /proc/cmdline; then
  export HOME=/ TERM=vt102 PS1='caja:\w# '
  echo "caja-linux: terminal lista"
  (setsid cttyhack sh)
fi
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


def kernel_pequeno(nombre="Image-caja", fragmento=FRAGMENTO_CAJA, carpeta="linux-build-caja"):
    """Una Image de tinyconfig con `fragmento`, para el paquete de arranque de DashOS: Image-caja (busybox) o Image-arch
    (Arch Linux ARM con systemd)."""
    imagen = CACHE / nombre
    salida = CACHE / carpeta
    # La configuración con que se hizo la Image: si cambia el fragmento, se vuelve a compilar (sobre lo ya compilado).
    hecha = salida / "caja.config.hecha"
    if imagen.exists() and hecha.exists() and hecha.read_text() == fragmento:
        print("ya está", imagen)
        return
    # Las fuentes de la 6.18 que ya estén bajadas; si no hay, la última de kernel.org.
    ya = sorted(CACHE.glob("linux-6.18.*/Makefile"), key=lambda m: [int(x) for x in m.parent.name[6:].split(".")])
    if ya:
        fuente = ya[-1].parent
    else:
        v = version_linux()
        tar = CACHE / f"linux-{v}.tar.xz"
        bajar(f"https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-{v}.tar.xz", tar)
        fuente = CACHE / f"linux-{v}"
        if not fuente.exists():
            run("tar", "-xf", tar, "-C", CACHE)
    salida.mkdir(exist_ok=True)
    mk = ["make", "-C", fuente, f"O={salida}", "ARCH=arm64", "LLVM=1", f"-j{NUCLEOS}"]
    run(*mk, "tinyconfig")
    (salida / "caja.config").write_text(fragmento)
    run(fuente / "scripts/kconfig/merge_config.sh", "-m", "-O", salida, salida / ".config", salida / "caja.config",
        cwd=fuente)
    run(*mk, "olddefconfig")
    config = (salida / ".config").read_text()
    faltan = [l for l in fragmento.split() if l.endswith("=y") and l not in config.splitlines()]
    if faltan:
        print("aviso: no quedaron en la configuración:", " ".join(faltan))
    run(*mk, "Image")
    shutil.copy(salida / "arch/arm64/boot/Image", imagen)
    hecha.write_text(fragmento)


def kernel_caja():
    """Image-caja: la de tinyconfig con FRAGMENTO_CAJA, para el paquete de arranque de DashOS."""
    kernel_pequeno()


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


# --- Arch Linux ARM (aarch64) en la caja ---
#
# El rootfs oficial (ArchLinuxARM-aarch64-latest.tar.gz) se baja de os.archlinuxarm.org y se comprueba con su MD5 y con
# su firma, hecha por la clave de compilación de Arch Linux ARM: la huella está en archlinuxarm.org/about/package-signing
# y la clave pública, en el propio rootfs (archlinuxarm-keyring), así que lo que cuenta es que la huella case.
# Con él se hace el disco del NVMe, como el de un DashOS instalado con su caja Linux: una GPT con dos particiones. La 1,
# la de DashOS (7 MB, del MB 1 al 8): `poner-dashos` copia en ella el RedoxFS del disco de las pruebas
# (`tools/disco_archivos.py`), para que el sistema tenga su servicio de archivos como siempre. La 2, la de la caja Linux
# (su tipo, `TIPO_CAJA_LINUX`, es el que busca el servicio de disco de DashOS, `dashos::particiones`), con un ext4
# hecho por mke2fs desde el tar (`mke2fs -d fichero.tar`, e2fsprogs 1.47.1 o más): conserva dueños, permisos y
# atributos sin ser root.
ARCH = CACHE / "arch"
ARCH_URL = "http://os.archlinuxarm.org/os/ArchLinuxARM-aarch64-latest.tar.gz"
ARCH_HUELLA = "68B3537F39A313B3E574D06777193F152BDBE6A6"
TIPO_CAJA_LINUX = "d0a18dbf-bbf4-4b0e-8627-ce5006b50018"
TIPO_DASHOS = "d0a18dbf-bbf4-4b0e-8627-ce5006b50017"
DISCO_ARCH = ARCH / "disco-arch.img"
TAM_DISCO_ARCH = 4 << 30
# La partición de DashOS: del MB 1 al 8, lo mismo que el RedoxFS del disco de las pruebas (`nvme.img`, 8 MB).
INICIO_DASHOS = 1 << 20
TAM_DASHOS = 7 << 20
INICIO_PARTICION = 8 << 20
# El final del disco: la copia de la GPT (33 sectores) y un MB de margen.
TAM_PARTICION_ARCH = TAM_DISCO_ARCH - INICIO_PARTICION - (1 << 20)

# Lo que se cambia en el rootfs (lo mínimo). Rutas como en el tar ("./etc/...").
ARCH_FICHEROS = {
    "./etc/hostname": "caja\n",
    # La raíz la monta el kernel (root=/dev/vda rw); sin fsck al arrancar.
    "./etc/fstab": "# La caja Linux de DashOS: la partición de la caja en el NVMe, por virtio-blk.\n"
                   "/dev/vda  /  ext4  rw,relatime  0 0\n",
    # La consola es hvc0 (virtio-console), que es la terminal de la caja en el escritorio de DashOS: allí entra root
    # solo, sin contraseña (la de root sigue siendo la de serie de Arch Linux ARM, "root", para las demás).
    "./etc/systemd/system/serial-getty@hvc0.service.d/autologin.conf":
        "[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin root --noreset --noclear --keep-baud "
        "115200,57600,38400,9600 - ${TERM}\n",
}
# Servicios apagados: timesyncd (iría a servidores de NTP de fuera; la hora la da el PL031 de la caja), el primer
# arranque interactivo (pediría idioma y contraseña por la consola) y sshd (genera sus claves al primer arranque: un
# minuto largo en QEMU sin aceleración, y nadie entra por SSH a la caja).
ARCH_MASCARAS = ["systemd-timesyncd.service", "systemd-firstboot.service", "sshdgenkeys.service", "sshd.service"]
ARCH_QUITAR = {"./etc/systemd/system/sysinit.target.wants/systemd-timesyncd.service",
               "./etc/systemd/system/multi-user.target.wants/sshd.service",
               "./etc/systemd/system/dbus-org.freedesktop.timesync1.service"}


def arch_bajar():
    ARCH.mkdir(parents=True, exist_ok=True)
    tar = ARCH / "ArchLinuxARM-aarch64-latest.tar.gz"
    for ext in (".md5", ".sig", ""):
        bajar(ARCH_URL + ext, ARCH / (tar.name + ext))
    md5 = hashlib.md5()
    with open(tar, "rb") as f:
        while b := f.read(1 << 20):
            md5.update(b)
    esperado = (ARCH / (tar.name + ".md5")).read_text().split()[0]
    if md5.hexdigest() != esperado:
        raise SystemExit(f"el MD5 del rootfs no casa ({md5.hexdigest()} y no {esperado}): bórralo y vuelve a bajarlo")
    # La firma, con la clave pública que trae el rootfs; vale si la huella es la que publica Arch Linux ARM.
    with tempfile.TemporaryDirectory() as gpg:
        with tarfile.open(tar) as t:
            clave = t.extractfile("./usr/share/pacman/keyrings/archlinuxarm.gpg").read()
        (Path(gpg) / "clave.gpg").write_bytes(clave)
        entorno = dict(os.environ, GNUPGHOME=gpg)
        subprocess.run(["gpg", "-q", "--import", Path(gpg) / "clave.gpg"], env=entorno, check=True,
                       capture_output=True)
        r = subprocess.run(["gpg", "--status-fd", "1", "--verify", str(tar) + ".sig", tar], env=entorno,
                           capture_output=True, text=True)
        if f"VALIDSIG {ARCH_HUELLA}" not in r.stdout:
            raise SystemExit("la firma del rootfs no es de la clave de Arch Linux ARM:\n" + r.stdout + r.stderr)
    print("rootfs de Arch Linux ARM: MD5 y firma bien (clave", ARCH_HUELLA + ")")
    return tar


def arch_tar(origen, destino):
    """El tar del rootfs con los cambios de ARCH_FICHEROS, ARCH_MASCARAS y ARCH_QUITAR; sin comprimir, para mke2fs."""
    hecho = set()
    mtime_usr = 0
    tmp = destino.with_suffix(".part")
    with tarfile.open(origen, "r:gz") as ent, tarfile.open(tmp, "w", format=tarfile.PAX_FORMAT) as sal:
        for m in ent:
            if m.name in ARCH_QUITAR:
                continue
            if m.name == "./usr":
                mtime_usr = m.mtime
            if m.name in ARCH_FICHEROS:
                datos = ARCH_FICHEROS[m.name].encode()
                m.size = len(datos)
                sal.addfile(m, io.BytesIO(datos))
                hecho.add(m.name)
                continue
            sal.addfile(m, ent.extractfile(m) if m.isreg() else None)
        ahora = int(time.time())

        def poner(nombre, tipo=tarfile.REGTYPE, datos=b"", enlace="", modo=0o644):
            i = tarfile.TarInfo(nombre)
            i.type, i.mode, i.mtime, i.uid, i.gid, i.uname, i.gname = tipo, modo, ahora, 0, 0, "root", "root"
            i.linkname, i.size = enlace, len(datos)
            sal.addfile(i, io.BytesIO(datos) if tipo == tarfile.REGTYPE else None)

        nuevos = [n for n in ARCH_FICHEROS if n not in hecho]
        carpetas = sorted({n.rsplit("/", 1)[0] for n in nuevos})
        for c in carpetas:
            if c not in ("./etc", "./etc/systemd", "./etc/systemd/system"):
                poner(c, tarfile.DIRTYPE, modo=0o755)
        for n in nuevos:
            poner(n, datos=ARCH_FICHEROS[n].encode())
        for s in ARCH_MASCARAS:
            poner(f"./etc/systemd/system/{s}", tarfile.SYMTYPE, enlace="/dev/null", modo=0o777)
        # /etc y /var al día respecto de /usr (ConditionNeedsUpdate): sin ldconfig, hwdb ni catálogo en el primer
        # arranque (el rootfs ya los trae hechos).
        marca = (f"TIMESTAMP_NSEC={mtime_usr * 1_000_000_000}\n").encode()
        poner("./etc/.updated", datos=marca)
        poner("./var/.updated", datos=marca)
    tmp.rename(destino)


def arch_disco(tar):
    raiz = ARCH / "raiz.tar"
    if not raiz.exists() or raiz.stat().st_mtime < tar.stat().st_mtime:
        print("haciendo", raiz, flush=True)
        arch_tar(tar, raiz)
    tmp = DISCO_ARCH.with_suffix(".part")
    if tmp.exists():
        tmp.unlink()
    with open(tmp, "wb") as f:
        f.truncate(TAM_DISCO_ARCH)
    primero = INICIO_PARTICION // 512
    ultimo = primero + TAM_PARTICION_ARCH // 512 - 1
    # El mke2fs del sistema (el del SDK de Android, si va antes en el PATH, no sabe leer un tar).
    mke2fs = next((m for m in ("/usr/bin/mke2fs", "/usr/sbin/mke2fs", "/sbin/mke2fs") if Path(m).exists()), "mke2fs")
    run(mke2fs, "-q", "-F", "-t", "ext4", "-L", "caja-arch", "-E", f"offset={INICIO_PARTICION},nodiscard", "-d", raiz, tmp,
        f"{TAM_PARTICION_ARCH // 1024}k")
    # La GPT, después: mke2fs con offset borra el principio del fichero (lo de otros sistemas de archivos).
    d0, d1 = INICIO_DASHOS // 512, (INICIO_DASHOS + TAM_DASHOS) // 512 - 1
    run("sgdisk", "-o", "-n", f"1:{d0}:{d1}", "-t", f"1:{TIPO_DASHOS}", "-c", "1:DashOS",
        "-n", f"2:{primero}:{ultimo}", "-t", f"2:{TIPO_CAJA_LINUX}", "-c", "2:caja Linux", tmp)
    tmp.rename(DISCO_ARCH)
    print("disco de Arch:", DISCO_ARCH, f"{TAM_DISCO_ARCH >> 30} GB: DashOS del MB 1 al 8, la caja Linux desde el 8")


def poner_dashos(args):
    """Copia el RedoxFS del disco de las pruebas (`nvme.img`: MBR y la partición de DashOS del MB 1 al 8) en la
    partición de DashOS del disco de Arch. Lo hace `make arranque-virt-arch` antes de cada prueba (QEMU usa el disco con
    snapshot=on: nada de lo que escriban DashOS o Arch se queda)."""
    nvme, disco = Path(args.nvme), Path(args.disco)
    datos = nvme.read_bytes()
    if len(datos) != INICIO_DASHOS + TAM_DASHOS or datos[510:512] != b"\x55\xaa":
        raise SystemExit(f"{nvme} no es el disco de las pruebas (8 MB con MBR)")
    with open(disco, "r+b") as f:
        f.seek(INICIO_DASHOS)
        f.write(datos[INICIO_DASHOS:])
    print(f"RedoxFS de {nvme} en la partición de DashOS de {disco}")


def preparar_arch(_args):
    """Image-arch (el kernel) y el disco de Arch Linux ARM, en ~/.cache/dashos-caja-linux."""
    CACHE.mkdir(parents=True, exist_ok=True)
    kernel_pequeno("Image-arch", FRAGMENTO_ARCH, "linux-build-arch")
    tar = arch_bajar()
    if DISCO_ARCH.exists() and DISCO_ARCH.stat().st_mtime > max(tar.stat().st_mtime, Path(__file__).stat().st_mtime):
        print("ya está", DISCO_ARCH)
    else:
        arch_disco(tar)


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


def probar_arch(args):
    """Image-arch con el disco de Arch en qemu-system-aarch64 -M virt, con el DTB y la carga del crate (como `probar`):
    la consola hvc0 por un socket, donde se espera el prompt de root y se teclean unas órdenes."""
    import socket
    for f in (CACHE / "Image-arch", DISCO_ARCH):
        if not f.exists():
            raise SystemExit(f"falta {f}: ejecuta antes 'preparar-arch'")
    run("cargo", "build", "--release", "--quiet", "--manifest-path", RAIZ / "caja-linux/vmm/Cargo.toml",
        "--bin", "caja-vmm")
    sal = CACHE / "prueba-arch"
    if sal.exists():
        shutil.rmtree(sal)
    sal.mkdir()
    base = 0x4000_0000 + (2 << 20)
    memoria = args.memoria * (1 << 20) - (2 << 20)
    cmd = [RAIZ / "caja-linux/vmm/target/release/caja-vmm", "preparar", "--kernel", CACHE / "Image-arch",
           "--memoria", f"{base:#x}:{memoria:#x}", "--cpus", str(args.cpus), "--gic", QEMU_VIRT["gic"],
           "--uart", QEMU_VIRT["uart"], "--rtc", "0x09010000:2", "--bootargs", args.bootargs, "--salida", sal]
    for v in QEMU_VIRT["virtio"]:
        cmd += ["--virtio", v]
    run(*cmd)
    plan = json.loads((sal / "plan.json").read_text())
    sock = sal / "hvc0.sock"
    qemu = ["qemu-system-aarch64", "-M", "virt,gic-version=2", "-cpu", "cortex-a72", "-smp", str(args.cpus),
            "-m", f"{args.memoria}M", "-display", "none", "-monitor", "none", "-no-reboot",
            "-global", "virtio-mmio.force-legacy=false", "-serial", f"file:{sal / 'pl011.log'}",
            "-device", "virtio-serial-device", "-chardev", f"socket,id=hvc,path={sock},server=on,wait=on",
            "-device", "virtconsole,chardev=hvc",
            # La partición de la caja, como disco entero (lo mismo que le da el servicio de disco de DashOS).
            "-drive", f"if=none,id=d0,format=raw,file={DISCO_ARCH},offset={INICIO_PARTICION},"
                      f"size={TAM_PARTICION_ARCH},snapshot=on",
            "-device", "virtio-blk-device,drive=d0",
            "-netdev", "user,id=n0", "-device", "virtio-net-device,netdev=n0,mac=02:da:50:00:00:01"]
    for trozo in plan["trozos"]:
        qemu += ["-device", f"loader,file={sal / trozo['fichero']},addr={trozo['direccion']:#x},force-raw=on"]
    qemu += ["-device", f"loader,addr={plan['pc']:#x},cpu-num=0"]
    print("+", " ".join(str(c) for c in qemu), flush=True)
    proc = subprocess.Popen([str(c) for c in qemu])
    for _ in range(200):
        if sock.exists():
            break
        time.sleep(0.05)
    s = socket.socket(socket.AF_UNIX)
    s.connect(str(sock))
    s.settimeout(1)
    visto = bytearray()
    log = open(sal / "hvc0.log", "wb")

    def esperar(texto, segundos):
        fin = time.time() + segundos
        while time.time() < fin:
            if texto.encode() in visto:
                return True
            try:
                d = s.recv(65536)
            except socket.timeout:
                continue
            if not d:
                break
            visto.extend(d)
            log.write(d)
            log.flush()
            sys.stdout.write(d.decode(errors="replace"))
            sys.stdout.flush()
        return texto.encode() in visto

    t0 = time.time()
    bien = esperar("]# ", args.tiempo)
    print(f"\n== prompt de root a los {time.time() - t0:.1f} s: {'sí' if bien else 'no'}", flush=True)
    if bien:
        for orden in args.ordenes or ["uname -a", "systemctl is-system-running --wait", "networkctl --no-pager",
                                    "ip -4 addr show", "date", "pacman -Sy --noconfirm", "echo FIN-ARCH"]:
            del visto[:]
            s.sendall(orden.encode() + b"\r")
            esperar("]# ", args.tiempo)
        s.sendall(b"poweroff\r")
    try:
        proc.wait(timeout=120)
    except subprocess.TimeoutExpired:
        proc.kill()
    if not bien:
        raise SystemExit("PRUEBA MAL: sin prompt de root")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="orden", required=True)
    sub.add_parser("preparar").set_defaults(f=preparar)
    sub.add_parser("preparar-arch").set_defaults(f=preparar_arch)
    pd = sub.add_parser("poner-dashos")
    pd.add_argument("nvme")
    pd.add_argument("disco", nargs="?", default=str(DISCO_ARCH))
    pd.set_defaults(f=poner_dashos)
    pa = sub.add_parser("probar-arch")
    pa.add_argument("--cpus", type=int, default=1)
    pa.add_argument("--memoria", type=int, default=256, help="MiB")
    pa.add_argument("--tiempo", type=int, default=300, help="segundos de espera para cada cosa")
    pa.add_argument("--bootargs", default="root=/dev/vda rw rootwait console=hvc0 earlycon panic=-1")
    pa.add_argument("--orden", dest="ordenes", action="append", help="órdenes para la shell (si no, unas de serie)")
    pa.set_defaults(f=probar_arch)
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
