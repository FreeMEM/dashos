#!/usr/bin/env python3
"""S32: el banco de pruebas del instalador, sobre los discos base del banco de AmiCachy.

REGLA: los discos base (`~/Projects/AmiCachy/dev/dualboot-vm/baselines/`) no se tocan nunca. Cada prueba va en una capa
qcow2 propia, con el base de respaldo y de solo lectura (`qemu-img create -b`), en BANCO (por omisión
~/recuperar/geosian-7d/instalador-vm/), con sus OVMF_VARS. Este guion se niega a crear, escribir o borrar nada fuera de
BANCO, y comprueba al empezar y al acabar que los base no han cambiado (tamaño y fecha; el vacío, además, su huella).

Órdenes (las capturas van a CAPTURAS, por omisión ~/recuperar/geosian-7d/instalador-capturas/):

  capa <nombre> <base|capa>          una capa nueva sobre un base (win11-ntfs, debian12-ext4-grub, empty-50g) u otra capa
  arrancar <capa> <segundos> [opciones] arranca la capa en QEMU (con KVM), con capturas cada pocos segundos
      --bus virtio|nvme|sata  el disco (virtio para Windows y Debian, como se instalaron; nvme o sata para DashOS)
      --vars <nombre>         sus OVMF_VARS (por omisión, los de la capa; nuevos si no hay)
      --pendrive <img>        un pendrive USB (usb-storage) que arranca primero
      --sata2 <capa>          un segundo disco (SATA)
      --serie <fichero>       el puerto serie a un fichero
      --esperar <texto>       acaba en cuanto el serie dice eso
      --teclas <guion>        teclas por QMP: "espera:5,esc,espera:2,abajo,intro...", "captura", "texto:Ana~" (~ es
                              Intro, teclado español) y "menu:<texto>"
                              (elige en el menú de arranque del firmware la entrada que dice eso, por OCR)
      --matar-en <texto>      mata QEMU (kill -9: corte de luz) en cuanto el serie dice eso
      --apagar <segundos>     al acabar, el botón de apagar (ACPI) y espera a que se apague
      --menu <segundos>       el firmware espera esos segundos con su menú de arranque (ESC lo abre)
  exportar <capa> <fichero>          la capa entera como fichero crudo de solo lectura (FUSE) para mirarla; `soltar`
  soltar <fichero>
  comparar <capa-antes> <capa-despues> [--deja N,...]   las particiones de antes, byte a byte; --deja: las que pueden
                                     cambiar (la ESP, a la que se añade EFI/DashOS: de ella se comparan los ficheros
                                     de los demás)
  bases                              comprueba que los discos base siguen como estaban
"""
import hashlib
import json
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time

AMICACHY = os.path.realpath(os.path.expanduser("~/Projects/AmiCachy/dev/dualboot-vm"))
BASES = os.path.join(AMICACHY, "baselines")
BANCO = os.path.realpath(os.path.expanduser(os.environ.get("BANCO", "~/recuperar/geosian-7d/instalador-vm")))
CAPTURAS = os.path.realpath(os.path.expanduser(os.environ.get("CAPTURAS", "~/recuperar/geosian-7d/instalador-capturas")))
QEMU_IMG = "/usr/bin/qemu-img"
OVMF_CODE = "/usr/share/edk2/x64/OVMF_CODE.4m.fd"
OVMF_VARS = os.path.join(AMICACHY, "ovmf", "OVMF_VARS-template.4m.fd")


def mio(ruta):
    """La ruta, si es de BANCO o de CAPTURAS; si no, se para (nada fuera de ahí se crea ni se escribe)."""
    r = os.path.realpath(ruta)
    if r.startswith(AMICACHY + os.sep) or r == AMICACHY:
        sys.exit(f"NO: {ruta} es del proyecto AmiCachy, no se toca")
    if not (r.startswith(BANCO + os.sep) or r.startswith(CAPTURAS + os.sep)):
        sys.exit(f"NO: {ruta} está fuera del banco ({BANCO})")
    return r


def capa_ruta(n):
    return mio(os.path.join(BANCO, n if n.endswith(".qcow2") else n + ".qcow2"))


def huella_bases():
    """Tamaño y fecha de cada base, y la huella del vacío (es pequeño)."""
    h = {}
    for n in sorted(os.listdir(BASES)):
        if n.endswith(".qcow2"):
            s = os.stat(os.path.join(BASES, n))
            h[n] = (s.st_size, s.st_mtime_ns)
    h["empty-50g.qcow2#sha256"] = hashlib.sha256(open(os.path.join(BASES, "empty-50g.qcow2"), "rb").read()).hexdigest()
    return h


def bases():
    guardada = os.path.join(BANCO, "bases.json")
    ahora = huella_bases()
    if not os.path.exists(guardada):
        json.dump(ahora, open(mio(guardada), "w"), indent=1)
        print("bases: huella guardada")
        return True
    antes = json.load(open(guardada))
    antes = {k: tuple(v) if isinstance(v, list) else v for k, v in antes.items()}
    if antes != ahora:
        print(f"MAL: los discos base han cambiado: {antes} -> {ahora}")
        return False
    print("bases: sin cambios")
    return True


def capa(nombre, sobre):
    ruta = capa_ruta(nombre)
    base = os.path.join(BASES, sobre + ".qcow2")
    if not os.path.exists(base):
        base = capa_ruta(sobre)
    if os.path.exists(ruta):
        os.unlink(ruta)
    subprocess.run([QEMU_IMG, "create", "-q", "-f", "qcow2", "-b", base, "-F", "qcow2", ruta], check=True)
    vars_ = vars_ruta(nombre)
    # Los OVMF_VARS: los de la capa de debajo, si es nuestra (lo que el sistema de debajo dejó en la NVRAM); si no, nuevos.
    debajo = vars_ruta(sobre)
    shutil.copyfile(debajo if os.path.exists(debajo) else OVMF_VARS, vars_)
    print(f"capa {ruta} sobre {base}")


def vars_ruta(nombre):
    return mio(os.path.join(BANCO, nombre.removesuffix(".qcow2") + ".vars.fd"))


class Qmp:
    def __init__(self, ruta):
        for _ in range(200):
            try:
                self.s = socket.socket(socket.AF_UNIX)
                self.s.connect(ruta)
                break
            except OSError:
                time.sleep(0.1)
        self.f = self.s.makefile("rwb")
        self.f.readline()
        self.orden("qmp_capabilities")

    def orden(self, nombre, **a):
        m = {"execute": nombre}
        if a:
            m["arguments"] = a
        self.f.write(json.dumps(m).encode() + b"\n")
        self.f.flush()
        while True:
            r = json.loads(self.f.readline())
            if "return" in r or "error" in r:
                return r

    def tecla(self, t):
        self.orden("send-key", keys=[{"type": "qcode", "data": t}])

    def escribir(self, texto):
        """Un texto con el teclado (letras, cifras, espacio, punto, dos puntos, ">", "-"; mayúsculas con mayúsculas)."""
        especiales = {" ": ["spc"], ".": ["dot"], ":": ["shift", "dot"], ">": ["shift", "less"], "-": ["slash"], "\n": ["ret"]}
        for c in texto:
            if c in especiales:
                ks = especiales[c]
            elif c.isupper():
                ks = ["shift", c.lower()]
            else:
                ks = [c]
            self.orden("send-key", keys=[{"type": "qcode", "data": k} for k in ks])
            time.sleep(0.08)


TECLAS = {"esc": "esc", "intro": "ret", "abajo": "down", "arriba": "up", "izquierda": "left", "derecha": "right", "f2": "f2", "f12": "f12", "tab": "tab", "espacio": "spc"}


def arrancar(nombre, segundos, bus="virtio", vars_nombre=None, pendrive=None, sata2=None, serie=None, esperar=None, teclas=None, matar_en=None, etiqueta=None, memoria="4G", apagar=None, menu=None):
    """Arranca la capa; devuelve (motivo del final, texto del serie). Capturas en CAPTURAS/<etiqueta>-NN.png."""
    ruta = capa_ruta(nombre)
    vars_ = vars_ruta(vars_nombre or nombre)
    if not os.path.exists(vars_):
        shutil.copyfile(OVMF_VARS, vars_)
    etiqueta = etiqueta or nombre
    os.makedirs(CAPTURAS, exist_ok=True)
    d = tempfile.mkdtemp(dir=BANCO, prefix="qemu-")
    qmp = os.path.join(d, "qmp")
    serie = mio(serie) if serie else os.path.join(d, "serie.txt")
    if os.path.exists(serie):
        os.unlink(serie)
    disco = f"file={ruta},format=qcow2,if=none,id=d0,cache=unsafe,discard=unmap"
    if bus == "virtio":
        dispositivo = ["-device", "virtio-blk-pci,drive=d0,bootindex=2"]
    elif bus == "nvme":
        dispositivo = ["-device", "nvme,serial=banco,drive=d0,bootindex=2"]
    else:
        dispositivo = ["-device", "ide-hd,drive=d0,bus=ide.0,bootindex=2"]
    a = ["qemu-system-x86_64", "-name", f"banco-{etiqueta}", "-M", "q35", "-accel", "kvm", "-cpu", "host", "-smp", "4", "-m", memoria,
         "-drive", f"if=pflash,format=raw,readonly=on,file={OVMF_CODE}", "-drive", f"if=pflash,format=raw,file={vars_}",
         "-drive", disco, *dispositivo, "-display", "none", "-vga", "std", "-qmp", f"unix:{qmp},server,nowait",
         "-serial", f"file:{serie}", "-device", "qemu-xhci,id=xhci", "-device", "usb-kbd,bus=xhci.0", "-device", "usb-tablet,bus=xhci.0",
         "-netdev", "user,id=n0,restrict=on", "-device", "virtio-net-pci,netdev=n0,romfile=", "-audiodev", "none,id=snd0"]
    if sata2:
        a += ["-drive", f"file={capa_ruta(sata2)},format=qcow2,if=none,id=d1,cache=unsafe", "-device", "ide-hd,drive=d1,bus=ide.2"]
    if menu:
        # El menú de arranque del firmware (OVMF): con ESC durante la espera, su Boot Manager.
        a += ["-boot", f"menu=on,splash-time={int(float(menu) * 1000)}"]
    if pendrive:
        a += ["-drive", f"file={os.path.realpath(pendrive)},format=raw,if=none,id=usb0,readonly=on", "-device", "usb-storage,bus=xhci.0,drive=usb0,bootindex=1"]
    p = subprocess.Popen(a, stdout=subprocess.DEVNULL, stderr=open(os.path.join(d, "qemu.err"), "w"))
    q = Qmp(qmp)
    inicio = time.time()
    n = 0
    motivo = "tiempo"
    pendientes = [t.strip() for t in teclas.split(",")] if teclas else []
    proxima_tecla = time.time() + 1
    proxima_captura = 0
    leer = lambda: open(serie, errors="replace").read() if os.path.exists(serie) else ""

    def captura():
        nonlocal n
        n += 1
        f = os.path.join(CAPTURAS, f"{etiqueta}-{n:02d}.png")
        q.orden("screendump", filename=f, format="png")
        return f

    try:
        while time.time() - inicio < segundos:
            if p.poll() is not None:
                motivo = "acabó"
                break
            t = leer()
            if matar_en and matar_en in t:
                captura()
                p.send_signal(signal.SIGKILL)
                p.wait()
                motivo = "matado"
                break
            if esperar and esperar in t:
                time.sleep(2)
                captura()
                motivo = "visto"
                break
            if pendientes and time.time() >= proxima_tecla:
                k = pendientes.pop(0)
                if k.startswith("espera:"):
                    proxima_tecla = time.time() + float(k[7:])
                elif k == "captura":
                    captura()
                elif k.startswith("texto:"):
                    q.escribir(k[6:].replace("~", "\n"))
                elif k.startswith("menu:"):
                    # Elegir en el menú de arranque del firmware la entrada que dice eso (por OCR de una captura).
                    i = elegir_en_menu(captura(), k[5:])
                    print(f"menú del firmware: '{k[5:]}' es la entrada {i}")
                    pendientes[:0] = ["abajo"] * i + ["intro"] if i is not None else []
                    proxima_tecla = time.time() + 0.5
                else:
                    q.tecla(TECLAS.get(k, k))
                    proxima_tecla = time.time() + 0.4
            if time.time() >= proxima_captura:
                captura()
                proxima_captura = time.time() + 15
            time.sleep(0.2)
        if p.poll() is None and motivo != "matado" and apagar:
            # Apagado limpio (el botón de encendido por ACPI): Windows y Debian se apagan del todo.
            captura()
            q.orden("system_powerdown")
            hasta = time.time() + float(apagar)
            while time.time() < hasta and p.poll() is None:
                time.sleep(1)
            motivo += ", apagado limpio" if p.poll() is not None else ", NO se apagó"
        if p.poll() is None and motivo != "matado":
            captura()
            try:
                q.orden("quit")
            except OSError:
                pass
            try:
                p.wait(20)
            except subprocess.TimeoutExpired:
                p.kill()
    finally:
        if p.poll() is None:
            p.kill()
    t = leer()
    shutil.rmtree(d, ignore_errors=True)
    print(f"arrancar {nombre}: {motivo} a los {time.time() - inicio:.0f} s, {n} capturas ({etiqueta}-NN.png)")
    return motivo, t


def lineas_menu(png):
    """Las entradas del menú de arranque de OVMF en una captura, por OCR (tesseract), en orden."""
    from PIL import Image, ImageOps
    im = Image.open(png).convert("L")
    w, h = im.size
    im = ImageOps.invert(im.crop((w // 5, h // 6, 4 * w // 5, 5 * h // 6)).resize((3 * w * 3 // 5, 3 * h * 2 // 3)))
    t = tempfile.mktemp(suffix=".png", dir=BANCO)
    im.save(t)
    r = subprocess.run(["tesseract", t, "-", "--psm", "6"], capture_output=True, text=True).stdout
    os.unlink(t)
    fuera = ("select boot device", "move selection", "ENTER", "ESC")
    return [l.strip() for l in r.splitlines() if l.strip() and not any(f.lower() in l.lower() for f in fuera)]


def elegir_en_menu(png, patron):
    """El número (desde 0) de la entrada del menú que casa con `patron` (sin distinguir mayúsculas, 0 y O iguales)."""
    norm = lambda x: x.lower().replace("0", "o")
    lineas = lineas_menu(png)
    print(f"menú del firmware: {lineas}")
    for i, l in enumerate(lineas):
        if norm(patron) in norm(l):
            return i
    return None


def exportar(nombre, fichero):
    """La capa entera como fichero crudo de solo lectura, por FUSE (qemu-storage-daemon). Devuelve el proceso."""
    fichero = mio(fichero)
    open(fichero, "a").close()
    base = os.path.join(BASES, nombre + ".qcow2")
    ruta = base if os.path.exists(base) else capa_ruta(nombre)
    p = subprocess.Popen(["qemu-storage-daemon", "--blockdev", f"driver=file,node-name=f,filename={ruta},read-only=on",
                          "--blockdev", "driver=qcow2,node-name=q,file=f,read-only=on",
                          "--export", f"type=fuse,id=e,node-name=q,mountpoint={fichero},writable=off,allow-other=off"], stderr=subprocess.DEVNULL)
    for _ in range(100):
        if os.path.getsize(fichero) > 0:
            return p
        time.sleep(0.1)
    p.kill()
    sys.exit(f"no se exporta {nombre}")


def soltar(fichero, p=None):
    subprocess.run(["fusermount3", "-u", mio(fichero)], stderr=subprocess.DEVNULL)
    if p:
        p.terminate()
        try:
            p.wait(10)
        except subprocess.TimeoutExpired:
            p.kill()


def particiones(fichero):
    """Las particiones de un disco (GPT), con sgdisk: [(número, desde, hasta exclusivo)]."""
    r = subprocess.run(["sgdisk", "-p", fichero], capture_output=True, text=True).stdout
    v = []
    for l in r.splitlines():
        m = re.match(r"\s+(\d+)\s+(\d+)\s+(\d+)\s", l)
        if m:
            v.append((int(m[1]), int(m[2]), int(m[3]) + 1))
    return v


def huella(fichero, desde, hasta):
    h = hashlib.sha256()
    with open(fichero, "rb") as f:
        f.seek(desde * 512)
        quedan = (hasta - desde) * 512
        while quedan:
            b = f.read(min(quedan, 8 << 20))
            if not b:
                break
            h.update(b)
            quedan -= len(b)
    return h.hexdigest()


def ficheros_fat(fichero, desde):
    """Los ficheros de una FAT (mtools), con su huella: {camino: sha256}."""
    with tempfile.TemporaryDirectory(dir=BANCO) as t:
        subprocess.run(["mcopy", "-s", "-n", "-i", f"{fichero}@@{desde * 512}", "::", t], check=True, capture_output=True)
        r = {}
        for raiz, _, fs in os.walk(t):
            for f in fs:
                c = os.path.join(raiz, f)
                r[os.path.relpath(c, t)] = hashlib.sha256(open(c, "rb").read()).hexdigest()
        return r


def comparar(antes, despues, deja=()):
    """Las particiones de `antes`, en `despues`: en el mismo sitio y con los mismos bytes; las de `deja` (ESP), con los
    mismos ficheros salvo los de EFI/DashOS. Devuelve la lista de lo que cambió (vacía: bien)."""
    fa, fd = os.path.join(BANCO, "comparar-antes.raw"), os.path.join(BANCO, "comparar-despues.raw")
    pa, pd = exportar(antes, fa), exportar(despues, fd)
    malas = []
    try:
        pda = particiones(fd)
        for num, a, b in particiones(fa):
            if (num, a, b) not in pda:
                malas.append(f"la partición {num} ({a}..{b}) ya no está igual: {pda}")
                continue
            if num in deja:
                xa, xd = ficheros_fat(fa, a), ficheros_fat(fd, a)
                for c, h in xa.items():
                    if xd.get(c) != h:
                        malas.append(f"partición {num}: el fichero {c} cambió")
                nuevos = [c for c in xd if c not in xa and not c.upper().startswith("EFI/DASHOS/")]
                if nuevos:
                    malas.append(f"partición {num}: ficheros nuevos fuera de EFI/DashOS: {nuevos}")
                print(f"  partición {num} (ESP): {len(xa)} ficheros de antes, iguales; nuevos: {sorted(c for c in xd if c not in xa)}")
                continue
            ha, hd = huella(fa, a, b), huella(fd, a, b)
            print(f"  partición {num} ({a}..{b}): {'igual' if ha == hd else 'CAMBIADA'}")
            if ha != hd:
                malas.append(f"la partición {num} cambió")
    finally:
        soltar(fa, pa)
        soltar(fd, pd)
    return malas


def main():
    a = sys.argv[1:]
    if not a:
        sys.exit(__doc__)
    if a[0] == "bases":
        sys.exit(0 if bases() else 1)
    if a[0] == "capa":
        capa(a[1], a[2])
    elif a[0] == "arrancar":
        op = {}
        i = 3
        while i < len(a):
            op[a[i].lstrip("-").replace("-", "_")] = a[i + 1]
            i += 2
        if "vars" in op:
            op["vars_nombre"] = op.pop("vars")
        arrancar(a[1], float(a[2]), **op)
    elif a[0] == "exportar":
        p = exportar(a[1], a[2])
        print(f"exportado en {a[2]} (proceso {p.pid}); soltar con: banco_instalador.py soltar {a[2]}")
    elif a[0] == "soltar":
        soltar(a[1])
    elif a[0] == "comparar":
        deja = tuple(int(x) for x in a[a.index("--deja") + 1].split(",")) if "--deja" in a else ()
        malas = comparar(a[1], a[2], deja)
        print("BIEN: lo de antes, intacto" if not malas else "MAL:\n  " + "\n  ".join(malas))
        sys.exit(1 if malas else 0)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
