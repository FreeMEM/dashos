#!/usr/bin/env python3
"""Medida de un dominio de controladores de Linux (S35, paso 1): memoria y tiempo de arranque en QEMU.

Nada de lo que se baja o se compila va a git: se queda en $DASHOS_MEDIR_LINUX (por defecto
~/recuperar/geosian-7d/medir-linux), con el código del kernel ya descomprimido en linux-<versión>/ (de kernel.org,
comprobado con su sha256sums.asc).

    tools/medir_dominios.py compilar ARQ DOMINIO [VARIANTE]   un kernel: tinyconfig + caja-linux/dominios/*.config
    tools/medir_dominios.py arrancar ARQ DOMINIO MB CPUS [VARIANTE] [--trafico]   un arranque, con la salida
    tools/medir_dominios.py minimo ARQ DOMINIO CPUS [VARIANTE] [--trafico]        la menor memoria que vale
    tools/medir_dominios.py todo [--vueltas N]                         la tabla entera, en resultados.json
    tools/medir_dominios.py variantes                                  solo las variantes que falten en resultados.json

ARQ: arm64 (qemu-system-aarch64 -M virt, emulado: sin KVM en este PC) o x86_64 (qemu-system-x86_64 -M q35 con KVM).
DOMINIO: red (e1000e), wifi (cfg80211, mac80211, mac80211_hwsim y rt2800usb con xHCI) o red+wifi. Siempre con
virtio-net, el canal hacia DashOS.

El /init es caja-linux/dominios/servidor.c: levanta las interfaces, abre un socket de paquetes en cada una, dice
"listo" y, tres segundos después, vuelca /proc/meminfo.

Qué se apunta de cada arranque:
- "listo" visto desde fuera: segundos desde que se lanza QEMU hasta que la línea sale por la consola serie (incluye
  el firmware de QEMU en x86 y la emulación en arm64);
- uptime del kernel al decir "listo" (desde que el kernel empieza);
- la línea "Memory:" del kernel (código, datos, init, bss, reservado) y /proc/meminfo en reposo.
Un arranque vale si llega a "memoria-fin" sin pánico ni OOM y sigue vivo dos segundos más.
"""

import json
import os
import re
import select
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

RAIZ = Path(__file__).resolve().parent.parent
DOMINIOS = RAIZ / "caja-linux" / "dominios"
MEDIR = Path(os.environ.get("DASHOS_MEDIR_LINUX", Path.home() / "recuperar" / "geosian-7d" / "medir-linux"))
VERSION = os.environ.get("DASHOS_MEDIR_VERSION", "7.3.4")
FUENTE = MEDIR / f"linux-{VERSION}"
OBJ = MEDIR / "obj"
TOOLCHAIN_ARM64 = Path.home() / ".local/opt/toolchains/aarch64--musl--stable-2026.08-1/bin/aarch64-buildroot-linux-musl-"
NUCLEOS = os.cpu_count() or 8

ARQS = {
    "arm64": dict(arch="arm64", cross=str(TOOLCHAIN_ARM64), imagen="arch/arm64/boot/Image",
                  comprimida="arch/arm64/boot/Image.gz", objetivo=["Image", "Image.gz"], cc=f"{TOOLCHAIN_ARM64}gcc"),
    "x86_64": dict(arch="x86_64", cross="", imagen="arch/x86/boot/bzImage", comprimida=None, objetivo=["bzImage"],
                   cc="gcc"),
}

# Variantes para ver qué opciones mueven la memoria (pregunta 3). Cada una cambia una cosa sobre la base.
VARIANTES = {
    "base": "",
    "nrcpus-serie": "CONFIG_NR_CPUS={nr_serie}\n",
    "sin-printk": "CONFIG_PRINTK=n\nCONFIG_PRINTK_TIME=n\n",
    "slub-normal": "CONFIG_SLUB_TINY=n\n",
    "o2": "CONFIG_CC_OPTIMIZE_FOR_SIZE=n\nCONFIG_CC_OPTIMIZE_FOR_PERFORMANCE=y\n",
    "kallsyms": "CONFIG_KALLSYMS=y\n",
    "base-small": "CONFIG_BASE_SMALL=y\n",
    "log-4k": "CONFIG_LOG_BUF_SHIFT=12\n",
    "inet": "CONFIG_INET=y\n",
    # Partir del defconfig de la arquitectura en vez de tinyconfig (con los mismos fragmentos y sin módulos).
    "defconfig": "",
    # Solo x86-64: el kernel se descomprime en CONFIG_PHYSICAL_START (16 MB de serie) y no puede ir más abajo, así que
    # la máquina necesita al menos 16 MB más lo que ocupa el kernel, aunque luego use la memoria de debajo. A 2 MB.
    "inicio-2m": "CONFIG_PHYSICAL_START=0x200000\n",
}
# Dónde se mide cada variante (pregunta 3): en arm64 (el caso de la Pi), salvo la que solo existe en x86-64.
ARQ_VARIANTE = {"inicio-2m": "x86_64"}


def run(*cmd, **kw):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    subprocess.run([str(c) for c in cmd], check=True, **kw)


def carpeta(arq, dominio, variante="base"):
    return OBJ / f"{arq}-{dominio.replace('+', '-')}-{variante}"


def fragmentos(arq, dominio, variante):
    partes = [(DOMINIOS / "comun.config").read_text(), (DOMINIOS / f"{arq}.config").read_text()]
    for d in dominio.split("+"):
        partes.append((DOMINIOS / f"{d}.config").read_text())
    extra = VARIANTES[variante].format(nr_serie=512 if arq == "arm64" else 64)
    if extra:
        partes.append(f"# variante {variante}\n" + extra)
    return "\n".join(partes)


def pedido(texto):
    """{CONFIG_X: valor} de lo que pide un fragmento (la última vez que se nombra manda)."""
    r = {}
    for linea in texto.splitlines():
        linea = linea.strip()
        if linea.startswith("CONFIG_") and "=" in linea:
            k, v = linea.split("=", 1)
            r[k] = v
    return r


def leer_config(ruta):
    r = {}
    for linea in ruta.read_text().splitlines():
        if linea.startswith("CONFIG_") and "=" in linea:
            k, v = linea.split("=", 1)
            r[k] = v
        else:
            m = re.match(r"# (CONFIG_\w+) is not set", linea)
            if m:
                r[m.group(1)] = "n"
    return r


def compilar(arq, dominio, variante="base"):
    a = ARQS[arq]
    salida = carpeta(arq, dominio, variante)
    salida.mkdir(parents=True, exist_ok=True)
    mk = ["make", "-C", FUENTE, f"O={salida}", f"ARCH={a['arch']}", f"-j{NUCLEOS}"]
    if a["cross"]:
        mk.append(f"CROSS_COMPILE={a['cross']}")
    run(*mk, "defconfig" if variante == "defconfig" else "tinyconfig")
    texto = fragmentos(arq, dominio, variante)
    (salida / "dominio.config").write_text(texto)
    run(FUENTE / "scripts/kconfig/merge_config.sh", "-m", "-O", salida, salida / ".config", salida / "dominio.config",
        cwd=FUENTE, env={**os.environ, "ARCH": a["arch"]}, stdout=subprocess.DEVNULL)
    run(*mk, "olddefconfig")
    # Lo pedido que no quedó así (una dependencia que falta): se dice, no se calla.
    hay = leer_config(salida / ".config")
    malas = [f"{k}={v} (queda {hay.get(k, 'n')})" for k, v in pedido(texto).items()
             if hay.get(k, "n") != v and not (v == "n" and k not in hay)]
    (salida / "no-aplicadas.txt").write_text("\n".join(malas) + ("\n" if malas else ""))
    if malas:
        print("AVISO, opciones que no quedaron como se pidió:", *malas, sep="\n  ", flush=True)
    run(*mk, *a["objetivo"], stdout=subprocess.DEVNULL)
    print("kernel:", salida / a["imagen"], (salida / a["imagen"]).stat().st_size, "bytes", flush=True)


def cpio_newc(entradas):
    """Un cpio "newc" (el formato del initramfs). entradas: (nombre, modo, datos, (mayor, menor))."""
    out = bytearray()
    ino = 1
    for nombre, modo, datos, rdev in entradas + [("TRAILER!!!", 0, b"", (0, 0))]:
        nb = nombre.encode() + b"\0"
        cab = "070701" + "".join(f"{x:08x}" for x in (
            ino, modo, 0, 0, 1, 0, len(datos), 0, 0, rdev[0], rdev[1], len(nb), 0))
        out += cab.encode() + nb + b"\0" * (-(110 + len(nb)) % 4)
        out += datos + b"\0" * (-len(datos) % 4)
        ino += 1
    return bytes(out)


def initramfs(arq):
    """El initramfs del dominio: solo /init (el servidor, sin biblioteca C), sin comprimir."""
    a = ARQS[arq]
    destino = OBJ / f"initramfs-{arq}.cpio"
    binario = OBJ / f"servidor-{arq}"
    OBJ.mkdir(parents=True, exist_ok=True)
    banderas = ["-Os", "-static", "-nostdlib", "-ffreestanding", "-fno-stack-protector", "-fno-builtin", "-Wall",
                "-Wl,--gc-sections", "-s"]
    if arq == "x86_64":
        banderas += ["-fno-pie", "-no-pie"]
    run(a["cc"], *banderas, "-o", binario, DOMINIOS / "servidor.c")
    entradas = [(d, 0o040755, b"", (0, 0)) for d in ("dev", "proc", "sys")]
    entradas.append(("dev/console", 0o020600, b"", (5, 1)))
    entradas.append(("init", 0o100755, binario.read_bytes(), (0, 0)))
    destino.write_bytes(cpio_newc(entradas))
    return destino


def orden_qemu(arq, dominio, mb, cpus, variante, trafico=False):
    a = ARQS[arq]
    kernel = carpeta(arq, dominio, variante) / a["imagen"]
    cpio = OBJ / f"initramfs-{arq}.cpio"
    linea = f"quiet dominio={dominio}" + (" trafico=1" if trafico else "")
    # Las dos tarjetas en un concentrador de QEMU, sin salida al PC ni fuera: lo que manda una lo recibe la otra.
    red = ["-netdev", "hubport,id=n1,hubid=0"]
    if "red" in dominio.split("+"):
        red += ["-netdev", "hubport,id=n0,hubid=0", "-device", "e1000e,netdev=n0"]
    if "wifi" in dominio.split("+"):
        red += ["-device", "qemu-xhci"]
        linea += " mac80211_hwsim.radios=1"
    if arq == "arm64":
        return ["qemu-system-aarch64", "-M", "virt,gic-version=3", "-cpu", "cortex-a72", "-accel", "tcg,thread=multi",
                "-smp", str(cpus), "-m", f"{mb}M", "-nographic", "-no-reboot", "-kernel", kernel, "-initrd", cpio,
                "-append", f"console=ttyAMA0 {linea}", *red, "-device", "virtio-net-device,netdev=n1"]
    return ["qemu-system-x86_64", "-M", "q35", "-accel", "kvm", "-cpu", "host", "-smp", str(cpus), "-m", f"{mb}M",
            "-nographic", "-no-reboot", "-kernel", kernel, "-initrd", cpio, "-append", f"console=ttyS0 {linea}",
            *red, "-device", "virtio-net-pci,netdev=n1"]


MALO = re.compile(r"Kernel panic|Out of memory|oom-kill|invoked oom-killer|fallo:|Unable to handle|Oops")
CAMPOS = ("MemTotal", "MemFree", "MemAvailable", "Cached", "Shmem", "Slab", "SReclaimable", "SUnreclaim",
          "KernelStack", "PageTables", "Percpu", "VmallocUsed")


def arrancar(arq, dominio, mb, cpus, variante="base", espera=None, ver=False, trafico=False):
    """Un arranque. Devuelve un dict con ok, tiempos y memoria."""
    espera = espera or (30 if arq == "arm64" else 15)
    orden = [str(x) for x in orden_qemu(arq, dominio, mb, cpus, variante, trafico)]
    final = b"memoria2-fin" if trafico else b"memoria-fin"
    t0 = time.monotonic()
    p = subprocess.Popen(orden, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    texto = b""
    t_listo = t_fin = None
    try:
        while True:
            ahora = time.monotonic()
            if ahora - t0 > espera or (t_fin and ahora - t_fin > 2):
                break
            r, _, _ = select.select([p.stdout], [], [], 0.05)
            if r:
                trozo = os.read(p.stdout.fileno(), 65536)
                if not trozo:
                    break
                texto += trozo
                if t_listo is None and b"\nlisto " in b"\n" + texto:
                    t_listo = time.monotonic() - t0
                if t_fin is None and final in texto:
                    t_fin = time.monotonic()
                if MALO.search(texto.decode(errors="replace")):
                    t_fin = t_fin or time.monotonic()  # pánico u OOM: se recoge lo que diga en 2 s y se acaba
    finally:
        if p.poll() is None:
            p.kill()  # nuestro propio hijo, por su PID
        p.wait()
    s = texto.decode(errors="replace")
    if ver:
        print(s)
    res = dict(arq=arq, dominio=dominio, variante=variante, mb=mb, cpus=cpus, listo_fuera=t_listo)
    malo = MALO.search(s)
    res["ok"] = bool(t_listo and t_fin and not malo and final.decode() in s)
    if malo:
        res["motivo"] = malo.group(0)
    elif not t_listo:
        res["motivo"] = "sin listo"
    m = re.search(r"uptime=([\d.]+)", s)
    res["uptime_listo"] = float(m.group(1)) if m else None
    m = re.search(r"Memory: (\d+)K/(\d+)K available \((\d+)K kernel code, (\d+)K rwdata, (\d+)K rodata, "
                  r"(\d+)K init, (\d+)K bss, (\d+)K reserved", s)
    if m:
        res["memory"] = dict(zip(("disponible", "total", "codigo", "rwdata", "rodata", "init", "bss", "reservado"),
                                 map(int, m.groups())))
    for clave, marca in (("meminfo", "memoria"), ("meminfo_trafico", "memoria2")):
        if f"{marca}-inicio" in s:
            bloque = s.split(f"{marca}-inicio", 1)[1].split(f"{marca}-fin", 1)[0]
            mi = {}
            for linea in bloque.splitlines():
                m = re.match(r"(\w+):\s+(\d+) kB", linea.strip())
                if m and m.group(1) in CAMPOS:
                    mi[m.group(1)] = int(m.group(2))
            res[clave] = mi
    m = re.search(r"trafico enviadas=(\d+) recibidas=(\d+)", s)
    if m:
        res["trafico"] = dict(enviadas=int(m.group(1)), recibidas=int(m.group(2)))
    if not res["ok"]:
        res["cola"] = s[-1500:]
    return res


def minimo(arq, dominio, cpus, variante="base", alto=256, trafico=False):
    """La menor memoria (en MB) con la que arranca y se queda quieto. Búsqueda binaria; se confirma dos veces."""
    bajo = 8
    if not arrancar(arq, dominio, alto, cpus, variante, trafico=trafico)["ok"]:
        raise SystemExit(f"{arq} {dominio} {variante}: no arranca ni con {alto} MB")
    while alto - bajo > 1:
        medio = (alto + bajo) // 2
        r = arrancar(arq, dominio, medio, cpus, variante, trafico=trafico)
        print(f"  {arq} {dominio} {variante} {cpus} cpu {medio} MB{' con tráfico' if trafico else ''}: "
              f"{'bien' if r['ok'] else r.get('motivo')}",
              flush=True)
        if r["ok"]:
            alto = medio
        else:
            bajo = medio
    # El borde puede bailar: el mínimo es el primero que va dos veces seguidas.
    while not all(arrancar(arq, dominio, alto, cpus, variante, trafico=trafico)["ok"] for _ in range(2)):
        alto += 1
    return alto


def mediana(xs):
    xs = [x for x in xs if x is not None]
    return round(statistics.median(xs), 3) if xs else None


def todo(vueltas=5):
    resultados = {"version": VERSION, "fecha": time.strftime("%Y-%m-%d %H:%M"), "pc": os.uname().release,
                  "carga": os.getloadavg(), "filas": [], "variantes": []}
    destino = MEDIR / "resultados.json"
    for arq in ARQS:
        initramfs(arq)
        for dominio in ("red", "wifi", "red+wifi"):
            if not (carpeta(arq, dominio) / ARQS[arq]["imagen"]).exists():
                compilar(arq, dominio)
            for cpus in (1, 2, 4):
                mb = minimo(arq, dominio, cpus)
                recomendado = -(-int(mb * 1.25) // 4) * 4
                runs = [arrancar(arq, dominio, recomendado, cpus) for _ in range(vueltas)]
                buenos = [r for r in runs if r["ok"]]
                fila = dict(arq=arq, dominio=dominio, cpus=cpus, minimo_mb=mb, medido_con_mb=recomendado,
                            buenos=len(buenos), vueltas=vueltas,
                            listo_fuera=mediana([r["listo_fuera"] for r in buenos]),
                            listo_fuera_min=min((r["listo_fuera"] for r in buenos), default=None),
                            listo_fuera_max=max((r["listo_fuera"] for r in buenos), default=None),
                            uptime_listo=mediana([r["uptime_listo"] for r in buenos]),
                            memory=buenos[0].get("memory") if buenos else None,
                            meminfo=buenos[0].get("meminfo") if buenos else None,
                            imagen=(carpeta(arq, dominio) / ARQS[arq]["imagen"]).stat().st_size)
                comp = ARQS[arq]["comprimida"]
                if comp:
                    fila["imagen_gz"] = (carpeta(arq, dominio) / comp).stat().st_size
                # La memoria en reposo con el mínimo justo, para ver cuánto queda libre en el borde.
                borde = arrancar(arq, dominio, mb, cpus)
                fila["meminfo_minimo"] = borde.get("meminfo")
                # Con tráfico (20000 tramas de la tarjeta con cable a la virtio-net), si hay tarjeta con cable.
                if "red" in dominio.split("+"):
                    fila["minimo_trafico_mb"] = minimo(arq, dominio, cpus, trafico=True)
                    t = arrancar(arq, dominio, recomendado, cpus, trafico=True)
                    fila["trafico"] = t.get("trafico")
                    fila["meminfo_trafico"] = t.get("meminfo_trafico")
                fila["carga_pc"] = os.getloadavg()
                resultados["filas"].append(fila)
                print(json.dumps(fila, ensure_ascii=False), flush=True)
                destino.write_text(json.dumps(resultados, indent=1, ensure_ascii=False))
    variantes(resultados)


def variantes(resultados=None):
    """Pregunta 3: qué opciones mueven la memoria. red+wifi, con 1 y 4 procesadores. Sigue donde se quedó."""
    destino = MEDIR / "resultados.json"
    if resultados is None:
        resultados = json.loads(destino.read_text())
    hechas = {(v["variante"], v["cpus"]) for v in resultados["variantes"]}
    for variante in VARIANTES:
        if variante == "base":
            continue
        arq, dominio = ARQ_VARIANTE.get(variante, "arm64"), "red+wifi"
        initramfs(arq)
        if not (carpeta(arq, dominio, variante) / ARQS[arq]["imagen"]).exists():
            compilar(arq, dominio, variante)
        for cpus in (1, 4):
            if (variante, cpus) in hechas:
                continue
            try:
                mb = minimo(arq, dominio, cpus, variante)
            except SystemExit as e:
                resultados["variantes"].append(dict(variante=variante, arq=arq, dominio=dominio, cpus=cpus,
                                                    error=str(e)))
                destino.write_text(json.dumps(resultados, indent=1, ensure_ascii=False))
                continue
            r = arrancar(arq, dominio, 64, cpus, variante)
            v = dict(variante=variante, arq=arq, dominio=dominio, cpus=cpus, minimo_mb=mb,
                     memory=r.get("memory"), meminfo_64=r.get("meminfo"),
                     imagen=(carpeta(arq, dominio, variante) / ARQS[arq]["imagen"]).stat().st_size,
                     no_aplicadas=(carpeta(arq, dominio, variante) / "no-aplicadas.txt").read_text())
            resultados["variantes"].append(v)
            print(json.dumps(v, ensure_ascii=False), flush=True)
            destino.write_text(json.dumps(resultados, indent=1, ensure_ascii=False))
    print("resultados:", destino)


def main():
    a = sys.argv[1:]
    if not a:
        print(__doc__)
        return
    trafico = "--trafico" in a
    a = [x for x in a if x != "--trafico"]
    if a[0] == "compilar":
        compilar(a[1], a[2], *(a[3:4] or ["base"]))
    elif a[0] == "arrancar":
        initramfs(a[1])
        r = arrancar(a[1], a[2], int(a[3]), int(a[4]), *(a[5:6] or ["base"]), ver=True, trafico=trafico)
        r.pop("cola", None)
        print(json.dumps(r, indent=1, ensure_ascii=False))
    elif a[0] == "minimo":
        initramfs(a[1])
        print(minimo(a[1], a[2], int(a[3]), *(a[4:5] or ["base"]), trafico=trafico), "MB")
    elif a[0] == "variantes":
        variantes()
    elif a[0] == "todo":
        todo(int(a[a.index("--vueltas") + 1]) if "--vueltas" in a else 5)
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
