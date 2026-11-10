#!/usr/bin/env python3
"""Banco de medidas de la multitarea de DashOS (M0, docs/MULTITAREA_LOOP.md).

    tools/medir.py paquete SALIDA            junta los hijos de prueba (Rust, C y Dash) en el paquete del kernel
    tools/medir.py correr IMAGEN JSON [-n N] arranca la imagen de `make medir` N veces en QEMU raspi4b y guarda las
                                             medianas de cada medida en JSON (con todas las vueltas)
    tools/medir.py linux JSON [-n N]         lo mismo con el kernel de Raspberry Pi OS y el banco de Linux (paso L;
                                             antes, tools/medir_linux/hacer.sh)
    tools/medir.py linux-fs JSON [-n N]      el banco de ficheros en Linux (Debian arm64, ext4 en NVMe, QEMU virt; paso F;
                                             antes, tools/medir_linux/hacer_fs.sh)
    tools/medir.py disco-fs DISCO REDOXFS_AR un disco de 1 + 64 MB con un RedoxFS vacío desde el MB 1 (paso F)
    tools/medir.py dashos-fs EFI DISCO JSON [-n N]  el banco de ficheros de DashOS en QEMU virt por UEFI (`make medir-fs`)
    tools/medir.py pi3 IMAGEN JSON [-n N]    lo mismo en QEMU raspi3b (P3, `make medir-pi3`)
    tools/medir.py comparar BASE NUEVO       compara dos JSON: dice "dentro del ruido" si la diferencia es menor que la
                                             dispersión (máximo menos mínimo de las vueltas) de alguno de los dos

R7 (B3, B4 de docs/REVISION_ARQUITECTURA.md): cada medida guarda mediana, mínimo y máximo de las vueltas (7 por
defecto en el Makefile); QEMU corre fijado (taskset) a los núcleos del PC más libres, y el JSON apunta cuáles y la
carga del PC al medir.

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
             "temporizador": ("us", "carga"), "dormilon": ("dormir_ms",), "ociosos": (), "fs": ("op",)}.get(prueba, ())
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


LINUX = os.path.expanduser("~/.cache/dashos-linux")


def orden_linux():
    """QEMU con kernel8.img de Raspberry Pi OS, el árbol sin Bluetooth y el banco como /init (tools/medir_linux)."""
    return ["-kernel", os.path.join(LINUX, "kernel8.img"), "-dtb", os.path.join(LINUX, "banco.dtb"), "-initrd",
            os.path.join(LINUX, "banco.cpio.gz"), "-append", "console=ttyAMA1,115200 rdinit=/init loglevel=3"]


def orden_linux_fs():
    """QEMU virt con el kernel de Debian, el banco de ficheros como /init y una copia del disco ext4 (nuevo cada vez)."""
    v = os.path.join(LINUX, "virt")
    import shutil
    disco = os.path.join(v, "ext4-vuelta.img")
    shutil.copyfile(os.path.join(v, "ext4.img"), disco)
    return ["-M", "virt,gic-version=2,highmem=off", "-cpu", "cortex-a72", "-smp", "4", "-m", "2G",
            "-kernel", os.path.join(v, "linux"), "-initrd", os.path.join(v, "fs.cpio.gz"),
            "-append", "console=ttyAMA0 rdinit=/init loglevel=3",
            "-drive", f"file={disco},if=none,id=nv,format=raw", "-device", "nvme,serial=dashos,drive=nv"]


def disco_fs(salida, ar):
    t = tempfile.mkdtemp(prefix="medir-fs-")
    raiz = os.path.join(t, "raiz")
    os.mkdir(raiz)
    fs = os.path.join(t, "fs.img")
    with open(fs, "wb") as f:
        f.truncate(64 << 20)
    subprocess.run([ar, fs, raiz], check=True, stdout=subprocess.DEVNULL)
    # La raíz, del usuario del banco (1000, como `medir-fs` en las fichas de `inicio`): desde R6 el servicio mira
    # dueños y permisos, y la raíz que hace redoxfs-ar es del 0.
    duenos = os.path.join(RAIZ, "archivos/herramientas/target/release/duenos")
    subprocess.run([duenos, fs, "0", "1000:1000"], check=True)
    with open(salida, "wb") as f:
        f.write(bytes(1 << 20))
        f.write(open(fs, "rb").read())


DASHOS_FS = None
PI3 = False


def dtb_pi3():
    f = os.path.join(RAIZ, "build/firmware/bcm2710-rpi-3-b.dtb")
    if not os.path.exists(f):
        os.makedirs(os.path.dirname(f), exist_ok=True)
        subprocess.run(["curl", "-sSfL", "-o", f,
                        "https://github.com/raspberrypi/firmware/raw/0ea2874060/boot/bcm2710-rpi-3-b.dtb"], check=True)
    return f


def orden_dashos_fs():
    """QEMU virt con EDK2, DashOS como BOOTAA64.EFI y una copia del disco del banco (nuevo cada vez)."""
    import shutil
    efi, disco = DASHOS_FS
    d = tempfile.mkdtemp(prefix="medir-efi-")
    os.makedirs(os.path.join(d, "EFI", "BOOT"))
    shutil.copyfile(efi, os.path.join(d, "EFI", "BOOT", "BOOTAA64.EFI"))
    copia = disco + ".vuelta"
    shutil.copyfile(disco, copia)
    return ["-M", "virt,gic-version=2,acpi=off,highmem=off", "-cpu", "cortex-a72", "-smp", "4", "-m", "2G",
            "-bios", os.environ.get("EDK2", "/usr/share/edk2/aarch64/QEMU_EFI.fd"),
            "-drive", f"file=fat:rw:{d},format=raw,if=virtio", "-net", "none",
            "-drive", f"file={copia},if=none,id=nv,format=raw", "-device", "nvme,serial=dashos,drive=nv"]


def nucleos_libres(cuantos):
    """Los `cuantos` núcleos del PC menos usados en medio segundo (de /proc/stat), para fijar QEMU (B4)."""
    import time

    def leer():
        r = {}
        for linea in open("/proc/stat"):
            if re.match(r"cpu\d+ ", linea):
                c = linea.split()
                v = [int(x) for x in c[1:]]
                r[int(c[0][3:])] = (sum(v), v[3] + v[4])
        return r
    a = leer()
    time.sleep(0.5)
    b = leer()
    uso = {n: 1 - (b[n][1] - a[n][1]) / max(1, b[n][0] - a[n][0]) for n in b if n in a}
    # Como mucho un hilo por núcleo físico: dos núcleos emulados en hilos hermanos se frenan entre sí (un 10 % en la
    # ida y vuelta).
    libres, fisicos = [], set()
    for n in sorted(uso, key=lambda n: uso[n]):
        try:
            hermanos = open(f"/sys/devices/system/cpu/cpu{n}/topology/thread_siblings_list").read().strip()
        except OSError:
            hermanos = str(n)
        if hermanos not in fisicos:
            fisicos.add(hermanos)
            libres.append(n)
        if len(libres) == cuantos:
            break
    return sorted(libres), {n: round(uso[n], 2) for n in libres}


def una_vuelta(imagen, segundos=300):
    """Arranca la imagen (o Linux, con imagen None) y devuelve la salida del puerto serie hasta "[medir] fin"."""
    import selectors
    import time
    if imagen == "linux-fs":
        maquina = orden_linux_fs()
    elif imagen == "dashos-fs":
        maquina = orden_dashos_fs()
    elif PI3:
        maquina = ["-M", "raspi3b", "-kernel", imagen, "-dtb", dtb_pi3()]
    else:
        maquina = ["-M", "raspi4b", *(orden_linux() if imagen is None else ["-kernel", imagen, "-dtb", dtb()])]
    # QEMU usa un hilo por núcleo emulado y alguno más: se fija a los 6 núcleos más libres del PC (B4).
    libres, uso = nucleos_libres(6)
    CARGA_PC.append({"nucleos": libres, "uso": uso, "carga_media": os.getloadavg()[0]})
    fijar = [] if os.environ.get("MEDIR_SIN_TASKSET") else ["taskset", "-c", ",".join(map(str, libres))]
    q = subprocess.Popen([*fijar, "qemu-system-aarch64", *maquina, "-display", "none",
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


CARGA_PC = []


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
    medianas, minimos, maximos = {}, {}, {}
    for k in sorted({k for m in todas for k in m}):
        campos, mins, maxs = {}, {}, {}
        for c in {c for m in todas if k in m for c in m[k]}:
            vals = [m[k][c] for m in todas if k in m and c in m[k]]
            try:
                nums = [float(x) for x in vals]
                campos[c] = statistics.median(nums)
                mins[c], maxs[c] = min(nums), max(nums)
            except ValueError:
                campos[c] = vals[0]
        medianas[k], minimos[k], maximos[k] = campos, mins, maxs
    with open(salida_json, "w") as f:
        json.dump({"medianas": medianas, "minimos": minimos, "maximos": maximos, "vueltas": todas,
                   "pc": CARGA_PC}, f, indent=1, sort_keys=True)
    for k, c in medianas.items():
        print(k, " ".join(f"{a}={b:g}" if isinstance(b, float) else f"{a}={b}" for a, b in sorted(c.items())))


def comparar(base, nuevo):
    ja, jb = json.load(open(base)), json.load(open(nuevo))
    a, b = ja["medianas"], jb["medianas"]

    def dispersion(j, k, c):
        lo, hi = j.get("minimos", {}).get(k, {}).get(c), j.get("maximos", {}).get(k, {}).get(c)
        return (hi - lo) if lo is not None and hi is not None else 0.0
    for k in sorted(set(a) | set(b)):
        if k not in a or k not in b:
            print(f"{k}: solo en {'nuevo' if k in b else 'base'}")
            continue
        partes = []
        for c in sorted(set(a[k]) & set(b[k])):
            x, y = a[k][c], b[k][c]
            if isinstance(x, float) and isinstance(y, float) and x != y:
                cambio = f" ({(y - x) / x * 100:+.0f} %)" if x else ""
                ruido = max(dispersion(ja, k, c), dispersion(jb, k, c))
                aviso = ", dentro del ruido" if abs(y - x) <= ruido else ""
                partes.append(f"{c} {x:g} -> {y:g}{cambio}{aviso}")
        print(f"{k}: " + ("; ".join(partes) if partes else "igual"))


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "paquete":
        paquete(sys.argv[2])
    elif len(sys.argv) >= 4 and sys.argv[1] == "correr":
        n = int(sys.argv[sys.argv.index("-n") + 1]) if "-n" in sys.argv else 1
        correr(sys.argv[2], sys.argv[3], n)
    elif len(sys.argv) >= 4 and sys.argv[1] == "pi3":
        PI3 = True
        n = int(sys.argv[sys.argv.index("-n") + 1]) if "-n" in sys.argv else 1
        correr(sys.argv[2], sys.argv[3], n)
    elif len(sys.argv) >= 3 and sys.argv[1] == "linux":
        n = int(sys.argv[sys.argv.index("-n") + 1]) if "-n" in sys.argv else 1
        correr(None, sys.argv[2], n)
    elif len(sys.argv) >= 3 and sys.argv[1] == "linux-fs":
        n = int(sys.argv[sys.argv.index("-n") + 1]) if "-n" in sys.argv else 1
        correr("linux-fs", sys.argv[2], n)
    elif len(sys.argv) == 4 and sys.argv[1] == "disco-fs":
        disco_fs(sys.argv[2], sys.argv[3])
    elif len(sys.argv) >= 5 and sys.argv[1] == "dashos-fs":
        DASHOS_FS = (sys.argv[2], sys.argv[3])
        n = int(sys.argv[sys.argv.index("-n") + 1]) if "-n" in sys.argv else 1
        correr("dashos-fs", sys.argv[4], n)
    elif len(sys.argv) == 4 and sys.argv[1] == "comparar":
        comparar(sys.argv[2], sys.argv[3])
    else:
        print(__doc__)
        sys.exit(2)
