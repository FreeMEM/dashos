#!/usr/bin/env python3
"""El vídeo de la demo de DashOS (`make demo`, docs/bitacoras/demo.md).

  demo.py programas "Nombre=ruta:ruta:..." CARPETA   copia los programas 68k del usuario con su nombre y dice la lista
  demo.py disco NVME DISCO_ARCH SALIDA                el disco de la demo: GPT con DashOS (el RedoxFS de NVME, del tamaño
                                                      que tenga) y la partición de la caja Linux de DISCO_ARCH
  demo.py grabar --efi E --disco D --pantalla AxB --dir DIR [--guion si|no]
                                                      arranca QEMU sin ventana y graba su pantalla; con el guion, entra y
                                                      recorre la demo esperando cada cosa en la salida serie
  demo.py orden DIR "orden del monitor"               (a mano, con --guion no) una orden del monitor de QEMU
  demo.py raton DIR X Y [clic|doble|derecho|nada]     (a mano) lleva la tableta a (X, Y) de la pantalla de DashOS
  demo.py teclear DIR "texto" [intro]                 (a mano) teclea con el mapa español

Cómo se graba: un hilo pide a QEMU `screendump` a ritmo fijo (FPS) por un monitor propio, y guarda cada cuadro en JPEG
con su hora (DIR/grabacion/cuadros.txt). El guion apunta en DIR/grabacion/marcas.txt cuándo empieza cada tramo. El
vídeo se monta después (`tools/demo_montar.py`), con los tramos a la medida de la locución. QEMU tiene tres sockets en
una carpeta corta de /tmp (DIR/grabacion/sockets.txt): mon.sock (el guion), grab.sock (la grabación) y qmp.sock (la
tableta, en posición absoluta).

Nada de lo que se graba va a git: DIR está fuera del repositorio.
"""
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

FPS = float(os.environ.get("DEMO_FPS", "8"))
TIPO_CAJA_LINUX = "d0a18dbf-bbf4-4b0e-8627-ce5006b50018"
TIPO_DASHOS = "d0a18dbf-bbf4-4b0e-8627-ce5006b50017"
INICIO_ARCH_ORIGEN = 8 << 20  # la partición de la caja en el disco de `preparar-arch`


# --- Preparar ---

def programas(spec, carpeta):
    carpeta = Path(carpeta)
    lista = []
    for trozo in filter(None, spec.split(":")):
        nombre, _, ruta = trozo.rpartition("=")
        ruta = Path(os.path.expanduser(ruta))
        if not ruta.exists():
            print(f"demo: no está {ruta}: se salta", file=sys.stderr)
            continue
        destino = carpeta / (nombre or ruta.name)
        if ruta.is_dir():
            shutil.copytree(ruta, destino)
        else:
            shutil.copy(ruta, destino)
        lista.append(str(destino.resolve()))
    print(":".join(lista))


def disco(nvme, origen, salida):
    """La GPT de la demo: la partición de DashOS tan grande como el RedoxFS de `nvme` (con el AmigaOS y los programas
    del usuario no cabe en los 7 MB del disco de Arch) y, detrás, la de la caja Linux copiada del disco de Arch."""
    datos = Path(nvme).read_bytes()
    if datos[510:512] != b"\x55\xaa":
        raise SystemExit(f"{nvme} no es el disco de las pruebas")
    fs = datos[1 << 20:]
    tam_origen = os.path.getsize(origen)
    tam_arch = tam_origen - INICIO_ARCH_ORIGEN - (1 << 20)
    inicio_arch = ((1 << 20) + len(fs) + (1 << 20) - 1) & ~((1 << 20) - 1)
    total = inicio_arch + tam_arch + (1 << 20)
    salida = Path(salida)
    if salida.exists():
        salida.unlink()
    with open(salida, "wb") as f:
        f.truncate(total)
        f.seek(1 << 20)
        f.write(fs)
    # La partición de la caja, copiada en trozos y sin escribir los ceros (el fichero sigue disperso).
    with open(origen, "rb") as o, open(salida, "r+b") as f:
        o.seek(INICIO_ARCH_ORIGEN)
        hecho, trozo = 0, 4 << 20
        while hecho < tam_arch:
            b = o.read(min(trozo, tam_arch - hecho))
            if not b:
                break
            if b.count(0) != len(b):
                f.seek(inicio_arch + hecho)
                f.write(b)
            hecho += len(b)
    d0, d1 = (1 << 20) // 512, ((1 << 20) + len(fs)) // 512 - 1
    a0, a1 = inicio_arch // 512, (inicio_arch + tam_arch) // 512 - 1
    subprocess.run(["sgdisk", "-n", f"1:{d0}:{d1}", "-t", f"1:{TIPO_DASHOS}", "-c", "1:DashOS",
                    "-n", f"2:{a0}:{a1}", "-t", f"2:{TIPO_CAJA_LINUX}", "-c", "2:caja Linux", str(salida)],
                   check=True, stdout=subprocess.DEVNULL)
    print(f"disco de la demo: {salida}, DashOS de {len(fs) >> 20} MB, la caja Linux desde el MB {inicio_arch >> 20}")


# --- QEMU ---

class Monitor:
    """Una conexión que se queda abierta con un monitor de QEMU (HMP): manda una orden y lee hasta el indicador."""

    def __init__(self, ruta):
        self.ruta = ruta
        self.s = None
        self.cerrojo = threading.Lock()

    def _conectar(self):
        hasta = time.time() + 30
        while True:
            try:
                s = socket.socket(socket.AF_UNIX)
                s.connect(self.ruta)
                break
            except OSError:
                if time.time() > hasta:
                    raise
                time.sleep(0.2)
        s.settimeout(30)
        self.s = s
        self._leer_hasta_indicador()

    def _leer_hasta_indicador(self):
        datos = b""
        while not datos.endswith(b"(qemu) "):
            t = self.s.recv(65536)
            if not t:
                raise OSError("el monitor se cerró")
            datos += t
        return datos.decode(errors="replace")

    def orden(self, texto):
        with self.cerrojo:
            if self.s is None:
                self._conectar()
            self.s.sendall(texto.encode() + b"\n")
            return self._leer_hasta_indicador()


class Qmp:
    """QMP, para la tableta: `input-send-event` con posiciones absolutas (0 a 32767 en cada eje)."""

    def __init__(self, ruta):
        hasta = time.time() + 30
        while True:
            try:
                self.s = socket.socket(socket.AF_UNIX)
                self.s.connect(ruta)
                break
            except OSError:
                if time.time() > hasta:
                    raise
                time.sleep(0.2)
        self.f = self.s.makefile("rwb")
        self.f.readline()
        self.mandar({"execute": "qmp_capabilities"})
        self.cerrojo = threading.Lock()

    def mandar(self, orden):
        self.f.write(json.dumps(orden).encode() + b"\n")
        self.f.flush()
        while True:
            r = json.loads(self.f.readline())
            if "return" in r or "error" in r:
                return r

    def eventos(self, eventos):
        with self.cerrojo:
            return self.mandar({"execute": "input-send-event", "arguments": {"events": eventos}})


class Grabadora(threading.Thread):
    """Pide `screendump` cada 1/FPS s y guarda los cuadros en JPEG con su hora."""

    def __init__(self, monitor, carpeta, inicio):
        super().__init__(daemon=True)
        self.monitor, self.carpeta, self.inicio = monitor, Path(carpeta), inicio
        self.parar = False
        self.n = 0
        self.tmp = Path(os.environ.get("DEMO_TMP", "/tmp")) / f"demo-cuadro-{os.getpid()}.ppm"

    def run(self):
        from PIL import Image
        cuadros = open(self.carpeta / "cuadros.txt", "a")
        siguiente = time.time()
        while not self.parar:
            ahora = time.time()
            if ahora < siguiente:
                time.sleep(siguiente - ahora)
            siguiente += 1 / FPS
            t = time.time() - self.inicio
            try:
                self.monitor.orden(f"screendump {self.tmp}")
                im = Image.open(self.tmp)
                im.load()
            except Exception:
                continue
            nombre = f"c{self.n:06d}.jpg"
            im.convert("RGB").save(self.carpeta / "cuadros" / nombre, quality=90)
            cuadros.write(f"{t:.3f} {nombre}\n")
            cuadros.flush()
            self.n += 1
            # Si va con retraso (la máquina está cargada), no se acumula: se sigue desde ahora.
            if time.time() > siguiente + 1:
                siguiente = time.time()
        cuadros.close()
        try:
            self.tmp.unlink()
        except OSError:
            pass


def sockets(d):
    """Los sockets de QEMU van en una carpeta corta aparte (la ruta de un socket Unix no pasa de 107 bytes); su ruta,
    en d/sockets.txt."""
    f = Path(d) / "sockets.txt"
    if not f.exists():
        import tempfile
        f.write_text(tempfile.mkdtemp(prefix="dashos-demo-") + "\n")
    return Path(f.read_text().strip())


def qemu_orden(args, d, disco_):
    return ["qemu-system-aarch64", "-name", "DashOS",
            "-M", "virt,gic-version=2,acpi=off,highmem=on,highmem-ecam=off,highmem-mmio=off,virtualization=on",
            "-cpu", "cortex-a72", "-smp", "4", "-m", os.environ.get("DEMO_MEMORIA", "8G"),
            "-bios", os.environ.get("EDK2", "/usr/share/edk2/aarch64/QEMU_EFI.fd"),
            "-drive", f"file=fat:rw:{d / 'arranque'},format=raw,if=virtio",
            "-display", "none", "-serial", f"file:{d / 'serie.txt'}",
            "-monitor", f"unix:{sockets(d) / 'mon.sock'},server,nowait",
            "-chardev", f"socket,id=grab,path={sockets(d) / 'grab.sock'},server=on,wait=off", "-mon", "chardev=grab,mode=readline",
            "-qmp", f"unix:{sockets(d) / 'qmp.sock'},server,nowait",
            "-device", "ramfb",
            "-device", "qemu-xhci,id=xhci", "-device", "usb-hub,bus=xhci.0,port=1",
            "-device", "usb-kbd,bus=xhci.0,port=1.1", "-device", "usb-tablet,bus=xhci.0,port=2",
            "-drive", f"file={disco_},if=none,id=nv,format=raw,snapshot=on", "-device", "nvme,serial=caja,drive=nv",
            "-audiodev", "none,id=snd0", "-device", "virtio-sound-pci,audiodev=snd0",
            # Sin salida al PC ni a Internet: DHCP sí (la caja Linux toma dirección), nada más.
            "-netdev", "user,id=red0,restrict=on",
            "-device", "virtio-net-pci,netdev=red0,disable-legacy=on,mac=52:54:00:12:34:56,romfile="]


def grabar(args):
    d = Path(args.dir) / "grabacion"
    if d.exists():
        shutil.rmtree(d)
    (d / "cuadros").mkdir(parents=True)
    (d / "arranque" / "EFI" / "BOOT").mkdir(parents=True)
    shutil.copy(args.efi, d / "arranque" / "EFI" / "BOOT" / "BOOTAA64.EFI")
    ancho, alto = map(int, args.pantalla.split("x"))
    (d / "pantalla.txt").write_text(f"{ancho} {alto}\n")
    inicio = time.time()
    (d / "inicio.txt").write_text(f"{inicio}\n")
    q = subprocess.Popen(qemu_orden(args, d, Path(args.disco).resolve()), stdout=subprocess.DEVNULL,
                         stderr=open(d / "qemu.err", "w"))
    (d / "qemu.pid").write_text(f"{q.pid}\n")
    try:
        grab = Grabadora(Monitor(str(sockets(d) / "grab.sock")), d, inicio)
        grab.start()
        if args.guion == "si":
            import demo_guion
            demo_guion.correr(Demo(d, inicio, ancho, alto))
        else:
            q.wait(timeout=args.tope)
    except subprocess.TimeoutExpired:
        pass
    finally:
        try:
            grab.parar = True
            grab.join(10)
        except NameError:
            pass
        if q.poll() is None:
            q.terminate()
            try:
                q.wait(20)
            except subprocess.TimeoutExpired:
                q.kill()
    print(f"demo: {grab.n} cuadros en {d / 'cuadros'}")


# --- Manejar la máquina (el guion y a mano) ---

# Teclado español: lo que hay que pulsar (nombres de `sendkey` de QEMU, posición de EE. UU.) para cada letra.
ESPECIALES = {" ": "spc", "-": "slash", ".": "dot", ",": "comma", "/": "shift-7", "=": "shift-0", ":": "shift-dot",
              "_": "shift-slash", "&": "shift-6", ";": "shift-comma", "<": "less", ">": "shift-less", '"': "shift-2",
              "#": "alt_r-3", "?": "shift-minus", "|": "alt_r-1", "'": "minus", "(": "shift-8", ")": "shift-9",
              "*": "shift-bracket_right", "+": "bracket_right", "!": "shift-1", "$": "shift-4"}


class Demo:
    """Lo que usa el guion: la salida serie, el teclado, la tableta y las marcas de los tramos."""

    def __init__(self, d, inicio, ancho, alto):
        self.d, self.inicio, self.ancho, self.alto = Path(d), inicio, ancho, alto
        self.mon = Monitor(str(sockets(self.d) / "mon.sock"))
        self.qmp = Qmp(str(sockets(self.d) / "qmp.sock"))
        self.x, self.y = 0, 0

    def t(self):
        return time.time() - self.inicio

    def apuntar(self, texto):
        with open(self.d / "guion.txt", "a") as f:
            f.write(f"{self.t():7.1f} s: {texto}\n")

    def marca(self, nombre):
        """Empieza un tramo del vídeo (lo que se monta con su locución y su escenario de EnMarcha)."""
        with open(self.d / "marcas.txt", "a") as f:
            f.write(f"{self.t():.3f} {nombre}\n")
        self.apuntar(f"tramo {nombre}")

    def texto(self):
        try:
            return re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", open(self.d / "serie.txt", errors="replace").read())
        except OSError:
            return ""

    def cuenta(self, patron):
        return len(re.findall(patron, self.texto()))

    def esperar(self, patron, segundos, desde=0):
        """Espera a que `patron` salga más de `desde` veces; devuelve los grupos de la última o None."""
        hasta = time.time() + segundos
        while time.time() < hasta:
            m = re.findall(patron, self.texto())
            if len(m) > desde:
                return m[-1]
            time.sleep(0.25)
        self.apuntar(f"no sale: {patron}")
        return None

    def pausa(self, s):
        time.sleep(s)

    def tecla(self, *teclas, pausa=0.12):
        for k in teclas:
            self.mon.orden("sendkey " + k)
            time.sleep(pausa)

    def teclear(self, texto, intro=False, pausa=0.12):
        teclas = []
        for c in texto:
            if c in ESPECIALES:
                teclas.append(ESPECIALES[c])
            elif c.isupper():
                teclas.append("shift-" + c.lower())
            elif c == "\n":
                teclas.append("ret")
            else:
                teclas.append(c)
        if intro:
            teclas.append("ret")
        self.tecla(*teclas, pausa=pausa)

    def _abs(self, x, y):
        return [{"type": "abs", "data": {"axis": "x", "value": int(x * 32767 / max(1, self.ancho - 1))}},
                {"type": "abs", "data": {"axis": "y", "value": int(y * 32767 / max(1, self.alto - 1))}}]

    def ir(self, x, y, pasos=12, s=0.4):
        """Lleva el puntero a (x, y) en `pasos` movimientos, para que se vea ir."""
        x0, y0 = self.x, self.y
        for i in range(1, pasos + 1):
            self.qmp.eventos(self._abs(x0 + (x - x0) * i / pasos, y0 + (y - y0) * i / pasos))
            time.sleep(s / pasos)
        self.x, self.y = x, y

    def boton(self, abajo, cual="left"):
        self.qmp.eventos([{"type": "btn", "data": {"down": abajo, "button": cual}}])

    def clic(self, x, y, cual="left", veces=1):
        self.ir(x, y)
        time.sleep(0.15)
        for _ in range(veces):
            self.boton(True, cual)
            time.sleep(0.08)
            self.boton(False, cual)
            time.sleep(0.12)

    def arrastrar(self, x0, y0, x1, y1):
        self.ir(x0, y0)
        self.boton(True)
        time.sleep(0.2)
        self.ir(x1, y1, pasos=20, s=1.0)
        time.sleep(0.2)
        self.boton(False)

    def captura(self, nombre):
        self.mon.orden(f"screendump {self.d / (nombre + '.ppm')}")


def a_mano(d):
    d = Path(d) / "grabacion"
    ancho, alto = map(int, (d / "pantalla.txt").read_text().split())
    inicio = float((d / "inicio.txt").read_text())
    return d, ancho, alto, inicio


def principal():
    import argparse
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    if len(sys.argv) > 1 and sys.argv[1] == "programas":
        return programas(sys.argv[2], sys.argv[3])
    if len(sys.argv) > 1 and sys.argv[1] == "disco":
        return disco(sys.argv[2], sys.argv[3], sys.argv[4])
    if len(sys.argv) > 1 and sys.argv[1] in ("orden", "raton", "teclear"):
        d, ancho, alto, inicio = a_mano(sys.argv[2])
        if sys.argv[1] == "orden":
            print(Monitor(str(sockets(d) / "mon.sock")).orden(sys.argv[3]))
            return
        m = Demo.__new__(Demo)
        m.d, m.inicio, m.ancho, m.alto, m.x, m.y = d, inicio, ancho, alto, 0, 0
        m.mon = Monitor(str(sockets(d) / "mon.sock"))
        if sys.argv[1] == "teclear":
            m.teclear(sys.argv[3], intro=len(sys.argv) > 4)
            return
        m.qmp = Qmp(str(sockets(d) / "qmp.sock"))
        x, y = int(sys.argv[3]), int(sys.argv[4])
        m.x, m.y = x, y
        que = sys.argv[5] if len(sys.argv) > 5 else "nada"
        m.ir(x, y, pasos=1, s=0)
        if que in ("clic", "doble", "derecho"):
            m.clic(x, y, "right" if que == "derecho" else "left", 2 if que == "doble" else 1)
        return
    p = argparse.ArgumentParser()
    p.add_argument("accion", choices=["grabar"])
    p.add_argument("--efi", required=True)
    p.add_argument("--disco", required=True)
    p.add_argument("--pantalla", default="1280x720")
    p.add_argument("--dir", required=True)
    p.add_argument("--guion", default="si")
    p.add_argument("--tope", type=float, default=3600)
    grabar(p.parse_args())


if __name__ == "__main__":
    principal()
