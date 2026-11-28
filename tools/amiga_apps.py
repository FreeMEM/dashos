#!/usr/bin/env python3
"""Programas Amiga de verdad en la caja Amiga (rama amiga-apps, docs/bitacoras/amiga-apps.md): `make amiga-apps`.

  amiga_apps.py preparar --escenas a,b,c --dir DIR     copia el lanzador (caja/pruebas68k/lanzador.exe) con el nombre
                                                       de cada escena y sus guiones (tools/amiga_apps/<escena>.txt) en
                                                       DIR/programas, y escribe en DIR/programas.txt la lista para
                                                       DASHOS_PROGRAMAS_AMIGA
  amiga_apps.py correr --efi E --disco D --dir DIR [--pantalla 1920x1080] [--tope S] [--shiva] [--red]
                                                       arranca QEMU sin ventana (la imagen de la demo: `uso` con la caja
                                                       Amiga), entra Ana, pone el perfil de shiva (--shiva), contesta
                                                       "no" a las preguntas de la guardia, y saca una captura cada vez
                                                       que el lanzador dice "marca captura <nombre>". La salida serie,
                                                       las capturas (PNG), el WAV del sonido y lo que hace el guion,
                                                       en DIR.

Cada escena es una copia del lanzador: la caja la ejecuta como un programa del usuario (paso 8b4, 30 s y un cierre de
sus ventanas, con la caja de las capturas) y el lanzador lee su guion y arranca los programas con sus argumentos.
Nada de lo que se lanza ni de lo que sale va a git: el sistema del usuario y sus programas están en su equipo
(DASHOS_AMIGAOS) y DIR está fuera del repositorio.

Con --red, la red de usuario de QEMU sin `restrict`: DHCP, DNS e Internet de verdad (solo para esta imagen y a mano;
las pruebas automáticas no salen a Internet).
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

AQUI = Path(__file__).resolve().parent
sys.path.insert(0, str(AQUI))
import demo  # noqa: E402  (Monitor, Qmp y Demo: el teclado, la tableta y la salida serie)

ESCENAS = AQUI / "amiga_apps"

PAGINA = """<html><head><title>DashOS: la caja Amiga</title></head><body>
<h1>Hola desde DashOS</h1>
<p>Esta p&aacute;gina la sirve el PC por la red de usuario de QEMU. La pide <b>Tachyon</b>, un navegador de 68k hecho
en Dash, que corre en la caja Amiga de DashOS con la <i>bsdsocket.library</i> nativa sobre el servicio de red.</p>
<ul><li>Uno</li><li>Dos</li><li>Tres</li></ul>
<p><a href="grande.bin">Un fichero de 4 MB</a> para medir la velocidad.</p>
</body></html>
"""


def preparar(args):
    d = Path(args.dir)
    prog = d / "programas"
    (prog / "lanzador").mkdir(parents=True, exist_ok=True)
    lanzador = AQUI.parent / "caja" / "pruebas68k" / "lanzador.exe"
    lista = []
    for e in [x for x in args.escenas.split(",") if x]:
        guion = ESCENAS / f"{e}.txt"
        if not guion.exists():
            raise SystemExit(f"no hay escena {e} ({guion})")
        shutil.copy(guion, prog / "lanzador" / f"{e}.txt")
        shutil.copy(lanzador, prog / e)
        lista.append(str((prog / e).resolve()))
    # Las pruebas 68k propias que lanzan las escenas (`lanza PROGDIR:lanzador/cia_prueba.exe`), en git.
    for p in ["cia_prueba.exe", "red_prueba.exe"]:
        shutil.copy(AQUI.parent / "caja" / "pruebas68k" / p, prog / "lanzador" / p)
    # El cajón del lanzador, al final: se copia entero (no se ejecuta).
    lista.append(str((prog / "lanzador").resolve()))
    (d / "programas.txt").write_text(":".join(lista) + "\n")


def qemu(args, d, disco):
    red = "user,id=red0" if args.red else "user,id=red0,restrict=on"
    s = demo.sockets(d)
    return ["qemu-system-aarch64", "-name", "DashOS",
            "-M", "virt,gic-version=2,acpi=off,highmem=on,highmem-ecam=off,highmem-mmio=off,virtualization=on",
            "-cpu", "cortex-a72", "-smp", "4", "-m", args.memoria,
            "-bios", os.environ.get("EDK2", "/usr/share/edk2/aarch64/QEMU_EFI.fd"),
            "-drive", f"file=fat:rw:{d / 'arranque'},format=raw,if=virtio",
            "-display", "none", "-serial", f"file:{d / 'serie.txt'}",
            "-monitor", f"unix:{s / 'mon.sock'},server,nowait",
            "-qmp", f"unix:{s / 'qmp.sock'},server,nowait",
            "-device", "ramfb",
            "-device", "qemu-xhci,id=xhci", "-device", "usb-hub,bus=xhci.0,port=1",
            "-device", "usb-kbd,bus=xhci.0,port=1.1", "-device", "usb-tablet,bus=xhci.0,port=2",
            "-drive", f"file={disco},if=none,id=nv,format=raw,discard=unmap", "-device", "nvme,serial=dashos,drive=nv",
            "-audiodev", f"wav,id=snd0,path={d / 'sonido.wav'},out.frequency=48000",
            "-device", "virtio-sound-pci,audiodev=snd0",
            "-netdev", red,
            "-device", "virtio-net-pci,netdev=red0,disable-legacy=on,mac=52:54:00:12:34:56,romfile="]


def png(ppm):
    try:
        from PIL import Image
        Image.open(ppm).save(str(ppm)[:-4] + ".png")
        os.remove(ppm)
    except Exception as e:  # noqa: BLE001
        print(f"amiga-apps: sin PNG de {ppm}: {e}")


def lanzar_barra(m, nombre):
    """Abre el lanzador de la barra, escribe `nombre` e Intro (como guion_dms.py)."""
    bb = m.esperar(r"\[barra\] botones: lanzador en \((\d+), (\d+)\)", 20)
    if not bb:
        return False
    m.clic(int(bb[0]), int(bb[1]))
    time.sleep(1.5)
    m.teclear(nombre)
    time.sleep(0.5)
    m.tecla("ret")
    return True


def shiva(m):
    """El aspecto del escritorio del usuario (shiva): tema oscuro y su barra, desde un Shell, y se cierra."""
    hay = m.cuenta(r"\[shell\] ventana en pantalla")
    if not lanzar_barra(m, "shell") or not m.esperar(r"\[shell\] ventana en pantalla", 60, hay):
        m.apuntar("sin Shell para el perfil de shiva")
        return
    time.sleep(2)
    # El foco en el Shell (como guion_dms.py): un clic en su barra, donde lo puso el compositor.
    v = m.esperar(r"\[compositor\] ventana de [Ss]hell en \((\d+), (\d+)\)", 10)
    if v:
        m.clic(int(v[0]) + 120, max(int(v[1]) - 20, 1))
        time.sleep(1)
    m.teclear("Ajuste tema oscuro", intro=True)
    time.sleep(2)
    m.teclear("Ajuste DMS SYS:Prefs/Presets/DMS/shiva.json GUARDAR", intro=True)
    m.esperar(r"\[barra\] barras: 1 configuradas", 15)
    time.sleep(2)
    m.teclear("EndShell", intro=True)
    m.apuntar("perfil de shiva puesto")


def correr(args):
    d = Path(args.dir)
    if d.exists():
        shutil.rmtree(d)
    (d / "arranque" / "EFI" / "BOOT").mkdir(parents=True)
    shutil.copy(args.efi, d / "arranque" / "EFI" / "BOOT" / "BOOTAA64.EFI")
    ancho, alto = map(int, args.pantalla.split("x"))
    (d / "pantalla.txt").write_text(f"{ancho} {alto}\n")
    disco = d / "disco.img"
    shutil.copy(args.disco, disco)
    inicio = time.time()
    (d / "inicio.txt").write_text(f"{inicio}\n")
    # Con red: un servidor HTTP en el PC (la caja lo ve en 10.0.2.2, por la red de usuario de QEMU) con una página
    # sencilla para Tachyon y un fichero de 4 MB para medir la velocidad (red_prueba.exe).
    web = None
    if args.red:
        (d / "web").mkdir()
        (d / "web" / "index.html").write_text(PAGINA, encoding="latin-1")
        (d / "web" / "grande.bin").write_bytes(os.urandom(4 << 20))
        web = subprocess.Popen([sys.executable, "-m", "http.server", "8099", "--bind", "127.0.0.1", "--directory",
                                str(d / "web")], stdout=subprocess.DEVNULL, stderr=open(d / "web.err", "w"))
    q = subprocess.Popen(qemu(args, d, disco.resolve()), stdout=subprocess.DEVNULL, stderr=open(d / "qemu.err", "w"))
    (d / "qemu.pid").write_text(f"{q.pid}\n")
    m = demo.Demo(d, inicio, ancho, alto)
    try:
        guion(m, args)
    finally:
        if web is not None:
            web.terminate()
        if q.poll() is None:
            q.terminate()
            try:
                q.wait(20)
            except subprocess.TimeoutExpired:
                q.kill()
    print(f"amiga-apps: hecho en {time.time() - inicio:.0f} s; salida en {d}")


def guion(m, args):
    hasta = time.time() + args.tope
    if m.esperar(r"\[guardian\] pantalla de entrada pintada", 400):
        time.sleep(2)
        m.ir(ancho_medio(m), int(m.alto * 0.83))
        m.teclear("Ana")
        time.sleep(0.6)
        m.tecla("ret")
        time.sleep(1)
        m.teclear("clave de Ana")
        time.sleep(0.6)
        m.tecla("ret")
        if m.esperar(r"\[guardian\] entra Ana", 60) is not None:
            m.apuntar("entra Ana")
            if args.shiva:
                shiva(m)
    preguntas = 0
    capturas = 0
    hechas = 0
    while time.time() < hasta:
        t = m.texto()
        # La guardia: "no" a los 3 s, como la demo.
        n = len(re.findall(r"\[guardian\] pregunta en pantalla: ", t))
        if n > preguntas:
            time.sleep(3)
            m.tecla("n")
            preguntas = n
        # Lo que hace una persona, pedido por la escena: "marca clic X Y" (en la pantalla de DashOS), "marca doble X Y",
        # "marca derecho X Y" y "marca teclea texto" (con "\n" para Intro).
        ordenes = re.findall(r"lanzador [\d.]+ s: marca (clic|doble|derecho|teclea|abre) ([^\n]*)", t)
        while hechas < len(ordenes):
            que, resto = ordenes[hechas]
            resto = resto.strip()
            if que == "teclea":
                m.teclear(resto.replace("\\n", "\n"))
            elif que == "abre":
                # Un programa nativo de DashOS, desde el lanzador de la barra (para las capturas con varios a la vez).
                lanzar_barra(m, resto)
            else:
                x, y = map(int, resto.split()[:2])
                m.clic(x, y, "right" if que == "derecho" else "left", 2 if que == "doble" else 1)
            m.apuntar(f"{que} {resto}")
            hechas += 1
        marcas = re.findall(r"lanzador [\d.]+ s: marca captura (\S+)", t)
        while capturas < len(marcas):
            nombre = marcas[capturas]
            ppm = m.d / f"{capturas:02d}-{nombre}.ppm"
            m.mon.orden(f"screendump {ppm}")
            time.sleep(0.5)
            png(ppm)
            m.apuntar(f"captura {nombre}")
            capturas += 1
        # La caja acaba sola al final de su batería: entonces no hay más que ver.
        if re.search(r"\[caja-amiga\] (fin de la caja|la caja acaba|MAL: la caja sigue viva)", t) or \
                re.search(r"\[inicio\] caja-amiga (acabó|terminó)", t):
            m.apuntar("la caja acabó")
            break
        time.sleep(0.5)


def resumen(args):
    """Por escena: cuándo se lanzó cada programa, cuándo salió su primera ventana (lo que tarda en abrir, con la hora
    de la salida serie que pone el guion en cada línea no la hay: se usa el reloj del lanzador), lo que falta y cómo
    acabó."""
    t = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", open(Path(args.dir) / "serie.txt", errors="replace").read())
    escenas = re.split(r"(?=  \[caja-amiga\] lanzador 0\.0 s: guion PROGDIR:lanzador/)", t)[1:]
    for e in escenas:
        nombre = re.search(r"guion PROGDIR:lanzador/(\S+)\.txt", e).group(1)
        print(f"== {nombre}")
        for m_ in re.finditer(r"lanzador ([\d.]+) s: (lanza \S+|marca captura \S+|acaba \S+|\S+ no carga.*|ventana .*)", e):
            print(f"   {float(m_.group(1)):6.1f} s  {m_.group(2)}")
        for v in re.findall(r"ventana Amiga en pantalla \(([^)]*)\)[^:]*: ([^\n]*)", e)[:6]:
            print(f"          ventana {v[0]}: {v[1].strip()[:60]}")
        f = re.findall(r"faltan \d+ funciones distintas \(\d+ llamadas\): ([^\n]*)", e)
        if f:
            print(f"          falta: {f[-1][:200]}")
        a = re.search(r"programa del usuario \S+ acabó con ([^\n]*)", e)
        if a:
            print(f"          {a.group(1)[:160]}")


def ancho_medio(m):
    return m.ancho // 2


def principal():
    p = argparse.ArgumentParser()
    p.add_argument("accion", choices=["preparar", "correr", "resumen"])
    p.add_argument("--escenas", default="")
    p.add_argument("--dir", required=True)
    p.add_argument("--efi")
    p.add_argument("--disco")
    p.add_argument("--pantalla", default="1920x1080")
    p.add_argument("--memoria", default="8G")
    p.add_argument("--tope", type=float, default=1500)
    p.add_argument("--shiva", action="store_true")
    p.add_argument("--red", action="store_true")
    a = p.parse_args()
    {"preparar": preparar, "correr": correr, "resumen": resumen}[a.accion](a)


if __name__ == "__main__":
    principal()
