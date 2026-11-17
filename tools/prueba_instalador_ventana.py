#!/usr/bin/env python3
"""S32: la ventana "Instalar DashOS" y la orden `Instalar` del Shell, como una persona (teclado y tableta por QMP), en
el banco (`banco_instalador.py`; los discos base no se tocan). Disco vacío (NVMe, capa propia):

1. Entra Ana (la cuenta de la imagen de pruebas, administradora). La ventana del instalador está en el escritorio.
2. "Borrar nvme0 entero": el plan con su dibujo, en la ventana (captura). "Instalar": la ventana del sistema pide la
   contraseña de un administrador (captura); con la de Ana, se instala.
3. En el Shell: `Instalar` (los discos) y `Instalar nvme0 reinstalar` (el plan), sin escribir nada.
4. DashOS arranca del disco por su entrada de la NVRAM.

Uso: prueba_instalador_ventana.py. Capturas en CAPTURAS/ventana-NN.png.
"""
import importlib.util
import os
import re
import subprocess
import sys
import time

AQUI = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("banco", os.path.join(AQUI, "banco_instalador.py"))
banco = importlib.util.module_from_spec(spec)
spec.loader.exec_module(banco)
PENDRIVE = os.path.join(os.path.dirname(AQUI), "kernel", "target", "pendrive-x86.img")
ANCHO, ALTO = 1280, 800
B = banco.BANCO


def main():
    banco.capa("ventana", "empty-50g")
    serie = os.path.join(B, "ventana.serie.txt")
    if os.path.exists(serie):
        os.unlink(serie)
    d = banco.mio(os.path.join(B, "qemu-ventana"))
    os.makedirs(d, exist_ok=True)
    qmp = os.path.join(d, "qmp")
    a = ["qemu-system-x86_64", "-name", "banco-ventana", "-M", "q35", "-accel", "kvm", "-cpu", "host", "-smp", "4", "-m", "2G",
         "-drive", f"if=pflash,format=raw,readonly=on,file={banco.OVMF_CODE}", "-drive", f"if=pflash,format=raw,file={banco.vars_ruta('ventana')}",
         "-drive", f"file={banco.capa_ruta('ventana')},format=qcow2,if=none,id=d0,cache=unsafe", "-device", "nvme,serial=banco,drive=d0,bootindex=2",
         "-display", "none", "-vga", "std", "-qmp", f"unix:{qmp},server,nowait", "-serial", f"file:{serie}",
         "-device", "qemu-xhci,id=xhci", "-device", "usb-kbd,bus=xhci.0", "-device", "usb-tablet,bus=xhci.0",
         "-drive", f"file={PENDRIVE},format=raw,if=none,id=usb0,readonly=on", "-device", "usb-storage,bus=xhci.0,drive=usb0,bootindex=1"]
    p = subprocess.Popen(a, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    q = banco.Qmp(qmp)
    n = [0]
    leer = lambda: re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", open(serie, errors="replace").read()) if os.path.exists(serie) else ""

    def esperar(patron, s, desde=0):
        hasta = time.time() + s
        while time.time() < hasta:
            m = re.findall(patron, leer()[desde:])
            if m:
                return m
            time.sleep(0.3)
        return None

    def captura(nombre):
        n[0] += 1
        q.orden("screendump", filename=os.path.join(banco.CAPTURAS, f"ventana-{n[0]:02d}-{nombre}.png"), format="png")

    def ir(x, y):
        q.orden("input-send-event", events=[{"type": "abs", "data": {"axis": "x", "value": int(x * 32767 / (ANCHO - 1))}},
                                            {"type": "abs", "data": {"axis": "y", "value": int(y * 32767 / (ALTO - 1))}}])
        time.sleep(0.15)

    def clic(x, y):
        ir(x, y)
        for down in (True, False):
            q.orden("input-send-event", events=[{"type": "btn", "data": {"down": down, "button": "left"}}])
            time.sleep(0.1)

    bien = []

    def apuntar(ok, que):
        print(f"{'BIEN' if ok else 'MAL'}: {que}", flush=True)
        bien.append(ok)

    try:
        ok = esperar(r"\[guardian\] pantalla de entrada pintada", 120)
        apuntar(bool(ok), "en vivo: la pantalla de entrada")
        captura("entrada")
        q.escribir("Ana\n")
        time.sleep(0.8)
        q.escribir("clave de Ana\n")
        apuntar(bool(esperar(r"\[guardian\] entra Ana", 60)), "entra Ana")
        time.sleep(5)
        v = esperar(r"\[compositor\] ventana de instalar en \((-?\d+), (-?\d+)\)", 30)
        b = esperar(r"\[instalar\] botones: hueco en \((\d+), (\d+)\), entero en \((\d+), (\d+)\), instalar en \((\d+), (\d+)\)", 10)
        captura("escritorio")
        apuntar(bool(v and b), f"la ventana Instalar DashOS en el escritorio ({v}, {b})")
        if not (v and b):
            return
        vx, vy = map(int, v[-1])
        hx, hy, ex, ey, ix, iy = map(int, b[-1])
        desde = len(leer())
        clic(vx + ex, vy + ey)
        apuntar(bool(esperar(r"\[instalar\] plan de nvme0 \(disco-entero\) en pantalla", 30, desde)), "el plan de disco entero, en la ventana")
        time.sleep(1)
        captura("plan")
        desde = len(leer())
        clic(vx + ix, vy + iy)
        apuntar(bool(esperar(r"\[guardian\]", 30, desde)), "la ventana del sistema pide la contraseña de un administrador")
        time.sleep(2)
        captura("contrasena")
        q.escribir("clave de Ana\n")
        apuntar(bool(esperar(r"\[instalar\] Instalado", 240, desde)), "instalado desde la ventana, con la confirmación del administrador")
        time.sleep(1)
        captura("instalado")
        # El Shell, por el lanzador de la barra.
        bb = esperar(r"\[barra\] botones: lanzador en \((\d+), (\d+)\)", 10)
        if bb:
            clic(int(bb[-1][0]), int(bb[-1][1]))
            time.sleep(1)
            q.escribir("shell")
            time.sleep(1)
            desde = len(leer())
            q.escribir("\n")
            sh = esperar(r"\[compositor\] ventana de [Ss]hell en \((-?\d+), (-?\d+)\)", 60, desde)
            if sh:
                sx, sy = map(int, sh[-1])
                time.sleep(2)
                clic(sx + 120, max(sy + 40, 1))
                desde = len(leer())
                q.escribir("Instalar\n")
                time.sleep(6)
                captura("shell-ver")
                q.escribir("Instalar nvme0 reinstalar\n")
                time.sleep(6)
                captura("shell-plan")
                t = leer()[desde:]
                apuntar("Shell pide instalador: 0" in t and "pregunta en pantalla" not in t,
                        "la orden Instalar en el Shell tiene el instalador sin preguntar (lo que dice, en las capturas shell-ver y shell-plan)")
            else:
                apuntar(False, "no se abre el Shell")
    finally:
        try:
            q.orden("quit")
        except OSError:
            pass
        p.wait(30)
    ok, t = banco.arrancar("ventana", 120, bus="nvme", serie=os.path.join(B, "ventana-disco.serie.txt"), esperar="[archivos] RedoxFS abierto", etiqueta="ventana-disco", memoria="2G")
    apuntar(ok == "visto" and "particiones: GPT; DashOS" in t and "vivo: arrancado" not in t, "DashOS arranca del disco tras instalar desde la ventana")
    print(f"{sum(bien)} bien, {len(bien) - sum(bien)} mal")
    sys.exit(0 if all(bien) else 1)


if __name__ == "__main__":
    main()
