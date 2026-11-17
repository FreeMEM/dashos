#!/usr/bin/env python3
"""El dominio Linux mínimo de x86-64 (docs/bitacoras/x86-dominio.md), lado invitado: su kernel, ntfsresize estático,
su /init y la prueba en QEMU. DashOS lo arrancará en su propio hipervisor para ejecutar ntfsresize sobre una partición
NTFS prestada.

Nada de lo que se baja o se compila va a git: se queda en $DASHOS_DOMINIO_X86 (por defecto
~/recuperar/geosian-7d/dominio-x86). El código de Linux es el de medir-linux ($DASHOS_MEDIR_LINUX/linux-<versión>, de
kernel.org, comprobado allí). Lo de fuera se baja de su sitio oficial y se comprueba su suma antes de usarlo:
ntfs-3g (Tuxera; la SHA-256 de su página de publicación) y musl (musl.libc.org; la de aquí, de la firma de su llave).

    tools/dominio_x86.py preparar          el kernel (tinyconfig + caja-linux/dominios/x86_ntfs.config): bzImage;
                                           musl y ntfsresize estático con ella; el /init
                                           (caja-linux/dominios/ntfs_dominio.c) y el initramfs: initramfs.cpio
    tools/dominio_x86.py qemu [kvm|tcg]    la prueba en qemu-system-x86_64 -M microvm (sin ACPI, PIC, PIT ni RTC) con
                                           una imagen NTFS de 2 GB hecha aquí: info, encoger a 1 GB (y se comprueba
                                           desde el PC con ntfsresize --info y ntfsfix -n) y el volumen sucio
                                           (se niega); tiempos hasta "dominio: fin" y memoria (128, 96 y 64 MB)
    tools/dominio_x86.py ver ORDEN [MB] [kvm|tcg]   un arranque con la salida entera (ORDEN: info o encoger:<bytes>)
    tools/dominio_x86.py minimo [kvm|tcg]  la menor memoria (MB) con la que "info" acaba bien
    tools/dominio_x86.py grande [kvm|tcg]  info y encoger en volúmenes de 256 GB y 1 TB (dispersos) con 128 MB
    ... [rtc]                              con un RTC en QEMU (rtc=on): sin él, el CMOS de microvm lee 0xff y el
                                           kernel espera 1 s; en DashOS lee 0 y no espera
    ... [contrato]                         sin el firmware de microvm: una ROM hecha aquí pone la tabla MP del contrato
                                           y entra por el arranque de 32 bits, como DashOS (ver firmware_contrato)
    tools/dominio_x86.py accesos [intel]   un arranque en TCG con las trazas de QEMU (puertos, MMIO, LAPIC, IOAPIC) y
                                           un complemento de TCG que apunta cada RDMSR/WRMSR: lo que el monitor de
                                           DashOS tendrá que atender, en accesos.txt

La línea del kernel es LINEA (abajo, con el porqué de cada parte); en QEMU cambia solo el aparato virtio-mmio, que
microvm pone en 0xfeb00e00 (512 bytes, IRQ 12) y no en 0xd0000000:5.
"""

import collections
import hashlib
import os
import re
import select
import statistics
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from medir_dominios import FUENTE, cpio_newc, leer_config, pedido, run  # noqa: E402

RAIZ = Path(__file__).resolve().parent.parent
DOMINIOS = RAIZ / "caja-linux" / "dominios"
TRABAJO = Path(os.environ.get("DASHOS_DOMINIO_X86", Path.home() / "recuperar" / "geosian-7d" / "dominio-x86"))
DESCARGA = TRABAJO / "descarga"
OBJ = TRABAJO / "obj"
BZIMAGE = TRABAJO / "bzImage"
INITRD = TRABAJO / "initramfs.cpio"
NUCLEOS = min(os.cpu_count() or 8, 8)  # la máquina es compartida

NTFS3G = ("ntfs-3g_ntfsprogs-2026.9.28.tgz", "https://download.tuxera.com/opensource/ntfs-3g_ntfsprogs-2026.9.28.tgz",
          "350d9415c59f7c3fa74e23985ad2d56423c6a168337368f672e2e362d8f295c8")
MUSL = ("musl-1.2.6.tar.gz", "https://musl.libc.org/releases/musl-1.2.6.tar.gz",
        "d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a")

# La línea del kernel en la máquina de DashOS. El porqué de cada parte:
#  - earlyprintk=serial,0x3f8,115200,keep: la consola es la temprana, escribiendo en el THR del 0x3F8 cuando el LSR
#    dice "listo" (sondeada, sin interrupción ni controlador 8250); "keep" la deja hasta el final.
#  - tsc_early_khz=<kHz>: la frecuencia del TSC, que no se puede medir (ni PIT ni HPET). Con ella el kernel no la
#    calibra; el retardo (lpj) sale de ella y no hace falta lpj=.
#  - tsc=reliable: el TSC es constante e invariante: que no lo vigile con otro reloj (no lo hay) ni mire cada rato
#    el MSR TSC_ADJUST (0x3B) por si alguien lo ha movido.
#  - virtio_mmio.device=4K@0xd0000000:5: el disco (virtio-blk), su página de registros y su IRQ (pin 5 del IOAPIC).
#  - printk.devkmsg=on: lo que escribe el /init en /dev/kmsg no se recorta (de serie, 10 líneas cada 5 s).
#  - dashos.ntfs=...: la orden para el /init (info o encoger:<bytes>); el kernel no la entiende y la deja estar.
# No hace falta nolapic_timer (el temporizador del LAPIC en plazo del TSC existe) ni no_timer_check (sin PIC no se
# comprueba el IRQ0 del PIT) ni lpj= (sale de tsc_early_khz). Sin "quiet": el registro entero sale por el 0x3F8 (unos
# 8,5 KB, una escritura de puerto por byte y una lectura del LSR); con quiet saldrían solo los errores, y el /init
# escribe con nivel 5 (notice), que quiet también calla.
LINEA = ("earlyprintk=serial,0x3f8,115200,keep tsc_early_khz={khz} tsc=reliable printk.devkmsg=on "
         "virtio_mmio.device=4K@0xd0000000:5 dashos.ntfs={orden}")
# En QEMU microvm el aparato virtio-mmio está en otro sitio (ver orden_qemu).
MICROVM_VIRTIO = "virtio_mmio.device=512@0x{base:x}:{irq}"


# ---- preparar ----

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


def desempaquetar(tar, nombre):
    destino = OBJ / "fuente" / nombre
    if not destino.exists():
        (OBJ / "fuente").mkdir(parents=True, exist_ok=True)
        run("tar", "-xf", tar, "-C", OBJ / "fuente")
    return destino


def kernel():
    salida = OBJ / "kernel"
    salida.mkdir(parents=True, exist_ok=True)
    mk = ["make", "-C", FUENTE, f"O={salida}", "ARCH=x86_64", f"-j{NUCLEOS}"]
    run(*mk, "tinyconfig", stdout=subprocess.DEVNULL)
    texto = (DOMINIOS / "x86_ntfs.config").read_text()
    (salida / "dominio.config").write_text(texto)
    run(FUENTE / "scripts/kconfig/merge_config.sh", "-m", "-O", salida, salida / ".config", salida / "dominio.config",
        cwd=FUENTE, env={**os.environ, "ARCH": "x86_64"}, stdout=subprocess.DEVNULL)
    run(*mk, "olddefconfig", stdout=subprocess.DEVNULL)
    hay = leer_config(salida / ".config")
    malas = [f"{k}={v} (queda {hay.get(k, 'n')})" for k, v in pedido(texto).items()
             if hay.get(k, "n") != v and not (v == "n" and k not in hay)]
    if malas:
        print("AVISO, opciones que no quedaron como se pidió:", *malas, sep="\n  ", flush=True)
    run(*mk, "bzImage", stdout=subprocess.DEVNULL)
    BZIMAGE.write_bytes((salida / "arch/x86/boot/bzImage").read_bytes())


def musl():
    """musl estática, para x86-64 de base (la glibc de este PC es x86-64-v4: un estático con ella pide AVX-512)."""
    prefijo = OBJ / "musl"
    if (prefijo / "bin" / "musl-gcc").exists():
        return prefijo
    fuente = desempaquetar(bajar(*MUSL), "musl-1.2.6")
    build = OBJ / "musl-build"
    build.mkdir(parents=True, exist_ok=True)
    run(fuente / "configure", f"--prefix={prefijo}", "--disable-shared", "CFLAGS=-Os -march=x86-64 -mtune=generic",
        cwd=build, stdout=subprocess.DEVNULL)
    run("make", f"-j{NUCLEOS}", cwd=build, stdout=subprocess.DEVNULL)
    run("make", "install", cwd=build, stdout=subprocess.DEVNULL)
    # Las cabeceras del kernel (linux/hdreg.h, linux/fs.h), del mismo Linux, para compilar ntfs-3g con musl.
    run("make", "-C", FUENTE, f"O={OBJ / 'cabeceras'}", "ARCH=x86_64", "headers_install",
        f"INSTALL_HDR_PATH={prefijo}", stdout=subprocess.DEVNULL)
    return prefijo


def ntfsresize():
    """Solo ntfsresize, de ntfs-3g, enlazado estático con musl."""
    prefijo = musl()
    fuente = desempaquetar(bajar(*NTFS3G), "ntfs-3g-2026.9.28")
    build = OBJ / "ntfs-3g"
    binario = build / "ntfsprogs" / "ntfsresize"
    if not binario.exists():
        build.mkdir(parents=True, exist_ok=True)
        run(fuente / "configure", f"CC={prefijo / 'bin' / 'musl-gcc'} -static",
            "CFLAGS=-Os -march=x86-64 -mtune=generic", "--enable-really-static", "--disable-shared", "--enable-static",
            "--disable-ntfs-3g", "--disable-plugins", "--without-uuid", "--without-hd", "--disable-crypto",
            cwd=build, stdout=subprocess.DEVNULL)
        run("make", f"-j{NUCLEOS}", "-C", "libntfs-3g", cwd=build, stdout=subprocess.DEVNULL)
        # -all-static: libtool se come -static.
        run("make", "-C", "ntfsprogs", "ntfsresize", "LDFLAGS=-all-static", cwd=build, stdout=subprocess.DEVNULL)
    limpio = OBJ / "ntfsresize"
    run("strip", "-o", limpio, binario)
    return limpio


def init():
    binario = OBJ / "ntfs_dominio"
    run("gcc", "-Os", "-static", "-nostdlib", "-ffreestanding", "-fno-stack-protector", "-fno-builtin", "-Wall",
        "-fno-pie", "-no-pie", "-Wl,--gc-sections", "-s", "-o", binario, DOMINIOS / "ntfs_dominio.c")
    return binario


def preparar():
    OBJ.mkdir(parents=True, exist_ok=True)
    kernel()
    nr, ini = ntfsresize(), init()
    entradas = [(d, 0o040755, b"", (0, 0)) for d in ("bin", "dev", "proc")]
    entradas.append(("init", 0o100755, ini.read_bytes(), (0, 0)))
    entradas.append(("bin/ntfsresize", 0o100755, nr.read_bytes(), (0, 0)))
    INITRD.write_bytes(cpio_newc(entradas))
    print(f"kernel: {BZIMAGE} ({BZIMAGE.stat().st_size} bytes); initramfs: {INITRD} ({INITRD.stat().st_size} bytes; "
          f"/init {ini.stat().st_size}, ntfsresize {nr.stat().st_size})", flush=True)


# ---- la prueba en QEMU ----

IMAGEN = OBJ / "prueba.ntfs"
GB = 1 << 30


def imagen_nueva(ruta=IMAGEN, tam=2 * GB):
    """Un NTFS de `tam` bytes, entero (sin tabla), con algunos ficheros dentro (unos 300 MB). Se hace una vez (la
    plantilla) y cada prueba parte de una copia."""
    plantilla = OBJ / f"plantilla-{tam >> 20}M.ntfs"
    if not plantilla.exists():
        nueva = plantilla.with_suffix(".nueva")
        nueva.unlink(missing_ok=True)
        with open(nueva, "wb") as f:
            f.truncate(tam)
        run("mkntfs", "-F", "-f", "-q", "-L", "prueba", nueva)
        datos = OBJ / "datos"
        datos.mkdir(exist_ok=True)
        for i, mb in enumerate((1, 10, 50, 100, 150)):
            d = datos / f"fichero{i}.bin"
            if not d.exists() or d.stat().st_size != mb << 20:
                d.write_bytes(os.urandom(mb << 20))
            run("ntfscp", "-q", nueva, d, f"fichero{i}.bin", stdout=subprocess.DEVNULL)
        nueva.rename(plantilla)
    ruta.unlink(missing_ok=True)
    subprocess.run(["cp", "--sparse=always", "--reflink=auto", str(plantilla), str(ruta)], check=True)


def ensuciar(ruta=IMAGEN):
    """Pone a 1 el bit "sucio" (VOLUME_IS_DIRTY, 0x0001) de las banderas de $Volume ($VOLUME_INFORMATION, registro 3
    de la MFT), en la MFT y en su espejo ($MFTMirr), como lo deja Windows si no se apaga bien: ntfsresize debe negarse
    sin --force. Se busca a mano (en un mkntfs, la MFT y el espejo empiezan contiguos)."""
    with open(ruta, "r+b") as f:
        arranque = f.read(512)
        bps = int.from_bytes(arranque[0x0b:0x0d], "little")
        cluster = bps * arranque[0x0d]
        c = int.from_bytes(arranque[0x40:0x41], "little", signed=True)
        tam_reg = cluster * c if c > 0 else 1 << -c
        for campo in (0x30, 0x38):  # $MFT y $MFTMirr
            pos = int.from_bytes(arranque[campo:campo + 8], "little") * cluster + 3 * tam_reg
            f.seek(pos)
            reg = bytearray(f.read(tam_reg))
            assert reg[:4] == b"FILE", "el registro 3 no es un FILE"
            # Arreglos de secuencia: el último par de bytes de cada sector se guarda aparte; se ponen en su sitio, se
            # cambia la bandera y se vuelven a quitar.
            usa_off, usa_n = int.from_bytes(reg[4:6], "little"), int.from_bytes(reg[6:8], "little")
            usn = bytes(reg[usa_off:usa_off + 2])
            for i in range(1, usa_n):
                fin = i * bps - 2
                assert reg[fin:fin + 2] == usn
                reg[fin:fin + 2] = reg[usa_off + 2 * i:usa_off + 2 * i + 2]
            a = int.from_bytes(reg[0x14:0x16], "little")
            while True:
                tipo = int.from_bytes(reg[a:a + 4], "little")
                assert tipo != 0xFFFFFFFF, "no está $VOLUME_INFORMATION"
                if tipo == 0x70:
                    banderas = a + int.from_bytes(reg[a + 0x14:a + 0x16], "little") + 0x0a
                    v = int.from_bytes(reg[banderas:banderas + 2], "little") | 0x0001
                    reg[banderas:banderas + 2] = v.to_bytes(2, "little")
                    break
                a += int.from_bytes(reg[a + 4:a + 8], "little")
            for i in range(1, usa_n):
                fin = i * bps - 2
                reg[usa_off + 2 * i:usa_off + 2 * i + 2] = reg[fin:fin + 2]
                reg[fin:fin + 2] = usn
            f.seek(pos)
            f.write(reg)


def khz_del_pc():
    """La frecuencia del TSC de este PC en kHz, como la dará DashOS: $DASHOS_TSC_KHZ, o la del registro del kernel
    del PC ("Refined TSC"), o la nominal de ACPI CPPC (en AMD, la del TSC). En QEMU con KVM el TSC del invitado va a la
    del PC; en TCG, QEMU le da otra (y da igual: es la prueba de que funciona sin calibrar)."""
    if os.environ.get("DASHOS_TSC_KHZ"):
        return int(os.environ["DASHOS_TSC_KHZ"])
    try:
        s = subprocess.run(["journalctl", "-k", "-b", "-o", "cat", "--no-pager", "-g", "Refined TSC"],
                           capture_output=True, text=True, timeout=10).stdout
        m = re.search(r"Refined TSC clocksource calibration: (\d+)\.(\d+) MHz", s)
        if m:
            return int(m.group(1)) * 1000 + int(m.group(2))
    except (OSError, subprocess.SubprocessError):
        pass
    try:
        return int(Path("/sys/devices/system/cpu/cpu0/acpi_cppc/nominal_freq").read_text()) * 1000
    except (OSError, ValueError):
        return 2_000_000


def orden_qemu(orden, mb=128, acel="kvm", disco=IMAGEN, extra=()):
    linea = LINEA.format(khz=khz_del_pc(), orden=orden)
    # El aparato virtio-mmio de microvm (lo que hay en DashOS en 0xd0000000:5): sin ioapic2, microvm pone 8 huecos
    # (0xfeb00000 + 512 * i, IRQ 5 + i) y el primer aparato va al último (lo que pone microvm con
    # auto-kernel-cmdline=on: 512@0xfeb00e00:12).
    linea = linea.replace("virtio_mmio.device=4K@0xd0000000:5", MICROVM_VIRTIO.format(base=0xFEB00E00, irq=12))
    cpu = ["-accel", "kvm", "-cpu", "host,-x2apic"] if acel == "kvm" else ["-accel", "tcg", "-cpu", CPU_TCG]
    if FIRMWARE == "contrato":
        arranque = firmware_contrato(linea, mb)
    else:
        arranque = ["-kernel", str(BZIMAGE), "-initrd", str(INITRD), "-append", linea]
    return ["qemu-system-x86_64",
            "-M", f"microvm,acpi=off,pic=off,pit=off,rtc={RTC},isa-serial=on,ioapic2=off,auto-kernel-cmdline=off",
            *cpu, "-smp", "1", "-m", f"{mb}M", "-nodefaults", "-no-user-config", "-nographic", "-serial", "stdio",
            "-monitor", "none", "-no-reboot",
            "-global", "virtio-mmio.force-legacy=false",
            "-drive", f"id=d0,file={disco},format=raw,if=none,cache=unsafe",
            "-device", "virtio-blk-device,drive=d0", *arranque, *extra]


# ---- el arranque del contrato, sin el firmware de microvm ----
#
# Con "contrato" en la orden, QEMU no arranca con su firmware (qboot, que hace de BIOS: entra por el código de 16 bits
# del kernel, pone su tabla MP en la EBDA y su e820). En su lugar, una ROM de 64 KB hecha aquí (en 0xF0000) hace lo
# que hará el monitor de DashOS: la tabla MP del contrato en 0xF0000 (1 CPU, bus ISA, IOAPIC 1 en 0xFEC00000, ISA 0-15
# a los pines 0-15) y, desde el vector de reinicio, a modo protegido plano y a code32_start con ESI = boot_params. La
# parte protegida de la bzImage, la zero page (con la e820 del contrato), la línea y el initrd los pone QEMU en la RAM
# antes de arrancar (-device loader).

ZP, LINEA_DIR = 0x10000, 0x20000
ARRANQUE_S = r"""
    .code16
    .text
    .org 0x1000
inicio16:
    cli
    cld
    lgdtl %cs:gdt_desc
    movl %cr0, %eax
    orl $1, %eax
    movl %eax, %cr0
    ljmpl $0x10, $(0xF0000 + inicio32)
    .code32
inicio32:
    movw $0x18, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss
    movl $ZP, %esi
    xorl %ebp, %ebp
    xorl %edi, %edi
    xorl %ebx, %ebx
    movl $0x100000, %eax
    jmp *%eax
    .p2align 3
gdt:
    .quad 0
    .quad 0
    .quad 0x00cf9a000000ffff
    .quad 0x00cf92000000ffff
gdt_desc:
    .word gdt_desc - gdt - 1
    .long 0xF0000 + gdt
    .org 0xFFF0
    .code16
    ljmp $0xF000, $inicio16
    .org 0x10000
"""


def tabla_mp():
    """La tabla MP (MP 1.4) del contrato, para 0xF0000: el puntero flotante y, detrás, la tabla."""
    import struct
    entradas = [struct.pack("<BBBBII8x", 0, 0, 0x14, 0x03, 0, 0),           # CPU: LAPIC 0, activa y de arranque
                struct.pack("<BB6s", 1, 0, b"ISA   "),                        # bus 0: ISA
                struct.pack("<BBBBI", 2, 1, 0x11, 1, 0xFEC00000)]            # IOAPIC 1
    entradas += [struct.pack("<BBHBBBB", 3, 0, 0, 0, irq, 1, irq) for irq in range(16)]  # ISA irq -> pin irq
    cuerpo = b"".join(entradas)
    cab = bytearray(struct.pack("<4sHBB8s12sIHHIHBB", b"PCMP", 44 + len(cuerpo), 4, 0, b"DASHOS  ", b"DOMINIO-X86 ",
                                0, 0, len(entradas), 0xFEE00000, 0, 0, 0))
    tabla = cab + cuerpo
    tabla[7] = -sum(tabla) & 0xFF
    fp = bytearray(struct.pack("<4sIBBBB4x", b"_MP_", 0xF0010, 1, 4, 0, 0))
    fp[10] = -sum(fp) & 0xFF
    return bytes(fp + tabla)


def firmware_contrato(linea, mb):
    import struct
    d = OBJ / "contrato"
    d.mkdir(parents=True, exist_ok=True)
    rom = d / "rom.bin"
    if not rom.exists():
        (d / "arranque.S").write_text(ARRANQUE_S)
        run("as", "--32", "--defsym", f"ZP={ZP:#x}", "-o", d / "arranque.o", d / "arranque.S")
        run("ld", "-m", "elf_i386", "-Ttext=0", "-e", "0", "-o", d / "arranque.elf", d / "arranque.o")
        run("objcopy", "-O", "binary", "-j", ".text", d / "arranque.elf", d / "arranque.bin")
        imagen = bytearray((d / "arranque.bin").read_bytes())
        assert len(imagen) == 0x10000
        mp = tabla_mp()
        assert len(mp) < 0x1000
        imagen[:len(mp)] = mp
        rom.write_bytes(imagen)
    bz = BZIMAGE.read_bytes()
    assert bz[0x202:0x206] == b"HdrS" and struct.unpack_from("<H", bz, 0x206)[0] >= 0x20C
    sectores = bz[0x1F1] or 4
    fin_cab = 0x202 + bz[0x201]
    zp = bytearray(4096)
    zp[0x1F1:fin_cab] = bz[0x1F1:fin_cab]
    zp[0x210] = 0xFF                                       # type_of_loader: sin identificar
    initrd = INITRD.read_bytes()
    fin_ram = mb << 20
    tope = min(fin_ram, struct.unpack_from("<I", zp, 0x22C)[0] + 1)
    dir_initrd = (tope - len(initrd)) & ~0xFFF
    struct.pack_into("<II", zp, 0x218, dir_initrd, len(initrd))   # ramdisk_image, ramdisk_size
    struct.pack_into("<I", zp, 0x228, LINEA_DIR)                  # cmd_line_ptr
    e820 = [(0, 0x9FC00, 1), (0x9FC00, 0x100000 - 0x9FC00, 2), (0x100000, fin_ram - 0x100000, 1)]
    zp[0x1E8] = len(e820)
    for i, (a, t, tipo) in enumerate(e820):
        struct.pack_into("<QQI", zp, 0x2D0 + 20 * i, a, t, tipo)
    (d / "zp.bin").write_bytes(zp)
    (d / "linea.bin").write_bytes(linea.encode() + b"\0")
    (d / "kernel.bin").write_bytes(bz[(sectores + 1) * 512:])
    carga = [(d / "zp.bin", ZP), (d / "linea.bin", LINEA_DIR), (d / "kernel.bin", 0x100000), (INITRD, dir_initrd)]
    orden = ["-bios", str(rom)]
    for f, dir_ in carga:
        orden += ["-device", f"loader,file={f},addr={dir_:#x},force-raw=on"]
    return orden


MALO = re.compile(r"Kernel panic|Out of memory|oom-kill|invoked oom-killer|Oops|BUG:|general protection")
# El CMOS de microvm con rtc=off lee 0xff: el kernel ve "actualizando" (UIP) y espera 1 s antes de rendirse. En
# DashOS lee 0 y no espera; con "rtc" en la orden, QEMU pone un RTC de verdad, que tampoco hace esperar.
RTC = "off"
FIRMWARE = "qboot"
# La CPU en TCG; con "intel" en la orden, la misma con el fabricante de Intel (el kernel toma otro camino y toca otros
# MSR que con la de AMD, que es la de serie de "max").
CPU_TCG = "max,-x2apic"


def arrancar(orden, mb=128, acel="kvm", espera=120, ver=False, disco=IMAGEN, extra=()):
    o = [str(x) for x in orden_qemu(orden, mb, acel, disco, extra)]
    t0 = time.monotonic()
    p = subprocess.Popen(o, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    texto, t_fin = b"", None
    try:
        while time.monotonic() - t0 < espera:
            r, _, _ = select.select([p.stdout], [], [], 0.05)
            if r:
                trozo = os.read(p.stdout.fileno(), 65536)
                if not trozo:
                    break
                texto += trozo
                s = texto.decode(errors="replace")
                if t_fin is None and re.search(r"dominio: fin \w+ \d+", s):
                    t_fin = time.monotonic() - t0
                # Tras el fin, el kernel apaga: sin ACPI se para ("System halted"). Se espera a eso o a un pánico.
                if (t_fin and re.search(r"System halted|Power down|reboot: ", s)) or MALO.search(s):
                    time.sleep(0.3)
                    if select.select([p.stdout], [], [], 0)[0]:
                        texto += os.read(p.stdout.fileno(), 65536)
                    break
    finally:
        if p.poll() is None:
            p.kill()  # nuestro propio hijo, por su PID
        p.wait()
    s = texto.decode(errors="replace")
    if ver:
        print(s)
    m = re.search(r"\[\s*([\d.]+)\] dominio: fin (\w+) (\d+)", s)
    r = dict(orden=orden, mb=mb, acel=acel, ok=bool(m) and not MALO.search(s), fuera_s=t_fin,
             paso=m and m.group(2), codigo=m and int(m.group(3)), reloj_s=m and float(m.group(1)),
             apagado="System halted" in s or "Power down" in s)
    mm = re.search(r"dominio: memoria ntfsresize_max_kb=(\d+) total_kb=(\d+) libre_kb=(\d+)", s)
    if mm:
        r.update(max_kb=int(mm.group(1)), total_kb=int(mm.group(2)), libre_kb=int(mm.group(3)))
    r["lineas"] = [x.split("dominio: ", 1)[1].rstrip("\r") for x in s.splitlines() if "dominio: " in x]
    if not r["ok"]:
        r["cola"] = s[-2500:]
    return r


def del_pc(*args):
    """Una herramienta de ntfs-3g del PC sobre la imagen; devuelve (código, salida)."""
    p = subprocess.run([str(a) for a in args], capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


def tam_volumen(salida):
    m = re.search(r"Current volume size: (\d+) bytes", salida)
    return int(m.group(1)) if m else None


def imprimir(r, todo=False):
    print(f"  {r['orden']} {r['mb']} MB {r['acel']}: {'bien' if r['ok'] else 'MAL'} paso={r['paso']} "
          f"código={r['codigo']} fin a {r['fuera_s'] and round(r['fuera_s'], 2)} s desde fuera, "
          f"{r['reloj_s']} s de reloj del dominio, ntfsresize_max={r.get('max_kb')} KB, total={r.get('total_kb')} KB, "
          f"apagado={'sí' if r['apagado'] else 'no'}", flush=True)
    if todo:
        for x in r["lineas"]:
            print("     |", x)
    if not r["ok"]:
        print(r.get("cola", ""))


def qemu(acel="kvm"):
    print("== info", flush=True)
    imagen_nueva()
    r = arrancar("info", acel=acel)
    imprimir(r, todo=True)

    print("== encoger a 1 GB", flush=True)
    r = arrancar(f"encoger:{GB}", acel=acel)
    imprimir(r, todo=True)
    # ntfsresize deja el volumen marcado para chkdsk: en el PC, --info necesita -f (solo lee).
    c, s = del_pc("ntfsresize", "--info", "--no-progress-bar", "-f", IMAGEN)
    print(f"  en el PC, ntfsresize -f --info: volumen de {tam_volumen(s)} bytes (código {c})")
    # La copia de seguridad del sector de arranque queda en el último sector del tamaño pedido: ntfsfix -n se mira
    # con la partición ya encogida a ese tamaño (lo que hará DashOS), en una copia.
    corta = OBJ / "encogida.ntfs"
    subprocess.run(["cp", "--sparse=always", "--reflink=auto", str(IMAGEN), str(corta)], check=True)
    os.truncate(corta, GB)
    c, s = del_pc("ntfsfix", "-n", corta)
    print(f"  en el PC, ntfsfix -n con la partición encogida a {GB} bytes: código {c}\n"
          + "".join(f"     | {x}\n" for x in s.splitlines()), end="")
    c, s = del_pc("ntfsfix", "-n", IMAGEN)
    print(f"  (sin encoger la partición, ntfsfix -n: código {c}; busca la copia del arranque al final del aparato)")
    corta.unlink()

    print("== otra vez info tras encoger, sin pasar por Windows", flush=True)
    r = arrancar("info", acel=acel)
    imprimir(r, todo=True)

    print("== sucio (bandera de $Volume)", flush=True)
    imagen_nueva()
    ensuciar()
    for orden in ("info", f"encoger:{GB}"):
        r = arrancar(orden, acel=acel)
        imprimir(r, todo=True)
    c, s = del_pc("ntfsresize", "--info", "--no-progress-bar", "-f", IMAGEN)
    print(f"  en el PC, el volumen sigue de {tam_volumen(s)} bytes")
    c, _ = del_pc("ntfsfix", "-d", IMAGEN)
    r = arrancar("info", acel=acel)
    print(f"  tras ntfsfix -d (código {c}) en el PC: info en el dominio, paso={r['paso']} código={r['codigo']}")

    print("== tiempos y memoria (info y encoger, 3 veces cada uno)", flush=True)
    for mb in (128, 96, 64):
        for orden in ("info", f"encoger:{GB}"):
            rs = []
            for _ in range(3):
                imagen_nueva()
                rs.append(arrancar(orden, mb=mb, acel=acel))
            buenos = [x for x in rs if x["ok"] and x["codigo"] == 0]
            for x in rs:
                if not (x["ok"] and x["codigo"] == 0):
                    imprimir(x)
            if buenos:
                print(f"  {orden} {mb} MB {acel}: {len(buenos)}/3 bien, fin a "
                      f"{statistics.median(x['fuera_s'] for x in buenos):.2f} s desde fuera (mediana), "
                      f"{statistics.median(x['reloj_s'] for x in buenos):.2f} s de reloj del dominio, "
                      f"ntfsresize hasta {max(x['max_kb'] for x in buenos)} KB, "
                      f"el dominio ve {buenos[0]['total_kb']} KB y le quedan {min(x['libre_kb'] for x in buenos)} "
                      f"KB libres", flush=True)


def grande(acel="kvm"):
    """Volúmenes grandes (imágenes dispersas, casi vacías): info y encoger a la mitad con 128 MB. ntfsresize guarda
    un bit por cluster (nr_clusters / 8 bytes); en un volumen vacío casi no se toca, en uno lleno sí."""
    img = OBJ / "grande.ntfs"
    for gb in (256, 1024):
        imagen_nueva(img, gb * GB)
        for orden in ("info", f"encoger:{gb * GB // 2}"):
            r = arrancar(orden, acel=acel, disco=img, espera=300)
            imprimir(r)
        (OBJ / f"plantilla-{gb << 10}M.ntfs").unlink()
    img.unlink()


def minimo(acel="kvm"):
    imagen_nueva()
    bajo, alto = 8, 128
    if not arrancar("info", alto, acel)["ok"]:
        raise SystemExit(f"no acaba ni con {alto} MB")
    while alto - bajo > 1:
        medio = (alto + bajo) // 2
        r = arrancar("info", medio, acel, espera=60)
        ok = r["ok"] and r["codigo"] == 0
        print(f"  {medio} MB: {'bien' if ok else 'mal'}", flush=True)
        alto, bajo = (medio, bajo) if ok else (alto, medio)
    print(f"mínimo en QEMU ({acel}) para info sobre 2 GB: {alto} MB", flush=True)


# ---- lo que toca el kernel: puertos, MMIO, LAPIC, IOAPIC y MSR ----

PLUGIN_C = r"""
/* Complemento de TCG (API 7, QEMU 11) para tools/dominio_x86.py accesos: apunta cada RDMSR y WRMSR que se
 * ejecuta (ECX, y EDX:EAX al escribir) en el fichero que se le da (fichero=...). */
#include <qemu-plugin.h>
#include <stdio.h>
#include <string.h>
#include <glib.h>
QEMU_PLUGIN_EXPORT int qemu_plugin_version = QEMU_PLUGIN_VERSION;
static FILE *fsal;
static struct qemu_plugin_register *rax, *rcx, *rdx;
static void al_ejecutar(unsigned int cpu, void *ud)
{
    GByteArray *b = g_byte_array_new();
    unsigned long ecx = 0, eax = 0, edx = 0;
    qemu_plugin_read_register(rcx, b); memcpy(&ecx, b->data, 4); g_byte_array_set_size(b, 0);
    if (ud) {
        qemu_plugin_read_register(rax, b); memcpy(&eax, b->data, 4); g_byte_array_set_size(b, 0);
        qemu_plugin_read_register(rdx, b); memcpy(&edx, b->data, 4);
        fprintf(fsal, "wrmsr %#lx %#lx\n", ecx, (edx << 32) | eax);
    } else {
        fprintf(fsal, "rdmsr %#lx\n", ecx);
    }
    g_byte_array_free(b, TRUE);
}
static void al_iniciar_cpu(unsigned int cpu, void *ud)
{
    GArray *regs = qemu_plugin_get_registers();
    for (guint i = 0; i < regs->len; i++) {
        qemu_plugin_reg_descriptor *d = &g_array_index(regs, qemu_plugin_reg_descriptor, i);
        if (!strcmp(d->name, "rax")) rax = d->handle;
        if (!strcmp(d->name, "rcx")) rcx = d->handle;
        if (!strcmp(d->name, "rdx")) rdx = d->handle;
    }
    g_array_free(regs, TRUE);
}
static void al_traducir(struct qemu_plugin_tb *tb, void *ud)
{
    size_t n = qemu_plugin_tb_n_insns(tb);
    for (size_t i = 0; i < n; i++) {
        struct qemu_plugin_insn *in = qemu_plugin_tb_get_insn(tb, i);
        unsigned char op[2];
        if (qemu_plugin_insn_size(in) != 2 || qemu_plugin_insn_data(in, op, 2) != 2 || op[0] != 0x0f) continue;
        if (op[1] == 0x30 || op[1] == 0x32)
            qemu_plugin_register_vcpu_insn_exec_cb(in, al_ejecutar, QEMU_PLUGIN_CB_R_REGS,
                                                   op[1] == 0x30 ? (void *)1 : NULL);
    }
}
QEMU_PLUGIN_EXPORT int qemu_plugin_install(qemu_plugin_id_t id, const qemu_info_t *info, int argc, char **argv)
{
    const char *f = "msr.txt";
    for (int i = 0; i < argc; i++) if (!strncmp(argv[i], "fichero=", 8)) f = argv[i] + 8;
    fsal = fopen(f, "w");
    setvbuf(fsal, NULL, _IOLBF, 0);
    qemu_plugin_register_vcpu_init_cb(id, al_iniciar_cpu, NULL);
    qemu_plugin_register_vcpu_tb_trans_cb(id, al_traducir, NULL);
    return 0;
}
"""


def accesos():
    """Un arranque de "info" en TCG con trazas. QEMU apunta los accesos a puertos (cpu_in/cpu_out), a MMIO
    (memory_region_ops_*: el del LAPIC, el IOAPIC y virtio) y el complemento, cada RDMSR/WRMSR. Se cuentan por
    dirección (y por registro dentro del LAPIC y el IOAPIC) y se separa lo del firmware de microvm (qboot) de lo del
    kernel por la primera línea del kernel."""
    carpeta = OBJ / "accesos"
    carpeta.mkdir(parents=True, exist_ok=True)
    c = carpeta / "msr_plugin.c"
    c.write_text(PLUGIN_C)
    so = carpeta / "msr_plugin.so"
    glib = subprocess.run(["pkg-config", "--cflags", "glib-2.0"], capture_output=True, text=True,
                          check=True).stdout.split()
    run("gcc", "-shared", "-fPIC", "-O2", *glib, "-o", so, c)
    sufijo = "-intel" if "Intel" in CPU_TCG else ""
    traza, msr = carpeta / f"traza{sufijo}.txt", carpeta / f"msr{sufijo}.txt"
    imagen_nueva()
    extra = ["-plugin", f"{so},fichero={msr}", "-d", "trace:cpu_in,trace:cpu_out,trace:memory_region_ops_read,"
             "trace:memory_region_ops_write,trace:ioapic_mem_write,trace:ioapic_mem_read", "-D", str(traza)]
    r = arrancar("info", acel="tcg", espera=300, extra=extra)
    imprimir(r)
    return resumir_accesos(traza, msr, carpeta / f"accesos{sufijo}.txt")


def resumir_accesos(traza, msr, destino):
    puertos, mmio, regs_msr = collections.Counter(), collections.Counter(), collections.Counter()
    ejemplos = {}
    for linea in traza.read_text(errors="replace").splitlines():
        m = re.search(r"cpu_(in|out) addr 0x([0-9a-f]+)(?: value 0x([0-9a-f]+))?(?: size (\d+))?", linea)
        if m:
            clave = (m.group(1), int(m.group(2), 16))
            puertos[clave] += 1
            if m.group(3):
                ejemplos.setdefault(clave, set()).add(int(m.group(3), 16)) if len(
                    ejemplos.get(clave, ())) < 8 else None
            continue
        m = re.search(r"memory_region_ops_(read|write) cpu \d+ mr \S+ addr 0x([0-9a-f]+) value 0x([0-9a-f]+) "
                      r"size (\d+) name '([^']*)'", linea)
        if m:
            nombre = m.group(5)
            if nombre in ("", "ram") or nombre.startswith("pc."):
                continue
            if nombre == "io":  # un puerto sin nada detrás (PIC, PIT, CMOS...): lee 0xff
                nombre = "puerto sin aparato"
            dir_ = int(m.group(2), 16)
            clave = (m.group(1), nombre, dir_)
            mmio[clave] += 1
            if len(ejemplos.get(clave, ())) < 6:
                ejemplos.setdefault(clave, set()).add(int(m.group(3), 16))
    for linea in msr.read_text().splitlines():
        p = linea.split()
        regs_msr[(p[0], int(p[1], 16))] += 1
        if p[0] == "wrmsr" and len(ejemplos.get((p[0], int(p[1], 16)), ())) < 6:
            ejemplos.setdefault((p[0], int(p[1], 16)), set()).add(int(p[2], 16))
    origen = ("con la ROM del contrato (sin firmware)" if FIRMWARE == "contrato"
              else "incluye el firmware qboot de microvm")
    out = [f"# Accesos en un arranque de info (TCG, -cpu {CPU_TCG}; {origen})", "",
           "## Puertos (dirección, veces, valores escritos)"]
    for (d, p), n in sorted(puertos.items(), key=lambda x: (x[0][1], x[0][0])):
        v = ejemplos.get((d, p))
        out.append(f"{d:3} {p:#06x} {n:7}" + (f"  {', '.join(hex(x) for x in sorted(v))}" if v else ""))
    out += ["", "## MMIO fuera de la RAM (región, desplazamiento, veces, valores)"]
    for (d, nombre, a), n in sorted(mmio.items(), key=lambda x: (x[0][1], x[0][2], x[0][0])):
        v = ejemplos.get((d, nombre, a))
        out.append(f"{d:5} {nombre:24} {a:#07x} {n:7}" + (f"  {', '.join(hex(x) for x in sorted(v))}" if v else ""))
    out += ["", "## MSR (veces, valores escritos)"]
    for (d, reg), n in sorted(regs_msr.items(), key=lambda x: (x[0][1], x[0][0])):
        v = ejemplos.get((d, reg))
        out.append(f"{d} {reg:#010x} {n:7}" + (f"  {', '.join(hex(x) for x in sorted(v))}" if v else ""))
    destino.write_text("\n".join(out) + "\n")
    print("\n".join(out))
    print("en", destino)


def main():
    a = sys.argv[1:]
    global RTC, FIRMWARE, CPU_TCG
    acel = "tcg" if "tcg" in a else "kvm"
    RTC = "on" if "rtc" in a else "off"
    FIRMWARE = "contrato" if "contrato" in a else "qboot"
    if "intel" in a:
        CPU_TCG = "max,-x2apic,vendor=GenuineIntel"
    a = [x for x in a if x not in ("kvm", "tcg", "rtc", "contrato", "intel")]
    if not a:
        print(__doc__)
    elif a[0] == "preparar":
        preparar()
    elif a[0] == "qemu":
        qemu(acel)
    elif a[0] == "ver":
        if not IMAGEN.exists():
            imagen_nueva()
        imprimir(arrancar(a[1] if len(a) > 1 else "info", int(a[2]) if len(a) > 2 else 128, acel, ver=True))
    elif a[0] == "minimo":
        minimo(acel)
    elif a[0] == "grande":
        grande(acel)
    elif a[0] == "accesos":
        accesos()
    elif a[0] == "imagen":
        imagen_nueva()
        if "sucia" in a:
            ensuciar()
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
